# CbC evidence: replay diagnostics

Status: pending.

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
    "working_tree": false
  },
  "claim": "Diagnostic timers preserve replay results, cache decisions, semaphore ownership, mergeable-channel persistence, and typed errors.",
  "adapter": "embedded",
  "status": "pending",
  "evidence": {
    "kind": "bounded-regression-tests",
    "ref": "docs/work-logs/task-soak-finalization-attribution-2026-09-17.md",
    "local_report": "target/soak-attribution-evidence/7f0ae8182-20260919-01/report.json",
    "local_report_sha256": "63f8934c369271eecf104438594a7b4484a10b55174306bc2a4fcbffa29e52d1",
    "counterexample": null,
    "detail": "Formal verification is unavailable. Regression tests do not discharge unrestricted semantic equivalence."
  },
  "waiver": null,
  "verified_at": null
}
```
