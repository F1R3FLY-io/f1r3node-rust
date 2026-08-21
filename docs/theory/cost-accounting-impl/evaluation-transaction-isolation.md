# Evaluation Transaction Isolation

**Status:** Implemented protocol-4 refinement

**Scope:** Rholang parsing and reducer failures, current-deploy linear authority,
Casper play validation, candidate-root materialization, and replay rejection.

**Governing sources:** `cost-accounted-rho.tex` transaction/witness and database-
atomicity requirements; `continued-gslt-cost-v2.tex` local-sufficiency boundary;
DR-47, DR-49, and DR-50.

The papers treat a paid reduction as one state transition with a witness. The
native node realizes that abstraction across several fallible subsystems: the
parser, runtime budget, reducer, RSpace hot store, history repository, authority
and byte witnesses, SystemVault settlement validation, and replay. Transaction
isolation means that every rejection returns a phase-appropriate witness while
restoring every state component that the rejected execution was not authorized
to publish.

![An authenticated base flows through parsing, fresh-witness initialization, reducer execution, candidate checkpointing, and evidence validation. Parser failure returns no witness; reducer failure retains attempted work but rolls back linear custody and RSpace; post-validation failure restores the soft base or explicitly resets a replay history root; only validated state is promoted.](../diagrams/evaluation-transaction-isolation.svg)

## Terms

| Term | Meaning |
| --- | --- |
| Authenticated base | The RSpace root named by the deployment or block before evaluation begins. |
| Current witness | Cost, authority, quantitative-byte, and stack-birth evidence produced by the current deployment only. |
| Attempt witness | Current-deploy work already reserved before a reducer error. Attempt cost remains chargeable under DR-47. |
| Linear effect | A rivalrous stack-cell debit and its corresponding stack birth. A rejected deployment publishes neither. |
| Soft checkpoint | An in-memory hot-store snapshot valid while the history repository still has the same base root. |
| Candidate checkpoint | A content-addressed root created from replayed state before its recorded post-state witness is accepted. |
| Active root | The history repository root from which the runtime reads and creates its next checkpoint. |

## Required rejection laws

Let $`B`$ be the authenticated base root, $`W_a`$ the current attempt witness,
$`W_o`$ the returned witness, $`L_o`$ the returned linear effects, and $`H_o`$
the active root after the operation returns.

```math
\begin{aligned}
ParserReject &\Rightarrow W_o=\varnothing \land H_o=B,\\
ReducerReject &\Rightarrow W_o=W_a \land L_o=\varnothing \land H_o=B,\\
PostValidateReject &\Rightarrow L_o=\varnothing \land H_o=B.
\end{aligned}
```

The second law does not refund ordinary attempted work. It removes linear stack
custody because a stack debit is valid only with a committed RSpace produce and
birth. The third law covers every error discovered after execution: causal-event
ordering, stack-birth resolution, status, cost, authority, byte evidence,
settlement, replay-data exhaustion, and adjacent-root equality.

## Transaction algorithm

The algorithm distinguishes a hot-store rewind from an active-history reset.
That distinction is necessary because `create_checkpoint` advances the replay
history repository before adjacent-root validation.

```text
evaluate(base, source, evidence):
    parsed = parse(source)
    if parsed failed:
        return Reject(witness = empty, activeRoot = base)

    clearCurrentWitness()
    softBase = createSoftCheckpoint()
    result = execute(parsed)

    if result is a reducer error:
        rollbackCurrentDeployStackEffects()
        revertToSoftCheckpoint(softBase)
        return Reject(witness = attemptedWork, activeRoot = base)

    candidate = validateExecutionEvidence(result)
    if candidate failed:
        revertToSoftCheckpoint(softBase)
        return Reject(witness = result.witness, activeRoot = base)

    if replay:
        candidateRoot = createCheckpoint(candidate.state)
        if validateRecordedPostState(candidateRoot, evidence) failed:
            resetHistoryRoot(base)
            return Reject(witness = result.witness, activeRoot = base)

    return Accept(witness = result.witness, activeRoot = candidate.root)
```

## Why a soft checkpoint is insufficient after replay checkpointing

RSpace soft checkpoints contain a hot-store snapshot, event log, and produce
counter. They do not capture the history repository root. `create_checkpoint`
persists the current changes and replaces that root. Reverting an older soft
checkpoint afterward would rebuild the old cache snapshot over the new history
base, leaving the rejected candidate active. Whole-block replay therefore calls
`reset` with the already authenticated block pre-state on every error. The reset
restores the active root and hot store together. Content-addressed nodes that are
not active may remain in storage; they cannot become the input to another replay,
settlement, or consensus decision without an authenticated root reference.

## Phase ownership

| Phase | Witness on rejection | State action |
| --- | --- | --- |
| Parse before budget reset | Empty current witness | No execution state exists; active root remains the base. |
| Reducer operation | Exact attempted work | Roll back every current-deploy stack event and birth; enclosing play restores RSpace. |
| Play authority/birth validation | Current attempted work | Revert the user-deploy soft checkpoint. |
| Per-deploy replay before checkpoint | Current attempted work | Revert the per-deploy soft checkpoint. |
| Adjacent-root validation after checkpoint | Current attempted work | Reset the entire replay runtime to the block pre-state. |
| Later system-deploy or block validation | Current block evidence is rejected | Reset the entire replay runtime to the block pre-state. |

## Concurrency boundary

Reducer sibling branches and persistent dispatch branches are spawned and joined
to completion. A sibling error is collected after the other branches finish; it
does not cancel them midway. This is why an enclosing deployment rollback is a
separate obligation from operation-local pending-reservation abort. Loom models
pre-mutation cancellation, competition, operation rejection, and enclosing-
deployment rollback. It deliberately does not pretend that a Rust destructor can
undo an arbitrary RSpace mutation that has already become visible.

## Implementation map

| Responsibility | Implementation |
| --- | --- |
| Fresh witness initialization and phase-aware error result | `rholang/src/rust/interpreter/interpreter.rs` |
| Current-deploy stack-effect rollback with attempt-cost retention | `rholang/src/rust/interpreter/accounting/mod.rs` |
| Stack reservation lifetime and commit | `rholang/src/rust/interpreter/reduce.rs` |
| Play checkpoint around evaluation plus authority/birth validation | `casper/src/rust/rholang/runtime.rs` |
| Per-deploy replay soft transaction | `casper/src/rust/rholang/replay_runtime.rs` |
| Whole-block replay reset after post-checkpoint rejection | `casper/src/rust/rholang/replay_runtime.rs` |
| RSpace soft-checkpoint and active-root semantics | `rspace++/src/rspace/{rspace,replay_rspace,checkpoint}.rs` |

## Verification matrix

| Obligation | Formal evidence | Executable evidence |
| --- | --- | --- |
| Parser failure cannot reuse a predecessor witness | Rocq `parser_failure_cannot_reuse_prior_witness`; TLA+ `ParserFailureHasNoWitness` | `parser_failure_after_a_paid_deploy_has_an_empty_authority_witness` |
| Reducer failure retains attempted work | Rocq `reducer_failure_retains_exact_attempted_work`; TLA+ `ReducerFailureRetainsAttemptedWork` | `reducer_operator_failure_preserves_prior_attempt_charges` |
| Enclosing deployment failure removes linear custody | Rocq `enclosing_deploy_failure_restores_linear_capacity`; TLA+ `FailedDeployHasNoLinearEffects` | `later_deploy_abort_rolls_back_stack_custody_without_refunding_work`; Loom deployment rollback |
| Attempted byte cost survives linear rollback | Rocq `enclosing_deploy_failure_preserves_attempted_byte_cost`; TLA+ `RolledBackWorkRetainsByteCharge` | runtime exact rollback property |
| Rejected play restores its base | Rocq `rejected_play_restores_its_base_state`; TLA+ `RejectedPlayIsStateAtomic` | play authority and stack-birth validation regressions |
| Rejected replay discards its candidate checkpoint | Rocq `rejected_replay_restores_its_base_state` and `rejected_replay_discards_its_prevalidation_checkpoint`; TLA+ `RejectedReplayIsStateAtomic` | `rejected_replay_post_state_witness_restores_block_pre_state` |
| Rejected block final state publishes no merge evidence | Rocq `rejected_replay_publishes_no_mergeable_evidence`; TLA+ `RejectedReplayPublishesNoEvidence` | `rejected_block_final_state_does_not_publish_mergeable_evidence` |
| Accepted block final state publishes validated merge evidence | Rocq `accepted_replay_publishes_mergeable_evidence` and `published_mergeable_evidence_requires_final_state_match`; TLA+ `AcceptedReplayPublishesValidatedEvidence` | valid replay after the forged-final-state rejection publishes the exact entry |

Five `EvaluationTransactionIsolation*Unsafe.cfg` controls must refute parser
witness reuse, reducer-attempt erasure, missing play rollback, missing replay
rollback after candidate-checkpoint publication, and merge-evidence publication
before final-state validation. Six
`StackIntroductionAtomicity*Unsafe.cfg` controls separately refute operation and
enclosing-deployment stack-transaction defects. Both safe models and every
control are mandatory in TLC and Apalache; the Rocq modules are axiom-free and
included in the aggregate proof gate.

## Security and operational consequences

- A malformed source cannot inherit another user's witness or settlement lane.
- A reducer error cannot obtain a work refund by changing its error class.
- A failed deployment cannot retain transferable stack custody.
- A forged replay post-state witness cannot move the validator's active root.
- Intermediate replay roots remain materialized for valid multi-deploy replay,
  but only an accepted adjacent-root chain advances the transaction.
- The repair changes transaction ownership, not majority voting, clique
  calculation, fork choice, RSpace matching, or byte tariff semantics.

See CA-P-196/197, TM-CA-185/186, UC-CA-176/177, E2E-059/060, DR-49/50, and
[vault-backed byte accounting](vault-backed-byte-accounting.md). Complete cache
identity and peer-input exclusion are specified by CA-P-198, DR-51, and
[mergeable evidence authentication](mergeable-evidence-authentication.md).
