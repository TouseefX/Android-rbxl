use std::env;
use std::path::{Path, PathBuf};

/// Relative location of the vendored ngtcp2 checkout from this crate's root.
const VENDOR_RELATIVE: &str = "vendor/ngtcp2";

/// The first OpenSSL release with the QUIC TLS API ngtcp2's `ossl` backend is
/// written against. See [`find_openssl`] for why an older one is a hard error
/// rather than a fallback.
const MIN_OPENSSL: &str = "3.5.0";

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());

    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=wrapper.h");
    println!("cargo::rerun-if-env-changed=NGTCP2_SOURCE_DIR");
    println!("cargo::rerun-if-env-changed=OPENSSL_DIR");

    let crypto_ossl = cfg!(feature = "crypto-ossl");

    let source_dir = ngtcp2_source_dir(&manifest_dir);
    for dir in ["lib", "crypto"] {
        println!("cargo::rerun-if-changed={}", source_dir.join(dir).display());
    }
    println!(
        "cargo::rerun-if-changed={}",
        source_dir.join("CMakeLists.txt").display()
    );

    let openssl = detect_openssl();

    let install_dir = build_ngtcp2(&source_dir, openssl.as_ref());
    let lib_dir = find_lib_dir(&install_dir, crypto_ossl);
    let include_dir = install_dir.join("include");

    println!("cargo::rustc-link-search=native={}", lib_dir.display());

    // Order matters, and only in one direction: a static archive is scanned once
    // for symbols the archives before it left undefined. `libngtcp2_crypto_ossl`
    // calls into both `libngtcp2` and OpenSSL, so it has to come first, and
    // OpenSSL last.
    if crypto_ossl {
        println!("cargo::rustc-link-lib=static=ngtcp2_crypto_ossl");
    }
    println!("cargo::rustc-link-lib=static=ngtcp2");
    if let Some(openssl) = &openssl {
        for dir in &openssl.link_paths {
            println!("cargo::rustc-link-search=native={}", dir.display());
        }
        for lib in &openssl.libs {
            // Dynamic: these are the system's OpenSSL, not something this crate
            // built, so vendoring them into the rlib would be wrong.
            println!("cargo::rustc-link-lib=dylib={lib}");
        }
    }

    // Consumers of this crate (via the `links = "ngtcp2"` key) get these as
    // DEP_NGTCP2_ROOT / DEP_NGTCP2_INCLUDE / DEP_NGTCP2_LIB.
    println!("cargo::metadata=root={}", install_dir.display());
    println!("cargo::metadata=include={}", include_dir.display());
    println!("cargo::metadata=lib={}", lib_dir.display());

    generate_bindings(&manifest_dir, &include_dir, &source_dir, openssl.as_ref());
}

/// Resolve the ngtcp2 sources, preferring an explicit `NGTCP2_SOURCE_DIR`
/// override and falling back to the git submodule vendored in this repo.
fn ngtcp2_source_dir(manifest_dir: &Path) -> PathBuf {
    if let Some(dir) = env::var_os("NGTCP2_SOURCE_DIR") {
        let dir = PathBuf::from(dir);
        assert!(
            dir.join("CMakeLists.txt").is_file(),
            "NGTCP2_SOURCE_DIR={} does not look like an ngtcp2 checkout",
            dir.display()
        );
        return dir;
    }

    let dir = manifest_dir.join(VENDOR_RELATIVE);
    if !dir.join("CMakeLists.txt").is_file() {
        panic!(
            "ngtcp2 sources not found at {}.\n\
             The submodule has not been checked out. Run:\n\n    \
             just submodules\n",
            dir.display()
        );
    }
    dir.canonicalize().unwrap_or(dir)
}

/// Where OpenSSL lives, in the form both CMake and bindgen need.
struct OpenSsl {
    include_paths: Vec<PathBuf>,
    link_paths: Vec<PathBuf>,
    libs: Vec<String>,
    /// Passed to CMake as `OPENSSL_ROOT_DIR` when known, so that CMake's own
    /// `FindOpenSSL` cannot pick a *different* OpenSSL than the one whose
    /// headers bindgen reads and whose libraries end up on the link line.
    root: Option<PathBuf>,
}

/// Locate an OpenSSL new enough for ngtcp2's `ossl` crypto backend.
///
/// Two functions rather than a runtime branch because `pkg-config` is an
/// optional dependency: without the feature it is not linked into the build
/// script at all, so a body naming it would fail to compile rather than merely
/// go unused.
#[cfg(feature = "crypto-ossl")]
fn detect_openssl() -> Option<OpenSsl> {
    Some(find_openssl())
}

#[cfg(not(feature = "crypto-ossl"))]
fn detect_openssl() -> Option<OpenSsl> {
    None
}

/// Locate an OpenSSL new enough for ngtcp2's `ossl` crypto backend.
///
/// `ENABLE_OPENSSL` does not select a backend by version; it probes for symbols
/// and picks one of two different libraries:
///
/// ```text
/// SSL_provide_quic_data present -> libngtcp2_crypto_quictls  (quictls, LibreSSL)
/// SSL_set_quic_tls_cbs  present -> libngtcp2_crypto_ossl     (OpenSSL >= 3.5)
/// ```
///
/// That makes an insufficient OpenSSL fail in a confusing place: with quictls or
/// LibreSSL, CMake succeeds and builds the *quictls* archive, and the build only
/// falls over later when the `ossl` archive this crate asks to link turns out
/// not to exist. Checking the version up front turns that into one clear
/// message, which is the whole reason this function does its own probing rather
/// than letting CMake find OpenSSL unaided.
#[cfg(feature = "crypto-ossl")]
fn find_openssl() -> OpenSsl {
    // The `openssl-sys` convention, and the escape hatch for platforms where
    // pkg-config is not the way OpenSSL is found.
    if let Some(dir) = env::var_os("OPENSSL_DIR") {
        let root = PathBuf::from(dir);
        let include = root.join("include");
        assert!(
            include.join("openssl/ssl.h").is_file(),
            "OPENSSL_DIR={} does not contain include/openssl/ssl.h",
            root.display()
        );
        return OpenSsl {
            include_paths: vec![include],
            link_paths: vec![root.join("lib")],
            libs: vec!["ssl".to_owned(), "crypto".to_owned()],
            root: Some(root),
        };
    }

    let lib = pkg_config::Config::new()
        .atleast_version(MIN_OPENSSL)
        // The link flags are emitted by hand further up, in an order that works
        // for static archives; letting pkg-config emit its own would put
        // OpenSSL before the ngtcp2 archives that need it.
        .cargo_metadata(false)
        .probe("openssl")
        .unwrap_or_else(|err| {
            panic!(
                "could not find OpenSSL >= {MIN_OPENSSL}, which ngtcp2's `ossl` crypto \
                 backend requires: {err}\n\n\
                 The QUIC TLS API this backend is built on (SSL_set_quic_tls_cbs) first \
                 shipped in OpenSSL 3.5. Older OpenSSL, quictls and LibreSSL instead \
                 provide SSL_provide_quic_data, for which ngtcp2 builds a different \
                 archive (libngtcp2_crypto_quictls) that this crate does not link.\n\n\
                 Either install OpenSSL >= {MIN_OPENSSL} and its development headers, point \
                 OPENSSL_DIR at one, or build with --no-default-features to skip the \
                 crypto backend entirely."
            )
        });

    OpenSsl {
        include_paths: lib.include_paths.clone(),
        link_paths: lib.link_paths.clone(),
        libs: lib.libs.clone(),
        root: pkg_config::get_variable("openssl", "prefix")
            .ok()
            .filter(|p| !p.is_empty())
            .map(PathBuf::from),
    }
}

/// Configure and build libngtcp2 as a static library, installing into OUT_DIR.
fn build_ngtcp2(source_dir: &Path, openssl: Option<&OpenSsl>) -> PathBuf {
    let mut config = cmake::Config::new(source_dir);

    config
        // Build the libraries only: no example client/server, and so no probe
        // for libev, nghttp3 or a C++ toolchain. This is also what keeps
        // `third-party/urlparse` out of the build — its target is guarded by
        // `if(LIBEV_FOUND AND LIBNGHTTP3_FOUND)` — so the nested submodules
        // never need checking out.
        .define("ENABLE_LIB_ONLY", "ON")
        // We want a static archive to link into rlibs, not a shared object.
        .define("ENABLE_STATIC_LIB", "ON")
        .define("ENABLE_SHARED_LIB", "OFF")
        // BUILD_TESTING is a dependent option that defaults to ON whenever
        // ENABLE_STATIC_LIB is on, and the test suite needs the tests/munit
        // submodule this repo deliberately does not check out.
        .define("BUILD_TESTING", "OFF")
        .define("ENABLE_WERROR", "OFF")
        // Required so the archive can be linked into cdylibs and proc macros.
        .define("CMAKE_POSITION_INDEPENDENT_CODE", "ON");

    match openssl {
        Some(openssl) => {
            config.define("ENABLE_OPENSSL", "ON");
            if let Some(root) = &openssl.root {
                config.define("OPENSSL_ROOT_DIR", root);
            }
        }
        // Not merely "leave it default": ngtcp2 defaults ENABLE_OPENSSL to ON,
        // so a --no-default-features build that stayed silent here would still
        // link OpenSSL in through the crypto helper.
        None => {
            config.define("ENABLE_OPENSSL", "OFF");
        }
    }

    if env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        let static_crt = env::var("CARGO_ENCODED_RUSTFLAGS")
            .unwrap_or_default()
            .contains("crt-static");
        config.define("ENABLE_STATIC_CRT", if static_crt { "ON" } else { "OFF" });
    }

    config.build()
}

/// ngtcp2 installs via GNUInstallDirs, so the archives land in `lib` or `lib64`
/// depending on the platform.
///
/// When the crypto backend was requested, its archive has to be found in the
/// same directory. It is built by a subdirectory CMake only descends into once
/// symbol probing has set `HAVE_OSSL`, and that probing cannot fail the
/// configure step on its own — so a missing archive here is the signal that
/// OpenSSL was not what we thought it was.
fn find_lib_dir(install_dir: &Path, crypto_ossl: bool) -> PathBuf {
    let mut found_core = None;

    for candidate in ["lib", "lib64"] {
        let dir = install_dir.join(candidate);
        if dir.join("libngtcp2.a").is_file() || dir.join("ngtcp2.lib").is_file() {
            if !crypto_ossl {
                return dir;
            }
            if dir.join("libngtcp2_crypto_ossl.a").is_file()
                || dir.join("ngtcp2_crypto_ossl.lib").is_file()
            {
                return dir;
            }
            found_core = Some(dir);
        }
    }

    if let Some(dir) = found_core {
        panic!(
            "libngtcp2 was built, but its OpenSSL crypto backend was not: no \
             libngtcp2_crypto_ossl archive in {}.\n\n\
             ngtcp2 only builds that archive when it finds SSL_set_quic_tls_cbs, so the \
             OpenSSL used for the build is older than {MIN_OPENSSL}, or is quictls or \
             LibreSSL. Point OPENSSL_DIR at a newer OpenSSL, or build with \
             --no-default-features.",
            dir.display()
        );
    }

    panic!(
        "could not locate the built ngtcp2 static library under {}",
        install_dir.display()
    );
}

/// Gives the constants restated in `wrapper.h` the width their C types have.
///
/// bindgen picks an integer macro's Rust type from its *value*, not from its
/// suffix or the type the header uses it as: anything that fits in 32 bits
/// becomes a `u32`. Left alone, `NGTCP2_SECONDS` (1e9) arrives as `u32` while
/// `NGTCP2_MINUTES` (6e10) arrives as `u64`, which is both inconsistent and
/// wrong — every one of these is an `ngtcp2_duration`, a `uint64_t`, and the
/// library takes them in arithmetic like `3 * NGTCP2_SECONDS`. In Rust that
/// multiplication would be `u32` and would overflow at a little over four
/// seconds.
#[derive(Debug)]
struct DurationTypes;

impl bindgen::callbacks::ParseCallbacks for DurationTypes {
    fn int_macro(&self, name: &str, _value: i64) -> Option<bindgen::callbacks::IntKind> {
        match name {
            // ngtcp2_duration, i.e. uint64_t.
            "NGTCP2_NANOSECONDS"
            | "NGTCP2_MICROSECONDS"
            | "NGTCP2_MILLISECONDS"
            | "NGTCP2_SECONDS"
            | "NGTCP2_MINUTES"
            | "NGTCP2_DEFAULT_INITIAL_RTT"
            | "NGTCP2_DEFAULT_MAX_ACK_DELAY" => Some(bindgen::callbacks::IntKind::U64),
            // The AEAD usage limits from the crypto helper's `shared.h`. Same
            // story as the durations above, and the same cause: these are
            // written `1ULL << 23` and `2965820ULL`, but bindgen sizes them by
            // value, so AES-GCM's confidentiality limit arrives as a `u32`
            // while ChaCha20-Poly1305's (1 << 62) arrives as a `u64`.
            //
            // Every one of them is assigned into `ngtcp2_crypto_ctx`'s
            // `max_encryption` / `max_decryption_failure`, both `uint64_t`, and
            // is compared against a packet count that is itself 64-bit. Leaving
            // them mixed would put a silent `u32` in the middle of that
            // comparison.
            "NGTCP2_CRYPTO_MAX_ENCRYPTION_AES_GCM"
            | "NGTCP2_CRYPTO_MAX_ENCRYPTION_CHACHA20_POLY1305"
            | "NGTCP2_CRYPTO_MAX_ENCRYPTION_AES_CCM"
            | "NGTCP2_CRYPTO_MAX_DECRYPTION_FAILURE_AES_GCM"
            | "NGTCP2_CRYPTO_MAX_DECRYPTION_FAILURE_CHACHA20_POLY1305"
            | "NGTCP2_CRYPTO_MAX_DECRYPTION_FAILURE_AES_CCM" => {
                Some(bindgen::callbacks::IntKind::U64)
            }
            // uint32_t. These already come out as u32 by value, but saying so
            // keeps them from following the value across a type boundary if
            // upstream ever changes one.
            "NGTCP2_PROTO_VER_V1"
            | "NGTCP2_PROTO_VER_V2"
            | "NGTCP2_PROTO_VER_MAX"
            | "NGTCP2_PROTO_VER_MIN" => Some(bindgen::callbacks::IntKind::U32),
            _ => None,
        }
    }
}

fn generate_bindings(
    manifest_dir: &Path,
    include_dir: &Path,
    source_dir: &Path,
    openssl: Option<&OpenSsl>,
) {
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());

    let mut builder = bindgen::Builder::default()
        .header(manifest_dir.join("wrapper.h").to_str().unwrap())
        // The *install* include dir, not the source tree: `ngtcp2/version.h`
        // is generated by CMake and exists only here.
        .clang_arg(format!("-I{}", include_dir.display()))
        // Matches the PUBLIC compile definition on the ngtcp2_static target,
        // so NGTCP2_EXTERN does not expand to a dllimport declspec.
        .clang_arg("-DNGTCP2_STATICLIB")
        .allowlist_function("ngtcp2_.*")
        .allowlist_type("ngtcp2_.*")
        .allowlist_var("NGTCP2_.*")
        // Plain constants rather than Rust enums: ngtcp2 returns error codes and
        // protocol values as ints, and values outside the enumerated set would
        // be undefined behaviour in a real Rust enum.
        .default_enum_style(bindgen::EnumVariation::Consts)
        .derive_default(true)
        .derive_debug(true)
        .prepend_enum_name(false)
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .parse_callbacks(Box::new(DurationTypes));

    if let Some(openssl) = openssl {
        builder = builder.clang_arg("-DNGNET_QUIC_SYS_CRYPTO_OSSL");
        // The crypto helper's *internal* header, `shared.h`, is not installed —
        // only `ngtcp2_crypto.h` and `ngtcp2_crypto_ossl.h` are. It declares the
        // QUIC key schedule (`derive_initial_secrets`,
        // `derive_packet_protection_key`, `hkdf_expand_label`) and the header
        // protection cipher constructor, all of which are compiled into the
        // archive with default visibility and are exactly what a TLS backend
        // needs in order not to reimplement them.
        //
        // Pointing bindgen at the vendored source rather than restating those
        // declarations by hand is what keeps the signatures honest: a change
        // upstream becomes a compile error here, not a silent argument
        // mismatch. The submodule is pinned, so nothing moves without a
        // deliberate bump.
        builder = builder.clang_arg(format!("-I{}", source_dir.join("crypto").display()));
        for dir in &openssl.include_paths {
            builder = builder.clang_arg(format!("-I{}", dir.display()));
        }
        // `wrapper.h` already includes `<ngtcp2/ngtcp2_crypto_ossl.h>`, which
        // includes `<openssl/ssl.h>`, so OpenSSL's declarations are parsed
        // whether or not anything is emitted from them. What follows is
        // therefore purely a decision about what to *emit*.
        //
        // `ngnet-quic` needs more than the `SSL *` that ngtcp2's own signatures
        // mention: it configures the TLS objects the crypto helper then drives.
        // The set below is what a client and a server need to complete a
        // verified handshake with ALPN, loading credentials from memory rather
        // than from paths.
        //
        // A missing entry here is a link error at build time, not a silent
        // runtime fault, so this errs towards generous.
        builder = builder
            .allowlist_type("SSL")
            .allowlist_type("SSL_CTX")
            .allowlist_type("SSL_METHOD")
            .allowlist_type("SSL_CIPHER")
            .allowlist_type("X509")
            .allowlist_type("X509_STORE")
            .allowlist_type("EVP_PKEY")
            .allowlist_type("BIO")
            .allowlist_type("BIO_METHOD")
            .allowlist_type("pem_password_cb")
            // Context and session lifecycle.
            .allowlist_function("SSL_CTX_new")
            .allowlist_function("SSL_CTX_free")
            .allowlist_function("SSL_new")
            .allowlist_function("SSL_free")
            .allowlist_function("TLS_client_method")
            .allowlist_function("TLS_server_method")
            .allowlist_function("SSL_set_connect_state")
            .allowlist_function("SSL_set_accept_state")
            // `SSL_set_app_data` / `SSL_get_app_data` are macros over these, and
            // bindgen does not emit function-like macros. Attaching the
            // `ngtcp2_crypto_conn_ref` is not optional -- every one of the ossl
            // dispatch callbacks begins by reading it back.
            .allowlist_function("SSL_set_ex_data")
            .allowlist_function("SSL_get_ex_data")
            // ALPN. Mandatory in QUIC rather than optional as it is over TCP.
            .allowlist_function("SSL_set_alpn_protos")
            .allowlist_function("SSL_CTX_set_alpn_select_cb")
            .allowlist_function("SSL_get0_alpn_selected")
            .allowlist_function("SSL_select_next_proto")
            // Credentials, read from memory so the API need not take paths.
            .allowlist_function("SSL_CTX_use_certificate")
            .allowlist_function("SSL_CTX_use_certificate_chain_file")
            .allowlist_function("SSL_CTX_use_PrivateKey")
            .allowlist_function("SSL_CTX_use_PrivateKey_file")
            .allowlist_function("SSL_CTX_check_private_key")
            .allowlist_function("BIO_new_mem_buf")
            .allowlist_function("BIO_free")
            .allowlist_function("PEM_read_bio_X509")
            .allowlist_function("PEM_read_bio_PrivateKey")
            .allowlist_function("X509_free")
            .allowlist_function("X509_up_ref")
            .allowlist_function("EVP_PKEY_free")
            // Verification. On by default in this crate, unlike the ngtcp2
            // examples, which verify nothing at all.
            .allowlist_function("SSL_CTX_set_verify")
            .allowlist_function("SSL_CTX_set_default_verify_paths")
            .allowlist_function("SSL_CTX_get_cert_store")
            .allowlist_function("X509_STORE_add_cert")
            .allowlist_function("SSL_set1_host")
            .allowlist_function("SSL_get_verify_result")
            // `SSL_set_tlsext_host_name` (SNI) is a macro over `SSL_ctrl`.
            .allowlist_function("SSL_ctrl")
            .allowlist_function("SSL_CTX_ctrl")
            // Diagnostics, so a handshake failure can say what went wrong
            // rather than surfacing as a bare -1.
            .allowlist_function("SSL_get_error")
            .allowlist_function("SSL_get_current_cipher")
            .allowlist_function("SSL_CIPHER_get_name")
            .allowlist_function("ERR_get_error")
            .allowlist_function("ERR_error_string_n")
            .allowlist_function("ERR_clear_error")
            .allowlist_function("X509_verify_cert_error_string")
            .allowlist_var("SSL_ERROR_.*")
            .allowlist_var("SSL_VERIFY_.*")
            .allowlist_var("SSL_TLSEXT_ERR_.*")
            .allowlist_var("SSL_CTRL_SET_TLSEXT_HOSTNAME")
            .allowlist_var("TLSEXT_NAMETYPE_host_name")
            .allowlist_var("SSL_FILETYPE_PEM")
            .allowlist_var("X509_V_OK")
            .allowlist_var("X509_V_ERR_.*")
            .allowlist_var("SSL_CTRL_CHAIN_CERT")
            // ---------------------------------------------------------------
            // OpenSSL's QUIC-TLS record layer, added in 3.5.
            //
            // This is the *only* part of `libngtcp2_crypto_ossl` that has to be
            // replaced. `SSL_set_quic_tls_cbs` swaps OpenSSL's record layer for
            // a dispatch table of our own, which is what the C helper does
            // (`vendor/ngtcp2/crypto/ossl/ossl.c:1252-1289`) -- it is an adapter
            // over this API, not a privileged path into OpenSSL.
            //
            // The helper's version of this is unusable here for one specific
            // reason: its callbacks recover the `ngtcp2_conn` by reading it
            // back out of the `SSL`'s application data, which is what forces
            // the TLS seam to hand a backend a connection pointer, and so what
            // forces the seam to be `unsafe`. Driving the handshake ourselves
            // is what removes that.
            //
            // Everything *else* the helper does -- packet protection, key
            // derivation, cipher-suite discovery -- stays with the helper, so
            // none of OpenSSL's EVP surface needs binding. See `shared.h`
            // above.
            //
            // Calling `SSL_set_quic_tls_cbs` also forces TLS 1.3 as the minimum
            // and disables middlebox compatibility, so no separate version
            // pinning is needed.
            .allowlist_function("SSL_set_quic_tls_cbs")
            // Local transport parameters travel out of band rather than in the
            // CRYPTO stream, and OpenSSL retains the buffer until it has sent
            // them -- one of the two retention rules the backend must honour.
            .allowlist_function("SSL_set_quic_tls_transport_params")
            .allowlist_type("OSSL_DISPATCH")
            // The six dispatch slots, named individually rather than by a
            // wildcard so a slot appearing in a future OpenSSL cannot be picked
            // up silently and left unimplemented.
            .allowlist_var("OSSL_FUNC_SSL_QUIC_TLS_CRYPTO_SEND")
            .allowlist_var("OSSL_FUNC_SSL_QUIC_TLS_CRYPTO_RECV_RCD")
            .allowlist_var("OSSL_FUNC_SSL_QUIC_TLS_CRYPTO_RELEASE_RCD")
            .allowlist_var("OSSL_FUNC_SSL_QUIC_TLS_YIELD_SECRET")
            .allowlist_var("OSSL_FUNC_SSL_QUIC_TLS_GOT_TRANSPORT_PARAMS")
            .allowlist_var("OSSL_FUNC_SSL_QUIC_TLS_ALERT")
            // OpenSSL reports secrets against its own protection levels, which
            // have to be mapped onto ngtcp2's encryption levels.
            .allowlist_var("OSSL_RECORD_PROTECTION_LEVEL_.*")
            // Driving the handshake. `SSL_read` looks out of place in a QUIC
            // stack: it is how post-handshake messages -- session tickets and
            // key updates -- get processed once the handshake is done. The C
            // helper calls it for exactly that reason (`ossl.c:993`), and
            // omitting it fails silently rather than loudly.
            .allowlist_function("SSL_do_handshake")
            .allowlist_function("SSL_read");
    }

    let bindings = builder
        .generate()
        .expect("failed to generate ngtcp2 bindings");

    bindings
        .write_to_file(out_dir.join("bindings.rs"))
        .expect("failed to write bindings.rs");
}
