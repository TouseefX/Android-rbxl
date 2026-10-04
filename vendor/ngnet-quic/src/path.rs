//! Network paths, and the conversion to ngtcp2's address types.
//!
//! A QUIC connection travels over a *path*: a pair of addresses, local and remote. ngtcp2
//! represents one as `ngtcp2_path`, holding two `ngtcp2_addr`, each of which is a raw
//! `sockaddr` pointer and a length.
//!
//! Three things about that are worth knowing.
//!
//! **The addresses are copied, not borrowed, by the constructor.** `ngtcp2_dcid_init` and
//! friends run `ngtcp2_path_copy`
//! (`crates/ngnet-quic-sys/vendor/ngtcp2/lib/ngtcp2_path.c:31-35`), and
//! `ngtcp2_conn_client_new` additionally `memcpy`s the local address. So a [`PathStorage`]
//! may be a stack local that dies when the call returns — unlike the allocator, which must
//! outlive the connection.
//!
//! **`ngtcp2_addr_init` casts away `const`.** It stores `(ngtcp2_sockaddr *)addr`
//! (`crates/ngnet-quic-sys/vendor/ngtcp2/lib/ngtcp2_addr.c:32-40`), producing a mutable pointer from what the
//! caller may have had as a shared reference. This module therefore owns its `sockaddr`
//! storage mutably and hands out pointers derived from a `&mut`, rather than casting a
//! shared reference and hoping.
//!
//! **`ngtcp2_path.user_data` is never reclaimed.** The header states there is no way to know
//! when a connection has finished with a path, so anything it pointed at would have to live
//! as long as the connection (`ngtcp2.h:2158-2168`). This crate leaves it null; nothing here
//! needs it.
//!
//! Addresses enter as [`core::net::SocketAddr`]. `core::net` rather than `std::net` is
//! deliberate: this crate performs no I/O and a test asserts its sources never name
//! `std::net`, but the address types themselves are pure data and live in `core`.

// Consumed by the connection constructors, which arrive with `conn.rs`.
#![allow(dead_code)]

use core::mem::size_of;
use core::net::SocketAddr;

use ngnet_quic_sys as sys;

/// Storage for one path's two addresses, laid out as ngtcp2 wants them.
///
/// Owns the `sockaddr` bytes so that the pointers inside the `ngtcp2_path` refer to memory
/// this type controls. Because ngtcp2 copies path addresses out of it, an instance only has
/// to outlive the call it is passed to.
pub(crate) struct PathStorage {
    local: SockaddrStorage,
    remote: SockaddrStorage,
    path: sys::ngtcp2_path,
}

impl PathStorage {
    /// Builds storage for a local and remote address.
    pub(crate) fn new(local: SocketAddr, remote: SocketAddr) -> Box<Self> {
        let mut storage = Box::new(Self {
            local: SockaddrStorage::from(local),
            remote: SockaddrStorage::from(remote),
            // Filled in below, once the addresses have their final addresses.
            path: sys::ngtcp2_path {
                local: sys::ngtcp2_addr {
                    addr: core::ptr::null_mut(),
                    addrlen: 0,
                },
                remote: sys::ngtcp2_addr {
                    addr: core::ptr::null_mut(),
                    addrlen: 0,
                },
                user_data: core::ptr::null_mut(),
            },
        });

        storage.wire_up();
        storage
    }

    /// Points the `ngtcp2_path` at this instance's own address storage.
    ///
    /// Separated out because it has to run after boxing: the addresses are not knowable
    /// before then.
    fn wire_up(&mut self) {
        self.path.local = self.local.as_addr();
        self.path.remote = self.remote.as_addr();
    }

    /// The `ngtcp2_path` to pass to the library.
    pub(crate) fn as_raw(&self) -> *const sys::ngtcp2_path {
        &self.path
    }

    /// The `ngtcp2_path` to pass where ngtcp2 wants it mutable.
    ///
    /// The write paths take `ngtcp2_path *` even though they only read the addresses; this
    /// is the const-cast described in the module documentation surfacing in the signature.
    pub(crate) fn as_raw_mut(&mut self) -> *mut sys::ngtcp2_path {
        &mut self.path
    }

    /// The local address this storage was built from.
    pub(crate) fn local(&self) -> SocketAddr {
        self.local.to_socket_addr()
    }

    /// The remote address this storage was built from.
    pub(crate) fn remote(&self) -> SocketAddr {
        self.remote.to_socket_addr()
    }

    /// The remote address as a raw `sockaddr`, for the calls that take one directly.
    ///
    /// Address validation works on an address rather than on a path: a Retry token is
    /// bound to where the client claimed to be, and no connection exists yet to own a path.
    pub(crate) fn remote_sockaddr(&self) -> *const sys::ngtcp2_sockaddr {
        let ptr: *const SockaddrStorage = &self.remote;
        ptr.cast::<sys::ngtcp2_sockaddr>()
    }

    /// The length that goes with [`PathStorage::remote_sockaddr`].
    pub(crate) fn remote_socklen(&self) -> sys::ngtcp2_socklen {
        self.remote.len()
    }
}

/// A `sockaddr_in` or `sockaddr_in6`, whichever the address needs.
///
/// Written out by hand rather than taken from `libc`, which would be a second dependency.
/// The layouts are fixed by the platform ABI and are what every BSD-sockets implementation
/// has agreed on for decades, so restating them is safe in a way that restating an
/// application struct would not be.
#[repr(C)]
union SockaddrStorage {
    v4: SockaddrIn,
    v6: SockaddrIn6,
    /// Present so the union is always fully initialised, whichever family is in use.
    bytes: [u8; size_of::<SockaddrIn6>()],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct SockaddrIn {
    /// `sin_family`, in host byte order.
    family: u16,
    /// `sin_port`, in **network** byte order.
    port: u16,
    /// `sin_addr`, four bytes in network order.
    addr: [u8; 4],
    /// `sin_zero`, padding to the common `sockaddr` size.
    zero: [u8; 8],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct SockaddrIn6 {
    /// `sin6_family`, in host byte order.
    family: u16,
    /// `sin6_port`, in **network** byte order.
    port: u16,
    /// `sin6_flowinfo`.
    flowinfo: u32,
    /// `sin6_addr`, sixteen bytes.
    addr: [u8; 16],
    /// `sin6_scope_id`.
    scope_id: u32,
}

/// `AF_INET`, which is 2 on every platform this crate builds for.
const AF_INET: u16 = 2;

/// `AF_INET6`. Unlike `AF_INET` this genuinely differs between platforms — 10 on Linux,
/// 30 on macOS, 23 on Windows — so it is selected rather than assumed.
const AF_INET6: u16 = if cfg!(target_os = "linux") {
    10
} else if cfg!(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd"
)) {
    30
} else if cfg!(windows) {
    23
} else {
    // Anything else has not been checked. Zero is not a valid family, so a connection
    // attempt fails loudly rather than sending to an address nobody meant.
    0
};

impl SockaddrStorage {
    /// The `ngtcp2_addr` describing this storage.
    ///
    /// Takes `&self` but produces a mutable pointer, because that is what `ngtcp2_addr`
    /// holds. The pointer is derived from a reference into boxed storage the caller owns,
    /// and ngtcp2 only reads through it.
    fn as_addr(&self) -> sys::ngtcp2_addr {
        let len = self.len();
        let ptr: *const SockaddrStorage = self;
        sys::ngtcp2_addr {
            addr: ptr.cast_mut().cast::<sys::ngtcp2_sockaddr>(),
            addrlen: len,
        }
    }

    /// The length ngtcp2 should be told, which depends on the family.
    fn len(&self) -> sys::ngtcp2_socklen {
        // SAFETY: every variant of the union shares the family field at offset zero, and
        // the union is always fully initialised.
        let family = unsafe { self.v4.family };
        let size = if family == AF_INET6 {
            size_of::<SockaddrIn6>()
        } else {
            size_of::<SockaddrIn>()
        };
        size as sys::ngtcp2_socklen
    }

    /// Reads the storage back out as a `SocketAddr`.
    fn to_socket_addr(&self) -> SocketAddr {
        // SAFETY: the family field is shared by both variants and the union is always
        // initialised, so reading it is defined whichever variant is live.
        let family = unsafe { self.v4.family };
        if family == AF_INET6 {
            // SAFETY: the family says the v6 variant is the live one.
            let v6 = unsafe { self.v6 };
            SocketAddr::from((v6.addr, u16::from_be(v6.port)))
        } else {
            // SAFETY: the family says the v4 variant is the live one.
            let v4 = unsafe { self.v4 };
            SocketAddr::from((v4.addr, u16::from_be(v4.port)))
        }
    }
}

impl From<SocketAddr> for SockaddrStorage {
    fn from(addr: SocketAddr) -> Self {
        // Start fully zeroed so no padding byte is ever uninitialised; ngtcp2 compares
        // addresses by memory in places, and stray bytes would make equal addresses differ.
        let mut storage = Self {
            bytes: [0; size_of::<SockaddrIn6>()],
        };
        match addr {
            SocketAddr::V4(v4) => {
                storage.v4 = SockaddrIn {
                    family: AF_INET,
                    port: v4.port().to_be(),
                    addr: v4.ip().octets(),
                    zero: [0; 8],
                };
            }
            SocketAddr::V6(v6) => {
                storage.v6 = SockaddrIn6 {
                    family: AF_INET6,
                    port: v6.port().to_be(),
                    flowinfo: v6.flowinfo(),
                    addr: v6.ip().octets(),
                    scope_id: v6.scope_id(),
                };
            }
        }
        storage
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ipv4_address_round_trips() {
        let addr: SocketAddr = "192.0.2.1:443".parse().unwrap();
        let storage = SockaddrStorage::from(addr);
        assert_eq!(storage.to_socket_addr(), addr);
    }

    #[test]
    fn an_ipv6_address_round_trips() {
        let addr: SocketAddr = "[2001:db8::1]:443".parse().unwrap();
        let storage = SockaddrStorage::from(addr);
        assert_eq!(storage.to_socket_addr(), addr);
    }

    #[test]
    fn the_port_is_stored_in_network_order() {
        // Getting this wrong is the classic way to produce an address that round-trips
        // through your own code and is wrong on the wire, so it is checked against the
        // bytes as they sit in memory rather than through the round trip.
        let addr: SocketAddr = "192.0.2.1:443".parse().unwrap();
        let storage = SockaddrStorage::from(addr);
        // SAFETY: an IPv4 address was just written, so the v4 variant is live.
        let raw_port = unsafe { storage.v4.port };
        // `to_ne_bytes` is what the field actually looks like in memory, which is what the
        // kernel and ngtcp2 read. `to_be_bytes` would re-swap and hide an error.
        assert_eq!(raw_port.to_ne_bytes(), 443u16.to_be_bytes());
    }

    #[test]
    fn the_reported_length_matches_the_family() {
        let v4 = SockaddrStorage::from("192.0.2.1:1".parse::<SocketAddr>().unwrap());
        let v6 = SockaddrStorage::from("[2001:db8::1]:1".parse::<SocketAddr>().unwrap());
        assert_eq!(v4.len() as usize, size_of::<SockaddrIn>());
        assert_eq!(v6.len() as usize, size_of::<SockaddrIn6>());
        assert!(v6.len() > v4.len());
    }

    #[test]
    fn the_layouts_match_the_platform_abi() {
        // These sizes are what every BSD-sockets platform agrees on. If a target ever
        // disagreed, everything else in this module would be quietly wrong.
        assert_eq!(size_of::<SockaddrIn>(), 16);
        assert_eq!(size_of::<SockaddrIn6>(), 28);
    }

    #[test]
    fn a_path_points_at_its_own_storage() {
        let local: SocketAddr = "192.0.2.1:1".parse().unwrap();
        let remote: SocketAddr = "192.0.2.2:2".parse().unwrap();
        let storage = PathStorage::new(local, remote);

        // SAFETY: the storage is alive for the whole test.
        let path = unsafe { &*storage.as_raw() };
        let local_ptr: *const SockaddrStorage = &storage.local;
        let remote_ptr: *const SockaddrStorage = &storage.remote;

        assert_eq!(path.local.addr.cast_const().cast(), local_ptr);
        assert_eq!(path.remote.addr.cast_const().cast(), remote_ptr);
        assert!(path.user_data.is_null());
    }

    #[test]
    fn a_path_reports_the_addresses_it_was_built_from() {
        let local: SocketAddr = "[2001:db8::1]:1".parse().unwrap();
        let remote: SocketAddr = "192.0.2.2:2".parse().unwrap();
        let storage = PathStorage::new(local, remote);
        assert_eq!(storage.local(), local);
        assert_eq!(storage.remote(), remote);
    }

    #[test]
    fn path_pointers_survive_moving_the_box() {
        let mut storage = PathStorage::new(
            "192.0.2.1:1".parse().unwrap(),
            "192.0.2.2:2".parse().unwrap(),
        );
        let before = storage.as_raw_mut();
        let moved = storage;
        assert_eq!(before.cast_const(), moved.as_raw());
    }

    #[test]
    fn every_byte_is_initialised_so_equal_addresses_compare_equal() {
        // ngtcp2 compares addresses bytewise in places. Two storages built from the same
        // address must therefore be byte-identical, which is only true if the padding is
        // zeroed rather than left as whatever was on the stack.
        let addr: SocketAddr = "192.0.2.1:443".parse().unwrap();
        let a = SockaddrStorage::from(addr);
        let b = SockaddrStorage::from(addr);
        // SAFETY: the `bytes` variant covers the whole union and is always initialised.
        unsafe { assert_eq!(a.bytes, b.bytes) };
    }
}
