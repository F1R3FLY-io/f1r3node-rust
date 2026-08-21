> Last updated: 2026-08-20

# Crate: crypto

**Path**: `crypto/`

Cryptographic primitives for hashing, signing, certificate management, and key serialization.

## Hash Functions

| Type | Output | Notes |
|------|--------|-------|
| `Blake2b256` | 32 bytes | Primary content hash |
| `Blake2b512Block` | 64 bytes | Online tree hashing with configurable fanout/depth |
| `Blake2b512Random` | 32 bytes | Splittable/mergeable PRNG for unforgeable name generation |
| `Keccak256` | 32 bytes | Ethereum-compatible hashing |
| `Sha256Hasher` | 32 bytes | Standard SHA-256 |

**`Blake2b512Random`** is notable -- it's a deterministic PRNG used to generate unique unforgeable names in Rholang. Supports `split_byte(i8)`, `split_short(i16)`, and `merge(Vec<Self>)` for parallel composition.

## Key Types

`PrivateKey` and `PublicKey` wrap byte strings. A production wallet private key
is a valid 32-byte secp256k1 scalar. Its public key is a validated 65-byte SEC1
uncompressed point (`0x04 || x || y`). `Secp256k1::new_key_pair` uses the
operating system random source.

The private key remains off chain. Its public key is a verifier identity and an
input to native wallet-address derivation; neither a public key nor a wallet
address authorizes a debit by itself.

## Signature Algorithms

**`SignaturesAlg` trait**:
- `verify(data, sig, pub_key) -> bool`
- `sign(data, sec_key) -> Vec<u8>`
- `to_public(PrivateKey) -> PublicKey`
- `new_key_pair() -> (PrivateKey, PublicKey)`

**Implementations**:
- **`Secp256k1`** -- Primary algorithm. DER-encoded ECDSA signatures over the Blake2b-256 hash of the canonical protobuf message. Supports encrypted PKCS#8 PEM parsing through OpenSSL via `parse_pem_file(path, password)`.
- **`Secp256k1Eth`** -- Ethereum-compatible variant named `secp256k1:eth`. Uses a fixed-width 64-byte `r || s` signature over Keccak-256 of the Ethereum length-prefixed canonical protobuf message.
- **`SchnorrSecp256k1` and `FrostSecp256k1`** -- Domain-separated experimental algorithms available only with the corresponding feature enabled.
- **`Ed25519`** -- Present but disabled for deploy signing per RCHAIN-3560.

**`Signed<A>`** -- Generic signed wrapper:
- `create(data, algorithm, private_key)` -- Signs and wraps
- `from_signed_data(data, pk, sig, algorithm)` -- Verifies and wraps
- `signature_hash(alg_name, serialized_data)` -- Keccak256 for Eth, Blake2b256 otherwise

A cosigned deploy verifies every present signer over the same canonical deploy
message, rejects duplicate public keys, and orders identities by raw public-key
bytes. Cost funding uses only verified signers. The stable wallet authority is
the verified public key, not the variable ECDSA signature bytes, so repeated
deploys from the same wallet resolve to the same purse.

## Wallet, capability, and evidence cryptography

Native wallet addresses and Rholang funding slots use different sources of
authority:

- A public-key wallet address is derived from the uncompressed secp256k1 point
  through the repository's two-stage Keccak transform, then framed with the
  four-byte native prefix and a four-byte Blake2b-256 checksum before Base58
  encoding.
- A funding-slot address is derived from the Keccak-256 hash of the canonical
  protobuf encoding of a consensus-generated `GPrivate` name. Publishing the
  address permits deposits; spending requires the name as a first-class
  Rholang capability. The name's bytes are not a confidentiality secret, and
  the private-name preview API can predict them; source code has no
  bytes-to-`GPrivate` constructor.
- Cost reservation identifiers, certificate identifiers, byte-schedule
  digests, and event identities use distinct Blake2b-256 domains. A validator
  recomputes them during replay rather than trusting peer-supplied evidence.
- Node TLS certificates use P-256 X.509 keys. TLS protects transport but does
  not authorize a wallet debit, process activation, settlement, or block state.

Read [Wallet-funded process lifecycle](../rholang/20-wallet-funded-processes.md)
for key storage, wallet refill, process funding, lollipop capability transfer,
certificate binding, replay, and finality as one end-to-end workflow.

## Certificate Operations

**`CertificateHelper`** -- P-256 (secp256r1) TLS certificate management:
- `generate_key_pair(use_non_blocking)` -- P-256 key generation
- `generate_certificate(secret, public)` -- Self-signed X.509
- `public_address(pub_key)` -- Ethereum-style address (Keccak256 of uncompressed key, last 20 bytes)
- `parse_certificate(der_bytes)` / `parse_certificate_pem(pem_str)` -- X.509 parsing

## Additional Utilities

- **`KeyUtil`** -- File I/O for keys: `write_keys()` writes AES-256-CBC password-encrypted PKCS#8 private-key PEM, public-key PEM, and uncompressed public-key hex
- **`CertificatePrinter`** -- PEM formatting for certificates and private keys

## Tests

Property-based tests (proptest) for DER encoding roundtrips, certificate generation, and key pair operations in `tests/util/`.

**See also:** [crypto/ crate README](../../crypto/README.md)

[← Back to docs index](../README.md)
