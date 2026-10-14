# Execution and Resource Authorization: CbC Casper Binding

## Scope and authority

This document applies the [platform execution and resource authorization reference](../execution-and-resource-authorization.md) to F1r3fly CbC Casper. The platform reference defines execution attempts, completion boundaries, concurrency levels, and accounting for every replication mechanism. This document gives the CbC Casper form of each topic. It does not introduce protocol rules or activate a proposal.

The [decision ledger](./design/decision-ledger/README.md#22-ratification-record-of-2026-09-16) controls the recorded Casper decisions. Its Decision sections supersede historical comparison sections.

The ledger compares PR #216 at `3980ed402`. This document does not establish the current pull-request head or implementation status.

Accounting authority version 8 is independent of Casper protocol 7, as [D-01](./design/decision-ledger/01-protocol-version-authority.md) records.

## Units of work

| Term | CbC Casper meaning | Not equivalent to |
|---|---|---|
| Block | A signed consensus artifact with parents, justifications, deploy results, and state commitments. It is the CbC Casper form of a platform commit. | A CPU task or an application lifetime. |
| Validator workload | The proposal, replay, validation, merge, and finalization work that one validator performs. | An exclusive portion of execution assigned across validators. |

The [Casper glossary](./GLOSSARY.md#block) defines these terms. The [platform glossary](../Glossary.md#execution-attempt) defines deploys, execution attempts, and atomic COMMs.

A proposer executes selected deploys and records results in a block. Receiving validators replay the block's deploys and check the state commitment.

Validators can propose different blocks concurrently. Their validation work overlaps because each validator checks received blocks, rather than trusting another validator's execution alone.

A multi-parent merge reconciles selected histories. It does not authorize unrelated execution orders or remove replay obligations.

## Completion boundaries

The [platform completion boundaries](../execution-and-resource-authorization.md#completion-boundaries) apply. CbC Casper gives two of them a specific form.

| Platform boundary | CbC Casper form |
|---|---|
| Commit validation completion | Block validation completion: replay and the block validity checks return a classification. |
| Finality | Finalization: Casper accepts a finality result under its safety and state-containment rules. |

Evaluation completion and application completion have the same meaning as in the platform reference. Finalization of a block does not establish completion of every application represented in its state.

## Concurrency levels

| Level | Current CbC Casper description or recorded decision | Required distinction |
|---|---|---|
| Between validators | Validators can propose and validate concurrently. | Replicated validation is not exclusive workload partitioning. |
| Between local block replays | The runtime manager permits bounded overlapping replay work. | Separate replay runtimes do not imply parallel deploy execution within one runtime. |
| Between deploys in one block | The current play and replay paths await user deploys sequentially. | A new deploy begins after the preceding execution attempt returns. |
| Within one execution attempt | Rholang parallel composition can schedule concurrent branches and enabled continuations. | `P \| Q` does not promise linear CPU scaling. |
| Between finalization evaluations | [D-05](./design/decision-ledger/05-finalization-publication.md) keeps single-flight as the default. Parallel evaluation remains conditional. | Evaluation overlap is distinct from publication order. |

The play path can stop after an executed prefix when its proposal budget expires. It defers remaining deploys instead of publishing partially evaluated deploy bodies.

The replay path checks the deploy sequence carried by the block. The recorded event log constrains communication choices, and replay checks the resulting state.

D-05 requires publication-order, restart, and equivalence tests before parallel finalization evaluation is used. It also requires atomic publication and stale-result refusal.

The optional ledger architecture must not expose a torn block, state root, and effect tuple. More workers do not relax that condition.

## Accounting versus resource protection

The [platform accounting rules](../execution-and-resource-authorization.md#accounting-versus-resource-protection) apply to CbC Casper. The decision ledger records the Casper decisions on them.

[D-12](./design/decision-ledger/12-deploy-cost-limits.md#decision-2026-09-16) rejects removal of `phloLimit` and `phloPrice`. Their signed fields, prepayment, refund, exhaustion, minimum-price validation, and APIs remain required by that decision.

Token accounting can add a F1R3FLY Improvement Proposal (FIP)-approved validity layer. It cannot replace either field under D-12. Multi-wallet funding requires a separate normative mapping.

This document does not choose a payer for stored continuations or resolve D-12's funding-authority questions.

## Source map

| Subject | Source |
|---|---|
| Sequential play and proposal budget | [`RuntimeOps::play_deploys_for_state`](../../casper/src/rust/rholang/runtime.rs) |
| Sequential replay and checkpoint | [`ReplayRuntimeOps::replay_deploys`](../../casper/src/rust/rholang/replay_runtime.rs) |
| Parallel branches and evaluation completion | [`DebruijnInterpreter::eval_inner` and `inj`](../../rholang/src/rust/interpreter/reduce.rs) |
| Bounded local replay concurrency | [`ReplayLock` and its tests](../../casper/src/rust/util/rholang/runtime_manager.rs) |
| Proposal, validation, and finalization | [Consensus Protocol](./CONSENSUS_PROTOCOL.md) |
| Finalization concurrency conditions | [D-05](./design/decision-ledger/05-finalization-publication.md#decision-2026-09-16) |
| Cost fields and accounting conditions | [D-12](./design/decision-ledger/12-deploy-cost-limits.md#decision-2026-09-16) |
| Historical PR #216 comparison | [CbC and formal-verification reconciliation](./design/cost-accounting-cbc-fv-reconciliation.md) |

The [platform review requirements](../execution-and-resource-authorization.md#review-requirements) apply to a CbC Casper execution or accounting change.
