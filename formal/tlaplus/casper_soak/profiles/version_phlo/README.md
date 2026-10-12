# Protocol and Phlo Profile

This directory contains the bounded classification model for [Claim 007](../../../../../docs/claims/casper-soak-version-phlo.md). The executable profile uses controlled transcripts, not a running node.

## Contract

Casper protocol 7 and accounting authority 8 have separate context labels. A separate proposed protocol version selects unsupported-version fixtures.

The profile preserves `phloLimit` and `phloPrice` in retained envelope bytes, generated requests, and captured measurements. Minimum-price and settlement outcomes use pinned expectations.

The synthetic envelope explicitly selects `controlled-json-v1`. Its digest binds the exact retained bytes. This format does not qualify protobuf encoding, cryptographic signatures, or a live deploy interface.

Each scenario requires one envelope observation, one admission observation, and one settlement observation. Producer sequences and a shared monotonic clock establish their order.

A signed-field mutation requires one applied receipt between envelope capture and admission. The receipt identifies the fixture, deploy, envelope commitment, field, old value, and new value.

Expected rejections can pass a controlled scenario. Unexpected acceptance or a settlement mismatch produces `product_failure`. Missing measurements remain unknown, including missing signed fields.

The classifier preserves independent failures when another measurement is malformed or absent. Conflicting event copies make the evidence invalid. Identical copies count once.

Live requests, post-merge requests, experimental policies, and undefined funding mappings remain blocked. The profile never activates protocol 7 or launches a node.

## Executable bounds

- One scenario and one identified member per invocation.
- At most one signed-field mutation.
- At most 64 transport records.
- At most one MiB per input artifact.
- Nonnegative signed Phlo fields and minimum price through `i64::MAX`.
- Canonical unsigned decimal strings through `u64::MAX` for captured settlement amounts, versions, sequences, and times.

The profile compares settlement values. It does not calculate node accounting or prove that pinned expectations implement economic policy.

Filesystem operations assume cooperating writers and available storage. The profile does not prove crash durability or containment against a hostile process.

## Model boundary

The model has two scenarios and three abstract observation steps per scenario. Boolean values represent version separation, signed-field capture, and an independently observed refund mismatch.

The model assumes valid provenance, ordering, capabilities, other measurements, and fault receipts. Executable fixtures test these additional boundaries separately.

| Property | Defect knob | Executable fixture |
| --- | --- | --- |
| `VersionLabelsSeparate` | `ConflateAuthorityVersions` | `phlo_version_labels` |
| `BothPhloFieldsCaptured` | `OmitPhloPrice` | `phlo_signed_field_missing` |
| `SettlementOutcomeClassified` | `IgnoreRefundMismatch` | `phlo_refund_mismatch` |

The clean model must complete with exit 0. Each negative control must produce exit 12 and its exact named invariant violation.

## Verification

Run the exact-inventory check from the repository root:

```bash
TLA_TOOLS_JAR=/path/to/tla2tools-1.7.4.jar \
  bash scripts/casper-soak/check-version-phlo.sh /path/to/new-evidence-directory
```

The runner checks 15 Rust tests, 102 fixture cases, 103 invocations, and four model controls. It verifies source stability and retains failed summaries after errors or interruption.

The required JAR SHA-256 is `936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88`.

Each invocation retains its input snapshot, expected verdict, actual verdict, exit status, and immutable report artifacts. The binary exposes `identity`, `run`, and `models` commands.

The new CI workflow has no accepted workflow tag. Hosted execution, isolated verification, final evidence review, binding acceptance, and claim discharge remain pending.
