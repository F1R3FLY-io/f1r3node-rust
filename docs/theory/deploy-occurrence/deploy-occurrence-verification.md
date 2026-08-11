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

Three implementation shortcuts then crossed the abstraction boundary:

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

### Property-based tests

- `DeployChainIndex::cmp` is a strict total order and `cmp == Equal` implies
  equality;
- total merge cost does not overflow;
- occurrence reduction is invariant under observation permutation;
- recovery projection preserves every untombstoned source;
- occurrence index and its compatibility representative are invariant under
  block insertion permutation.

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
cargo test -p block-storage --features test-internals --test block_dag_storage_test deploy_index
cargo test -p casper --lib deploy_finalization_status::tests
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
