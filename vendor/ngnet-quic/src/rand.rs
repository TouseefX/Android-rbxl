//! Where randomness comes from.
//!
//! QUIC needs unpredictable bytes for connection identifiers, path-challenge data and
//! stateless reset tokens. ngtcp2 does not generate them: it asks the application, through
//! the `rand` callback and `get_new_connection_id`.
//!
//! # Why this is a separate seam rather than part of the handler surface
//!
//! The `rand` callback is unlike every other ngtcp2 callback. It receives neither the
//! connection nor the `user_data` pointer — its only parameter besides the output buffer is
//! a `const ngtcp2_rand_ctx *` (`ngtcp2.h:3112-3113`). Worse, it fires *during*
//! `ngtcp2_conn_client_new`, before `*pconn` has been assigned and before `user_data` has
//! been stored (`ngtcp2_conn.c:1357,1360,1582`, with `user_data` set at `:1592`).
//!
//! So the trampoline this crate uses for every other callback cannot serve this one: at the
//! moment it first fires, there is nothing to recover state from. The only channel is
//! `settings.rand_ctx.native_handle`, an opaque pointer ngtcp2 passes straight through.
//! That is what this module is for.
//!
//! # No default
//!
//! There is deliberately no built-in generator. This crate holds itself to one non-optional
//! dependency, so it has no RNG to reach for, and inventing one from a clock would produce
//! predictable identifiers — which is a real weakness, not a theoretical one. A caller
//! supplies the source, and the type system makes that unavoidable.

use crate::error::Result;

/// A source of unpredictable bytes.
///
/// # Implementing this
///
/// The bytes must be unpredictable to an observer, so a counter or a clock-seeded PRNG is
/// not acceptable outside tests. In production this should be the operating system's
/// generator, or a CSPRNG seeded from it.
///
/// `fill` is called from inside ngtcp2, including during connection construction. It must
/// not panic: a panic there unwinds into a C stack frame and aborts the process. Report
/// failure by returning an error instead.
pub trait EntropySource {
    /// Fills `dest` entirely with unpredictable bytes.
    ///
    /// # Errors
    ///
    /// Return an error if the underlying generator failed. ngtcp2 treats this as a
    /// callback failure, which is fatal to the connection.
    fn fill(&mut self, dest: &mut [u8]) -> Result<()>;
}

impl<T: EntropySource + ?Sized> EntropySource for &mut T {
    fn fill(&mut self, dest: &mut [u8]) -> Result<()> {
        (**self).fill(dest)
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    /// A deterministic entropy source for tests.
    ///
    /// Emphatically not suitable for anything else: it produces a counting sequence, which
    /// is exactly what an attacker would predict. It exists so tests can assert *how much*
    /// entropy was drawn and can reproduce a run byte for byte.
    #[derive(Default)]
    pub(crate) struct CountingEntropy {
        next: u8,
        produced: usize,
    }

    impl CountingEntropy {
        /// How many bytes have been handed out so far.
        pub(crate) fn bytes_produced(&self) -> usize {
            self.produced
        }
    }

    impl EntropySource for CountingEntropy {
        fn fill(&mut self, dest: &mut [u8]) -> Result<()> {
            for slot in dest.iter_mut() {
                *slot = self.next;
                self.next = self.next.wrapping_add(1);
                self.produced += 1;
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::CountingEntropy;
    use super::*;

    #[test]
    fn a_source_fills_the_whole_buffer() {
        let mut source = CountingEntropy::default();
        let mut buf = [0xffu8; 4];
        source.fill(&mut buf).unwrap();
        assert_eq!(buf, [0, 1, 2, 3]);
        assert_eq!(source.bytes_produced(), 4);
    }

    #[test]
    fn the_blanket_impl_forwards_through_a_mutable_reference() {
        // This matters because the connection holds the source behind a pointer and calls
        // it through a reference; if the blanket impl were missing, every caller would have
        // to name a concrete type.
        let mut source = CountingEntropy::default();
        let indirect = &mut &mut source;
        let mut buf = [0u8; 2];
        indirect.fill(&mut buf).unwrap();
        assert_eq!(buf, [0, 1]);
    }
}
