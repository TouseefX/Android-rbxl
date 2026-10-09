//! The custom allocator handed to a connection.
//!
//! ngtcp2 accepts an `ngtcp2_mem` describing how to allocate. Two facts about it shape this
//! module.
//!
//! First, **the struct is retained by pointer, not copied**. `ngtcp2_conn` stores
//! `const ngtcp2_mem *mem` (`crates/ngnet-quic-sys/vendor/ngtcp2/lib/ngtcp2_conn.h:645`), assigned at
//! `ngtcp2_conn.c:1591`, and dereferences it again during `ngtcp2_conn_del`
//! (`ngtcp2_conn.c:1827`). A stack-local `ngtcp2_mem` would therefore be read after it had
//! gone — and the read happens in the destructor, so the corruption would appear at
//! teardown, far from its cause. Everything here exists to make that impossible.
//!
//! Second, the struct carries a `user_data` pointer passed to each allocator function,
//! which is how per-connection accounting is possible at all.
//!
//! Both halves are kept in one boxed allocation so there is a single object to keep alive
//! rather than two that have to agree.

// The allocator is handed to a connection, which arrives with `conn.rs`. It is written and
// tested here first because its correctness is about pointer stability rather than about
// any connection, and that is testable on its own.
#![allow(dead_code)]

use core::ffi::c_void;
use core::sync::atomic::{AtomicI64, AtomicU64, Ordering};

use ngnet_quic_sys as sys;

// Declared here rather than taken from the `libc` crate: a second runtime dependency would
// violate the single-dependency rule this crate is held to.
unsafe extern "C" {
    fn malloc(size: usize) -> *mut c_void;
    fn free(ptr: *mut c_void);
    fn calloc(nmemb: usize, size: usize) -> *mut c_void;
    fn realloc(ptr: *mut c_void, size: usize) -> *mut c_void;
}

/// Live-block accounting for one connection's native allocations.
///
/// Counts live blocks rather than pairing allocation and free counts, because ngtcp2
/// reallocates from a null pointer as an ordinary allocation and freeing a null pointer is
/// a no-op. A paired model would never balance.
#[derive(Debug, Default)]
pub(crate) struct AllocState {
    live: AtomicI64,
    total: AtomicU64,
}

impl AllocState {
    /// Blocks currently allocated and not yet freed.
    ///
    /// Zero once a connection has been dropped, if the connection leaked nothing.
    pub(crate) fn live_blocks(&self) -> i64 {
        self.live.load(Ordering::Relaxed)
    }

    /// Blocks allocated over the lifetime of the connection, never decremented.
    ///
    /// Used by tests to confirm the allocator was actually exercised, so a balance
    /// assertion cannot pass vacuously.
    #[cfg(test)]
    pub(crate) fn total_allocations(&self) -> u64 {
        self.total.load(Ordering::Relaxed)
    }

    fn record_alloc(&self) {
        self.live.fetch_add(1, Ordering::Relaxed);
        self.total.fetch_add(1, Ordering::Relaxed);
    }

    fn record_free(&self) {
        self.live.fetch_sub(1, Ordering::Relaxed);
    }
}

/// The allocator handed to a connection, owning both the accounting state and the
/// `ngtcp2_mem` struct that points at it.
///
/// Both halves must stay at fixed addresses for as long as the connection lives, so this is
/// constructed into a `Box` and never moved out of one.
pub(crate) struct Allocator {
    state: AllocState,
    mem: sys::ngtcp2_mem,
}

impl Allocator {
    /// Builds an allocator whose `ngtcp2_mem` points at its own state.
    ///
    /// The self-reference is set up after boxing, because the address is not knowable
    /// before then.
    pub(crate) fn new() -> Box<Self> {
        let mut allocator = Box::new(Self {
            state: AllocState::default(),
            mem: sys::ngtcp2_mem {
                user_data: core::ptr::null_mut(),
                malloc: Some(malloc_cb),
                free: Some(free_cb),
                calloc: Some(calloc_cb),
                realloc: Some(realloc_cb),
            },
        });

        let state: *const AllocState = &allocator.state;
        allocator.mem.user_data = state.cast_mut().cast::<c_void>();
        allocator
    }

    /// The accounting counters.
    pub(crate) fn state(&self) -> &AllocState {
        &self.state
    }

    /// The `ngtcp2_mem` to hand to a constructor.
    ///
    /// # Safety
    ///
    /// ngtcp2 stores this pointer, not a copy of what it points at, so the `Allocator` must
    /// outlive the connection it is given to. It is dereferenced during `ngtcp2_conn_del`.
    pub(crate) fn as_mem_ptr(&self) -> *const sys::ngtcp2_mem {
        &self.mem
    }
}

// SAFETY: `Allocator` owns only atomics and a struct of function pointers, all of which are
// `Send`. The `user_data` pointer refers into the same allocation, so moving the `Box`
// between threads moves the whole thing consistently.
unsafe impl Send for Allocator {}

/// Recovers the accounting state from the `user_data` pointer.
///
/// # Safety
///
/// `user_data` must be the pointer installed by [`Allocator::new`], and its `Allocator`
/// must still be alive.
unsafe fn state(user_data: *mut c_void) -> Option<&'static AllocState> {
    if user_data.is_null() {
        return None;
    }
    // SAFETY: the caller guarantees this is the pointer installed by `Allocator::new`,
    // which refers into a live boxed `Allocator`.
    Some(unsafe { &*user_data.cast::<AllocState>() })
}

unsafe extern "C" fn malloc_cb(size: usize, user_data: *mut c_void) -> *mut c_void {
    // SAFETY: an ordinary allocation; the returned pointer is handed straight back.
    let ptr = unsafe { malloc(size) };
    if !ptr.is_null() {
        // SAFETY: `user_data` is what `Allocator::new` installed.
        if let Some(state) = unsafe { state(user_data) } {
            state.record_alloc();
        }
    }
    ptr
}

unsafe extern "C" fn free_cb(ptr: *mut c_void, user_data: *mut c_void) {
    // Freeing a null pointer is a no-op in C and must not be counted, or the live count
    // drifts negative.
    if ptr.is_null() {
        return;
    }
    // SAFETY: `user_data` is what `Allocator::new` installed.
    if let Some(state) = unsafe { state(user_data) } {
        state.record_free();
    }
    // SAFETY: forwarding the caller's own contract.
    unsafe { free(ptr) }
}

unsafe extern "C" fn calloc_cb(nmemb: usize, size: usize, user_data: *mut c_void) -> *mut c_void {
    // SAFETY: an ordinary allocation.
    let ptr = unsafe { calloc(nmemb, size) };
    if !ptr.is_null() {
        // SAFETY: `user_data` is what `Allocator::new` installed.
        if let Some(state) = unsafe { state(user_data) } {
            state.record_alloc();
        }
    }
    ptr
}

unsafe extern "C" fn realloc_cb(
    ptr: *mut c_void,
    size: usize,
    user_data: *mut c_void,
) -> *mut c_void {
    let was_null = ptr.is_null();
    // SAFETY: forwarding the caller's own contract.
    let out = unsafe { realloc(ptr, size) };
    if !out.is_null() {
        // SAFETY: `user_data` is what `Allocator::new` installed.
        if let Some(state) = unsafe { state(user_data) } {
            // A realloc from null is a fresh allocation; one from a real pointer replaces a
            // block that is already counted, so nothing moves. Counting blocks rather than
            // bytes is what makes this unambiguous -- a byte total would have to decide
            // whether to subtract the old size, which is not knowable here.
            if was_null {
                state.record_alloc();
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Calls the allocator's own callbacks the way ngtcp2 would.
    fn exercise(allocator: &Allocator) {
        let mem = allocator.as_mem_ptr();
        // SAFETY: the allocator is alive for the whole function, and the callbacks are the
        // ones it installed.
        unsafe {
            let user_data = (*mem).user_data;
            let a = ((*mem).malloc.unwrap())(64, user_data);
            let b = ((*mem).calloc.unwrap())(4, 16, user_data);
            ((*mem).free.unwrap())(a, user_data);
            ((*mem).free.unwrap())(b, user_data);
        }
    }

    #[test]
    fn allocations_are_counted_and_balance_after_freeing() {
        let allocator = Allocator::new();
        exercise(&allocator);
        assert_eq!(allocator.state().live_blocks(), 0);
        assert_eq!(
            allocator.state().total_allocations(),
            2,
            "the balance above must not be able to pass without the allocator being used"
        );
    }

    #[test]
    fn freeing_null_is_not_counted() {
        // ngtcp2 frees null pointers routinely. Counting them would drive the live count
        // negative and make the balance assertion above meaningless.
        let allocator = Allocator::new();
        let mem = allocator.as_mem_ptr();
        // SAFETY: the allocator is alive, and freeing null is defined to be a no-op.
        unsafe {
            let user_data = (*mem).user_data;
            ((*mem).free.unwrap())(core::ptr::null_mut(), user_data);
        }
        assert_eq!(allocator.state().live_blocks(), 0);
    }

    #[test]
    fn the_mem_pointer_is_stable_across_moves_of_its_owner() {
        // This is the property the whole module exists for: ngtcp2 keeps the pointer and
        // dereferences it in `ngtcp2_conn_del`, so it must not move when the `Box` does.
        let allocator = Allocator::new();
        let before = allocator.as_mem_ptr();

        let moved = allocator;
        let after = moved.as_mem_ptr();

        assert_eq!(before, after);
    }

    #[test]
    fn the_mem_struct_points_at_its_own_state() {
        let allocator = Allocator::new();
        let mem = allocator.as_mem_ptr();
        // SAFETY: the allocator is alive.
        let user_data = unsafe { (*mem).user_data };
        let expected: *const AllocState = allocator.state();
        assert_eq!(user_data.cast::<AllocState>().cast_const(), expected);
    }

    #[test]
    fn a_realloc_from_null_counts_as_a_new_block() {
        let allocator = Allocator::new();
        let mem = allocator.as_mem_ptr();
        // SAFETY: the allocator is alive; realloc from null is a plain allocation.
        unsafe {
            let user_data = (*mem).user_data;
            let p = ((*mem).realloc.unwrap())(core::ptr::null_mut(), 32, user_data);
            assert_eq!(allocator.state().live_blocks(), 1);
            ((*mem).free.unwrap())(p, user_data);
        }
        assert_eq!(allocator.state().live_blocks(), 0);
    }
}
