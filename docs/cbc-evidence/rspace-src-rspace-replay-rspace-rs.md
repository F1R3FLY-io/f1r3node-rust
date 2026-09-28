# CbC Evidence: rspace++/src/rspace/replay_rspace.rs

- **Status:** waived
- **Adapter:** waiver
- **Commit:** aa599cb49
- **Verified:** 2026-08-27T01:17:25Z

Claim:

> (none)

```json
{
  "artifact": {
    "path": "rspace++/src/rspace/replay_rspace.rs",
    "commit": "aa599cb49",
    "id": "rspace-src-rspace-replay-rspace-rs"
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
    "reason": "CLAIM-RSPACE-001 mechanized: formal/rocq/rspace_guards proves the guard-parity capstones (5x Closed under the global context; gate scripts/check-rspace-guards-ALL.sh green). This artifact is the replay side of the parity claim: rspace_replay_log_gated (D2) shows replay's log-gated consume path (replay_rspace.rs:787-811) commits only op ids recorded in the play log, exactly substituting for the check_commit verdict it never evaluates, and rspace_replay_guard_complete shows every replayed COMM therefore passed its play-time guard. The replay produce path (replay_rspace.rs:1335-1355) reuses the shared extract_first_match guard gate. Waiver retained because the CbC adapters cannot run a prover against .rs artifacts directly; the Rocq development plus the cited call sites are the evidence. Residual: D1 install-path guard deviation, bounded and conservative",
    "by": "1573697+jeffrey-l-turner@users.noreply.github.com",
    "at": "2026-08-27T01:17:25Z"
  },
  "verified_at": "2026-08-27T01:17:25Z"
}
```
