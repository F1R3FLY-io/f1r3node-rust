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

**C2 — Replay COMM-sequence equivalence.** For any event log that play
produced, replay commits the same COMM sequence:

- Replay consume commits only COMMs recorded in the log: candidate data is
  pre-filtered to a recorded COMM's produces (`run_matcher_consume`,
  `replay_rspace.rs:787-811`), so play's guard verdict is encoded in the
  log and replay does not need to re-evaluate it on the consume side.
- Replay produce re-evaluates the guard: its `run_matcher_for_channels`
  (`replay_rspace.rs:1335-1355`) uses the same `extract_first_match`, so
  the guard runs again during replay. Equivalence therefore requires C3.

Final-STATE equivalence is a consequence of C2 only under the additional
premise that the post-state is a deterministic function of the operation
sequence and its COMM decisions (store determinism). That premise is part
of the wider replay soundness argument, not of this claim, and the
mechanization does not prove it (see Seam premises).

**C3 — Guard determinism (seam premise).** `check_commit` is a pure
function of the continuation's guard expression and the matched data: no
side effects, no dependence on node-local state, randomness, or time.
`guard_passes` (rho-pure-eval) returns the same verdict for the same
inputs at play time and at replay time, and both call sites present the
matched data in the same receive-bind order. C3 is ASSUMED by the
mechanization, not proved: the model shows that IF the guard is one pure
function shared by play and replay, THEN parity holds. The purity and
bind-order obligations remain Rust-side (rho-pure-eval's evaluator and
the two `matched` constructions).

## Known deviations (in scope for discharge or explicit waiver)

**D1 — Install path ignores the guard.** `locked_install_internal` (play,
`ops_install.rs`; replay twin at `replay_rspace.rs:1242`) runs
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

### Seam premises (assumed by the model, enforced by Rust)

The mechanization is an abstract model. The following are premises of the
theorems, deliberately NOT proved, in the same "Rocq assumes what Rust
enforces" style as `fork_choice`'s GuardBridge seams:

1. **Guard purity and bind order (C3).** `guard_eval` is a Section
   Variable — one pure function shared by the play and replay models.
   Rust must enforce that `guard_passes` is deterministic and that both
   `matched` constructions agree on receive-bind order.
2. **Op-identity keying.** The model keys COMMs by op index (`comm_id`),
   standing in for `replay_data`'s hash-keyed map
   (`IOEvent::Produce/Consume` identity). The correspondence between the
   model's index keying and Rust's cryptographic-hash keying — including
   hash-collision freedom — is assumed, not derived.
3. **Log-gate data binding.** The model's replay gate is id-membership in
   the log. Rust's replay consume additionally binds candidate data to
   the recorded COMM's produces (`self.matches(comm, ..)` filtering); the
   model abstracts that filter into the gate and does not verify it.
4. **COMM structure and nondeterminism metadata.** The model's `Comm`
   carries guard + data only. `peeks`, `times_repeated`, and the
   `Produce` nondeterminism fields (`is_deterministic`, `output_value`,
   `failed`) are outside the model's scope.
5. **Store determinism.** State equality is not modeled; theorems speak
   about COMM sequences only (see C2's narrowed statement).

D1 is by construction of the model (`OpInstall` has no commit branch),
matching the code; the code-level install deviation (guard not consulted
before the "installing only on startup" error) remains open as a bounded,
conservative residual — deliberately out of scope for this change, to be
fixed or formally waived in a follow-up.

## Acceptance

`status: mechanized` means obligations 1-3 are mechanized in Rocq; it is
not `discharged`. Discharge is blocked on obligation 4 — D1 must be fixed
or formally waived — which remains open.

The claim is discharged when obligations 1-3 hold with cited evidence and
obligation 4 is resolved, recorded in `docs/cbc-evidence/` with this file
as the claim reference. A counterexample to any obligation refutes the
claim and must link the failing site.
