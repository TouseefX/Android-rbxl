# Test credentials

A self-signed certificate for `localhost`, generated once and committed.

It is here rather than generated at test time because `ngnet-quic` may have **no
dev-dependencies** — an invariant test asserts it, on the grounds that test-only needs
belong in `ngnet-quic-tests`. A certificate-generation crate would violate that, and these
tests need credentials only to prove a server connection can be *constructed*.

Generated with a 100-year validity so the suite does not begin failing on a date nobody
chose:

```sh
openssl req -x509 -newkey rsa:2048 \
  -keyout test-key.pem -out test-cert.pem \
  -days 36500 -nodes -subj "/CN=localhost" \
  -addext "subjectAltName=DNS:localhost"
```

**This key is public and worthless.** It has never protected anything and must never be
used outside this suite.
