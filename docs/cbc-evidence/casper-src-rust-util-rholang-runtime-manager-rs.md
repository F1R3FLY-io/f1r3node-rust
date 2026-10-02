# CbC evidence: replay diagnostics

Status: discharged. The named maintainer accepted this record at revision `68312594d` on 2026-10-01 (PR #441, comment 5936251202). The artifact bytes changed on 2026-10-02 through the dev merge `369dbfcc4` (`spawn_runtime` result type). The branch change is unchanged, and the acceptance stands for it.

The diagnostic timers must preserve replay results, cache decisions, semaphore ownership, mergeable-channel persistence, and typed errors.

The regression tests provide bounded evidence. No formal proof or waiver covers this change.

The earlier embedded verifier returned exit code 3 because Verus was unavailable. This refresh did not rerun formal verification.

A Rust proof specification and implementation connection remain necessary.

The [work log](../work-logs/task-soak-finalization-attribution-2026-09-17.md) records the test commands and verification limits.

```json
{
  "artifact": {
    "path": "casper/src/rust/util/rholang/runtime_manager.rs",
    "commit": "42b990dfde6c2dfe59f25a6463fd1709a741039e",
    "id": "casper-src-rust-util-rholang-runtime-manager-rs",
    "sha256": "dc895a96ad3e54d246749e3e1a8476e5ad15760233108577daa3e8a99f66ff9b",
    "working_tree": false,
    "commit_is_base": false,
    "accepted_revision": "68312594d6bb48ab6b57f95eb6e03bf7371eb818",
    "sha256_at_acceptance": "70e0c44b642b35aa0470a021a16bcda3f456ffeafecffd6bdebe627a272f7809"
  },
  "claim": "Diagnostic timers preserve replay results, cache decisions, semaphore ownership, mergeable-channel persistence, and typed errors.",
  "adapter": "embedded",
  "status": "discharged",
  "acceptance": {
    "accepted_by": "jltatbeach",
    "record": "https://github.com/F1R3FLY-io/f1r3node-rust/pull/441#issuecomment-5936251202",
    "revision": "68312594d6bb48ab6b57f95eb6e03bf7371eb818",
    "reviewed_at": "2026-10-01T16:54:31Z",
    "statement": "I accept casper-src-rust-validate-rs (29dc4691…768) and casper-src-rust-util-rholang-runtime-manager-rs (70e0c44b…809) at 68312594d on the evidence in task-soak-finalization-attribution-2026-09-17.md."
  },
  "evidence": {
    "kind": "bounded-regression-tests",
    "ref": "docs/work-logs/task-soak-finalization-attribution-2026-09-17.md",
    "local_report": "target/soak-attribution-evidence/7f0ae8182-20260919-01/report.json",
    "local_report_sha256": "63f8934c369271eecf104438594a7b4484a10b55174306bc2a4fcbffa29e52d1",
    "counterexample": null,
    "detail": "Formal verification is unavailable. Regression tests do not discharge unrestricted semantic equivalence."
  },
  "waiver": null,
  "verified_at": "2026-10-02T14:33:57+00:00",
  "previous_record": {
    "path": "docs/cbc-evidence/casper-src-rust-util-rholang-runtime-manager-rs.md",
    "sha256": "4641696e0c186799ec8c4e41dbb1095c1649dfb6f7a83a6e61d78d3df5e06652",
    "status": "discharged",
    "artifact_sha256": "70e0c44b642b35aa0470a021a16bcda3f456ffeafecffd6bdebe627a272f7809"
  },
  "refresh": {
    "date": "2026-10-02",
    "reason": "The dev merge 369dbfcc4 changed spawn_runtime and spawn_replay_runtime to return Result<RhoRuntimeImpl, CasperError> and made mergeable_tags crate-private. The branch diagnostic timers are unchanged.",
    "delta_source": "dev fbeb927f8..369dbfcc4 (PR #469, PR #457)",
    "branch_bytes_unchanged": true,
    "evidence": {
      "cargo check --locked -p casper -p node --tests": "ok",
      "cargo test -p casper --test mod runtime_manager_test": "36 passed",
      "cargo test -p casper --test mod repeat_deploy": "24 passed",
      "cargo test -p casper --lib rust::util::rholang::runtime_manager::tests": "5 passed"
    },
    "acceptance_scope": "The acceptance of 2026-10-01 covers the branch change. The merged dev bytes are outside the branch scope and carry their own review in PR #469 and PR #457."
  }
}
```
