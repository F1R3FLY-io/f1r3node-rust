# Cost-accounting concurrency checks

This crate runs bounded Loom models without the full node dependency graph.
The cost-accounting gate is [`scripts/check-cost-accounted-rho-loom.sh`](../../../scripts/check-cost-accounted-rho-loom.sh).
Its test inventory is the set of Rust files in [`tests/`](tests), and the gate's source-import checks identify the production modules used by those tests.

The native history tests in [`rspace++/tests/native_history_loom.rs`](../../../rspace++/tests/native_history_loom.rs) exercise the borrowed reader with shared reservation credit. The native cache and source tests in the same directory check that payload preparation and source construction reserve work before completion. These tests use a Loom reservation meter; they do not instrument every allocator or backend lock.

Several tests import production transitions directly:

| Test | Production boundary |
| --- | --- |
| [`loom_native_authority_sparse_ledger.rs`](tests/loom_native_authority_sparse_ledger.rs) | Per-key authority debit updates |
| [`loom_native_replay_ledger.rs`](tests/loom_native_replay_ledger.rs) | Occurrence-ledger reservation, publication, and rollback |
| [`loom_native_replay_dependencies.rs`](tests/loom_native_replay_dependencies.rs) | Dependency readiness and checkpoint validation |
| [`loom_native_publication_closure.rs`](tests/loom_native_publication_closure.rs) | Native-session publication guard and closure ordering |
| [`loom_production_sparse_transaction.rs`](tests/loom_production_sparse_transaction.rs) | Shared sparse-store transaction staging and guard lifetime |

The remaining tests model smaller concurrency contracts, including settlement ownership, funding cursors, host-work admission, deterministic reduction, and replay scheduling. Their assertions establish the modeled interleavings only. Native and interpreter tests check the corresponding integration boundaries.

Casper-specific production-import tests and the finalization, recovery, and buffer test implementations are retained on `feature/casper-cost-accounting-completion`. They are not part of this branch's cost-accounting Loom gate.
