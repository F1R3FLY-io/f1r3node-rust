# Deploy recovery protocol model

| TLA⁺ element | Rust realization |
| --- | --- |
| `LocalActiveSources(v)` | `interpreter_util::canonical_disposition_sets` over a validator's visible parent scope |
| `RetryEligible(v)` | rejected-buffer selection in `block_creator::prepare_user_deploys_with_policy` |
| `LeaderAt(observedFinalizedHeights[v])` | finalized-height rotation in `block_creator::recovered_deploy_leader` |
| `CandidateBlockHeight(v)` | the validator-local `CasperSnapshot::max_block_num + 1` expiry boundary |
| `PrepareRetry` / `PublishRetry` | block construction followed by asynchronous block visibility |
| `survivesSelfChainFilter` | selected recovery signatures bypass only the legacy self-chain duplicate filter in `block_creator::create` |
| `excludedSources(v)` | historical self-chain occurrences outside validator `v`'s selected-parent closure |
| `CandidateActiveSources(v)` | active occurrences reachable from validator `v`'s immutable candidate parents |
| `ObserveOccurrence` / `ObserveTombstone` | eventual DAG propagation of exact occurrence dispositions |
| `Advance` | ordinary heartbeat/finality-support blocks, including non-leaders |

The model separates proposal height from finalized height and gives each
validator independently lagging observations of both. It also models delayed
visibility for winning occurrences and their exact-source tombstones. A leader
is unique for one committed finalized-height view. Validators temporarily on
different finalized views can therefore prepare concurrent retries, as an
asynchronous protocol must permit. Those retries remain bounded to one pending
retry per distinct finalized view and one per validator. Once any retry is
visible as an active occurrence, an occurrence-aware validator cannot prepare
another until every visible exact source is tombstoned.

Selection authorization is preserved through packaging. The self-chain filter
removes only occurrences active in the selected-parent closure. It cannot
remove a retry already selected from the exact-source recovery projection or a
retained deploy whose only historical occurrence lies on an excluded branch.
The two packaging controls distinguish those authorization paths.

| Configuration | Expected result | Defect isolated |
| --- | --- | --- |
| `MC_DeployRecovery.cfg` | pass | occurrence-aware, expiry-bounded, view-relative elected recovery with eventual progress |
| `MC_DeployRecovery_signature_pre_fix.cfg` | violate `Inv_RetryRequiresNoActiveSource` | any visible rejection authorizes retry despite a surviving visible source |
| `MC_DeployRecovery_expiry_pre_fix.cfg` | violate `Inv_NoExpiredRetry` | recovery bypasses the validator's proposal-height lifespan boundary |
| `MC_DeployRecovery_multi_leader_pre_fix.cfg` | violate `Inv_OneRecoveryProposerPerFinalizedView` | multiple validators prepare retries from the same finalized-height view |
| `MC_DeployRecovery_heartbeat_pre_fix.cfg` | violate `Live_RecoveryOrExpiry` | an offline elected leader prevents proposal and finality views from advancing |
| `MC_DeployRecovery_packaging_pre_fix.cfg` | violate `Inv_SelectedRetrySurvivesSelfChainFilter` | a canonically authorized retry is selected and then silently removed by downstream self-chain filtering |
| `MC_DeployRecovery_rehome_pre_fix.cfg` | violate `Inv_SelectedRehomeSurvivesCandidateFilter` | an excluded historical self-chain occurrence masks retained work selected against different candidate parents |

`MergeRecoveryCoherence.tla` closes the refinement boundary between occurrence
records and the state rooted at the finalized merge floor. Base-committed
receipts, rather than raw floor ancestry, identify effects already materialized
in the floor state. The model filters authoritative exact above-floor tombstones
before state application, expands both a tombstone and a finalized-base
duplicate to their complete dependent chains, and requires ordinary effects and
merge metadata to follow the same selected occurrence projection. An occurrence
rejected before the floor remains recoverable because it has no base-committed
receipt.

| Configuration | Expected result | Defect isolated |
| --- | --- | --- |
| `MC_MergeRecoveryCoherence.cfg` | pass | finalized-base precedence, exact tombstone filtering, base/scope deduplication, state-record coherence, and numeric single-datum settlement |
| `MC_MergeRecoveryCoherence_base_precedence_unsafe.cfg` | violate `Inv_AtMostOneEffectPerSignature` | a sibling tombstone masks an effect already materialized in the finalized base and authorizes a duplicate retry |
| `MC_MergeRecoveryCoherence_tombstone_filter_unsafe.cfg` | violate `Inv_TombstonedScopeNotApplied` | an exact above-floor tombstone is recorded after the source chain has already entered state application |
| `MC_MergeRecoveryCoherence_base_duplicate_unsafe.cfg` | violate `Inv_AtMostOneEffectPerSignature` | deduplication compares only above-floor candidates and misses a same-signature effect in the finalized base |
| `MC_MergeRecoveryCoherence_metadata_coverage_unsafe.cfg` | violate `Inv_TaggedNumberSingleDatum` | an exact numeric effect bypasses merge metadata and leaves a second datum beside the folded value |
| `MC_MergeRecoveryCoherence_tombstone_authority_unsafe.cfg` | violate `Inv_InvalidTombstoneCannotErase` | a non-causal or unvalidated tombstone is allowed to erase a scope occurrence |
| `MC_MergeRecoveryCoherence_partial_chain_unsafe.cfg` | violate `Inv_ChainAtomic` | only the named tombstone/base duplicate is filtered while a dependent effect from the same chain remains selected |
| `MC_MergeRecoveryCoherence_ordinary_retention_unsafe.cfg` | violate `Inv_StateRecordCoherence` | the rejected occurrence is removed but its ordinary state effect remains |
| `MC_MergeRecoveryCoherence_mergeable_retention_unsafe.cfg` | violate `Inv_StateRecordCoherence` | the rejected occurrence is removed but its mergeable contribution remains |
| `MC_MergeRecoveryCoherence_effect_identity_unsafe.cfg` | violate `Inv_EffectIdentityConsistency` | one causal effect identity resolves to inconsistent normalized effect data |

`EffectCausalClosure.tla` refines chain atomicity to the exact per-execution
state-witness level. A rejected sibling `closeBlock` effect seeds rejection. One
effect physically consumes that stale resource, a second depends transitively on
the first, a merge effect consumes the retained finalized-base `closeBlock`, and
a user effect is independent. Classification order is nondeterministic but
dependency-ready. The safe model computes the least transitive causal closure:
the stale causal chain is rejected, while the retained-base merge and independent
user effects survive. Mergeable materializations are outside the physical
dependency relation and remain governed by their typed algebra.

| Configuration | Expected result | Defect isolated |
| --- | --- | --- |
| `MC_EffectCausalClosure.cfg` | pass | complete order-independent causal rejection closure, independent-effect survival, and eventual classification |
| `MC_EffectCausalClosure_block_lineage_unsafe.cfg` | violate `Inv_IndependentEffectsSurvive` | blanket source-block descendant expansion deletes exact effects unrelated to the rejected state transition |
| `MC_EffectCausalClosure_direct_only_unsafe.cfg` | violate `Inv_NoAcceptedDependsOnRejected` | a one-hop scan accepts a transitive dependent after its direct source is rejected |

`FinalizedOccurrenceStatus.tla` closes the observability boundary after state
merge. Occurrences and exact tombstones are delivered in every order from the
complete LFB causal closure. The safe projection removes a source named by a
secondary-parent tombstone and reports the distinct surviving occurrence,
matching committed state. The unsafe projection deliberately consults only the
main-parent spine and therefore reports two active sources after state has kept
one.

| Configuration | Expected result | Defect isolated |
| --- | --- | --- |
| `MC_FinalizedOccurrenceStatus.cfg` | pass | all-parent exact status equals committed active occurrence state under every evidence order |
| `MC_FinalizedOccurrenceStatus_main_chain_unsafe.cfg` | violate `Inv_StatusMatchesCommittedState` | main-chain-only status ignores a secondary-parent exact tombstone |
| `MC_FinalizedOccurrenceStatusApalache.cfg` | pass | typed bounded verification of the same all-parent projection |
| `MC_FinalizedOccurrenceStatus_main_chain_unsafe_Apalache.cfg` | violate `Inv_StatusMatchesCommittedState` | symbolic counterexample to main-chain-only exact status |

`RejectionReasonConfluence.tla` covers the diagnostic refinement carried by an
exact tombstone. Concurrent descendants can reject the same source occurrence
for different valid reasons because each descendant observes a different merge
closure. The reason field does not authorize suppression; the exact occurrence
key does. Reasons therefore combine under a canonical precedence join:
`duplicate_occurrence` dominates `merge_conflict`, which dominates
`collateral_chain_drop`, with `unspecified` as the identity used only before a
current-protocol cause is observed. The join is commutative, associative, and
idempotent, so validators with the same evidence converge independently of
arrival order.

| Configuration | Expected result | Defect isolated |
| --- | --- | --- |
| `MC_RejectionReasonConfluence.cfg` | pass | equal causal observations produce one canonical reason under every interleaving |
| `MC_RejectionReasonConfluence_last_writer_unsafe.cfg` | violate `Inv_EqualObservationConverges` | last-writer replacement makes the serialized reason depend on observation order |

`ProtocolActivationCoherence.tla` models the consensus migration boundary. The
**active protocol version** is the shard-wide version that validates and creates
the candidate block. The **floor protocol version** is historical metadata on the
finalized block whose state is the merge base. The active version, never the
historical floor version, selects exact finalized-receipt precedence. Every
above-floor block admitted to one merge scope has the active version, and every
disposition record satisfies the encoding required by its own block header.
Consequently, the merge algebra remains coherent if it is presented with a
current-protocol scope and a historically encoded floor: a sibling tombstone or
duplicate occurrence cannot mask an effect already materialized in the floor
state. This is a defensive state-composition invariant, not an in-place upgrade
path. The D3 wire migration is fresh-genesis: this binary admits only protocol 3
as an approved running protocol and rejects protocols 1 and 2 before Casper
starts. Protocol 2 remains the historical exact rejected-deploy threshold.

| Configuration | Expected result | Defect isolated |
| --- | --- | --- |
| `MC_ProtocolActivationCoherence.cfg` | pass | active-version base precedence, homogeneous scope, version-bound encoding, and legacy-floor composition |
| `MC_ProtocolActivationCoherence_floor_version_unsafe.cfg` | violate `Inv_AtMostOneEffectPerSignature` | the legacy floor version disables exact base receipts and admits a duplicate effect |
| `MC_ProtocolActivationCoherence_mixed_scope_unsafe.cfg` | violate `Inv_ActiveScopeVersionHomogeneous` | a legacy above-floor block enters an exact-protocol merge scope |
| `MC_ProtocolActivationCoherence_encoding_unsafe.cfg` | violate `Inv_EncodingMatchesVersion` | a current block carries a legacy disposition encoding |

`ProtocolVersionLifecycle.tla` closes the lifecycle that the activation model
intentionally abstracts away. It follows the version from genesis candidate
construction through validator approval, approved-block admission, node-wide
adoption, proposal, and peer reception. The configured current version is 3;
the supported active set is exactly `{3}`. A current ceremony therefore reaches
running consensus with every node using one adopted version, while recovery from
a protocol-1, protocol-2, or otherwise unsupported approved block fails closed before any
proposal can be made. There is no accounting enable/disable flag and no
block-height transition between two charging engines.

The original disagreement is retained as an executable negative control. A
protocol-1 approved genesis combined with a locally configured current-protocol
proposer produces current-version blocks while receivers compare against version 1;
`Inv_AllReceiversAccept` then fails. Separate controls demonstrate that the same
single-authority property is necessary at ceremony, adoption, proposal, and
unsupported-version admission.

| Configuration | Expected result | Defect isolated |
| --- | --- | --- |
| `MC_ProtocolVersionLifecycle.cfg` | pass | current ceremony, approval, adoption, proposal, and reception use protocol 3 end to end |
| `MC_ProtocolVersionLifecycle_legacy_rejected.cfg` | pass | a protocol-1 approved block fails closed before running |
| `MC_ProtocolVersionLifecycle_unsupported_rejected.cfg` | pass | an unknown approved version fails closed before running |
| `MC_ProtocolVersionLifecycle_ceremony_unsafe.cfg` | violate `Inv_CeremonyCandidateCurrent` | genesis construction emits a stale protocol version |
| `MC_ProtocolVersionLifecycle_adoption_unsafe.cfg` | violate `Inv_RunningNodesAdoptApproved` | nodes retain local configuration instead of adopting the approved version |
| `MC_ProtocolVersionLifecycle_proposer_unsafe.cfg` | violate `Inv_ProposalUsesApprovedVersion` | proposal construction bypasses the adopted running version |
| `MC_ProtocolVersionLifecycle_receiver_unsafe.cfg` | violate `Inv_AllReceiversAccept` | the exact configured-current proposer versus approved-legacy receiver disagreement |
| `MC_ProtocolVersionLifecycle_unsupported_unsafe.cfg` | violate `Inv_ApprovedVersionSupported` | an unsupported approved block starts Casper |

`ApprovedStateReplay.tla` closes the approved-state bootstrap boundary. A
historical block is replayed from the immutable context serialized by that
block: its pre-state root, block data, genesis allocation payload, successful
slash evidence, and protocol version. The node may have a newer approved tip
and a different local configuration, but neither is an input to historical
execution. This makes reconstructed roots a function of consensus bytes rather
than the order in which the joiner learned them.

| Configuration | Expected result | Defect isolated |
| --- | --- | --- |
| `MC_ApprovedStateReplay.cfg` | pass | every historical replay uses its block-bound context, reconstructs the declared root, and reaches running state |
| `MC_ApprovedStateReplay_current_context_unsafe.cfg` | violate `Inv_ReplayUsesConsensusContext` | replay substitutes the joiner's current approved-tip context and falsely invalidates valid history |

`LocalValidationRecovery.tla` separates objective invalidity from a local,
inconclusive validation fault. A locally faulted parent leaves the ready queue,
opens one recovery request, and stays deferred if the transport attempt fails.
Only successful recovery can make it ready again. Its ordinary descendants
remain blocked until that exact parent has validated; local faults never create
slash evidence or an invalid-block terminal state. Weak fairness proves that a
transient fault followed by recovery validates both parent and child.

| Configuration | Expected result | Defect isolated |
| --- | --- | --- |
| `MC_LocalValidationRecovery.cfg` | pass | bounded deferred recovery, transport-failure safety, parent gating, and eventual parent/child validation |
| `MC_LocalValidationRecovery_ready_unsafe.cfg` | violate `Inv_NoImmediateSelfRequeue` | retaining an inconclusive parent as a dependency-free pendant causes immediate self-requeue |

`FundingAdmissionLifecycle.tla` closes the client-visible lifecycle for a
state-bound funding decision. Proposal records both the decision and the exact
pre-state supply from which it was derived. Validation recomputes from that
recorded state. An underfunded attempt is therefore a terminal rejected record
with no user effects, rather than an unrecorded candidate that remains pending
until an unrelated later top-up changes its classification. A fundable deploy
cannot be forged as rejected, and later supply cannot resurrect a finalized
rejection.

| Configuration | Expected result | Defect isolated |
| --- | --- | --- |
| `MC_FundingAdmissionLifecycle.cfg` | pass | proposal/validation agreement, terminal rejection, zero rejected effects, and eventual finalization |
| `MC_FundingAdmissionLifecycle_live_state_unsafe.cfg` | violate `Inv_ValidatorUsesProposalPreState` | a validator reclassifies the block after an unrelated live-state top-up |
| `MC_FundingAdmissionLifecycle_pending_unsafe.cfg` | violate `Inv_UnderfundedAttemptLeavesPending` | an attempted underfunded deploy has no consensus-visible terminal record and remains pending |

## Admission records and runtime-effect metadata

A **status record** is a consensus-visible statement about a deploy lifecycle.
An **effect record** is a deploy or system execution that entered the runtime and
therefore has one position in the ordered merge-metadata stream. A terminal
funding-admission rejection is a status record but not an effect record. An
ordinary deploy that entered the runtime and failed is both an execution-failure
status and an effect record, because its attempted execution still owns its
position in the state-witness and merge-metadata sequence.

For user records $U$, system executions $S$, and merge metadata $M$, the
alignment boundary is:

$$
\operatorname{effects}(U)
  = [u \in U \mid u.\operatorname{admissionStatus} \ne \text{Rejected}]
$$

$$
|M| = |\operatorname{effects}(U)| + |S|
$$

`AdmissionEffectAlignment.tla` checks this refinement independently at three
validators. Each validator indexes the same block containing one terminal
funding rejection and one executed `closeBlock`. Under the effect projection,
one merge-metadata entry aligns with `closeBlock`; all validators can propose a
successor, and a later deploy finalizes. The unsafe control counts both status
records as effects, expects two metadata entries, blocks every validator at
parent indexing, and prevents later finalization.

| Configuration | Expected result | Defect isolated |
| --- | --- | --- |
| `MC_AdmissionEffectAlignment.cfg` | pass, including `Live_AllValidatorsPropose` and `Live_LaterDeployFinalizes` | exact status/effect projection preserves proposal liveness |
| `MC_AdmissionEffectAlignment_status_count_unsafe.cfg` | violate `Inv_StatusOnlyRecordCannotBlock` | a terminal admission record is incorrectly assigned an execution slot |
| `MC_AdmissionEffectAlignmentApalache.cfg` | pass | symbolic bounded verification of the exact effect projection |
| `MC_AdmissionEffectAlignmentUnsafeApalache.cfg` | violate `Inv_StatusOnlyRecordCannotBlock` | symbolic reproduction of validator proposal failure |

The Rocq refinement in `AdmissionEffectAlignment.v` proves that inserting an
admission rejection anywhere in the user-record sequence does not change the
effect projection, an executed failure retains one slot, permutation does not
change the required cardinality, and aligned metadata splits exactly between
user and system executions. Its concrete regression theorem proves that one
funding rejection plus one `closeBlock` requires one metadata entry, whereas
counting block-body status records would incorrectly require two.

The liveness property permits two outcomes once every published occurrence is
tombstoned and at least one online validator still sees the deploy in its valid
proposal-height window: a new active occurrence is published or every online
validator advances beyond the window. It assumes weakly fair publication,
observation, and heartbeat/finality progress. It does not assume simultaneous
validator observations or a globally unique leader during transient view lag.
