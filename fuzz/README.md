# Slashing fuzz targets

These targets expand the slashing search horizon with coverage-guided
fuzzing. They are seedable from Sage/Hypothesis fixtures and are not proof
authority; crashes must be minimized, replayed deterministically, classified
in `docs/casper/theory/slashing/slashing-traceability.md`, and promoted to Rocq/TLA+
only after review.

Run smoke checks:

```sh
cargo install cargo-fuzz
ASAN_OPTIONS=detect_leaks=0 RUSTFLAGS="-C target-feature=+aes,+sse2" cargo fuzz run slashing_arithmetic -- -runs=10000 -max_len=64
ASAN_OPTIONS=detect_leaks=0 RUSTFLAGS="-C target-feature=+aes,+sse2" cargo fuzz run slash_deploy_roundtrip -- -runs=10000 -max_len=512
ASAN_OPTIONS=detect_leaks=0 RUSTFLAGS="-C target-feature=+aes,+sse2" cargo fuzz run block_message_roundtrip -- -runs=10000 -max_len=4096
ASAN_OPTIONS=detect_leaks=0 RUSTFLAGS="-C target-feature=+aes,+sse2" cargo fuzz run slash_authorization_paths -- -runs=10000 -max_len=2048
ASAN_OPTIONS=detect_leaks=0 RUSTFLAGS="-C target-feature=+aes,+sse2" cargo fuzz run equivocation_detector_paths -- -runs=10000 -max_len=2048
ASAN_OPTIONS=detect_leaks=0 RUSTFLAGS="-C target-feature=+aes,+sse2" cargo fuzz run slash_lifecycle_trace -- -runs=10000 -max_len=4096
```

Run longer campaigns:

```sh
ASAN_OPTIONS=detect_leaks=0 RUSTFLAGS="-C target-feature=+aes,+sse2" cargo fuzz run slashing_arithmetic -- -max_len=64
ASAN_OPTIONS=detect_leaks=0 RUSTFLAGS="-C target-feature=+aes,+sse2" cargo fuzz run slash_deploy_roundtrip -- -max_len=512
ASAN_OPTIONS=detect_leaks=0 RUSTFLAGS="-C target-feature=+aes,+sse2" cargo fuzz run block_message_roundtrip -- -max_len=4096
ASAN_OPTIONS=detect_leaks=0 RUSTFLAGS="-C target-feature=+aes,+sse2" cargo fuzz run slash_authorization_paths -- -max_len=2048
ASAN_OPTIONS=detect_leaks=0 RUSTFLAGS="-C target-feature=+aes,+sse2" cargo fuzz run equivocation_detector_paths -- -max_len=2048
ASAN_OPTIONS=detect_leaks=0 RUSTFLAGS="-C target-feature=+aes,+sse2" cargo fuzz run slash_lifecycle_trace -- -max_len=4096
```

`slashing_arithmetic` probes the sequence-number helpers in
`casper/src/rust/slashing_authorization.rs` (`checked_base_seq`,
`checked_next_seq`) against the libFuzzer u64/i32 input space. It asserts
that they return `None` on every overflow and underflow path — the kani
proofs cover the same predicates over a bounded domain; this fuzzer
extends the search over the unbounded domain.

`slash_deploy_roundtrip` is a proto idempotency check for the
`SystemDeployData::Slash` payload only: a slash whose evidence is two
arbitrary block commitments converts to proto and back unchanged.

`block_message_roundtrip` is a full `BlockMessage` proto idempotency
check: `from_proto ∘ to_proto ∘ from_proto = from_proto`. It is the
catch-all for proto field-encoding regressions across the entire block
schema, not just the slashing subset.

`slash_authorization_paths` builds snapshots holding equivocations of
really signed blocks, and blocks carrying fuzzed slashes. It checks that
production candidate selection and received-slash validation agree with an
independent oracle: issuer, shard and chain, one sender at one sequence
number, two distinct blocks ordered by hash, valid signatures, positive
stake, and one slash per offender.

`equivocation_detector_paths` fuzzes the equivocation verdict: the sibling
is the lowest-hash other block held at the sender's sequence number, and
the verdict is admissible or ignorable by requested-dependency status.

`slash_lifecycle_trace` fuzzes the production path from authorized candidate
selection to received `SlashDeploy` validation and confirms that a block
slashing one offender twice is rejected.
