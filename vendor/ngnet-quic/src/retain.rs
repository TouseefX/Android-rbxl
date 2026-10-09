//! Holding sent stream data until the peer acknowledges it.
//!
//! ngtcp2 does not copy the stream data it accepts. `ngtcp2_conn_writev_stream` serialises
//! some of it into the packet being built and **keeps the caller's pointer** so it can
//! retransmit if the packet is lost:
//!
//! > The caller must keep the portion of data covered by `|*pdatalen|` bytes in tact until
//! > `ngtcp2_callbacks.acked_stream_data_offset` indicates that they are acknowledged by a
//! > remote endpoint or the stream is closed.
//! >
//! > — `crates/ngnet-quic-sys/vendor/ngtcp2/lib/includes/ngtcp2/ngtcp2.h:5244-5248`
//!
//! A safe API therefore cannot pass the caller's borrow through and return. The caller is
//! free to drop that buffer the instant the call ends, and any later retransmission would
//! read freed memory — a use-after-free reachable from entirely safe code.
//!
//! So the connection keeps its own copy of every byte ngtcp2 accepted, and hands ngtcp2 a
//! pointer into *that*. The copy is released when the acknowledgement arrives, or when the
//! stream ends.
//!
//! # Why chunks rather than one buffer per stream
//!
//! A `Vec` would reallocate as it grew, moving bytes ngtcp2 still holds pointers to. Each
//! accepted write therefore becomes its own `Box<[u8]>`, whose address is fixed for as long
//! as it lives, and chunks are dropped whole once fully acknowledged.
//!
//! # The cost, stated plainly
//!
//! One copy of every byte sent, held until acknowledged. `ngnet-h3` avoids the equivalent
//! copy by making its callers hand over ownership; this crate takes the copy instead,
//! because a `&[u8]` that the caller may reuse immediately is the more ordinary signature
//! and the safety of the API should not depend on reading a paragraph of documentation.
//! See `docs/quic/pending-work.md`.

use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::io::IoSlice;
use std::sync::Arc;

use crate::stream::StreamId;

/// A buffer whose ownership the caller hands over, retained without a copy.
///
/// The borrowing write path copies every accepted byte, because a `&[u8]` the caller may
/// reuse the instant the call returns cannot be handed to ngtcp2, which keeps the pointer
/// until acknowledgement (`ngtcp2.h:5244-5248`). A caller who *can* give up ownership avoids
/// that copy: the bytes live behind an [`Arc`] whose address is fixed for as long as any
/// handle to it survives, so ngtcp2's pointer stays valid without a copy of its own.
///
/// Shareable because one allocation may be offered in pieces -- ngtcp2 routinely accepts a
/// prefix and leaves the rest -- and [`split_to`](Self::split_to) hands back the unaccepted
/// suffix as a second handle into the same allocation, with no copy and no second address to
/// account for.
///
/// This is deliberately not [`bytes::Bytes`]. That would be the obvious choice, but the crate
/// declares a fixed set of dependencies that a test enforces, so the handle is defined here.
/// [`OwnedBytes::from_owner`] is how a caller who *does* have a `Bytes` -- or any other
/// reference-counted buffer -- hands it over without a copy.
///
/// [`bytes::Bytes`]: https://docs.rs/bytes
#[derive(Clone)]
pub struct OwnedBytes {
    store: Store,
    start: usize,
    end: usize,
}

/// Where the bytes actually live.
///
/// Two cases rather than one because the crate has no `bytes` dependency to name in its own
/// types, but a caller who has one should not have to copy to use this API. The erased case
/// costs a second pointer indirection on every read; the owned case stays flat.
#[derive(Clone)]
enum Store {
    Owned(Arc<[u8]>),
    Erased(Arc<dyn Owner>),
}

/// A buffer whose bytes stay put for as long as it is alive.
///
/// Sealed by being private: the blanket implementation below covers every type that can
/// satisfy it, so there is nothing for a caller to implement.
trait Owner: Send + Sync {
    fn bytes(&self) -> &[u8];
}

impl<T: AsRef<[u8]> + Send + Sync + 'static> Owner for T {
    fn bytes(&self) -> &[u8] {
        self.as_ref()
    }
}

impl Store {
    fn bytes(&self) -> &[u8] {
        match self {
            Self::Owned(buffer) => buffer,
            Self::Erased(owner) => owner.bytes(),
        }
    }
}

impl OwnedBytes {
    /// Takes ownership of a buffer.
    pub fn new(bytes: impl Into<Arc<[u8]>>) -> Self {
        let buffer: Arc<[u8]> = bytes.into();
        let end = buffer.len();
        Self {
            store: Store::Owned(buffer),
            start: 0,
            end,
        }
    }

    /// Retains a buffer this crate cannot name, without copying it.
    ///
    /// The motivating case is [`bytes::Bytes`]: it is reference-counted already, so copying
    /// it into an `Arc<[u8]>` to satisfy [`new`](Self::new) would defeat the point. Anything
    /// that can lend a stable slice works -- `Vec<u8>`, `Arc<Vec<u8>>`, a memory map, a
    /// caller's own buffer type.
    ///
    /// # Safety
    ///
    /// This is `unsafe` because the handle it returns is handed to ngtcp2 as a raw pointer
    /// and a length, and ngtcp2 keeps that pair valid across many later calls
    /// (`ngtcp2.h:5244-5248`). The bytes ultimately reach a C read that trusts the length,
    /// so the caller must guarantee, for the entire life of the returned value and every
    /// handle [`split_to`](Self::split_to) derives from it:
    ///
    /// - [`AsRef::as_ref`] returns a slice with the **same address and the same length**
    ///   every time it is called. This crate takes the pointer and the length together from
    ///   a single borrow, so the two cannot disagree at the moment of staging; the
    ///   requirement is that they still describe the same bytes at every *later* point that
    ///   consults the owner -- construction, staging, [`split_to`](Self::split_to), commit
    ///   and release accounting each borrow it afresh. An owner that shortened itself
    ///   between staging and a retransmission would leave ngtcp2 holding a pointer valid for
    ///   fewer bytes than the length it was told, and C would read out of bounds. Clamping
    ///   the length this crate records does **not** rescue such an owner, because the pair
    ///   ngtcp2 keeps was taken before the change.
    /// - The bytes those slices refer to are neither moved nor freed until this value and
    ///   every handle derived from it are dropped. Reallocating the backing store -- again
    ///   reachable through interior mutability under the `Sync` bound -- would dangle the
    ///   pointer ngtcp2 still holds for retransmission.
    ///
    /// A well-behaved owner -- `Vec<u8>`, `Arc<[u8]>`, [`bytes::Bytes`], a read-only memory
    /// map -- satisfies both trivially, which is why [`new`](Self::new) can wrap an
    /// `Arc<[u8]>` from entirely safe code: an `Arc<[u8]>` cannot change its address or
    /// length behind a shared reference. `Send + Sync` is required because the buffer is
    /// stored behind an [`Arc`], which is `Send` only when its contents are both.
    ///
    /// [`bytes::Bytes`]: https://docs.rs/bytes
    pub unsafe fn from_owner(owner: impl AsRef<[u8]> + Send + Sync + 'static) -> Self {
        let owner: Arc<dyn Owner> = Arc::new(owner);
        let end = owner.bytes().len();
        Self {
            store: Store::Erased(owner),
            start: 0,
            end,
        }
    }

    /// The bytes this handle refers to.
    pub fn as_slice(&self) -> &[u8] {
        // Clamped rather than indexed. The bounds cannot be wrong for an owner that behaves,
        // and for one that does not this is fewer bytes rather than a panic that would unwind
        // into a C frame and abort.
        let all = self.store.bytes();
        let end = self.end.min(all.len());
        let start = self.start.min(end);
        &all[start..end]
    }

    /// How many bytes this handle refers to.
    pub fn len(&self) -> usize {
        self.as_slice().len()
    }

    /// Whether this handle refers to no bytes.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Splits off the first `n` bytes, leaving the remainder here.
    ///
    /// Both halves share one allocation, which is what lets a partially accepted write keep
    /// its accepted prefix retained while the suffix is offered again -- neither is copied,
    /// and the address ngtcp2 was handed does not move.
    pub fn split_to(&mut self, n: usize) -> Self {
        let n = n.min(self.len());
        let head = Self {
            store: self.store.clone(),
            start: self.start,
            end: self.start + n,
        };
        self.start += n;
        head
    }
}

impl core::fmt::Debug for OwnedBytes {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // The bytes, not the sharing behind them: a handle prints as the run it refers to.
        f.debug_struct("OwnedBytes")
            .field("len", &self.len())
            .finish()
    }
}

/// What a chunk's retained bytes are, and where they came from.
///
/// The two arms are the two write paths: a borrowed write is copied into a boxed slice this
/// module owns, and an owned write keeps the caller's [`OwnedBytes`] handle alive without a
/// copy. Either way the address is fixed for the chunk's life, which is all ngtcp2 requires.
enum Payload {
    /// A copy of a borrowed write.
    Copied(Box<[u8]>),
    /// A handle to a buffer the caller gave up, retained without a copy.
    Owned(OwnedBytes),
}

impl Payload {
    /// How many bytes were offered from this payload.
    fn len(&self) -> usize {
        match self {
            Self::Copied(data) => data.len(),
            Self::Owned(bytes) => bytes.len(),
        }
    }

    /// The pointer and length ngtcp2 is handed, taken from a **single** borrow.
    ///
    /// Separate [`as_ptr`](Self::as_ptr) and [`len`](Self::len) calls would ask an
    /// [`Owned`](Self::Owned) payload's owner for its slice twice, and a misbehaving owner
    /// could answer differently each time -- a short slice for the pointer, a long one for
    /// the length -- handing ngtcp2 a pointer valid for fewer bytes than the length claims.
    /// Deriving both from one `as_slice` closes that gap: the pair ngtcp2 keeps is always
    /// self-consistent, whatever the owner does on the next call.
    fn ptr_and_len(&self) -> (*const u8, usize) {
        match self {
            Self::Copied(data) => (data.as_ptr(), data.len()),
            Self::Owned(bytes) => {
                let slice = bytes.as_slice();
                (slice.as_ptr(), slice.len())
            }
        }
    }
}

/// One accepted write, held at a fixed address.
struct Chunk {
    /// The bytes, at an address that does not move.
    ///
    /// This is the *allocation*, which is what ngtcp2 holds a pointer into. Whether it is a
    /// copy of a borrowed write or a handle to a buffer the caller gave up, it is never
    /// reallocated, moved, or shrunk for as long as the chunk lives — see [`Chunk::len`].
    data: Payload,
    /// Offset in the stream of this chunk's first byte.
    start: u64,
    /// How many of `data`'s bytes ngtcp2 actually accepted.
    ///
    /// Separate from `data.len()` because ngtcp2 routinely takes less than it was offered —
    /// a packet fills, and the remainder is offered again as a *new* write. The obvious
    /// response, shrinking the allocation to the accepted prefix, is a use-after-free: the
    /// address ngtcp2 was given must stay valid until the acknowledgement arrives
    /// (`ngtcp2.h:5244-5248`), and reallocating changes it while ngtcp2 still holds the old
    /// one for retransmission. Nothing detects that on a lossless link, because nothing
    /// retransmits.
    ///
    /// So the tail beyond `len` is left allocated and unused until the chunk is released.
    /// That waste is bounded by one packet's worth per outstanding chunk, and it is the
    /// price of the address staying put.
    len: usize,
}

impl Chunk {
    /// Offset one past this chunk's last accepted byte.
    fn end(&self) -> u64 {
        self.start + self.len as u64
    }
}

/// Everything still held for one stream.
#[derive(Default)]
struct Stream {
    chunks: VecDeque<Chunk>,
    /// Offset the next accepted byte will have.
    next_offset: u64,
}

/// Per-connection retention of sent stream data.
#[derive(Default)]
pub(crate) struct Retained {
    streams: BTreeMap<i64, Stream>,
}

impl Retained {
    /// Copies `data` and returns a pointer and length to hand to ngtcp2.
    ///
    /// The returned pointer stays valid until [`Retained::acknowledge`] covers it or
    /// [`Retained::forget`] is called for the stream, which is exactly the contract ngtcp2
    /// states.
    ///
    /// Returns `None` for empty input; ngtcp2 treats a zero-length write specially and
    /// there is nothing to retain.
    /// Copies one or more ranges as a single contiguous chunk, and returns a pointer to it.
    ///
    /// The ranges are concatenated rather than handed to ngtcp2 as a vector array. That
    /// sounds like it gives up something, and it does not: the copy has to happen regardless,
    /// because ngtcp2 keeps the pointer and the caller's buffers are borrowed only for the
    /// call. Copying into one chunk costs exactly what copying into several would, and
    /// leaves a single address and length to account for on acknowledgement instead of a set
    /// that ngtcp2 may accept a prefix of.
    pub(crate) fn stage_many(
        &mut self,
        stream: StreamId,
        ranges: &[IoSlice<'_>],
    ) -> Option<(*const u8, usize)> {
        let total: usize = ranges.iter().map(|r| r.len()).sum();
        if total == 0 {
            return None;
        }
        let mut buffer = Vec::with_capacity(total);
        for range in ranges {
            buffer.extend_from_slice(range);
        }
        let entry = self.streams.entry(stream.get()).or_default();
        let chunk = Chunk {
            data: Payload::Copied(buffer.into_boxed_slice()),
            start: entry.next_offset,
            // Provisional: the whole staged buffer is on offer. `commit` records how much
            // ngtcp2 took, and is always called before the write path returns.
            len: total,
        };
        let (ptr, len) = chunk.data.ptr_and_len();
        entry.chunks.push_back(chunk);
        Some((ptr, len))
    }

    /// Retains an owned buffer without copying it, and returns a pointer to hand to ngtcp2.
    ///
    /// Unlike [`stage_many`](Self::stage_many), nothing is copied: the caller gave up
    /// ownership, so the [`OwnedBytes`] handle is kept alive here and ngtcp2 is handed a
    /// pointer straight into it. The returned pointer stays valid until
    /// [`acknowledge`](Self::acknowledge) covers it or [`forget`](Self::forget) is called,
    /// which is exactly ngtcp2's contract.
    ///
    /// Returns `None` for an empty buffer; ngtcp2 treats a zero-length write specially and
    /// there is nothing to retain.
    pub(crate) fn stage_owned(
        &mut self,
        stream: StreamId,
        bytes: OwnedBytes,
    ) -> Option<(*const u8, usize)> {
        let total = bytes.len();
        if total == 0 {
            return None;
        }
        let entry = self.streams.entry(stream.get()).or_default();
        let chunk = Chunk {
            data: Payload::Owned(bytes),
            start: entry.next_offset,
            len: total,
        };
        let (ptr, len) = chunk.data.ptr_and_len();
        entry.chunks.push_back(chunk);
        Some((ptr, len))
    }

    /// Records how much of the staged chunk ngtcp2 accepted.
    ///
    /// Anything beyond `accepted` was never handed over, so it must not count towards the
    /// stream's offset — the caller will submit those bytes again, and they would otherwise
    /// be retained twice and numbered wrongly.
    ///
    /// The allocation is deliberately **not** shrunk to fit. ngtcp2 was given its address a
    /// moment ago and holds it until the accepted prefix is acknowledged, so moving those
    /// bytes to a smaller box would leave ngtcp2 pointing at freed memory and corrupt any
    /// retransmission. Only the recorded length changes.
    pub(crate) fn commit(&mut self, stream: StreamId, accepted: usize) {
        let Some(entry) = self.streams.get_mut(&stream.get()) else {
            return;
        };
        let Some(chunk) = entry.chunks.back_mut() else {
            return;
        };

        if accepted == 0 {
            // Nothing was taken, so ngtcp2 kept no pointer into this allocation and it can
            // go. This is the one case where dropping the chunk is safe.
            entry.chunks.pop_back();
            return;
        }
        debug_assert!(
            accepted <= chunk.data.len(),
            "ngtcp2 cannot accept more than it was offered"
        );
        chunk.len = accepted.min(chunk.data.len());
        entry.next_offset = chunk.end();
    }

    /// The pointer ngtcp2 was given for the most recent staged write, and how much of it
    /// was accepted.
    ///
    /// The pointer is the allocation's, which never moves; the length is the accepted
    /// prefix. Used by the tests to assert exactly that.
    #[cfg(test)]
    pub(crate) fn last_pointer(&self, stream: StreamId) -> Option<(*const u8, usize)> {
        let entry = self.streams.get(&stream.get())?;
        let chunk = entry.chunks.back()?;
        let (ptr, _) = chunk.data.ptr_and_len();
        Some((ptr, chunk.len))
    }

    /// Releases everything acknowledged up to `offset + len` on a stream.
    pub(crate) fn acknowledge(&mut self, stream: StreamId, offset: u64, len: u64) {
        let Some(entry) = self.streams.get_mut(&stream.get()) else {
            return;
        };
        let acked_to = offset.saturating_add(len);
        while let Some(front) = entry.chunks.front() {
            if front.end() <= acked_to {
                entry.chunks.pop_front();
            } else {
                break;
            }
        }
        if entry.chunks.is_empty() && entry.next_offset <= acked_to {
            self.streams.remove(&stream.get());
        }
    }

    /// Releases everything held for a stream, which a closed stream no longer needs.
    pub(crate) fn forget(&mut self, stream: StreamId) {
        self.streams.remove(&stream.get());
    }

    /// Bytes currently held, across every stream.
    ///
    /// Exposed so a caller can see whether acknowledgements are being processed: an
    /// application that never sees them will watch this grow without bound.
    pub(crate) fn bytes_held(&self) -> usize {
        self.streams
            .values()
            .flat_map(|s| s.chunks.iter())
            .map(|c| c.len)
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sid(id: i64) -> StreamId {
        StreamId::new(id).unwrap()
    }

    #[test]
    fn an_owned_write_is_retained_without_a_copy() {
        // The point of the owned path: the caller's bytes are handed over, and ngtcp2 is
        // pointed straight into them. The pointer retained here is the buffer's own address,
        // not a copy's.
        let mut retained = Retained::default();
        let buffer: Arc<[u8]> = Arc::from(vec![1u8, 2, 3, 4].into_boxed_slice());
        let source = buffer.as_ptr();
        let (ptr, len) = retained
            .stage_owned(sid(0), OwnedBytes::new(buffer))
            .unwrap();
        retained.commit(sid(0), 4);
        assert_eq!(len, 4);
        assert_eq!(ptr, source, "ngtcp2 is pointed into the caller's buffer");
        // SAFETY: the retained handle keeps the allocation alive.
        let held = unsafe { core::slice::from_raw_parts(ptr, len) };
        assert_eq!(held, &[1, 2, 3, 4]);
    }

    #[test]
    fn an_empty_owned_write_stages_nothing() {
        let mut retained = Retained::default();
        assert!(retained
            .stage_owned(sid(0), OwnedBytes::new(Vec::new()))
            .is_none());
        assert_eq!(retained.bytes_held(), 0);
    }

    #[test]
    fn a_partially_accepted_owned_write_keeps_the_prefix_and_leaves_the_suffix_intact() {
        // The partial-acceptance contract for the owned path, checked directly. The accepted
        // prefix stays retained at the address ngtcp2 was handed, and the unaccepted suffix
        // -- a second handle into the same allocation -- reads back the bytes that were not
        // taken, ready to be offered again.
        let mut retained = Retained::default();
        let mut data = OwnedBytes::new(vec![1u8, 2, 3, 4, 5, 6, 7, 8]);
        let (staged, offered) = retained.stage_owned(sid(0), data.clone()).unwrap();
        assert_eq!(offered, 8);

        // A packet fills after three bytes.
        retained.commit(sid(0), 3);

        let (held, len) = retained.last_pointer(sid(0)).unwrap();
        assert_eq!(len, 3, "only the accepted prefix counts as retained");
        assert_eq!(
            staged, held,
            "the allocation ngtcp2 points into must not move"
        );

        // The suffix the caller keeps shares the same allocation and holds exactly the bytes
        // ngtcp2 did not take.
        let _prefix = data.split_to(3);
        assert_eq!(data.as_slice(), &[4, 5, 6, 7, 8]);
        assert_eq!(retained.bytes_held(), 3);
    }

    #[test]
    fn an_owned_suffix_offered_again_starts_where_the_prefix_ended() {
        // Offering the suffix as a fresh owned write must number the stream from where the
        // accepted prefix ended, exactly as a borrowed re-offer does.
        let mut retained = Retained::default();
        let mut data = OwnedBytes::new(vec![1u8, 2, 3, 4, 5, 6, 7, 8]);
        retained.stage_owned(sid(0), data.clone());
        retained.commit(sid(0), 3);
        let _prefix = data.split_to(3);

        retained.stage_owned(sid(0), data);
        retained.commit(sid(0), 5);
        assert_eq!(retained.bytes_held(), 8);

        // Acknowledging the first three releases exactly the first chunk.
        retained.acknowledge(sid(0), 0, 3);
        assert_eq!(retained.bytes_held(), 5);
        let (ptr, len) = retained.last_pointer(sid(0)).unwrap();
        // SAFETY: the chunk is alive.
        let held = unsafe { core::slice::from_raw_parts(ptr, len) };
        assert_eq!(held, &[4, 5, 6, 7, 8], "offsets follow the accepted length");
    }

    #[test]
    fn a_rejected_owned_write_retains_nothing() {
        let mut retained = Retained::default();
        retained.stage_owned(sid(0), OwnedBytes::new(vec![1u8, 2, 3]));
        retained.commit(sid(0), 0);
        assert_eq!(retained.bytes_held(), 0);
    }

    #[test]
    fn an_erased_owner_is_retained_without_a_copy_too() {
        // A caller with its own buffer type hands it over through `from_owner` and is not
        // made to copy it into an `Arc<[u8]>` first.
        struct CallerBuffer(Vec<u8>);
        impl AsRef<[u8]> for CallerBuffer {
            fn as_ref(&self) -> &[u8] {
                &self.0
            }
        }

        let mut retained = Retained::default();
        let owner = CallerBuffer(vec![9u8, 8, 7]);
        let source = owner.0.as_ptr();
        // SAFETY: `CallerBuffer` wraps an immutable `Vec` and lends the same slice on every
        // call, so its address and length are stable for the life of the handle.
        let owned = unsafe { OwnedBytes::from_owner(owner) };
        let (ptr, len) = retained.stage_owned(sid(0), owned).unwrap();
        retained.commit(sid(0), 3);
        assert_eq!(
            ptr, source,
            "ngtcp2 is pointed into the caller's own buffer"
        );
        // SAFETY: the retained handle keeps the owner alive.
        let held = unsafe { core::slice::from_raw_parts(ptr, len) };
        assert_eq!(held, &[9, 8, 7]);
    }

    #[test]
    fn staged_data_is_copied_not_borrowed() {
        // The whole point: the caller's buffer must be free to die immediately.
        let mut retained = Retained::default();
        let (ptr, len) = {
            let caller_buffer = vec![1u8, 2, 3, 4];
            let staged = retained
                .stage_many(sid(0), &[IoSlice::new(&caller_buffer)])
                .unwrap();
            retained.commit(sid(0), 4);
            staged
            // `caller_buffer` is dropped here.
        };
        assert_eq!(len, 4);
        // SAFETY: the retained copy is alive; this is the pointer ngtcp2 would hold.
        let held = unsafe { core::slice::from_raw_parts(ptr, len) };
        assert_eq!(held, &[1, 2, 3, 4]);
    }

    #[test]
    fn an_empty_write_stages_nothing() {
        let mut retained = Retained::default();
        assert!(retained.stage_many(sid(0), &[IoSlice::new(&[])]).is_none());
        assert_eq!(retained.bytes_held(), 0);
    }

    #[test]
    fn a_partial_acceptance_retains_only_what_was_accepted() {
        // Bytes ngtcp2 did not take will be offered again; retaining them here would both
        // waste memory and number the stream wrongly.
        let mut retained = Retained::default();
        retained.stage_many(sid(0), &[IoSlice::new(&[1, 2, 3, 4, 5, 6, 7, 8])]);
        retained.commit(sid(0), 3);
        assert_eq!(retained.bytes_held(), 3);

        let (ptr, len) = retained.last_pointer(sid(0)).unwrap();
        // SAFETY: the chunk is alive.
        let held = unsafe { core::slice::from_raw_parts(ptr, len) };
        assert_eq!(held, &[1, 2, 3]);
    }

    #[test]
    fn a_partial_acceptance_leaves_the_address_ngtcp2_was_given_alone() {
        // The one ngtcp2 actually cares about. It keeps the pointer it was handed until the
        // accepted bytes are acknowledged, so shrinking the allocation to fit the accepted
        // prefix -- which is the obvious tidy-up, and what this code used to do -- leaves
        // ngtcp2 reading freed memory the moment it retransmits.
        //
        // Nothing catches that on a lossless loopback, because nothing retransmits. It
        // needs a test that looks at the address rather than at the bytes.
        let mut retained = Retained::default();
        let (staged, offered) = retained
            .stage_many(sid(0), &[IoSlice::new(&[7u8; 1200])])
            .unwrap();
        assert_eq!(offered, 1200);

        // A packet fills before the offer is exhausted -- the ordinary case, not an edge
        // one, since the endpoint offers up to a full datagram and framing overhead takes
        // its share.
        retained.commit(sid(0), 1100);

        let (held, len) = retained.last_pointer(sid(0)).unwrap();
        assert_eq!(len, 1100, "only the accepted prefix counts as retained");
        assert_eq!(
            staged, held,
            "the allocation ngtcp2 points into must not move"
        );
        assert_eq!(retained.bytes_held(), 1100);
    }

    #[test]
    fn a_partially_accepted_chunk_still_reads_back_correctly_after_more_writes() {
        // The tail beyond the accepted prefix stays allocated. That must not leak into the
        // stream's offsets: the next chunk has to start where the accepted prefix ended,
        // not where the allocation ended.
        let mut retained = Retained::default();
        retained.stage_many(sid(0), &[IoSlice::new(&[1, 2, 3, 4, 5, 6, 7, 8])]);
        retained.commit(sid(0), 3);
        retained.stage_many(sid(0), &[IoSlice::new(&[4, 5, 6])]);
        retained.commit(sid(0), 3);
        assert_eq!(retained.bytes_held(), 6);

        // Acknowledging the first three must release exactly the first chunk.
        retained.acknowledge(sid(0), 0, 3);
        assert_eq!(retained.bytes_held(), 3);
        let (ptr, len) = retained.last_pointer(sid(0)).unwrap();
        // SAFETY: the chunk is alive.
        let held = unsafe { core::slice::from_raw_parts(ptr, len) };
        assert_eq!(held, &[4, 5, 6], "offsets must follow the accepted length");
    }

    #[test]
    fn a_rejected_write_retains_nothing() {
        let mut retained = Retained::default();
        retained.stage_many(sid(0), &[IoSlice::new(&[1, 2, 3])]);
        retained.commit(sid(0), 0);
        assert_eq!(retained.bytes_held(), 0);
    }

    #[test]
    fn offsets_advance_across_writes() {
        let mut retained = Retained::default();
        retained.stage_many(sid(0), &[IoSlice::new(&[0; 10])]);
        retained.commit(sid(0), 10);
        retained.stage_many(sid(0), &[IoSlice::new(&[0; 5])]);
        retained.commit(sid(0), 5);
        assert_eq!(retained.bytes_held(), 15);

        // Acknowledging the first ten releases exactly the first chunk.
        retained.acknowledge(sid(0), 0, 10);
        assert_eq!(retained.bytes_held(), 5);
    }

    #[test]
    fn a_partial_acknowledgement_releases_nothing_it_should_not() {
        // A chunk is released only when every one of its bytes is acknowledged; releasing
        // early would be the same use-after-free this module exists to prevent.
        let mut retained = Retained::default();
        retained.stage_many(sid(0), &[IoSlice::new(&[0; 10])]);
        retained.commit(sid(0), 10);
        retained.acknowledge(sid(0), 0, 4);
        assert_eq!(retained.bytes_held(), 10);
        retained.acknowledge(sid(0), 0, 10);
        assert_eq!(retained.bytes_held(), 0);
    }

    #[test]
    fn forgetting_a_stream_releases_everything_it_held() {
        let mut retained = Retained::default();
        retained.stage_many(sid(0), &[IoSlice::new(&[0; 32])]);
        retained.commit(sid(0), 32);
        retained.forget(sid(0));
        assert_eq!(retained.bytes_held(), 0);
    }

    #[test]
    fn streams_are_accounted_separately() {
        let mut retained = Retained::default();
        retained.stage_many(sid(0), &[IoSlice::new(&[0; 4])]);
        retained.commit(sid(0), 4);
        retained.stage_many(sid(4), &[IoSlice::new(&[0; 6])]);
        retained.commit(sid(4), 6);
        assert_eq!(retained.bytes_held(), 10);

        retained.acknowledge(sid(0), 0, 4);
        assert_eq!(retained.bytes_held(), 6);
    }

    #[test]
    fn acknowledging_an_unknown_stream_is_harmless() {
        let mut retained = Retained::default();
        retained.acknowledge(sid(99), 0, 100);
        retained.forget(sid(99));
        assert_eq!(retained.bytes_held(), 0);
    }

    #[test]
    fn chunk_addresses_do_not_move_when_another_chunk_is_added() {
        // A `Vec` would reallocate and move bytes ngtcp2 still points at. Boxed chunks are
        // what makes that impossible.
        let mut retained = Retained::default();
        retained.stage_many(sid(0), &[IoSlice::new(&[1, 2, 3])]);
        retained.commit(sid(0), 3);
        let (first, _) = retained.last_pointer(sid(0)).unwrap();

        for _ in 0..64 {
            retained.stage_many(sid(0), &[IoSlice::new(&[9; 16])]);
            retained.commit(sid(0), 16);
        }

        // The first chunk is still where it was.
        let entry = retained.streams.get(&0).unwrap();
        assert_eq!(entry.chunks.front().unwrap().data.ptr_and_len().0, first);
    }

    /// The safe surface of `OwnedBytes` stays safe: `new` and every reader can be used from
    /// code that forbids `unsafe` outright. `from_owner` is deliberately not exercised here
    /// -- it is now `unsafe`, and its safety contract is what keeps the raw pointer ngtcp2
    /// retains valid. Only `new`, which wraps an `Arc<[u8]>` that cannot misbehave, is on the
    /// safe path to FR-007, and this proves that path needs no `unsafe` of its own.
    #[test]
    #[forbid(unsafe_code)]
    fn the_safe_owned_bytes_surface_needs_no_unsafe() {
        let mut bytes = OwnedBytes::new(vec![1u8, 2, 3, 4, 5, 6]);
        assert_eq!(bytes.len(), 6);
        assert!(!bytes.is_empty());
        assert_eq!(bytes.as_slice(), &[1, 2, 3, 4, 5, 6]);

        let prefix = bytes.split_to(2);
        assert_eq!(prefix.as_slice(), &[1, 2]);
        assert_eq!(bytes.as_slice(), &[3, 4, 5, 6]);
        let _clone = bytes.clone();

        // Staging the safely-constructed handle retains it without a copy, all from code that
        // forbids `unsafe`.
        let mut retained = Retained::default();
        let (_, len) = retained
            .stage_owned(sid(0), OwnedBytes::new(vec![7u8, 8, 9]))
            .unwrap();
        retained.commit(sid(0), len);
        assert_eq!(retained.bytes_held(), 3);
    }
}
