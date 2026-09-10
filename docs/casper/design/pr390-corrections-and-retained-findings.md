---
doc_type: design_review
task: ca-pr390-correction-record
status: proposed
date: 2026-09-08
---

# PR 390: retained findings and required corrections

## Result and scope

PR 390 identifies real consensus and economic decisions that require upstream review.
Its broad conclusion has merit: shared Casper algorithms do not establish identical architecture or semantics.
Several proposed resolutions need correction before ratification.
This record does not authorize a protocol choice or modify PR 390.

The [status ledger](cost-accounting-ratification-status.md) records approval boundaries for all 12 decisions.
The [comparison work log](../../work-logs/task-casper-current-dev-classification-2026-09-08.md) links the full architecture comparison and source inventory.
These records complement this critique rather than replace upstream authority.

## Pinned evidence

| Source | Revision |
|---|---|
| Dev implementation | `cdf447ac18710d9702a27379bce6c946f421be46` |
| PR 390 documents | `1d9ae249d75a39d8c0851bf71264307047c532e7` |
| Public PR 216 head | `3980ed402b4b3248f065d1f07daedbd3dc8b4493` |
| Local feature HEAD | `559eb07fac98da2e6392e3a84c93bac41558c86c` |

The local candidate also contains uncommitted and untracked source.
A claim about that candidate does not automatically describe the public PR head.
Current dev contains 67 commits beyond the feature's previously merged dev revision.
PR 390's decision index still cites older baseline revisions.
PR 390 now targets dev, so the earlier concern about its temporary base is stale.

This review inspected all 12 current decision entries, selected production paths, formal-gate scripts, and relevant specification passages.
It did not rerun the formal gates or reproduce a concurrent network failure.
Test and model presence below means source presence, not a passing result for the current candidate.

## Disposition by decision

| Decision | Finding to retain | Correction or unresolved issue |
|---|---|---|
| D-01 Version authority | Genesis, approval, proposal, validation, and restart need coherent version authority. | Current running Casper supports protocol 6 from fresh genesis. Authority-accounting protocol 8 is a separate identifier. Do not imply an implemented rolling upgrade. |
| D-02 Certified authority | Receiver-local trackers and unauthenticated state must not establish validator authority. | A shared invariant does not prove a particular certificate encoding necessary. Committee provenance changes votes and denominators, even without a wire change. |
| D-03 Fork choice | Shared GHOST descent needs fixed, authenticated inputs and deterministic ordering. | Compare stake provenance, missing slots, eligibility, LCA filtering, and secondary-parent depth separately. Equal-input descent does not establish equal outputs from different inputs. |
| D-04 Finality | Exact settlement effects, preservation, missing evidence, and observability need explicit contracts. | Threshold strictness is a runtime difference. Both implementations retain the two-sided disagreement rule and collect parent frontiers. Correct the contradictory differential checklist. |
| D-05 Publication | Durable publication, stale workers, and restart require an explicit observable boundary. | Do not classify the whole publication architecture as proven equivalent from local atomicity results alone. Required concurrent and crash evidence remains open. |
| D-06 Recovery | Proposal amplification deserves measured bounds without changing validity by host timing. | Dev also has a leader-only lane. Feature leader selection is not global serialization. Removing frontier-follow needs a separate liveness decision. |
| D-07 Retry and custody | Canonical identity, rejection reasons, custody, and lifespan must agree across validators. | Collective coverage differs from requiring one parent to cover the frontier. Ratify that packaging choice separately from record canonicalization. |
| D-08 Merge algebra | Additive composition can change contract-visible multiplicity. Evidence authentication requires independent review. | Describe exact, legacy, and rejected mixed modes. Do not extend a per-datum algebra claim to every state component. Correct the activation description. |
| D-09 Slashing | Evidence authority must survive restart without punishing a new validator lifetime for old evidence. | Distinguish bond generation from block epoch. Reconcile economic-neglect prose with the disabled formal-model setting before selecting a penalty rule. |
| D-10 Carrier index | The index must refine a scoped scan over the same deploy identity. | Reconcile current dev's new claims and measurements with feature identity tags and atomic publication. Invariant counts do not prove subsumption. |
| D-11 Verification | Required claims, negative controls, budgets, and approval authority need explicit continuity. | Separate central registration, local invocation, workflow reachability, and successful execution. Do not treat historical counts as current proof coverage. |
| D-12 Deploy controls | Missing client exposure semantics and top-level binary funding are genuine gaps. | Restoring signed phlo controls does not require rejecting token accounting. The approved M1 plan requires phlo restoration and arbitrary-payer settlement. |

## 1. Threshold equality is not a text-only repair

D-04/S4.3 proposes strict comparison and requires no evidence beyond a text correction.
Current dev's production floor calls use the non-strict mode of `ft_decides_exact`.
The feature removes that mode and uses strict comparison.

Let agreeing stake be 6, clique stake be 6, total stake be 10, and the threshold be 200000/1000000.
Both branches pass the separate strict-majority check on agreeing stake.
Their exact comparison sides both equal 12,000,000.
Dev accepts this equality, while the feature rejects it.

This is an arithmetic witness from the production predicates.
It is not evidence that this exact network state occurred in a reported CI failure.
It proves that the proposed change cannot be described as text-only.

Upstream must select the intended boundary and its activation scope.
The chosen rule needs equality, arithmetic-boundary, asymmetric-stake, committee-transition, and concurrent replay coverage.
The review must not assume that the feature's stricter rule is correct merely because its models encode it.

Source: feature `casper/src/rust/safety/clique_oracle.rs:98`, dev same file at line 73, and dev `finality/floor.rs:916`, `:937`, `:988`.

## 2. Separate preserved behavior from intentional repairs

D-04/S4.1 requires every dev-valid promotion to remain valid under the proposed state certificate.
The same checklist requires a case where the certificate rejects a promotion that dev permits.
These requirements conflict unless they refer to different input domains.

The corrected checklist needs three parts.

1. Define the shared safe domain and prove observational equivalence over that domain.
2. Identify each excluded trace with a regression, invariant violation, and approved semantic change.
3. Test the revised boundary across independent validator observations, replay, and restart.

The state-certificate representation remains a separate design decision from the settled-effect preservation invariant.
The accounting papers do not make those two propositions equivalent by themselves.

D-04/S4.5 has a direct source answer.
Both implementations reverse the ancestry query when the visited message is below the target height.
The feature implementation is at `clique_oracle.rs:302–306`, and dev retains the corresponding branch at lines 268–272.
A removed explanatory sentence does not establish removal of that behavior.

Both current floor implementations also collect inherited floors and parent frontiers.
The meaningful differences concern effect identity, support, preservation, and acceptance rules.
The review must not describe all-parent candidate discovery as wholly absent from current dev.

## 3. Keep recovery concurrency decisions explicit

Dev includes frontier-follow, all-eligible stale recovery, and leader-only convergence paths.
The feature selects stale-recovery eligibility using committee, height, and round.
Its permit separately binds the floor hash.
Other pending-deploy and backstop paths remain independent.

Thus, neither a leader-free dev description nor a globally serialized feature description is accurate.
D-06's proposed immediate adoption also removes frontier-follow as a proposal trigger.
That removal changes recovery behavior, even if peer messages remain evidence rather than authority.

Required comparison must hold workload, stake, delivery assumptions, and resource limits constant.
It must include delayed rounds, silent selected validators, backlog, changing committees, and independent concurrent proposals.
Throughput measurements alone cannot establish eventual finalization.
No recovery policy changes through this record.

Source: current dev `heartbeat_proposer.rs:998–1027`, feature same file at lines 492–501, and feature `casper/mod.rs:71`.

## 4. Scope the merge claim and activation contract

The feature selects additive composition when exact execution witnesses are present.
Feature legacy indices use maximum union, then cancel once after the complete fold.
Dev cancels at each fold step.
One merge epoch rejects mixed witness modes.
Repeated observation of one exact effect differs from two distinct effects with identical payloads.

The per-datum maximum-union-with-cancellation operator is commutative but not associative.
Dev and feature legacy mode use right-biased join maps.
Feature exact mode instead rejects inconsistent serialized joins for the same key.
Neither a per-datum statement nor an integer-channel statement proves a property of every state component.

For ordered changes `add(x), remove(x), add(x)`, pairwise cancellation leaves one `x`.
Deferred cancellation leaves none.
This is an algebraic witness, not a reproduced admissible network trace.
It prevents an unconditional claim that feature legacy folding preserves dev semantics.

D-08 correctly identifies a contract-visible semantic decision.
Its description of activation after an in-place finalized cut is not implemented by the current running-version contract.
The present candidate requires fresh genesis.
Any later migration needs its own approved representation, replay, and compatibility evidence.

Evidence authentication and admission-effect projection can be reviewed independently from the multiplicity decision.
They still need their actual regressions and cross-view evidence.
A paper-level multiplicity argument does not automatically verify every implementation component.

Source: feature `conflict_set_merger.rs:531–641`, `deploy_chain_index.rs:223`, `state_change.rs:569–585`, and `channel_change.rs:17–40`.
Dev calls `combine` in `conflict_set_merger.rs:585` and uses right bias in `state_change.rs:621–624`.

## 5. Preserve the approved phlo and arbitrary-payer scope

D-12 correctly identifies the current top-level binary funding shape.
`GroupShape::Compound` exposes only a combined pool and two components.
`DefaultApportionment` draws from the combined pool before equal component draws.
The nested-compound regression still expects rejection when the intermediate pool is absent.

Those observations do not satisfy the user's arbitrary-payer requirement.
Equal logical authority contributions do not by themselves define a conserved division of physical wallet charges.
The new policy must state both units and their relationship.

D-12's option C incorrectly treats retaining phlo fields as necessarily rejecting token-based admission.
Authenticated supply can remain the funding authority while signed limits cap client exposure.
The solver must prove compatibility between demand, exposure limits, conversion, reserved custody, and settlement.
A fee is not interchangeable with a per-unit price.

The approved plan requires arbitrary-payer allocation and signed phlo controls before PR amendment.
It includes deterministic residual distribution, shared-wallet conservation, refund provenance, and canonical batch behavior.
The policy task remains pending, so planned rules must not be presented as implemented or fully ratified.
A client-selectable apportionment menu is not implied by approval of a deterministic default.

The continued-GSLT paper explicitly distinguishes static consumption from data-dependent interaction.
Data-dependent cases require dependent proofs or conservative bounds.
Its located-purse composition result assumes that surfaces partition the interactions.
A global solver claim must establish that partition and preserve shared-custody constraints.

D-12's categorical statement about absent in-deploy authority transfer also needs a narrower revision-bound analysis.
Current source has located authority regions and settlement witnesses beyond the older decision records it cites.
Their presence does not establish complete lollipop support, but older ingress-only descriptions cannot settle that question.

Sources: `resource_logic.rs:263–375`, `acceptance.rs:1940`, `:7341`, and the continued-GSLT paper's resource-sufficiency section at lines 1474–1534.

## 6. Distinguish formal coverage from gate execution

D-11's demand for claim continuity has merit.
No proof should disappear from the required gate without a statement-level replacement and the required approval.
A new theorem name or larger invariant count does not establish that replacement.

Current dev's central TLA+ registration includes entries absent from the feature's central registration.
However, `scripts/check-fork-choice-ALL.sh:169` still invokes `MC_ForkChoice.cfg` locally.
That script declares itself local-only.
Its presence does not establish CI execution, and central-registry absence does not establish deletion.

The workflow `slashing-tests.yml` invokes `scripts/ci/check-formal-invariants.sh`.
That script dispatches TLA+ checks through `scripts/ci/check-tla-invariants.sh`.
The required comparison must trace each claim through these actual entry points and conditions.
It must record configurations, assumptions, expected negative controls, resource bounds, and revision-bound outcomes.

The source also contains a direct normative inconsistency for economic neglect.
`CONSENSUS_PROTOCOL.md:966` describes stake loss, while `MC_TwoLevelSlashing.tla:13` sets `MC_EconomicNeglectSlashing` to FALSE.
This record does not resolve the inconsistency by activating slashing.

The user's regressor remains outside required CI.
Licensed Wolfram exploration remains optional.
Neither exception authorizes removal of unrelated required assurance.

## 7. Incorporate newer upstream work without claiming causal proof

Current dev adds proposal-prefix budgeting, an independent unresolved-request clock, and bounded detached announcements.
The reviewed feature paths lack those same controls.
The proposal budget checks between deploys and does not bound one deploy's execution time.
Both branches preserve retry quarantine through different ownership mechanisms.

These observations require a semantic integration review, not blind replacement or a new protocol.
They do not prove that each missing control caused a reported CI failure.
The full comparison records commits, implementation anchors, and overlaps.
This critique does not promote additional audits into M1 without evidence that they block assigned work.

## Required review outcome

Upstream review must separate confirmed observations, intended invariants, representation choices, and changed observable behavior.
Each accepted change needs an exact scope and a link to the applicable approval.
Required proofs and regressions must accompany the accepted implementation, not only its abstract algorithm.

This record retains PR 390's valid concerns and corrects its unsupported conclusions.
It does not ratify any entry, edit consensus code, change verification gates, or publish a PR comment.
