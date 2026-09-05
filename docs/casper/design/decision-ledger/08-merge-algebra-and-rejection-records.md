# D-08 Merge Algebra, Rejection Records, and Mergeable Evidence

**Status:** Proposed. Pending maintainer ratification.
**Kind:** Protocol. Changes observable merge results.
**Sources:** dev [merge-algebra specification](../../theory/merge-algebra/merge-algebra-specification.md) rules R-ORDER, R-KEEP1, R-FOLD, R-NET, N-SEMANTICS, [Consensus Protocol](../../CONSENSUS_PROTOCOL.md) section 6. PR #216 `merge-algebra-specification.md` rules R-ORDER, R-WITNESS, R-CAUSAL, R-CAUSAL-REJECT, R-FOLD, R-NUMERIC, R-NET, R-ACTIVATION, R-RECORD-VERSION, N-MAX, N-WHOLE, invariants S8 to S12, DR-51, DR-53, `mergeable-evidence-authentication.md`, `admission-effect-alignment.md`, `MergeableEvidenceAuthentication.tla`, `AdmissionEffectAlignment.tla`, `formal/z3/merge_algebra`.

## 1. Question

How do surviving deploy chains compose into one state, what identifies an execution during composition, and where does the merge get its mergeable-channel evidence?

## 2. Position on dev

- **R-ORDER.** The keep-one comparator is a strict total order with a five-key sequence ending in the injective `deploys_with_cost` key.
- **R-FOLD.** The merged-state fold proceeds in the single canonical order induced by the comparator, because the fold operator is non-associative.
- **R-NET, Finding A.** The per-channel `ChannelChange::combine` is an idempotent max-multiset union with cancellation. It is commutative but not associative. This is a disclosed property, benign only because R-ORDER pins one fold order.
- **N-SEMANTICS.** The hardening must not change merge semantics. The non-associative max-union must not be replaced by the associative sum-union monoid as part of a determinism fix, because that alters which data survive a merge. A detection-only runtime guard is allowed because it changes no post-state.
- Mergeable-channel vectors come from the block index cache keyed by post-state, creator, and sequence number. Last-finalized-state synchronization imports peer-supplied vectors.

## 3. Position on PR #216

- **R-WITNESS.** Every execution in a new-format block carries the state root before and after it. The witnesses form one chain from block pre-state to post-state. A delta is computed from the execution's own roots.
- **R-CAUSAL.** Deduplication operates on `(source block hash, execution index)`, never on serialized content. Repeated observations of one identity with equal contributions count once. Unequal contributions fail closed.
- **R-FOLD.** After deduplication, distinct execution deltas compose by additive multiset union and normalize once. The fold is associative and commutative. It is intentionally not idempotent: two independent sends with identical payloads are two data.
- **N-MAX and S8.** Max-union must not compose distinct execution effects. Two distinct executions that emit identical data and collapse to one datum is a safety violation.
- **R-CAUSAL-REJECT.** Rejection propagates through the least transitive closure of physical effect dependencies. An exact effect outside that closure survives even when its source block descends from a rejected block.
- **R-NUMERIC.** Integer-add channels decide from the simultaneous total. The survivor selector and the trie-action builder call the same aggregate.
- **R-ACTIVATION and R-RECORD-VERSION.** The change activates atomically at a protocol boundary after a finalized cut. A merge epoch never mixes exact and legacy indices above the floor. The header version selects the record encoding.
- **R-ORDER.** The comparator's terminal key becomes `(deploys_with_cost, source_block_hash, effect_indices, witness_mode)`.
- **DR-51.** Mergeable evidence is reconstructible auxiliary state. Only successful local execution or replay publishes it. The cache key binds pre-state, post-state, creator, sequence, and a canonical executed-payload digest. Synchronization ignores every peer vector. Missing entries are replayed locally.
- **DR-53.** Merge metadata counts only records whose admission status is not rejected. A terminal admission rejection consumes no execution index.
- The specification cites the RSpace denotation paper for parallel composition as keyed multiset union.

## 4. Divergence

| Aspect | dev | PR #216 |
|---|---|---|
| Fold operator | Max-multiset union, non-associative, canonical order | Additive multiset union, associative, one normalization |
| Duplicate identical outputs | Collapse to one datum | Two data |
| Rule on changing the operator | N-SEMANTICS forbids it | N-MAX forbids the old operator |
| Execution identity | Deploy chain | `(source block, execution index)` |
| Rejection closure | Dependents of rejected deploys | Least fixed point over physical dependencies |
| Numeric channels | Checked addition per diff | Simultaneous total, one range check |
| Evidence source | Local cache, peer import on sync | Local replay only |
| Cache key | Post-state, creator, sequence | Plus pre-state and payload digest |
| Wire | None | Per-execution witnesses in the block |

## 5. Options

- **A. Ratify the additive semantics at the protocol boundary.** N-SEMANTICS is re-scoped to forbid a silent change, which R-ACTIVATION satisfies.
- **B. Keep max-union and ratify only DR-51 and DR-53.** Evidence authentication and the admission projection do not depend on the fold operator.
- **C. Reject the change.** Treat S8 as intended behavior.

## 6. Unification proposal

Adopt option A, with DR-51 and DR-53 ratified now under option B's reasoning.

This is the one entry where a dev rule and a PR #216 rule forbid each other. N-SEMANTICS was written to stop a determinism fix from changing merge results by accident. R-ACTIVATION honors that intent: the change is explicit, versioned, and never mixed above a floor. The remaining question is which semantics is correct, and the PR #216 argument is that max-union loses valid multiplicity, which contradicts the RSpace denotation. A maintainer must decide that question. The ledger cannot.

DR-51 follows principle P2. A peer vector no block commits to is not consensus data. DR-53 is a refinement repair with a named liveness defect.

## 7. Ratification checklist

- 8.1, additive semantics: a maintainer statement on whether S8 is a defect. A differential test over a corpus of dev merges showing which results change.
- 8.2, causal rejection closure: `formal/z3/merge_algebra` witnesses and the S12 controls.
- 8.3, evidence authentication (DR-51): the forged-response regression and the opposite-order Loom test.
- 8.4, admission projection (DR-53): the funding-rejection-plus-closeBlock regression.
- After ratification, replace the merge-algebra specification sections 4 to 6 and the protocol section 6 algorithm.

## 8. Open questions

1. Does any deployed contract depend on the collapse of identical outputs? A contract that relies on it would change behavior at the boundary.
2. R-ACTIVATION allows a legacy floor as the base of the first exact epoch. Which rules apply to the receipts of that floor during the first merge?
