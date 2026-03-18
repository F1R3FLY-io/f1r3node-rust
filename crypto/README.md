# crypto

Cryptography primitives used across the Rust workspace.

## Responsibilities

| Area | Implementation |
| --- | --- |
| Hashes | Blake2b, Keccak256, SHA-256 |
| Signatures | Ed25519 and secp256k1 |
| Keys | Public/private key parsing and helpers |
| TLS | Certificate generation and address derivation helpers |

## Build

```bash
cargo build -p crypto
cargo build --release -p crypto
```

## Test

```bash
cargo test -p crypto
cargo test -p crypto --release
```

## Key Source Areas

| Path | Purpose |
| --- | --- |
| `src/rust/hash/` | Hash implementations |
| `src/rust/signatures/` | Signature algorithms and helpers |
| `src/rust/private_key.rs` | Private key parsing and conversion |
| `src/rust/public_key.rs` | Public key parsing and conversion |
| `src/rust/util/certificate_helper.rs` | TLS certificate helpers and peer identity support |

## Dependency Notes

- OpenSSL headers and libraries are required for this crate to compile.
- Higher-level networking code in `comm` uses the certificate helpers defined here.
