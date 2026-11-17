# Identity Registry

The identity registry links secp256k1 keys to stable identity names on a
F1R3FLY shard. A party registers an identity, rotates its key, and reads the
full key history. A quorum of admins can link a new key to an identity whose
current key is lost or compromised.

This crate contains the Rust client library, the `registry_cli` binary, and
the integration tests. The contract itself,
[`IdentityRegistry.rhox`](../casper/src/main/resources/IdentityRegistry.rhox),
ships in the genesis block. [`DECISIONS.md`](DECISIONS.md) records the
design decisions made along the way.

## The contract

### Deployment

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

### Names and records

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

### Payloads and signatures

The client and the contract build the same payload bytes from the call
arguments. Every signature is a DER-encoded secp256k1 signature over
`blake2b256(payload)`.

| Operation | Payload |
|---|---|
| `register` | `0x01 ‖ pk(65) ‖ nonce(32)` |
| `rotate` | `0x02 ‖ name(32) ‖ new_pk(65) ‖ seq.toByteArray()` |
| `admin_relink` | `0x03 ‖ name(32) ‖ new_pk(65) ‖ seq.toByteArray() ‖ admin_evidence` |

### Operations

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

## How the client calls the contract

The client sends every operation as a deploy, reads included. A read costs
a deploy and a block.

```mermaid
sequenceDiagram
    participant CLI as registry_cli
    participant Node as Validator (internal gRPC)
    participant IR as Identity registry
    CLI->>CLI: build payload, sign with identity or admin keys
    CLI->>Node: deploy term (signed with F1R3FLY_SIGNING_KEY)
    CLI->>Node: propose
    Node->>IR: run term, call @[registry, op]
    IR-->>Node: reply sent to a unique reply slot
    Node-->>CLI: block hash
    CLI->>Node: get_data_at_name(reply slot, block hash)
    Node-->>CLI: ("ok", value) or ("err", reason)
```

There are two kinds of keys:

- The **deploy key**, `F1R3FLY_SIGNING_KEY`, signs and pays for the deploy.
  It needs a REV balance. It has no authority over any identity.
- The **identity keys and admin keys** sign the payloads. They do not
  necessarily sign a deploy.

The client proposes the block itself. The node must accept a propose
request on its internal gRPC port.

## Using the CLI

### Prerequisites

1. Start a local shard with the 2-of-3 admin quorum:

   ```bash
   make firefly
   ```

   This target builds the `f1r3fly-rust:local` image if it does not exist,
   and starts `docker/shard.yml` with the `docker/quorum.yml` overlay. The
   admin keys are the bootstrap, validator1, and validator2 dev keys from
   `docker/.env.example`.

2. Set a deploy key that has a REV balance on the shard:

   ```bash
   export F1R3FLY_SIGNING_KEY=<32-byte hex secret key>
   ```

Use `make clear-firefly` to stop the shard and remove its data. Do this when
the proposer refuses new blocks because too many blocks are not finalized.

### Global options

| Option | Default | Purpose |
|---|---|---|
| `--node-host` | `127.0.0.1` | Validator host |
| `--grpc-port` | `40422` | Validator internal gRPC port |
| `--registry-uri` (or `IDENTITY_REGISTRY_URI`) | `rho:system:identityRegistry` | Registry to call |

Put global options before the subcommand. All byte values are hex. Secret
keys and names have 32 bytes. Public keys have 65 bytes (uncompressed).

The examples use `cargo run`. Each subcommand also has a Makefile target,
shown below each example.

### keygen

Generate a keypair. This command does not contact the node.

```bash
cargo run --bin registry_cli -- keygen
# sk: <32-byte hex>
# pk: <65-byte hex>
```

`make keygen`

### register

Register an identity. If you omit `--sk` or `--nonce`, the CLI generates a
random value.

```bash
cargo run --bin registry_cli -- register --sk <SK> --nonce <NONCE>
# identity_sk (keep safe): …
# identity_pk:             …
# nonce:                   …
# name:                    <32-byte hex>
```

`make register SK=<SK> NONCE=<NONCE>`

> [!WARNING]
> Keep the secret key, the nonce, and the name. You need the secret key for
> each rotation. You cannot recover the name without the public key and the
> nonce.

### resolve-current

Read the current key, the `seq`, and the registration height.

```bash
cargo run --bin registry_cli -- resolve-current --name <NAME>
# pk:           …
# seq:          0
# block_height: 17
```

`make resolve-current NAME=<NAME>`

### rotate

Link a new key to the identity. The current key and the new key both sign.
`--seq` is the current `seq` from `resolve-current`.

```bash
cargo run --bin registry_cli -- rotate \
  --name <NAME> --current-sk <CURRENT_SK> --new-sk <NEW_SK> --seq 0
# new_seq: 1
```

`make rotate NAME=<NAME> CURRENT_SK=<CURRENT_SK> NEW_SK=<NEW_SK> SEQ=0`

### admin-relink

Link a new key when the current key cannot sign. The call needs signatures
from a quorum of distinct admins, and a signature from the new key. Give the
admin signatures in one of two ways:

- `--admin-sk <ADMIN_SK>`: the CLI signs with the admin key. Repeat the
  option for each admin.
- `--admin-sig <ADMIN_PK>:<SIG>`: a signature that an admin made offline
  with `admin-sign`. Repeat the option for each admin.

You can mix the two forms. `--evidence` is hex bytes, for example the hex of
a ticket reference. The contract stores it as given.

```bash
cargo run --bin registry_cli -- admin-relink \
  --name <NAME> --new-sk <NEW_SK> --seq 1 --evidence <EVIDENCE_HEX> \
  --admin-sk <ADMIN_SK_1> --admin-sk <ADMIN_SK_2>
# new_seq: 2
```

`make admin-relink NAME=<NAME> NEW_SK=<NEW_SK> SEQ=1 EVIDENCE=<EVIDENCE_HEX> ADMIN_SKS="<ADMIN_SK_1> <ADMIN_SK_2>"`

### admin-sign

Make one admin signature offline, so the admin does not share a secret key.
This command does not contact the node and does not read
`F1R3FLY_SIGNING_KEY`. All admins must sign the same `name`, `new-pk`,
`seq`, and `evidence`.

```bash
cargo run --bin registry_cli -- admin-sign \
  --admin-sk <ADMIN_SK> --name <NAME> --new-pk <NEW_PK> --seq 1 --evidence <EVIDENCE_HEX>
# <ADMIN_PK>:<SIG>
```

`make admin-sign ADMIN_SK=<ADMIN_SK> NAME=<NAME> NEW_PK=<NEW_PK> SEQ=1 EVIDENCE=<EVIDENCE_HEX>`

Then give each output line to `admin-relink`:

```bash
cargo run --bin registry_cli -- admin-relink \
  --name <NAME> --new-sk <NEW_SK> --seq 1 --evidence <EVIDENCE_HEX> \
  --admin-sig <ADMIN_PK_1>:<SIG_1> --admin-sig <ADMIN_PK_2>:<SIG_2>
```

`make admin-relink … ADMIN_SIGS="<ADMIN_PK_1>:<SIG_1> <ADMIN_PK_2>:<SIG_2>"`

### resolve-history

Read every key that was linked to the identity, oldest first.

```bash
cargo run --bin registry_cli -- resolve-history --name <NAME>
# entry 0:
#   op:           register
#   seq:          0
#   pk:           …
#   block_height: 17
# entry 1:
#   op:           admin_relink
#   …
#   admin_evidence: …
#   admin_signer: …
```

`make resolve-history NAME=<NAME>`

### Errors

The CLI prints `error: …` and exits with status 1 when an operation fails.

| Message | Cause |
|---|---|
| `F1R3FLY_SIGNING_KEY must be set` | The deploy key is not set. |
| `contract rejected the request: bad_signature` | A signature does not verify, or the admin signatures do not reach the quorum. |
| `contract rejected the request: seq_mismatch` | `--seq` is not the current `seq`. Run `resolve-current`. |
| `contract rejected the request: unknown_name` | The name is not registered. |
| `F1R3FLY node error: propose skipped: …` | The node did not create a block. See `make clear-firefly`. |
| `could not parse contract response: no data at reply slot` | The block has no reply. The deploy can be out of phlo or it can have failed. |

## Tests

| Suite | Command | Needs |
|---|---|---|
| Integration tests | `make test` | A running `make firefly` shard and `F1R3FLY_SIGNING_KEY` |
| RhoSpec suite (`casper/src/test/resources/IdentityRegistryTest.rho`) | `cargo test -p casper --release --test mod identity_registry_spec` | Nothing. It runs on a genesis state in memory. |

Both suites check the acceptance criteria REG-AC1 to REG-AC9, except
REG-AC7. Only the integration tests run on a node with real blocks.
