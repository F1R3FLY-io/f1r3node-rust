# Case studies — index

This directory documents **how each of the sixteen bug fixes was
discovered**, classified, and promoted to mechanized evidence. Each
chapter is a "summary + pointers" walk-through of one bug — what
the witness looked like, which tool produced it, what classification
it received, and which artifacts protect against regression.

The chapters are **methodology-oriented**, not architecture-oriented;
the bug mechanics themselves live in
[`../../design/09-bug-fixes-and-rationale.md`](../../design/09-bug-fixes-and-rationale.md).
This directory is the *companion* that answers *“how did we find
this?”*.

## Index

| #  | Bug                                                                                                              | Discovery technique(s)                                                           |
|----|------------------------------------------------------------------------------------------------------------------|----------------------------------------------------------------------------------|
| 01 | [Bug #1 — IgnorableEquivocation non-slashable (DOS vector)](./01-bug-01-ignorable-equivocation.md)               | Code-walking review of `is_slashable()` taxonomy; Sage differential model        |
| 02 | [Bug #2 — Lock-free tracker access (Rust regression)](./02-bug-02-tracker-race.md)                               | TLA⁺ `ConcurrentTracker` toggle + Loom + Sage `tracker_race_model`               |
| 03 | [Bug #3 — Generic slash dispatcher stub](./03-bug-03-dispatcher-stub.md)                                         | Code-walking review of slashable variants; Sage differential model               |
| 04 | [Bug #4 — PoS transfer-failure FIXME](./04-bug-04-pos-transfer-fixme.md)                                         | FIXME audit pass + Rocq `BugFixTransferFailure`                                  |
| 05 | [Bug #5 — Stake-0 silent classification](./05-bug-05-stake-zero.md)                                              | Sage `weighted_closure_model` + Rocq                                             |
| 06 | [Bug #6 — Self-regression slips through](./06-bug-06-self-regression.md)                                         | Sage `differential_bisimilarity_model` + Hypothesis state-machine search         |
| 07 | [Bug #7 — Off-by-one seq-number density](./07-bug-07-bfs-density.md)                                             | Sage `closure_certificate_model` + Hypothesis                                    |
| 08 | [Bug #8 — `prepare_slashing_deploys` did not check proposer is bonded](./08-bug-08-unbonded-proposer.md)         | Hypothesis multi-epoch state machine + Rocq `BugFixUnbondedProposer`             |
| 09 | [Bug #9 — Scala rejects self-correcting blocks (Scala bug, Rust-fixed)](./09-bug-09-scala-self-correcting.md)    | Sage differential model surfacing Scala-side defect                              |
| 10 | [Bug #10 — PoS withdrawal transfer-failure FIXME](./10-bug-10-withdrawal-fixme.md)                               | FIXME audit + Rocq `BugFixWithdrawTransferFailure` + Hypothesis multi-epoch      |
| 11 | [Bug #11 — Detector traversal was partial and duplicate-child sensitive](./11-bug-11-detector-partiality.md)     | Hypothesis `assumption_minimization` + Sage `theorem_assumption_counterexamples` |
| 12 | [Bug #12 — Received slash deploys were not locally authorized](./12-bug-12-received-slash-deploy.md)             | Threat-modeling STRIDE pass + Kani harness for authorization predicate           |
| 13 | [Bug #13 — Same-key rebond could inherit stale evidence](./13-bug-13-rebond-stale-evidence.md)                   | Sage `epoch_churn_attack_model` + TLA⁺ `AuthorizedSlashFlow`                     |
| 14 | [Bug #14 — Slash liveness depended on invalid latest messages](./14-bug-14-slash-liveness.md)                    | TLA⁺ liveness invariant + Hypothesis `liveness_as_safety`                        |
| 15 | [Bug #15 — Sequence arithmetic used unchecked boundaries](./15-bug-15-unchecked-arithmetic.md)                   | Kani harness for `checked_base_seq` + libFuzzer `slashing_arithmetic`            |
| 16 | [Bug #16 — Duplicate justifications made detector projection ambiguous](./16-bug-16-duplicate-justifications.md) | TLA⁺ `JustificationProjection` + Sage `differential_bisimilarity_model`          |

## How to read these chapters

Each chapter is short (≤ 200 lines) and follows the template:

1. **One-paragraph summary** — what the bug is, in three sentences.
2. **Discovery technique** — which tool emitted the witness; why
   that tool was the right choice for this property class.
3. **Witness reproduction** — minimum input that reproduces the
   bug; pointer to the deterministic regression fixture.
4. **Classification trace** — threat class → ledger status → action.
5. **Evidence stack** — list of artifacts protecting against
   regression (Rocq theorem, TLA⁺ invariant, Rust regression test,
   bug-fix manifest entry).
6. **Lessons for the methodology** — what general principle the bug
   illustrates.

The chapters are deliberately uniform so that the **methodology
patterns** become visible across the corpus.
