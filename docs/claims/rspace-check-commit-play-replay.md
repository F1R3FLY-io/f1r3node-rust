# Claim: `check_commit` guard parity between play and replay

```yaml
claim_id: CLAIM-RSPACE-001
artifacts:
  - rspace++/src/rspace/rspace/ops_consume.rs      # play consume commit gate
  - rspace++/src/rspace/rspace/ops_produce.rs      # play produce matcher driver
  - rspace++/src/rspace/rspace/ops_install.rs      # play install (known deviation D1)
  - rspace++/src/rspace/space_matcher.rs           # shared extract_first_match guard gate
  - rspace++/src/rspace/match.rs                   # Match::check_commit contract
  - rspace++/src/rspace/replay_rspace.rs           # replay consume/produce paths
  - rholang/src/rust/interpreter/matcher/match.rs  # production guard implementation
status: mechanized
adapter: agentic
mechanization: formal/rocq/rspace_guards   # scripts/check-rspace-guards-ALL.sh
references:
  - docs/plans/where-clauses-and-match-guards-2026-04-29.md   # plan §7.12
```

## Context

`Match::check_commit` (`match.rs:24`, default `true`) is the cross-channel
commit veto behind Rholang `where`-clause guards. The production
implementation (`rholang/.../matcher/match.rs:75`) evaluates the guard
expression against the combined cross-bind variables through rho-pure-eval
(plan §7.12). A COMM must form only when the spatial match succeeds AND the
guard passes. Replay must reproduce play's COMM set exactly, or validators
diverge on state roots.

## Claim statements

**C1 — Play-side completeness.** Every code path in the play space that can
form a COMM consults `check_commit` before committing:

- Consume side: `locked_consume` computes `commit_ok` from
  `matcher.check_commit` after `extract_data_candidates` succeeds
  (`ops_consume.rs:79`); a vetoed match stores the waiting continuation,
  identical to a spatial miss.
- Produce side: `run_matcher_for_channels` delegates to
  `extract_first_match`, which evaluates `check_commit` per candidate
  continuation and rolls back plus continues on veto
  (`space_matcher.rs:161`).

**C2 — Replay equivalence.** For any event log that play produced, replay
reaches the same COMM set and the same final state:

- Replay consume commits only COMMs recorded in the log: candidate data is
  pre-filtered to a recorded COMM's produces (`run_matcher_consume`,
  `replay_rspace.rs:787-811`), so play's guard verdict is encoded in the
  log and replay does not need to re-evaluate it on the consume side.
- Replay produce re-evaluates the guard: its `run_matcher_for_channels`
  (`replay_rspace.rs:1383-1403`) uses the same `extract_first_match`, so
  the guard runs again during replay. Equivalence therefore requires C3.

**C3 — Guard determinism.** `check_commit` is a pure function of the
continuation's guard expression and the matched data: no side effects, no
dependence on node-local state, randomness, or time. `guard_passes`
(rho-pure-eval) returns the same verdict for the same inputs at play time
and at replay time, and both call sites present the matched data in the
same receive-bind order.

## Known deviations (in scope for discharge or explicit waiver)

**D1 — Install path ignores the guard.** `locked_install_internal` (play,
`ops_install.rs`; replay twin at `replay_rspace.rs:1271`) runs
`extract_data_candidates` without `check_commit` and treats any spatial
match as the error "Installing can be done only on startup". A spatial
match whose guard would fail still errors, although guard semantics say it
is not a match. Consequence is conservative (startup failure, not state
divergence), but the deviation must be discharged as acceptable or fixed.

**D2 — Consume-side veto asymmetry between play and replay.** Play evaluates
the guard on the consume side (`commit_ok`); replay does not. This is safe
only while C2's log-gating argument holds: replay consume must be unable to
commit any COMM absent from the log. Any future change that lets replay
consume match un-rigged data breaks the claim silently. The discharge must
state this dependency explicitly.

## Verification obligations

1. Enumerate every COMM-forming site in `RSpace` and `ReplayRSpace`; show
   each either consults `check_commit` or is log-gated (C1, C2).
2. Show `guard_passes` is deterministic and side-effect-free over its
   inputs, and that both `matched` constructions (ops_consume.rs:74-77 and
   space_matcher.rs:157-160) present bind results in the same order (C3).
3. Exhibit the log-gating invariant for replay consume: candidates are
   drawn only from data matching a recorded COMM (C2/D2).
4. Record D1 as fixed or as a waived, bounded deviation.

## Mechanization

The Rocq development `formal/rocq/rspace_guards` (2 modules, zero
`Admitted`, no custom axioms) mechanizes the claim over an abstract model
of COMM formation. Capstones, each "Closed under the global context"
(gate: `scripts/check-rspace-guards-ALL.sh`):

| Capstone | Discharges |
|----------|------------|
| `rspace_first_match_guard` | C1 produce site (guard veto = spatial miss) |
| `rspace_play_guard_complete` | C1 (every play COMM passed its guard) |
| `rspace_replay_log_gated` | D2 (replay commits only logged op ids) |
| `rspace_replay_equivalent` | C2 (replay of a play log = play COMMs) |
| `rspace_replay_guard_complete` | C2∘C1 (replayed COMMs all passed guards) |

C3 is by construction (one shared `guard_eval` premise serves play and
replay; guard purity and bind-order agreement are the seam premises Rust
enforces). D1 is by construction of the model (`OpInstall` has no commit
branch), matching the code; the code-level install deviation (guard not
consulted before the "installing only on startup" error) remains open as a
bounded, conservative behavior.

## Acceptance

The claim is discharged when obligations 1-3 hold with cited evidence and
obligation 4 is resolved, recorded in `docs/cbc-evidence/` with this file
as the claim reference. A counterexample to any obligation refutes the
claim and must link the failing site.
