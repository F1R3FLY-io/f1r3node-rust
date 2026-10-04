# Cordial native admission v2

Status: local, uncommitted correction approved for implementation on 2026-10-02.
This is a native regression gate, not approval to deploy a Cordial node.

## Why this changes

The pinned upstream revision accepted empty signatures when signature checking was enabled.
It also sorted predecessors only by content hash. Two validators can sign the same content,
so their distinct identities can have equal content hashes. HashSet iteration order then
changed the serialized bytes and the resulting block hash.

Finally, strict validation required every receiver-known tip. For example, validators A
and B can both build valid round-one blocks over the same round-zero quorum. Receiving A
must not invalidate B merely because B did not reference A. A local proposal can be
required to use newly learned tips; a previously signed received block cannot.

## Signed content format

`CONTENT_HASH_VERSION` is 2. The default content hash is Blake2b-256 over:

1. The exact bytes `cordial-miners:block-content:v2` followed by a zero byte.
2. Payload byte length as an unsigned, little-endian 64-bit integer, then payload bytes.
3. Predecessor count in the same integer encoding.
4. Each predecessor, sorted lexicographically by the full tuple
   `(content_hash, creator_bytes, signature_bytes)`.
5. For each predecessor: its 32 hash bytes, creator byte length and bytes, then signature
   byte length and bytes. Lengths use the same integer encoding.

The current default signature remains Secp256k1 over the content hash, encoded as DER.
The canonical-vector regression pins an independently computed expected hash.

**Every hash changes**, including initial blocks with no predecessors. This is a new-chain
format, not an in-place database migration. There is no fallback to the legacy hash.
Existing blocks cannot be rehashed without invalidating their signatures and descendants.
Leave old data directories intact and use a distinct, explicitly configured v2 chain.

The native domain distinguishes content format versions. It does **not** authenticate chain identity or membership by itself.
The new `cordial-consensus` integration adds signed chain context and a versioned packet format.
See [the integration profile](INTEGRATION-PROFILE.md) for its implemented boundaries and remaining production gates.

## Which function to call

| Entry point | Purpose |
| --- | --- |
| `validate_received_block` | Mandatory native received-block checks. No flags can disable authentication. Does not mutate state. |
| `validated_received_insert` | Same checks, followed by idempotent in-memory insertion. Does not provide durable storage. |
| `validate_block` / `validated_insert` with `ValidationConfig` | Retained local-policy and structural-test compatibility surface. Not the production ingress contract. |
| Raw `Blocklace::insert` | Low-level insertion with a supplied verifier. Not a replacement for full received-block admission. |

Received admission requires:

- Matching v2 content hash and a nonempty, valid Secp256k1 signature.
- A creator with positive weight in the caller-supplied authoritative membership view.
- Complete causal history already admitted under the same profile. Missing history returns
  `MissingPredecessors` without mutation and can be retried later.
- For a noninitial block, distinct creators of direct predecessors at the immediately previous
  round contribute strictly more than two thirds of the configured weight. Repeated branches
  of one creator count once. Older rounds do not count toward that quorum.
- No mutually incomparable blocks by the candidate creator inside its own declared history.

Native round numbers begin at zero. The first-round quorum rule uses the existing weighted
integration's threshold. This is not a claim that the paper proves this entire weighted profile.
The distinction between locally known tips and a block's own causal history follows the
[Cordial Miners paper, Algorithm 1 and Definition 25](https://drops.dagstuhl.de/storage/00lipics/lipics-vol281-disc2023/LIPIcs.DISC.2023.26/LIPIcs.DISC.2023.26.pdf).

Initial blocks are authenticated but have no predecessor-quorum requirement. The adapter
must constrain genesis and membership. This API alone is not a genesis ceremony.

## Equivocation behavior

Two validly signed, incompatible branches can both enter the blocklace. The second does not
become invalid solely because the receiver saw the first. Native approval can then observe
the conflict and refuse approval of either branch. A candidate that merges its **own**
incompatible histories is rejected. An honest validator can acknowledge another validator's
conflict without being rejected for that conflict.

This change does not introduce automatic ejection, weight changes, or slashing. Those remain
separate protocol/accountability decisions. Native approval, finality, and tau code is unchanged.

## Explicit limitations before production registration

- A checkpoint-pruned view returns `UnsupportedPrunedHistory`. It cannot prove full creator
  comparability with this implementation. Keep pruning disabled for this admission profile
  until proof-aware recovery is implemented and tested.
- Stored ancestors must already be validated. A raw, externally populated Blocklace is not
  a trusted recovery source. Validate replay or authenticate a supported checkpoint.
- The caller supplies authoritative weights. Validator key normalization and duplicate-key
  rejection are required when constructing that view.
- Admission traverses causal history. Packet limits, bounded queues, dependency recovery,
  admission work budgets, and denial-of-service tests remain necessary in the adapter.
- The imported `PendingBlockBuffer` still retries through the older configurable validation
  path. Do not attach it unchanged to production ingress: received-block retry must use the
  mandatory receive policy, including preservation of valid conflicting branches.
- The separate `cordial-consensus` crate now implements durable admission, chain manifests, chain-bound blocks, and persisted native output.
  Its ingress-only adapter uses the shared lifecycle. Production node selection and execution remain incomplete.
  Replayable application delivery still requires consumer acknowledgments and execution recovery.
- The node still selects Casper only. Real Cordial TLS peers, deploy/proposal APIs, late join,
  restart recovery, and failure recovery have not passed a production network suite.

## Verification

Run from the repository root:

```bash
rtk proxy python3 scripts/check_cordial_import.py
rtk cargo test --locked --offline --release -j 1 -p cordial-miners-core --test test_production_admission
rtk cargo run --locked --offline --release -j 1 -p cordial-miners-core --example check_integration_readiness
rtk cargo test --locked --offline --release -j 1 -p cordial-miners-core
rtk cargo test --locked --offline --release -j 1 -p cordial-app-runtime
```

The signed multi-wave regression checks four validators, opposite delivery order, identical
native final leaders and tau output, and an append-only committed prefix. These are in-process
native tests; they do not simulate a completed production adapter or real networking.
