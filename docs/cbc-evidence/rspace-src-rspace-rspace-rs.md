# CbC Evidence: rspace++/src/rspace/rspace.rs

- **Status:** waived
- **Adapter:** waiver
- **Commit:** dad806fa1
- **Verified:** 2026-08-25T23:25:21Z

Claim:

> (none)

```json
{
  "artifact": {
    "path": "rspace++/src/rspace/rspace.rs",
    "commit": "dad806fa1",
    "id": "rspace-src-rspace-rspace-rs"
  },
  "claim": "",
  "adapter": "waiver",
  "status": "waived",
  "evidence": {
    "kind": "waiver",
    "ref": null,
    "counterexample": null,
    "detail": "explicit waiver"
  },
  "waiver": {
    "reason": "CLAIM-RSPACE-001 mechanized: formal/rocq/rspace_guards proves the guard-parity capstones (5x Closed under the global context; gate scripts/check-rspace-guards-ALL.sh green). Waiver retained because the CbC adapters cannot run a prover against .rs artifacts directly; the Rocq development plus the claim's cited call sites (ops_consume.rs:79, space_matcher.rs:161, replay_rspace.rs:787-811,1383-1403) are the evidence. Residual: D1 install-path guard deviation, bounded and conservative",
    "by": "1573697+jeffrey-l-turner@users.noreply.github.com",
    "at": "2026-08-25T23:25:21Z"
  },
  "verified_at": "2026-08-25T23:25:21Z"
}
```
