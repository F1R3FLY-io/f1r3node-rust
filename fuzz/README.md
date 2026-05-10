# Slashing fuzz targets

These targets expand the slashing search horizon with coverage-guided
fuzzing. They are seedable from Sage/Hypothesis fixtures and are not proof
authority; crashes must be minimized, replayed deterministically, classified
in `docs/theory/slashing/slashing-traceability.md`, and promoted to Rocq/TLA+
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

`slash_authorization_paths` builds synthetic `CasperSnapshot` DAG
metadata, invalid-block indices, bond maps, and received SlashDeploy
blocks. It checks that production candidate selection and received
SlashDeploy validation agree with the independent scenario oracle for
current epoch, evidence epoch, issuer, invalid-hash lookup, positive
bond, and duplicate target-key behavior.

`equivocation_detector_paths` fuzzes the public detector boundary for
creator justification, latest-message lookup, requested dependency status,
and admissible-versus-ignorable classification.

`slash_lifecycle_trace` fuzzes the production path from authorized candidate
selection to received `SlashDeploy` validation and confirms that duplicate
slash targets are rejected.
