# ngnet-quic-sys

Raw Rust FFI bindings to [libngtcp2](https://github.com/ngtcp2/ngtcp2), built from the
ngtcp2 source bundled with this crate.

The build script compiles a static, library-only copy of ngtcp2 and generates bindings from
the matching headers with `bindgen`. This crate exposes the C API directly and is inherently
unsafe. Most applications should use [`ngnet-quic`](https://crates.io/crates/ngnet-quic).

## TLS feature

The default `crypto-ossl` feature also builds ngtcp2's OpenSSL crypto helper. It requires
OpenSSL 3.5 or newer and uses `pkg-config` unless `OPENSSL_DIR` is set.

```toml
# Build the transport library without a TLS helper.
ngnet-quic-sys = { version = "0.0.1", default-features = false }
```

The OpenSSL backend is upstream experimental. Disabling it leaves libngtcp2 itself, which
has no TLS dependency.

## Build requirements

- A C compiler
- CMake 3.20 or newer
- libclang, used by `bindgen`
- OpenSSL 3.5 or newer for the default feature

The bundled source is the default. Set `NGTCP2_SOURCE_DIR` to build against another ngtcp2
checkout.

This crate is released at `0.0.x` while its API settles and is versioned alongside
`ngnet-quic`.

License: MIT
