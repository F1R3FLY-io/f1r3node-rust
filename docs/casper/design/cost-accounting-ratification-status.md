---
doc_type: design_review
task: ca-casper-decision-status-ledger
status: proposed
date: 2026-09-08
---

# Cost-accounting Casper ratification status

## Purpose and authority

This ledger records decision status, not new protocol rules.
It connects the current feature implementation with the proposed [PR 390 decision ledger][ledger].
It does not replace the upstream ledger or change its entries.

The user requires upstream Casper review before disputed consensus changes.
The accounting papers govern the accounting behavior they specify.
They do not independently select threshold equality, committee provenance, recovery leadership, or a certificate encoding.

PR 390 proposes approval by a maintainer with merge rights on dev.
It also requires the PR 216 author's approval when a decision changes that branch's position.
Its proposed procedure requires linked approval comments for each decision.
That procedure remains a proposal, not an authority created by this document.

The current PR 390 entries all have status Proposed.
The September 8 API check found no submitted reviews and no inline review comments.
The two issue comments contain an automated review and the author's response, not individual protocol approvals.

An implementation decision record is evidence of branch intent.
Its labels, such as accepted, implemented, or verified, do not establish upstream Casper ratification.
A proof can support a stated model without proving that maintainers approved its protocol choices.

## Evidence pins

| Source | Revision or record |
|---|---|
| Current dev | `cdf447ac18710d9702a27379bce6c946f421be46` |
| Local feature HEAD | `559eb07fac98da2e6392e3a84c93bac41558c86c` |
| Public PR 216 head | `3980ed402b4b3248f065d1f07daedbd3dc8b4493` |
| PR 390 documentation head | `1d9ae249d75a39d8c0851bf71264307047c532e7` |
| Current implementation classification | [September 8 work log](../../work-logs/task-casper-current-dev-classification-2026-09-08.md) |
| Historical feature decisions | [Cost-accounting decision records](../theory/cost-accounting-decision-records.md) |
| Automated review | [PR discussion, September 5][review-comment] |
| Author's response | [PR discussion, September 5][response-comment] |

The feature candidate includes uncommitted and untracked source.
Its committed HEAD does not identify the complete candidate.
The comparison work log links the requested report and its principal source hashes.

The governing papers are the cost-accounted calculus, continued GSLT cost model, and applicable knotted-topoi construction.
Their workspace paths are `../publications/cost-accounting/cost-accounted-rho.tex`, `../publications/cost-accounting-as-monad/continued-gslt-cost-v2.tex`, and `../publications/knotted-topoi/knotted-topoi.tex`.
The calculus's Casper subsection, lines 1706–1714, requires matching-token backing and accounting-aware block validity.

## Status vocabulary

| Status | Meaning |
|---|---|
| Observed | Pinned source or an identified result supports the stated fact. |
| Proposed | A decision or repair awaits the required approval. |
| Historical claim | An earlier record states a result that this review did not revalidate. |
| Approval not found | The inspected approval sources contain no decision-specific authorization. |
| Ratified | The authorized upstream record links the applicable approval and exact decision scope. |

All decisions below have upstream Casper ratification status **Approval not found** at this snapshot.
This does not revoke user approval for the economic requirements or their implementation.
None receives status Ratified through this document.
Required evidence below describes obligations, not claims that those obligations have passed.

## Decision index

| ID | Decision boundary | Current classification | Activation consequence |
|---|---|---|---|
| D-01 | Version authority and deployment | Shared need for coherent version authority, feature-only supported-set enforcement | Candidate runs only from fresh Casper protocol-6 genesis. |
| D-02 | Floor, committee, and certificates | Architectural and semantic extension | Signed commitments, sidecars, and generation evidence affect validation and wire data. |
| D-03 | Fork-choice inputs and parent bounds | Shared descent with different scores, eligibility, and bounds | Changes can alter heads and parents without new wire fields. |
| D-04 | State preservation and finality | Different effect identity, support, and threshold decisions | Exact-effect checks are version-sensitive. Threshold strictness is not confined to those checks. |
| D-05 | Finalization publication | Local architecture with externally observable publication obligations | Storage and restart migration require separate qualification. |
| D-06 | Heartbeat and recovery | Different proposer eligibility and concurrency policy | No new validity rule follows merely from a heartbeat policy. |
| D-07 | Retry identity, custody, and coverage | Mixed protocol identity and proposer policy | New record identity must agree with the active deploy identity. |
| D-08 | Merge algebra and evidence | Contract-visible difference with exact and legacy modes | Current candidate does not implement an in-place finalized-cut upgrade. |
| D-09 | Slashing authority and lifetime | Different evidence and authorization boundaries | Generation and pair evidence need coherent encoding and replay. |
| D-10 | Carrier-index refinement | Intended optimization over a versioned identity function | Legacy signatures and protocol-6 commitments require separate domains. |
| D-11 | Verification governance | Unresolved claim, gate, scope, and approval reconciliation | No silent gate removal or transfer of proof authority. |
| D-12 | Phlo controls and arbitrary funding | Current implementation and approved M1 requirements differ | Signed intent, solver semantics, settlement, clients, and replay must change together. |

## D-01: Version authority

**Observed state.** `casper.rs:41–58` accepts only Casper protocol 6 for a running shard.
The authority-accounting protocol separately uses version 8.
Historical helpers do not establish legacy-node interoperability or rolling upgrades.

**Source and motivation.** DR-34 records a ceremony/proposal version mismatch.
Current protocol documentation describes the authority chain from approved genesis through proposal and reception.
DR-34 still contains historical version-3 and version-4 language.

**Required evidence.** Ceremony, approval, restart, proposer, validator, and read-only nodes must use the same approved version.
Rejected-version controls must exercise the real startup and message paths.

**Open decision.** Decide whether fresh genesis remains a release-specific choice or a permanent protocol requirement.
Reconcile historical wording without presenting an unimplemented upgrade path as available.

**Required approval.** Upstream review must approve the authority rule and the release's activation contract.
Reference: [D-01][d01].

## D-02: Certified floor and committee

**Observed state.** The feature uses certified active floor weights for clique inputs and sender authority.
Current dev's clique oracle obtains the target's main-parent weight map.
The feature also requires signed floor commitments and supports detachable certificates.

**Source and motivation.** `clique_oracle.rs:150`, `causal_equivocation.rs`, and `validation_dispatcher.rs:105` define the current boundaries.
The intended repair prevents unverified post-state bytes or receiver-local trackers from creating authority.
That invariant does not prove every chosen certificate representation necessary.

**Required evidence.** Compare asymmetric stake, bond, withdrawal, slash, rebond, and delayed observations across validators.
Distinguish unavailable evidence from invalid evidence.
Test certificate retrieval, retention, restart, and equivalent certificate carriers.

**Open decision.** Select the committee used for authorization, synchrony, GHOST contributions, and clique denominators.
Decide exact-justification requirements and availability behavior under sustained missing evidence.

**Required approval.** Upstream review must approve committee semantics separately from certificate encoding and retrieval.
Reference: [D-02][d02].

## D-03: Fork choice and parent bounds

**Observed state.** Both estimators retain GHOST descent and deterministic tie-breaking.
The feature changes stake provenance, eligible messages, and missing-slot behavior.
It removes the upstream LCA filter for messages at least 1000 heights below the maximum.

The feature measures secondary-parent depth from the tallest ranked parent, not the selected main parent's height.
Its snapshot checks frontier capacity after fork-choice calculation.
Estimator truncation remains in both implementations.

**Source and motivation.** `estimator.rs:48`, `:112`, `:211`, `:290`, and `snapshot.rs:564`, `:707` identify these decisions.
Historical DR-44 describes lost sibling-state concerns.
The current comparison does not establish a necessary one-to-one relationship between that defect and every changed bound.

**Required evidence.** Prove equal descent for equal inputs separately from changes to those inputs.
Compare unequal stake and changing committees, not only fixed equal-stake examples.
Exercise permanent frontier pressure, expiry, silent validators, missing data, and bounded search work.

**Open decision.** Ratify context and vote projections, parent-cap behavior, LCA work bounds, and secondary-parent depth separately.
Do not adopt a fallback solely because one soak passes.

**Required approval.** Upstream review must approve sub-decisions D-03/S3.1 through S3.4 and the corrected depth rule.
Reference: [D-03][d03].

## D-04: State preservation and finality

**Observed state.** The feature adds exact effects and filters state-supporting tips.
Current dev retains signature-based containment with a qualified re-collection assumption.
Both implementations discover inherited floors and parent frontiers.

Current dev accepts equality at its production clique threshold.
The feature requires a strictly greater value.
The comparison contains a concrete predicate witness, not a reproduced multi-node failure.

Both source implementations retain the two-sided disagreement check.
Feature `clique_oracle.rs:302–306` reverses the ancestry query below the target height, as current dev does at `:268–272`.
The missing protocol sentence does not establish missing implementation behavior.

**Source and motivation.** DR-43 through DR-46 describe exact effects and state-floor preservation.
Current dev records its re-collection assumption at `finality/floor.rs:596–605`.
These are evidence boundaries, not blanket correctness guarantees.

**Required evidence.** Separate unchanged safe traces from intentional changes in admitted promotions.
Test failed-body settlement, partial effect rejection, incomparable observations, concurrent finalization, and restart.
Model threshold equality with real stake arithmetic and the selected committee rule.

**Open decision.** Ratify exact preservation, supporting votes, search completeness, threshold convention, and observability independently.
D-04/S4.3 is not a text-only correction against current dev.

The proposed S4.1 checklist needs a qualified domain.
It cannot require all dev promotions to remain accepted while also requiring rejection of a promotion dev accepts.
Define the unchanged subset and the intentional counterexample separately.

**Required approval.** Upstream review must resolve each S4.1–S4.5 item with its precise scope.
Reference: [D-04][d04].

## D-05: Durable publication

**Observed state.** The feature uses bounded parallel immutable finalizer evaluations and a durable publication ledger.
It adds receipt, projection, restart, and proposal-readiness obligations.
These mechanisms are architectural choices, not consequences of the clique formula alone.

**Source and motivation.** `block-storage/src/rust/finality/finalization_ledger.rs` and the finalization runner define publication and recovery.
Historical records identify stale workers and partial publication as motivating defects.

**Required evidence.** Fix the observable publication boundary and test competing evaluations against it.
Cover cancellation, stale results, partial writes, restart, event delivery, cleanup, and floor readiness.
Keep immutable certification values distinct from raised display projections.

**Open decision.** Establish which externally visible results remain equivalent to dev.
Decide accepted storage migration and operator-facing behavior without serializing independent validators.

**Required approval.** Upstream review must accept the architecture and its publication, eviction, and fault-tolerance identity contracts.
Reference: [D-05][d05].

## D-06: Heartbeat and recovery

**Observed state.** Dev has frontier-follow, all-eligible stale recovery, and leader-only convergence lanes.
The feature rotates stale-recovery eligibility by committee, height, and local round.
A permit separately binds the floor hash.
Pending-deploy and backstop proposals remain independent.

**Source and motivation.** `heartbeat_proposer.rs:492–501` and `casper/mod.rs:71` define the feature selection.
Historical records cite proposal amplification as the reason.
This review does not establish that selected recovery is the best or necessary remedy.

**Required evidence.** Model parallel validators with different rounds, delayed messages, offline selected validators, and committee changes.
Compare recovery latency, useful witnesses, proposal count, and resource growth under identical workloads.
State delivery and scheduler fairness assumptions explicitly.

**Open decision.** Ratify recovery selection and frontier-follow removal separately.
Removing frontier-follow changes a proposal lane even if it is described as evidence-only handling.
It is not independent of all liveness decisions.

**Required approval.** Upstream review must approve the chosen eligibility policy and any bound on recovery delay.
Reference: [D-06][d06].

## D-07: Retry identity and custody

**Observed state.** The feature uses exact occurrences, canonical rejection reasons, carrier-owner custody, and collective parent coverage.
The upstream packaging rule asks one parent to cover the relevant frontier.
Collective coverage can permit a retry that the one-parent rule delays.

**Source and motivation.** DR-33, DR-35, DR-55, and DR-56 record duplicate observations, rejection-order ambiguity, and split-frontier stalls.
These historical diagnoses need their revision-bound regression evidence at closure.

**Required evidence.** Preserve floor settlement, custody, lifespan, and replay constraints under either packaging rule.
Test shared deploy identity, distinct source occurrences, missing bodies, lease expiry, and concurrent independent owners.
Rejection-reason order must not change consensus bytes.

**Open decision.** Ratify identity and reason canonicalization separately from retry custody and packaging policy.
Do not use recovery leadership as authority to retry another carrier owner's deploy.

**Required approval.** Upstream review must approve D-07/S7.1–S7.4 and the applicable record boundary.
Reference: [D-07][d07].

## D-08: Merge algebra and evidence

**Observed state.** Feature exact witnesses add distinct causal contributions and deduplicate repeated effect identities.
Feature legacy inputs use maximum union and cancel once after the complete fold.
Dev cancels at each fold step.
Mixed witness modes fail within one merge epoch.

The per-datum max operator is commutative but not associative.
Dev and feature legacy mode use right-biased join maps.
Feature exact mode rejects inconsistent serialized joins for the same key.
Per-datum algebra does not establish properties of every state component.

**Source and motivation.** `conflict_set_merger.rs:531–619`, `deploy_chain_index.rs:223`, and DR-51 define the boundary.
Upstream N-SEMANTICS forbids a silent change to observable merge results.
The feature's multiplicity argument therefore requires explicit semantic adjudication.

**Required evidence.** Compare contract-visible data multiplicity, joins, numeric channels, and rejection closure.
Test repeated observation of one identity separately from identical data produced by distinct identities.
Qualify local evidence reconstruction against cache loss, forged responses, restart, and opposite arrival orders.

**Open decision.** Ratify the algebra, evidence authentication, and admission-effect projection as separate sub-decisions.
The current candidate has fresh-genesis activation, not the in-place finalized-cut transition described in D-08.

**Required approval.** Upstream review must approve D-08/S8.1–S8.4 and resolve activation with D-01.
Reference: [D-08][d08].

## D-09: Slashing authority and validator lifetime

**Observed state.** The feature reconstructs authority from persisted evidence and canonical pre-state.
It identifies validator lifetimes by bond generation and supports objective sibling evidence.
This changes evidence authorization, not only storage keys.

The local protocol document still states that neglect loses stake.
The normative `MC_TwoLevelSlashing.tla` configuration sets `MC_EconomicNeglectSlashing` to FALSE.
This contradiction needs reconciliation, not activation of a penalty to match prose.

**Source and motivation.** DR-3, DR-7, DR-18, `slashing_authorization.rs`, and PoS bond lifecycle state describe the intent.
The intended invariant prevents old evidence from authorizing punishment in a new lifetime.

**Required evidence.** Compare rejected-slash recovery with canonical reconstruction.
Cover epoch crossing, completed withdrawal, rebond, redemption, duplicate evidence, and independent validator observations.
Prove the replacement correctness statement before retiring an existing gated statement.

**Open decision.** Ratify authorization, lifetime identity, neglect behavior, and the proof reference separately.
Historical epoch labels must not substitute silently for bond generation.

**Required approval.** Upstream review must approve D-09/S9.1–S9.4 together with the relevant D-11 proof decision.
Reference: [D-09][d09].

## D-10: Carrier index

**Observed state.** The feature uses tagged deploy identities and atomic admission records.
Newer dev includes narrower carrier-cost claims and additional measurements.
An index hit does not establish scope membership, and an unreadable row does not establish absence.

**Source and motivation.** The carrier index avoids a measured ancestor-scan cost.
It is not a complete repair for all finality or memory failures.

**Required evidence.** Compare forced-index and forced-scan verdicts over the same immutable DAG and identity function.
Cover equal raw bytes from distinct identity domains, invalid and approved carriers, watermark boundaries, read errors, and restart.
Reconcile both model obligations without assuming that a larger invariant count proves subsumption.

**Open decision.** Ratify the identity function, claim wording, and mandatory evidence.
Decide cache claims by observable behavior, not by whether the cache is described as auxiliary.

**Required approval.** Upstream review must approve D-10 and its links to D-07 and D-11.
Reference: [D-10][d10].

## D-11: Verification governance

**Observed state.** Current dev's central TLA+ registry includes entries absent from the feature's central registry.
Some feature scripts still invoke models absent from that registry.
Registry absence therefore does not prove model deletion or absence from every execution path.

The feature's assumption check names `main_slashing_algorithm_correct`.
Property-test jobs use different budgets for cheap and runtime-heavy cases.
Historical totals and one configuration value do not describe the complete present gate.

Every sub-decision remains Proposed.

| Sub-decision | Required resolution and evidence |
|---|---|
| S11.1 Practice rules | Preserve refutation blocking, cross-view claims, and resource-bounded liveness unless upstream explicitly supersedes a requirement. |
| S11.2 Negative controls | Record which controls run, their expected failures, and how the gate rejects unexpected success. |
| S11.3 Gate entries | Map every upstream claim through workflow, script, model, configuration, property, and result. Require an approved replacement before removal. |
| S11.4 Test tiers | Record each suite's actual cases, memory envelope, duration, and coverage. Obtain approval for changed qualification budgets. |
| S11.5 Decision authority | Keep paper scope, implementation history, upstream ratification, and evidence acceptance distinct. |
| S11.6 Formal completion | Map the proposed completion checklist to current artifacts and explicit assumptions. A bounded search is not an unbounded theorem. |
| S11.7 New consensus scope | Recompute the current file inventory. Attach each production boundary to a claim or an approved exception. |
| S11.8 Slashing reference | Compare theorem statements and implementation relations. A renamed theorem alone does not subsume bisimilarity. |
| S11.9 Workflow reachability | Trace each required verifier from an enabled workflow through its actual conditions. Keep optional Wolfram exploration outside the required gate. |
| S11.10 Contract concurrency | Establish the current waiver, implemented job, required trace, and approval status without inferring one from another. |
| S11.11 Scan benchmark | Locate the executable benchmark and its revision-bound measurements. Preserve the requirement if the executable or evidence is missing. |

**Activation boundary.** Verification requirements apply to the reviewed integration revision, not only after protocol deployment.
The user's separate regressor remains excluded from required CI until the user restores it.

**Required approval.** Upstream review must approve claim supersession and qualification changes.
User acceptance and machine verification must retain their distinct pgmcp provenance.
Reference: [D-11][d11].

## D-12: Phlo controls and arbitrary funding

**Observed state.** The current protobuf schema reserves the retired phlo tags.
The approved M1 scope requires restoration of signed phlo controls and arbitrary-payer funding before PR amendment.
The pgmcp policy task remains pending.

**Source and motivation.** Early DR-5 and DR-9 moved enforcement into the token model.
Later records replaced historical supply mirrors with native custody and reservation.
Those earlier decisions do not establish that the current client-exposure contract is complete.

The current plan separates logical authority from physical custody.
It requires capped max-min allocation, a pre-state residual cursor, canonical batch advancement, conversion provenance, and the configured payer cap.
These requirements must enter the up-front solver, signed intent, replay, and atomic settlement together.

**Required evidence.** Specify units, price validation, maximum exposure, residual allocation, and failure/refund behavior.
Test arbitrary configured payer counts, shared wallets across concurrent deploys, transfer of authority, deposits, insufficient balances, and arithmetic boundaries.
Neither two-payer tests nor signer-count multiplication establishes the required funding model.

**Open decision.** Reconcile D-12's removal proposal with the approved restoration scope.
Keeping client-signed phlo controls does not necessarily reject signature-indexed accounting.
A bounded, authenticated solver contract can relate both.

**Activation boundary.** New or restored signed fields require a coherent identity and encoding decision.
Do not reuse reserved tags with a different meaning or advertise unimplemented client behavior.

**Required approval.** Preserve recorded user economic decisions and obtain upstream approval for consensus-visible validation and wire changes.
Do not classify this required work as optional or postpone it beyond PR amendment.
Reference: [D-12][d12].

## Approval record requirements

Each approved sub-decision needs an exact statement, source revision, approving authority, comment link, date, and affected release boundary.
It also needs the required proof and regression evidence, including assumptions and unresolved exclusions.
A partial approval must name its sub-decision and must not flip the whole entry.

The current ledger records no such approvals.
This is a statement about the inspected sources, not a claim that approval could not exist elsewhere.
Any additional approval must be linked and checked before its status changes.

## Completion and verification limits

This status ledger completes a documentation deliverable only.
It does not implement, approve, or verify the proposed protocol choices.
The [PR 390 correction record](pr390-corrections-and-retained-findings.md) states the retained findings and required corrections.

Required qualification remains end-to-end across concurrent validators, replay, storage, resource bounds, clients, and the approved 24-hour soak.
No source-code or verification-gate change occurred while writing this ledger.

[ledger]: https://github.com/F1R3FLY-io/f1r3node-rust/blob/1d9ae249d75a39d8c0851bf71264307047c532e7/docs/casper/design/decision-ledger/README.md
[review-comment]: https://github.com/F1R3FLY-io/f1r3node-rust/pull/390#issuecomment-5552582235
[response-comment]: https://github.com/F1R3FLY-io/f1r3node-rust/pull/390#issuecomment-5552679288
[d01]: https://github.com/F1R3FLY-io/f1r3node-rust/blob/1d9ae249d75a39d8c0851bf71264307047c532e7/docs/casper/design/decision-ledger/01-protocol-version-authority.md
[d02]: https://github.com/F1R3FLY-io/f1r3node-rust/blob/1d9ae249d75a39d8c0851bf71264307047c532e7/docs/casper/design/decision-ledger/02-certified-floor-authority.md
[d03]: https://github.com/F1R3FLY-io/f1r3node-rust/blob/1d9ae249d75a39d8c0851bf71264307047c532e7/docs/casper/design/decision-ledger/03-fork-choice-certified-context.md
[d04]: https://github.com/F1R3FLY-io/f1r3node-rust/blob/1d9ae249d75a39d8c0851bf71264307047c532e7/docs/casper/design/decision-ledger/04-state-preserving-finality.md
[d05]: https://github.com/F1R3FLY-io/f1r3node-rust/blob/1d9ae249d75a39d8c0851bf71264307047c532e7/docs/casper/design/decision-ledger/05-finalization-publication.md
[d06]: https://github.com/F1R3FLY-io/f1r3node-rust/blob/1d9ae249d75a39d8c0851bf71264307047c532e7/docs/casper/design/decision-ledger/06-heartbeat-recovery-leadership.md
[d07]: https://github.com/F1R3FLY-io/f1r3node-rust/blob/1d9ae249d75a39d8c0851bf71264307047c532e7/docs/casper/design/decision-ledger/07-deploy-recovery-custody.md
[d08]: https://github.com/F1R3FLY-io/f1r3node-rust/blob/1d9ae249d75a39d8c0851bf71264307047c532e7/docs/casper/design/decision-ledger/08-merge-algebra-and-rejection-records.md
[d09]: https://github.com/F1R3FLY-io/f1r3node-rust/blob/1d9ae249d75a39d8c0851bf71264307047c532e7/docs/casper/design/decision-ledger/09-slashing-authorization.md
[d10]: https://github.com/F1R3FLY-io/f1r3node-rust/blob/1d9ae249d75a39d8c0851bf71264307047c532e7/docs/casper/design/decision-ledger/10-repeat-deploy-carrier-index.md
[d11]: https://github.com/F1R3FLY-io/f1r3node-rust/blob/1d9ae249d75a39d8c0851bf71264307047c532e7/docs/casper/design/decision-ledger/11-cbc-fv-governance.md
[d12]: https://github.com/F1R3FLY-io/f1r3node-rust/blob/1d9ae249d75a39d8c0851bf71264307047c532e7/docs/casper/design/decision-ledger/12-deploy-cost-limits.md
