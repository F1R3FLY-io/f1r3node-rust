# Identity Registry Contract

The identity registry links secp256k1 keys to stable identity names on a
F1R3FLY shard. A party registers an identity, rotates its key, and reads the
full key history. A quorum of admins can link a new key to an identity whose
current key is lost or compromised.

The contract,
[`IdentityRegistry.rhox`](../casper/src/main/resources/IdentityRegistry.rhox),
ships in the genesis block. Its RhoSpec suite is
[`IdentityRegistryTest.rho`](../casper/src/test/resources/IdentityRegistryTest.rho).
The Rust client library, the `registry_cli` binary, and the integration tests
are in the `identity-tools` repository, at
<https://github.com/mlabs-haskell/identity-tools>.

## Deployment

The contract is a standard genesis deploy. Genesis registers it in the
system registry at `rho:system:identityRegistry`. Two genesis settings
configure the admin set:

```hocon
casper.genesis-block-data {
  identity-registry-admin-public-keys = [ "04…", "04…", "04…" ]  # uncompressed secp256k1, hex
  identity-registry-admin-quorum = 2
}
```

The node validates these settings at startup. The quorum must be between 1
and the number of keys, and the keys must be distinct. The values are fixed
at genesis. A change needs a new genesis.

## Names and records

A name is the blake2b-256 hash of the registration payload. The payload
contains the public key and a 32-byte nonce, so one key can back more than
one identity. A replay of the same registration gives the same name.

Each name maps to one record:

```text
{ "current": ByteArray(65),        // the current public key
  "seq":     Int,                  // increments on each key change
  "history": [                     // append-only, oldest first
    { "pk": ByteArray(65), "op": "register" | "rotate" | "admin_relink",
      "seq": Int, "blockHeight": Int,
      "admin_evidence"?: ByteArray, "admin_signers"?: [ByteArray(65)] }, … ] }
```

`blockHeight` comes from `rho:block:data` when the operation runs. It is
never a value from the payload. No operation removes a name or a history
entry.

The records live in a system `TreeHashMap` of depth 3. Writes to different
names do not serialize. Registration takes a global lock as a workaround for
a merge race on new leaves.

## Payloads and signatures

The client and the contract build the same payload bytes from the call
arguments. Every signature is a DER-encoded secp256k1 signature over
`blake2b256(payload)`.

| Operation | Payload |
|---|---|
| `register` | `0x01 ‖ pk(65) ‖ nonce(32)` |
| `rotate` | `0x02 ‖ name(32) ‖ new_pk(65) ‖ seq.toByteArray()` |
| `admin_relink` | `0x03 ‖ name(32) ‖ new_pk(65) ‖ seq.toByteArray() ‖ admin_evidence` |

## Operations

Each operation is a contract on `@[registry, "<op>"]`. Each reply is a tuple,
`("ok", value)` or `("err", reason)`.

| Operation | Arguments | Signed by | Success reply | Error reasons |
|---|---|---|---|---|
| `register` | `pk, nonce, sig, ret` | `pk` | `("ok", name)`. A replay also returns the name. | `bad_signature` |
| `rotate` | `name, newPk, seq, sig, newSig, ret` | the current key (`sig`) and the new key (`newSig`) | `("ok", seq + 1)` | `unknown_name`, `seq_mismatch`, `bad_signature` |
| `admin_relink` | `name, newPk, seq, adminEvidence, adminSigs, newSig, ret` | a quorum of admins (`adminSigs`) and the new key (`newSig`) | `("ok", seq + 1)` | `unknown_name`, `seq_mismatch`, `bad_signature` |
| `resolve_current` | `name, ret` | — | `("ok", {"pk", "seq", "blockHeight"})` | `unknown_name` |
| `resolve_history` | `name, ret` | — | `("ok", [entry, …])` | `unknown_name` |

Notes:

- `seq` must equal the current `seq` of the record. This check stops a
  replayed or out-of-date key change.
- The new key signs `rotate` and `admin_relink` too. This signature proves
  that the submitter holds the new key.
- `adminSigs` is a list of `(adminPk, sig)` pairs. The contract counts each
  admin key that is in the admin set and whose signature verifies. A repeated
  key counts once. The call fails if fewer than the quorum count.
- `adminEvidence` is stored as given. The contract does not validate it.
- `resolve_current` returns the height of the registration, not of the
  last key change.

A minimal Rholang call looks like this:

```rholang
new ret, lookup(`rho:registry:lookup`), registryCh in {
  lookup!(`rho:system:identityRegistry`, *registryCh) |
  for (@(_, registry) <- registryCh) {
    @[registry, "resolve_current"]!("<name hex>".hexToBytes(), *ret)
  }
}
```

