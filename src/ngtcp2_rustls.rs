//! Rustls TLS 1.3 adapter for the vendored ngtcp2 transport.
//!
//! Rustls' QUIC packet API deliberately hides IVs inside its packet keys, while ngtcp2
//! installs an IV separately and passes an already-composed nonce to its AEAD callbacks.
//! The small provider wrapper below captures only each derived IV (never key bytes); the
//! adapter recovers the QUIC packet number from `nonce = IV XOR packet_number` and then calls
//! Rustls' packet-key API with that number and the original header.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock};

use ngnet_quic as ng;
use ring::aead::{self, Aad, LessSafeKey, Nonce, UnboundKey};
use rustls::crypto::cipher::AeadKey;
use rustls::crypto::cipher::Iv as RustlsIv;
use rustls::crypto::ring::default_provider;
use rustls::quic::{
    Algorithm as RustlsQuicAlgorithm, DirectionalKeys as RustlsDirectionalKeys,
    KeyChange as RustlsKeyChange, PacketKey as RustlsPacketKeyTrait,
};
use rustls::{
    CipherSuite, CipherSuiteCommon, ClientConfig, SupportedCipherSuite, Tls13CipherSuite,
};

const RBX_TRANSPORT_ALPN: &[u8] = b"RbxTransport";
const QUIC_IV_LEN: usize = 12;
const TLS13_SHA256_SECRET_LEN: usize = 32;

#[derive(Clone)]
pub(super) struct RbxTransportRustlsBackend {
    config: Arc<ClientConfig>,
    server_name: String,
}

impl RbxTransportRustlsBackend {
    pub(super) fn new(
        expected_public_key: [u8; 32],
        server_name: &str,
        enable_sni: bool,
    ) -> Result<Self, String> {
        let provider = Arc::new(rbx_transport_crypto_provider());
        let verifier = Arc::new(ExpectedRpkVerifier::new(expected_public_key));
        let mut config = ClientConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(|error| format!("failed to configure TLS 1.3: {error}"))?
            .dangerous()
            .with_custom_certificate_verifier(verifier)
            .with_no_client_auth();
        config.alpn_protocols = vec![RBX_TRANSPORT_ALPN.to_vec()];
        // The pure qdmux route uses GameFqdn as SNI. The RUPP route intentionally omits it.
        config.enable_sni = enable_sni;
        Ok(Self {
            config: Arc::new(config),
            server_name: server_name.to_owned(),
        })
    }
}

impl ng::Backend for RbxTransportRustlsBackend {
    type Session = RbxTransportRustlsSession;

    fn new_session(
        &self,
        role: ng::Role,
        server_name: Option<&str>,
    ) -> ng::Result<Self::Session> {
        if role != ng::Role::Client {
            return Err(ng::Error::backend("RbxTransport Rustls adapter is client-only"));
        }
        Ok(RbxTransportRustlsSession {
            config: Arc::clone(&self.config),
            server_name: server_name.unwrap_or(&self.server_name).to_owned(),
            connection: None,
            captured_ivs: Arc::new(Mutex::new(VecDeque::new())),
            events: VecDeque::new(),
            failure: None,
            peer_params_set: false,
            handshake_complete_reported: false,
            next_one_rtt_secrets: None,
            one_rtt_secret_len: TLS13_SHA256_SECRET_LEN,
            write_level: ng::Level::Initial,
        })
    }
}

#[derive(Debug)]
struct ExpectedRpkVerifier {
    expected_raw: [u8; 32],
    expected_spki_der: Vec<u8>,
}

impl ExpectedRpkVerifier {
    fn new(expected_raw: [u8; 32]) -> Self {
        const ED25519_SPKI_PREFIX: [u8; 12] = [
            0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
        ];
        let mut expected_spki_der = Vec::with_capacity(ED25519_SPKI_PREFIX.len() + 32);
        expected_spki_der.extend_from_slice(&ED25519_SPKI_PREFIX);
        expected_spki_der.extend_from_slice(&expected_raw);
        Self {
            expected_raw,
            expected_spki_der,
        }
    }

    fn presented_rpk_matches(&self, presented: &[u8]) -> bool {
        presented == self.expected_raw.as_slice() || presented == self.expected_spki_der.as_slice()
    }

    fn verify_ed25519_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        if dss.scheme != rustls::SignatureScheme::ED25519 {
            return Err(rustls::Error::InvalidCertificate(
                rustls::CertificateError::UnsupportedSignatureAlgorithm,
            ));
        }
        if !self.presented_rpk_matches(cert.as_ref()) {
            return Err(rustls::Error::InvalidCertificate(
                rustls::CertificateError::ApplicationVerificationFailure,
            ));
        }
        ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, &self.expected_raw)
            .verify(message, dss.signature())
            .map_err(|_| {
                rustls::Error::InvalidCertificate(rustls::CertificateError::BadSignature)
            })?;
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }
}

impl rustls::client::danger::ServerCertVerifier for ExpectedRpkVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        if self.presented_rpk_matches(end_entity.as_ref()) {
            Ok(rustls::client::danger::ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::InvalidCertificate(
                rustls::CertificateError::ApplicationVerificationFailure,
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        self.verify_ed25519_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        self.verify_ed25519_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![rustls::SignatureScheme::ED25519]
    }

    fn requires_raw_public_keys(&self) -> bool {
        true
    }
}

fn rbx_transport_crypto_provider() -> rustls::crypto::CryptoProvider {
    let mut provider = default_provider();
    // Keep the complete ring TLS 1.3 offer (AES-GCM and ChaCha20-Poly1305) so the RPK
    // verifier does not narrow Roblox's cipher-suite compatibility. Wrap each QUIC algorithm
    // only to capture the IVs ngtcp2 needs separately; packet cryptography remains ring's.
    provider.cipher_suites = provider
        .cipher_suites
        .iter()
        .filter_map(|supported| {
            let base = supported.tls13()?;
            base.quic?;
            Some(SupportedCipherSuite::Tls13(rbx_transport_tls13_suite(base)))
        })
        .collect();
    provider
}

fn rbx_transport_tls13_suite(base: &'static Tls13CipherSuite) -> &'static Tls13CipherSuite {
    static AES_128_SUITE: OnceLock<Tls13CipherSuite> = OnceLock::new();
    static AES_128_ALGORITHM: OnceLock<CapturedQuicAlgorithm> = OnceLock::new();
    static AES_256_SUITE: OnceLock<Tls13CipherSuite> = OnceLock::new();
    static AES_256_ALGORITHM: OnceLock<CapturedQuicAlgorithm> = OnceLock::new();
    static CHACHA_SUITE: OnceLock<Tls13CipherSuite> = OnceLock::new();
    static CHACHA_ALGORITHM: OnceLock<CapturedQuicAlgorithm> = OnceLock::new();

    let (suite_slot, algorithm_slot) = match base.common.suite {
        CipherSuite::TLS13_AES_128_GCM_SHA256 => (&AES_128_SUITE, &AES_128_ALGORITHM),
        CipherSuite::TLS13_AES_256_GCM_SHA384 => (&AES_256_SUITE, &AES_256_ALGORITHM),
        CipherSuite::TLS13_CHACHA20_POLY1305_SHA256 => (&CHACHA_SUITE, &CHACHA_ALGORITHM),
        _ => panic!("ring exposed an unexpected QUIC-capable TLS 1.3 suite"),
    };
    suite_slot.get_or_init(|| {
        let inner = base
            .quic
            .expect("a wrapped Rustls TLS 1.3 suite must have a QUIC algorithm");
        let capture = algorithm_slot.get_or_init(|| CapturedQuicAlgorithm { inner });
        Tls13CipherSuite {
            common: CipherSuiteCommon {
                suite: base.common.suite,
                hash_provider: base.common.hash_provider,
                confidentiality_limit: base.common.confidentiality_limit,
            },
            hkdf_provider: base.hkdf_provider,
            aead_alg: base.aead_alg,
            quic: Some(capture),
        }
    })
}

type CapturedIvQueue = Arc<Mutex<VecDeque<Vec<u8>>>>;

thread_local! {
    static ACTIVE_IV_CAPTURE: RefCell<Option<CapturedIvQueue>> = const { RefCell::new(None) };
}

struct IvCaptureGuard {
    previous: Option<CapturedIvQueue>,
}

impl IvCaptureGuard {
    fn install(queue: CapturedIvQueue) -> Self {
        let previous = ACTIVE_IV_CAPTURE.with(|active| active.replace(Some(queue)));
        Self { previous }
    }
}

impl Drop for IvCaptureGuard {
    fn drop(&mut self) {
        ACTIVE_IV_CAPTURE.with(|active| {
            active.replace(self.previous.take());
        });
    }
}

struct CapturedQuicAlgorithm {
    inner: &'static dyn RustlsQuicAlgorithm,
}

impl RustlsQuicAlgorithm for CapturedQuicAlgorithm {
    fn packet_key(
        &self,
        key: AeadKey,
        iv: RustlsIv,
    ) -> Box<dyn RustlsPacketKeyTrait> {
        let bytes = iv.as_ref().to_vec();
        ACTIVE_IV_CAPTURE.with(|active| {
            if let Some(queue) = active.borrow().as_ref()
                && let Ok(mut queue) = queue.lock()
            {
                queue.push_back(bytes);
            }
        });
        self.inner.packet_key(key, iv)
    }

    fn header_protection_key(
        &self,
        key: AeadKey,
    ) -> Box<dyn rustls::quic::HeaderProtectionKey> {
        self.inner.header_protection_key(key)
    }

    fn aead_key_len(&self) -> usize {
        self.inner.aead_key_len()
    }

    fn fips(&self) -> bool {
        self.inner.fips()
    }
}

pub(super) struct RbxTransportRustlsPacketKey {
    inner: Box<dyn RustlsPacketKeyTrait>,
    iv: Vec<u8>,
}

impl RbxTransportRustlsPacketKey {
    fn packet_number(&self, nonce: &[u8]) -> Result<u64, ng::CryptoError> {
        if self.iv.len() != QUIC_IV_LEN || nonce.len() != QUIC_IV_LEN {
            return Err(ng::CryptoError::Fatal);
        }
        let pn_offset = self.iv.len() - 8;
        if nonce[..pn_offset] != self.iv[..pn_offset] {
            return Err(ng::CryptoError::Fatal);
        }
        let mut packet_number = [0u8; 8];
        for (index, byte) in packet_number.iter_mut().enumerate() {
            *byte = nonce[pn_offset + index] ^ self.iv[pn_offset + index];
        }
        Ok(u64::from_be_bytes(packet_number))
    }
}

impl ng::PacketKey for RbxTransportRustlsPacketKey {
    fn seal(
        &self,
        buf: &mut [u8],
        plaintext_len: usize,
        nonce: &[u8],
        aad: &[u8],
    ) -> Result<(), ng::CryptoError> {
        let packet_number = self.packet_number(nonce)?;
        let tag_len = self.inner.tag_len();
        let end = plaintext_len
            .checked_add(tag_len)
            .filter(|end| *end <= buf.len())
            .ok_or(ng::CryptoError::Fatal)?;
        let tag = self
            .inner
            .encrypt_in_place(packet_number, aad, &mut buf[..plaintext_len])
            .map_err(|_| ng::CryptoError::Fatal)?;
        if tag.as_ref().len() != tag_len {
            return Err(ng::CryptoError::Fatal);
        }
        buf[plaintext_len..end].copy_from_slice(tag.as_ref());
        Ok(())
    }

    fn open(
        &self,
        dest: &mut [u8],
        ciphertext: &[u8],
        nonce: &[u8],
        aad: &[u8],
    ) -> Result<usize, ng::CryptoError> {
        let packet_number = self.packet_number(nonce)?;
        let tag_len = self.inner.tag_len();
        if ciphertext.len() < tag_len || dest.len() < ciphertext.len() - tag_len {
            return Err(ng::CryptoError::Fatal);
        }
        let mut plaintext = ciphertext.to_vec();
        let plaintext_len = self
            .inner
            .decrypt_in_place(packet_number, aad, &mut plaintext)
            .map(|plaintext| plaintext.len())
            .map_err(|_| ng::CryptoError::Decrypt)?;
        dest[..plaintext_len].copy_from_slice(&plaintext[..plaintext_len]);
        Ok(plaintext_len)
    }

    fn tag_len(&self) -> usize {
        self.inner.tag_len()
    }

    fn confidentiality_limit(&self) -> u64 {
        self.inner.confidentiality_limit()
    }

    fn integrity_limit(&self) -> u64 {
        self.inner.integrity_limit()
    }
}

pub(super) struct RbxTransportRustlsHeaderKey(Box<dyn rustls::quic::HeaderProtectionKey>);

impl ng::HeaderKey for RbxTransportRustlsHeaderKey {
    fn mask(&self, sample: &[u8]) -> Result<[u8; ng::HP_MASK_LEN], ng::CryptoError> {
        if sample.len() != self.0.sample_len() || ng::HP_MASK_LEN != 5 {
            return Err(ng::CryptoError::Fatal);
        }
        // A zero first byte selects the short-header mask width (five low bits). ngtcp2 then
        // applies the appropriate four- or five-bit mask to the real first header byte.
        let mut first = 0u8;
        let mut packet_number = [0u8; 4];
        self.0
            .encrypt_in_place(sample, &mut first, &mut packet_number)
            .map_err(|_| ng::CryptoError::Fatal)?;
        Ok([
            first,
            packet_number[0],
            packet_number[1],
            packet_number[2],
            packet_number[3],
        ])
    }
}

fn take_iv_pair(queue: &CapturedIvQueue) -> ng::Result<(Vec<u8>, Vec<u8>)> {
    let mut queue = queue
        .lock()
        .map_err(|_| ng::Error::backend("Rustls QUIC IV capture queue was poisoned"))?;
    if queue.len() != 2 {
        queue.clear();
        return Err(ng::Error::backend(
            "Rustls did not produce exactly two directional QUIC IVs",
        ));
    }
    let local = queue
        .pop_front()
        .ok_or(ng::Error::backend("Rustls local QUIC IV was missing"))?;
    let remote = queue
        .pop_front()
        .ok_or(ng::Error::backend("Rustls remote QUIC IV was missing"))?;
    Ok((local, remote))
}

fn rustls_directional_keys(
    keys: RustlsDirectionalKeys,
    iv: Vec<u8>,
) -> ng::Result<ng::DirectionalKeys<RbxTransportRustlsPacketKey, RbxTransportRustlsHeaderKey>> {
    let ng_iv = ng::Iv::new(&iv)
        .map_err(|_| ng::Error::backend("Rustls produced an invalid QUIC initialization vector"))?;
    Ok(ng::DirectionalKeys {
        packet: RbxTransportRustlsPacketKey {
            inner: keys.packet,
            iv,
        },
        header: RbxTransportRustlsHeaderKey(keys.header),
        iv: ng_iv,
    })
}

struct RetryPacketKey(LessSafeKey);

impl ng::PacketKey for RetryPacketKey {
    fn seal(
        &self,
        buf: &mut [u8],
        plaintext_len: usize,
        nonce: &[u8],
        aad: &[u8],
    ) -> Result<(), ng::CryptoError> {
        if nonce.len() != QUIC_IV_LEN || plaintext_len > buf.len() {
            return Err(ng::CryptoError::Fatal);
        }
        let nonce = Nonce::try_assume_unique_for_key(nonce).map_err(|_| ng::CryptoError::Fatal)?;
        let tag = self
            .0
            .seal_in_place_separate_tag(nonce, Aad::from(aad), &mut buf[..plaintext_len])
            .map_err(|_| ng::CryptoError::Fatal)?;
        let end = plaintext_len
            .checked_add(tag.as_ref().len())
            .filter(|end| *end <= buf.len())
            .ok_or(ng::CryptoError::Fatal)?;
        buf[plaintext_len..end].copy_from_slice(tag.as_ref());
        Ok(())
    }

    fn open(
        &self,
        dest: &mut [u8],
        ciphertext: &[u8],
        nonce: &[u8],
        aad: &[u8],
    ) -> Result<usize, ng::CryptoError> {
        if nonce.len() != QUIC_IV_LEN || ciphertext.len() < self.tag_len() {
            return Err(ng::CryptoError::Fatal);
        }
        let mut plaintext = ciphertext.to_vec();
        let nonce = Nonce::try_assume_unique_for_key(nonce).map_err(|_| ng::CryptoError::Fatal)?;
        let plaintext_len = self
            .0
            .open_in_place(nonce, Aad::from(aad), &mut plaintext)
            .map(|plaintext| plaintext.len())
            .map_err(|_| ng::CryptoError::Decrypt)?;
        if dest.len() < plaintext_len {
            return Err(ng::CryptoError::Fatal);
        }
        dest[..plaintext_len].copy_from_slice(&plaintext[..plaintext_len]);
        Ok(plaintext_len)
    }

    fn tag_len(&self) -> usize {
        16
    }

    fn confidentiality_limit(&self) -> u64 {
        u64::MAX
    }

    fn integrity_limit(&self) -> u64 {
        u64::MAX
    }
}

pub(super) struct RbxTransportRustlsSession {
    config: Arc<ClientConfig>,
    server_name: String,
    connection: Option<rustls::quic::ClientConnection>,
    captured_ivs: CapturedIvQueue,
    events: VecDeque<ng::SessionEvent>,
    failure: Option<String>,
    peer_params_set: bool,
    handshake_complete_reported: bool,
    next_one_rtt_secrets: Option<rustls::quic::Secrets>,
    one_rtt_secret_len: usize,
    write_level: ng::Level,
}

impl RbxTransportRustlsSession {
    fn set_failure(&mut self, reason: impl Into<String>) {
        self.failure = Some(reason.into());
    }

    fn connection_mut(&mut self) -> ng::Result<&mut rustls::quic::ClientConnection> {
        self.connection
            .as_mut()
            .ok_or(ng::Error::backend("Rustls QUIC session was not initialized"))
    }

    fn install_rustls_keys(
        &mut self,
        level: ng::Level,
        keys: rustls::quic::Keys,
        conn: &mut dyn ng::Handshaking<RbxTransportRustlsPacketKey, RbxTransportRustlsHeaderKey>,
        secret: &[u8],
    ) -> ng::Result<()> {
        let (local_iv, remote_iv) = take_iv_pair(&self.captured_ivs)?;
        let local = rustls_directional_keys(keys.local, local_iv)?;
        let remote = rustls_directional_keys(keys.remote, remote_iv)?;
        conn.install_keys(level, ng::Direction::Read, remote, secret)?;
        conn.install_keys(level, ng::Direction::Write, local, secret)?;
        Ok(())
    }

    fn feed_peer_transport_params(
        &mut self,
        conn: &mut dyn ng::Handshaking<RbxTransportRustlsPacketKey, RbxTransportRustlsHeaderKey>,
    ) -> ng::Result<()> {
        if self.peer_params_set {
            return Ok(());
        }
        let params = self
            .connection
            .as_ref()
            .and_then(|connection| connection.quic_transport_parameters())
            .map(<[u8]>::to_vec);
        if let Some(params) = params {
            conn.set_peer_transport_params(&params)?;
            self.peer_params_set = true;
        }
        Ok(())
    }

    fn negotiated_secret_len(&self) -> ng::Result<usize> {
        let suite = self
            .connection
            .as_ref()
            .and_then(|connection| connection.negotiated_cipher_suite())
            .ok_or(ng::Error::backend(
                "Rustls did not negotiate a TLS 1.3 cipher suite before installing 1-RTT keys",
            ))?;
        match suite.suite() {
            CipherSuite::TLS13_AES_256_GCM_SHA384 => Ok(48),
            CipherSuite::TLS13_AES_128_GCM_SHA256
            | CipherSuite::TLS13_CHACHA20_POLY1305_SHA256 => Ok(TLS13_SHA256_SECRET_LEN),
            _ => Err(ng::Error::backend(
                "the negotiated TLS cipher suite has an unsupported traffic-secret length",
            )),
        }
    }

    fn write_handshake(
        &mut self,
        conn: &mut dyn ng::Handshaking<RbxTransportRustlsPacketKey, RbxTransportRustlsHeaderKey>,
    ) -> ng::Result<()> {
        let (output, key_change) = {
            let _capture = IvCaptureGuard::install(Arc::clone(&self.captured_ivs));
            let connection = self.connection_mut()?;
            let mut output = Vec::new();
            let key_change = connection.write_hs(&mut output);
            (output, key_change)
        };

        // Rustls returns bytes from one TLS encryption level at a time. A KeyChange marks
        // the transition that follows those bytes, so submit the current buffer before
        // installing the next level's keys.
        if !output.is_empty() {
            conn.submit_handshake(self.write_level, &output)?;
        }
        match key_change {
            Some(RustlsKeyChange::Handshake { keys }) => {
                self.install_rustls_keys(ng::Level::Handshake, keys, conn, &[])?;
                self.write_level = ng::Level::Handshake;
            }
            Some(RustlsKeyChange::OneRtt { keys, next }) => {
                // ngtcp2 retains a traffic-secret-sized opaque buffer only to pass it back to
                // this backend on key update. Rustls intentionally keeps the actual traffic
                // secrets private, so this adapter supplies a hash-sized placeholder and uses
                // Rustls' opaque `Secrets` state to derive every subsequent packet key.
                self.one_rtt_secret_len = self.negotiated_secret_len()?;
                let placeholder = vec![0u8; self.one_rtt_secret_len];
                self.install_rustls_keys(ng::Level::OneRtt, keys, conn, &placeholder)?;
                self.next_one_rtt_secrets = Some(next);
                self.write_level = ng::Level::OneRtt;
            }
            None => {}
        }
        self.feed_peer_transport_params(conn)?;

        let handshake_complete = self
            .connection
            .as_ref()
            .is_some_and(|connection| !connection.is_handshaking());
        if handshake_complete && !self.handshake_complete_reported {
            self.events.push_back(ng::SessionEvent::HandshakeComplete);
            self.handshake_complete_reported = true;
        }
        Ok(())
    }
}

impl ng::Session for RbxTransportRustlsSession {
    type PacketKey = RbxTransportRustlsPacketKey;
    type HeaderKey = RbxTransportRustlsHeaderKey;

    fn initial_keys(
        &mut self,
        version: u32,
        dcid: &[u8],
    ) -> ng::Result<ng::InitialKeys<Self::PacketKey, Self::HeaderKey>> {
        let quic_version = match version {
            ng::VERSION_V1 => rustls::quic::Version::V1,
            _ => return Err(ng::Error::backend("ngtcp2 requested an unsupported QUIC version")),
        };
        // QUIC v1 Initial protection is fixed to AES-128-GCM/SHA-256; it is
        // independent of the cipher suite negotiated by TLS.
        let suite = default_provider()
            .cipher_suites
            .iter()
            .find_map(|supported| {
                let suite = supported.tls13()?;
                (suite.common.suite == CipherSuite::TLS13_AES_128_GCM_SHA256)
                    .then(|| rbx_transport_tls13_suite(suite))
            })
            .ok_or(ng::Error::backend(
                "Rustls AES-128-GCM/SHA-256 QUIC Initial suite is unavailable",
            ))?;
        let algorithm = suite
            .quic
            .ok_or(ng::Error::backend("Rustls QUIC packet protection is unavailable"))?;
        let keys = {
            let _capture = IvCaptureGuard::install(Arc::clone(&self.captured_ivs));
            rustls::quic::Keys::initial(
                quic_version,
                suite,
                algorithm,
                dcid,
                rustls::Side::Client,
            )
        };
        let (local_iv, remote_iv) = take_iv_pair(&self.captured_ivs)?;
        Ok(ng::InitialKeys {
            rx: rustls_directional_keys(keys.remote, remote_iv)?,
            tx: rustls_directional_keys(keys.local, local_iv)?,
        })
    }

    fn retry_key(&mut self, version: u32) -> ng::Result<Self::PacketKey> {
        let key: &[u8; 16] = match version {
            ng::VERSION_V1 => &[
                0xbe, 0x0c, 0x69, 0x0b, 0x9f, 0x66, 0x57, 0x5a,
                0x1d, 0x76, 0x6b, 0x54, 0xe3, 0x68, 0xc8, 0x4e,
            ],
            _ => return Err(ng::Error::backend("no Retry integrity key for this QUIC version")),
        };
        let key = UnboundKey::new(&aead::AES_128_GCM, key)
            .map_err(|_| ng::Error::backend("failed to initialize the QUIC Retry key"))?;
        Ok(RetryPacketKey(LessSafeKey::new(key)))
    }

    fn set_local_transport_params(&mut self, params: &[u8]) -> ng::Result<()> {
        if self.connection.is_some() {
            return Err(ng::Error::backend("Rustls QUIC transport parameters were set twice"));
        }
        let name = rustls::pki_types::ServerName::try_from(self.server_name.clone())
            .map_err(|_| ng::Error::backend("invalid RbxTransport TLS server name"))?;
        let connection = rustls::quic::ClientConnection::new_with_alpn(
            Arc::clone(&self.config),
            rustls::quic::Version::V1,
            name,
            params.to_vec(),
            vec![RBX_TRANSPORT_ALPN.to_vec()],
        )
        .map_err(|_| ng::Error::backend("failed to create Rustls QUIC client connection"))?;
        self.connection = Some(connection);
        Ok(())
    }

    fn start_handshake(
        &mut self,
        conn: &mut dyn ng::Handshaking<Self::PacketKey, Self::HeaderKey>,
    ) -> ng::Result<()> {
        self.write_handshake(conn)
    }

    fn read_handshake(
        &mut self,
        _level: ng::Level,
        data: &[u8],
        conn: &mut dyn ng::Handshaking<Self::PacketKey, Self::HeaderKey>,
    ) -> ng::Result<()> {
        let result = self.connection_mut()?.read_hs(data);
        if let Err(error) = result {
            self.set_failure(format!("Rustls rejected QUIC handshake data: {error}"));
            return Err(ng::Error::backend("Rustls rejected QUIC handshake data"));
        }
        self.feed_peer_transport_params(conn)?;
        self.write_handshake(conn)
    }

    fn poll_event(&mut self) -> Option<ng::SessionEvent> {
        self.events.pop_front()
    }

    fn rotate_keys(
        &mut self,
        _rx_secret: &[u8],
        _tx_secret: &[u8],
    ) -> ng::Result<ng::RotatedKeys<Self::PacketKey>> {
        let mut secrets = self
            .next_one_rtt_secrets
            .take()
            .ok_or(ng::Error::backend("Rustls has no application key-update state"))?;
        let keys = {
            let _capture = IvCaptureGuard::install(Arc::clone(&self.captured_ivs));
            secrets.next_packet_keys()
        };
        let (local_iv, remote_iv) = take_iv_pair(&self.captured_ivs)?;
        self.next_one_rtt_secrets = Some(secrets);
        let placeholder = vec![0u8; self.one_rtt_secret_len];
        Ok(ng::RotatedKeys {
            rx_packet: RbxTransportRustlsPacketKey {
                inner: keys.remote,
                iv: remote_iv.clone(),
            },
            rx_iv: ng::Iv::new(&remote_iv)
                .map_err(|_| ng::Error::backend("Rustls produced an invalid receive IV"))?,
            rx_secret: placeholder.clone(),
            tx_packet: RbxTransportRustlsPacketKey {
                inner: keys.local,
                iv: local_iv.clone(),
            },
            tx_iv: ng::Iv::new(&local_iv)
                .map_err(|_| ng::Error::backend("Rustls produced an invalid transmit IV"))?,
            tx_secret: placeholder,
        })
    }

    fn negotiated_alpn(&self) -> Option<Vec<u8>> {
        self.connection
            .as_ref()
            .and_then(|connection| connection.alpn_protocol())
            .map(<[u8]>::to_vec)
    }

    fn failure_reason(&self) -> Option<String> {
        self.failure.clone()
    }
}
