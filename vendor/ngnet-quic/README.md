# ngnet-quic

Safe Rust bindings to [ngtcp2](https://github.com/ngtcp2/ngtcp2) for QUIC transport.

The core is a sans-I/O client/server state machine: callers provide received datagrams and a
monotonic timestamp, then send the datagrams and honor the expiry it returns. The default
`endpoint` feature adds an asynchronous layer that owns a UDP socket and routes packets
among connections.

```toml
[dependencies]
ngnet-quic = "0.0.1"
```

## Features

| Feature | Default | Purpose |
| --- | --- | --- |
| `tls-ossl` | Yes | Safe OpenSSL TLS backend using ngtcp2's crypto helper. |
| `endpoint` | Yes | Asynchronous endpoint and connection driver. |
| `tokio` | No | Ready-made UDP socket and clock for tokio. Implies `endpoint`. |

The default TLS backend requires OpenSSL 3.5 or newer. Build with
`default-features = false` to supply another TLS backend and drive the state machine
directly.

Current scope includes client and server connections and address validation. 0-RTT,
unreliable datagrams, connection migration and key update are not implemented.

The bundled native dependency requires a C compiler, CMake 3.20 or newer, and libclang when
this crate is built from source.

The API is released at `0.0.x` and may change between every release. See the
[API documentation](https://docs.rs/ngnet-quic) and
[design notes](https://github.com/youyuanwu/ngnet/tree/main/docs/quic).

License: MIT
