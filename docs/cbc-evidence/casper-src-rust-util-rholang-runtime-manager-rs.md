# CbC evidence: replay diagnostics

Status: discharged. The named maintainer accepted this record at revision `68312594d` on 2026-10-01 (PR #441, comment 5936251202). The artifact bytes are unchanged since the record commit `7f0ae8182`.

The diagnostic timers must preserve replay results, cache decisions, semaphore ownership, mergeable-channel persistence, and typed errors.

The regression tests provide bounded evidence. No formal proof or waiver covers this change.

The earlier embedded verifier returned exit code 3 because Verus was unavailable. This refresh did not rerun formal verification.

A Rust proof specification and implementation connection remain necessary.

The [work log](../work-logs/task-soak-finalization-attribution-2026-09-17.md) records the test commands and verification limits.

```json
{
  "artifact": {
    "path": "casper/src/rust/util/rholang/runtime_manager.rs",
    "commit": "7f0ae8182d76005590976b8989f0173c49fe1d33",
    "id": "casper-src-rust-util-rholang-runtime-manager-rs",
    "sha256": "70e0c44b642b35aa0470a021a16bcda3f456ffeafecffd6bdebe627a272f7809",
    "working_tree": false,
    "commit_is_base": true,
    "accepted_revision": "68312594d6bb48ab6b57f95eb6e03bf7371eb818"
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
  "verified_at": "2026-10-01T17:09:21+00:00",
  "previous_record": {
    "path": "docs/cbc-evidence/casper-src-rust-util-rholang-runtime-manager-rs.md",
    "sha256": "902e2f75e925aac8be3f506975151fc530a46ffb02c6403e90051923882253e6",
    "status": "pending"
  }
}
```
