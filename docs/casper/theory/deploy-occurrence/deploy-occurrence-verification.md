# Deploy occurrence verification and operations

## Verification claims

| Claim | Formal artifact | Rust evidence |
| --- | --- | --- |
| An exact tombstone removes its source and preserves another source of the same deploy | `OccurrenceDisposition.rejection_is_source_exact`, `distinct_source_survives_rejection` | `source_tombstone_removes_only_its_exact_occurrence` |
| Tombstone application order does not affect membership | `OccurrenceDisposition.rejection_order_independent` | `occurrence_reduction_is_independent_of_observation_order` |
| Rejecting one duplicate preserves one winner | `OccurrenceDisposition.one_winner_preserved` and `finalized_floor_occurrence_correct` | dedup and multi-validator recovery specifications |
| Equal observation sets produce equal canonical projections | TLA⁺ `Inv_ObservationOrderConverges` | occurrence-index insertion-order property and total-order proptest |
| Signature-only rejection can erase every duplicate | TLA⁺ pre-fix config violates `Inv_OneWinnerPreserved` | legacy records are compatibility-only; new records require source provenance |
| Finalization cannot jump to a sibling of the current LFB | exact-hash ancestry premise in the finalized-floor design | `finalizer_never_moves_to_a_sibling_of_the_exact_lfb` |
| Stale rejection history cannot narrow parents indefinitely | deploy lifecycle admission model | `historical_rejection_without_local_backlog_does_not_trigger_recovery` |
| Retry requires every exact source to be tombstoned | Rocq `no_active_iff_all_sources_tombstoned`, `retry_requires_no_active_source`, TLA⁺ `Inv_RetryRequiresNoActiveSource` | `recovery_projection_preserves_every_untombstoned_source`, `exact_rejection_preserves_another_source_as_canonical_win` |
| Recovery cannot cross the deploy lifespan boundary | Rocq `expiry_closes_recovery`, TLA⁺ `Inv_NoExpiredRetry` | `recovered_buffered_deploy_is_purged_after_block_expiry`, `rejected_buffer_backlog_requires_selectable_deploy` |
| Only a rejected carrier's owner packages its retry | Rocq `recovery_custody_authorization_unique_per_carrier`; TLA⁺ `Inv_RetryHasCarrierOwnerCustody`, `Inv_OneRecoveryProposerPerCarrier` | unchanged retry-gate and carrier-owner integration tests |
| Candidate-relative packaging suppresses only an active duplicate and rehomes a historical occurrence excluded from the selected-parent closure | Rocq `finalized_floor_candidate_scope_rehome_correct`; TLA⁺ `Inv_SelectedRehomeSurvivesCandidateFilter` and `MC_DeployRecovery_rehome_pre_fix.cfg` | `self_chain_filter_is_candidate_scope_relative`, `candidate_scope_packaging_matches_the_captured_authorization`, and `loom_candidate_scope_deploy_rehome` |
| Distinct carrier owners can recover distinct work concurrently | Rocq `distinct_carrier_owners_recover_independently`; TLA⁺ `Inv_ParallelRecoveryIsSourceDistinct`; Loom `loom_recovery_custody` | exact-occurrence admission tests |
| Recovery expiry uses proposal height, not finalized height | TLA⁺ `Inv_RecoveryHeightUsesCommittedDagView`, `Inv_NoExpiredRetry` | exact proposal-height expiry tests |
| An unavailable carrier owner cannot halt finality | TLA⁺ `Live_RecoveryOrExpiry` | heartbeat and consensus-safety system integration scenarios |
| Multiple exact tombstones in one block are one rejection event | occurrence-aware status reducer | `multiple_exact_rejections_in_one_block_count_as_one_rejection_event` |
| An exact tombstone in a secondary-parent ancestor affects status exactly as it affects committed state | Rocq `finalized_closure_rejection_is_authoritative`, TLA⁺ `Inv_StatusMatchesCommittedState` | `source_aware_rejection_in_secondary_parent_is_authoritative` |
| Restricting exact tombstones to the main-parent spine is unsound | Rocq `main_chain_only_projection_is_incomplete`; TLC and Apalache `MC_FinalizedOccurrenceStatus_main_chain_unsafe*` | the secondary-parent regression fails under the old filter |
| Atomic v6 admission keeps metadata, occurrence, and lifecycle state aligned | TLA⁺ `DeployOccurrenceStorage.tla`, Rocq `deploy_occurrence_storage_contract` | strict transaction rollback and occurrence insertion tests |
| Fresh-v6 activation rejects legacy and partial state | TLA⁺ `Inv_FreshActivation`, Rocq `successful_activation_has_no_legacy_or_partial_rows` | `fresh_activation_rejects_legacy_or_partial_rows` |
| Terminalization preserves exact history while it removes open hot state | Rocq `terminalization_preserves_exact_archive`, `terminalization_is_atomic_across_occurrence_and_lifecycle_state` | compaction, LMDB reopen, and terminal integration tests |
| Concurrent insertion, lookup, and compaction preserve one canonical view | TLA⁺ `Inv_ReplicaConvergence`; Rocq `canonical_rank_is_permutation_invariant` | property tests and Loom occurrence-store tests |
| Equal bytes in the legacy-signature and v6-commitment domains remain distinct throughout recovery | TLA⁺ `DeployIdentitySeparation`, including the raw-key control; Rocq `equal_payload_cross_domain_ids_are_distinct` and both rejection-preservation theorems | `recovery_projection_keeps_legacy_and_v6_identity_domains_disjoint` and `concurrent_legacy_and_v6_dispositions_do_not_alias` |

The Rocq capstone is checked with `Print Assumptions` and `coqchk`. The TLA⁺
post-fix models must exhaust their bounded state spaces without violation. Each
pre-fix configuration is successful only when it reproduces its named
counterexample.

## Why the earlier verification missed the defect

The previous merge proof represented a deploy by its signature. That abstraction
was adequate while the implementation also treated one signature as one stored
occurrence. Cost accounting changed the interference relation: same-signer
deploys now compete for the same linear funding resource, so independent
validators can create source-distinct occurrences that reach the same merge.

Four implementation shortcuts then crossed the abstraction boundary:

1. `DeployChainIndex` equality collapsed chains with equal deploy sets even when
   their source blocks differed.
2. rejected deploys lost their source block before block serialization;
3. the deploy index stored one block and overwrote it according to arrival order;
4. recovery admission reduced exact source events back to a latest-height
   signature verdict.

The formal model proved deterministic set merge after occurrence identity had
already been erased. It therefore could not state, much less falsify, the
one-winner-preservation property. This was a modeling omission exposed by the
cost-accounting branch, not evidence that the rho calculus promised a particular
block winner.

The first occurrence repair still stopped one layer too early. Its model ended
at the canonical active/rejected projection and then represented the shard with
one height and one instantaneous validator view. It did not include the
proposer that consumes the projection, the distinction between proposal and
finalized height, delayed occurrence/tombstone visibility, concurrent
validators taking different finalized views, or the heartbeat transition
needed to rotate past an offline leader. Consequently, a raw signature-wide
rejection set, an expiry exemption, and a recovery-only heartbeat suppression
could each fit behind a formally correct occurrence reducer, while the model's
global single-leader claim was stronger than an asynchronous implementation can
realize. `DeployRecovery.tla` now composes independently lagging proposal and
finalized views, asynchronous exact-source visibility, preparation/publication,
and heartbeat/finality progress. Its safe invariants permit different carrier
owners to prepare independent retries in the same finalized view while proving
one owner per carrier. A reachable-state witness prevents that concurrency
claim from passing vacuously. The foreign-custody control rejects a validator
that retries another validator's carrier, and the heartbeat control now targets
finalization progress directly.

The next abstraction leak reused the same 32 bytes as both a pre-v6 signature
and a v6 envelope commitment after storage had already decoded their protocol
domains. Recovery, snapshot, admission, and merge caches then keyed those
values by untagged bytes. `DeployIdentitySeparation.tla` models the exact
collision: rejecting one domain erases the other under a raw key, while the
pair $`(\mathit{protocol\ domain}, \mathit{payload})`$ preserves both. Rocq proves
the separation for arbitrary payloads, the Rust property ranges over every
generated 32-byte value and both insertion orders, and Loom explores concurrent
legacy/v6 disposition publication. The persistent formats remain unchanged;
tag erasure is forbidden only inside decoded in-memory consensus state.

The next liveness defect crossed the same boundary in a different direction.
Admission used `snapshot.deploys_in_scope`, but packaging later rescanned the
validator's raw historical self-chain. After floor advancement excluded an old
branch, admission correctly authorized the deploy while packaging still removed
it as a duplicate. `DeployRecovery.tla` now models excluded historical sources
and a candidate-captured authorization. The safe model exhausts every reachable
state while `MC_DeployRecovery_rehome_pre_fix.cfg` reproduces the stale-filter
counterexample. The Rocq classifier proves that only an active candidate-scope
duplicate is suppressed. Three Loom models cover concurrent floor advancement,
parallel validator rehome, and concurrent cleanup against the captured context.

## Relationship to the publications

The publications supply the design constraints, not the complete node protocol:

- *A Reflective Higher-Order Calculus* establishes the calculus foundation,
  DOI [10.1016/j.entcs.2005.05.016](https://doi.org/10.1016/j.entcs.2005.05.016).
- [Cost-Accounted Rho Calculus](https://github.com/F1R3FLY-io/publications/blob/main/cost-accounting/cost-accounted-rho.tex)
  specifies deployment atomicity, linear funding competition, and the intended
  shift from post-hoc conflict analysis toward acceptance-time resource proofs.
- [Quoting is Colour-Swap](https://github.com/F1R3FLY-io/publications/blob/main/denotational-semantics-for-rho/knot-rho.tex)
  treats parallel composition as keyed multiset union and explicitly locates
  nondeterminism in communication pairing.
- [Choice and Scheduling](https://github.com/F1R3FLY-io/publications/blob/main/choice-types/choice-scheduling.tex)
  distinguishes confluent protocol structure from genuine nondeterministic
  resolution.

None of these documents defines the F1R3node DAG's exact cost/hash tie-break or
wire-format tombstone. Those are protocol obligations resolved here under the
published constraints.

## Test ladder

### Example-based tests

- protobuf round-trip for source and reason;
- legacy protobuf compatibility;
- exact-source tombstone preservation;
- stale-history recovery regression;
- exact-LFB sibling exclusion;
- invalid block exclusion from occurrence lookup;
- finalization API returns the surviving source block.
- finalization API applies an exact rejection recorded in a secondary-parent
  ancestor and returns the distinct surviving source block.

### Property-based tests

- `DeployChainIndex::cmp` is a strict total order and `cmp == Equal` implies
  equality;
- total merge cost does not overflow;
- occurrence reduction is invariant under observation permutation;
- recovery projection preserves every untombstoned source;
- occurrence index and its compatibility representative are invariant under
  block insertion permutation.
- retry eligibility is equivalent to an empty active-source set;
- retry selection is invariant under parent and validator observation order;
- exact lifespan boundaries are closed to both backlog probing and selection;
- finalized-height leader rotation is invariant under parent sender and order;
- each fixed finalized-height view elects exactly one validator after validator-set normalization;
- proposal-height expiry and finalized-height leader rotation remain independent.
- candidate-scope packaging matches the immutable authorization captured before
  concurrent floor movement.

### Concurrency tests

- a captured excluded-branch rehome survives concurrent floor advancement;
- independent validators rehome without a global lock and their results commute;
- an occurrence active in the captured candidate scope remains suppressed while
  another thread cleans historical state.

### Integration tests

- dedup orphan recovery;
- multi-validator recovery;
- repeat-deploy recovery misfire;
- deploy summary agreement across every validator and read-only node;
- read-only block polling through body-received/DAG-pending state;
- bounded-memory system integration with per-node RSS attribution.

The consensus tests must compare exact source block hashes across nodes. A test
must never weaken this to “all nodes returned some block.”

## Running verification

```bash
scripts/check-deploy-lifecycle-ALL.sh
scripts/check-finalized-floor-ALL.sh
cargo test -p models rejected_deploy
cargo test -p block-storage deploy_occurrence_store --lib
cargo test -p block-storage --test loom_deploy_occurrence_store
cargo test -p block-storage --test block_dag_storage_test invalid_blocks_are_diagnostic_only_and_do_not_enter_deploy_indices
cargo test -p block-storage --test block_dag_storage_test v6_terminal_write_prunes_lifecycle_and_active_occurrence_state_atomically
cargo test -p casper --lib deploy_finalization_status::tests
cargo test -p casper --test mod multiple_exact_rejections_in_one_block_count_as_one_rejection_event
cargo test -p casper --test mod source_aware_rejection_in_secondary_parent_is_authoritative
cargo test -p casper --test mod -- finalizer
```

Verification logs are written under `target/verification`, not `/tmp`. A clean
gate removes its own logs. Failed logs remain on disk for diagnosis.

## Operational signals

| Signal | Meaning | Operator response |
| --- | --- | --- |
| `DeployDispositionAmbiguity` | More than one finalized source survived exact tombstones | Stop rollout; preserve DAG and block-store data; compare rejection provenance across validators |
| HTTP `409 block_pending_admission` | Body received before local DAG admission | Retry with bounded backoff; do not treat as block absence |
| `block-processing.in-flight` | Blocks currently retained by processing | Correlate sustained growth with queue depth and role |
| `process.rss-kb` | Node resident memory after block processing | Attribute by validator/read-only role before changing limits |

## Completion criteria

The repair is complete only when:

1. all post-fix formal gates pass and all pre-fix gates reproduce the intended
   counterexamples;
2. focused unit and property tests pass;
3. the full workspace test suite passes;
4. the system-integration deploy-summary test reports one identical block hash
   on every node across repeated runs;
5. the memory guardian remains below its existing ceiling without relaxing the
   ceiling.
