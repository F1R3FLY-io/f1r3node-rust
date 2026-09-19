# CbC evidence: repeat-deploy diagnostics

Status: pending.

The diagnostic timers must preserve repeat-deploy verdicts, storage reads, retry exemptions, carrier probes, and ancestor scans.

The regression tests provide bounded evidence. No formal proof or waiver covers this change.

The embedded verifier returned exit code 3 because Verus is unavailable. A Rust proof specification and implementation connection also remain necessary.

The [work log](../work-logs/task-soak-finalization-attribution-2026-09-17.md) records the test commands and verification limits.

```json
{
  "artifact": {
    "path": "casper/src/rust/validate.rs",
    "commit": "bc23c8667ebef0f3fb7c3310caf85ce106df25fa",
    "id": "casper-src-rust-validate-rs",
    "working_tree": true
  },
  "claim": "Diagnostic timers preserve repeat-deploy verdicts, storage reads, retry exemptions, carrier probes, and ancestor scans.",
  "adapter": "embedded",
  "status": "pending",
  "evidence": {
    "kind": "bounded-regression-tests",
    "ref": "docs/work-logs/task-soak-finalization-attribution-2026-09-17.md",
    "counterexample": null,
    "detail": "Formal verification is unavailable. Regression tests do not discharge unrestricted semantic equivalence."
  },
  "waiver": null,
  "verified_at": null
}
```
