# Merge-Algebra Determinism — Verification Dossier

> **Status:** the merger uses replay-authenticated per-execution state witnesses,
> causal-identity deduplication, and additive RSpace multiset composition. The Rocq
> capstones are axiom-free, the Z3 cross-witnesses cover both rejected alternatives, and
> Rust example/property tests exercise the executable algebra. Formal checks are local;
> the Rust suite remains the CI gate.

The [normative specification](./merge-algebra-specification.md) states the requirements.
The [glossary and literate algorithms](./merge-algebra-glossary.md) defines the terms used
here.

## 1. Why this repair is necessary

The observed integration failure reported two groups of nodes returning different block
hashes for one deploy. The web assertion was not itself a consensus algorithm; it exposed
that the nodes had persisted different merge results. The earlier merger had two separate
defects that could make that outcome reachable:

1. `DeployChainIndex::new` computed each chain's state change from the same whole-block
   pre-state and post-state, filtered only by that chain's affected channels. If more than
   one chain touched a channel, each chain could inherit the whole channel delta.
2. Folding those overlapping deltas with inline max-union/cancellation was
   non-associative. Deferring cancellation made it order-independent, but max-union then
   collapsed valid multiplicity when different executions emitted equal bytes.

The second point is not optional RSpace behavior. In the repository-adjacent publication
`../publications/denotational-semantics-for-rho/knot-rho.tex`, section “The RSpace
denotation: parallel composition as keyed multiset union,” parallel composition is the free
commutative monoid. Two equal outputs form a two-element bag, not a set element. Therefore
content-level idempotence contradicts the denotation.

### Why this branch exposed the defect

The cost-accounting work added block-close settlement, fee, redemption, and supply effects.
Those system effects increased the number of sequential transitions and the probability
that several deploy chains touched the same storage channels. `dev` did not exercise the
same effect topology, so the old whole-block attribution happened not to surface in the
reported scenario. The branch did not change the mathematical requirement; it made the
latent attribution error reachable.

The previous formal model missed it because it started after attribution: it assumed each
survivor already carried the correct delta, then proved properties of the fold. That proof
could establish determinism of the modeled operator but could not establish that a Rust
`StateChange` represented one execution. It also treated max-union idempotence as shared
history deduplication, conflating causal identity with serialized content.

## 2. Correct two-level algebra

An **execution identity** is `(source_block_hash, execution_index)`. An **exact delta** is
the state difference between the roots immediately before and after that execution. An
**effect map** maps each execution identity to its exact state and number-channel deltas.

For a finite key set `$`K`$` and effect map `$`M`$`, the content delta is:

```math
\Delta(M) = \operatorname{normalize}\!\left(\sum_{k \in K} M(k)\right).
```

The two operations have intentionally different laws:

| Layer | Operation | Required laws | Reason |
|---|---|---|---|
| causal observation | compatible effect-map union | associative, commutative, idempotent | observing the same execution twice must not apply it twice |
| RSpace content | signed multiset addition | associative, commutative, non-idempotent | two distinct equal sends are two data |

If one identity appears with unequal contributions, the map is incompatible and the merge
fails. If two identities carry equal contributions, both map entries survive and their
multiplicities add.

## 3. Implementation correspondence

| Obligation | Implementation |
|---|---|
| produce exact boundaries | `runtime.rs`: checkpoint after each user deploy; system deploy results retain their input and output roots |
| authenticate boundaries | `replay_runtime.rs`: require complete witnesses, contiguous pre-roots, and replayed post-root equality |
| bind cache results to witnesses | `runtime_manager.rs::replay_payload_hash`: hashes every user/system pre-root and post-root |
| derive one transition's delta | `block_index.rs`: `StateChange::new(effect_pre, effect_post, effect_event_log)` |
| retain causal identity | `DeployIndex::execution_index`; `DeployChainIndex::source_block_hash` and `exact_effect_changes` |
| deduplicate compatible repeats | `conflict_set_merger.rs::compute_merged_state`: ordered map keyed by `CausalEffectId` |
| reject incompatible repeats | the same map insertion compares canonical state and number-channel contributions and returns `HistoryError::MergeError` on disagreement |
| preserve content multiplicity | `ChannelChange::additive_join` and `StateChange::additive_join` |
| canonicalize once | `StateChange::normalized` after the complete unique-effect fold |
| prevent mixed epochs | `compute_merged_state` rejects exact/legacy mixtures |

The legacy whole-block path remains readable for historical blocks whose witness fields are
both empty. It is not mixed with exact-witness indices in one merge epoch.

## 4. Negative models

### 4.1 Inline max-union cancellation

Let `a = add(x)`, `b = add(x)`, and `c = remove(x)`. The legacy operator combines each
side with maximum multiplicity and cancels after every pair:

```math
(a \circ b) \circ c = \varnothing,
\qquad
a \circ (b \circ c) = \{x\}.
```

`ChannelNetting.combine_not_assoc_exhibit`, the Z3 script, and the ignored Rust negative
regression retain this witness.

### 4.2 Deferred max-union

Removing intermediate cancellation makes max-union associative, but it still gives:

```math
\max(1,1)=1.
```

That collapses two distinct executions that each emit `x`. Rocq
`max_union_collapses_distinct_effects` and the Z3 witness pin the semantic violation.

### 4.3 Naive addition of whole-block deltas

Additive composition is sound only after attribution. If two deploy chains each receive
the same whole-block delta `add(x)`, adding the chain deltas gives multiplicity two even
when the block executed one effect. Rocq `whole_block_replication_double_counts` records
the counterexample. Per-execution witnesses remove the overlap before addition.

## 5. Formal verification catalog

### 5.1 Rocq

The development is under `formal/rocq/merge_algebra/` and uses only the Rocq standard
library.

| Module | Principal results |
|---|---|
| `KeepOneOrder.v` | strict-total survivor order, `cmp = Equal` implies identity, permutation-independent sorted winner |
| `ChannelNetting.v` | additive commutative monoid; permutation-independent fold; net-preserving cancellation; compatible causal-map union; equal distinct effects preserve multiplicity; same identity projects once; max and whole-block counterexamples |
| `ConflictSoundness.v` | retained conflict detector plus single-value-number overfill guard |
| `EventLogSplit.v` | user/system event-index split recombines to the monolithic index |
| `MainTheorem.v` | four end-to-end capstones: survivor order, exact causal netting, conflict soundness, split soundness |

The causal-map proofs are pointwise, avoiding functional extensionality. `Print
Assumptions` for every capstone must report `Closed under the global context`, and `coqchk`
must accept `MergeAlgebra.MainTheorem`.

### 5.2 Z3

`formal/z3/merge_algebra/channel_netting_monoid.py` independently checks:

- additive associativity, commutativity, and identity;
- multiplicity two for distinct equal outputs;
- one projection for one repeated causal identity;
- rejection of unequal content under one identity;
- dependent add/remove telescoping;
- the max-union content-collapse counterexample;
- the whole-block replication counterexample; and
- legacy inline-cancellation non-associativity.

`keep_one_total_order.py` continues to cross-check the survivor comparator abstraction.

### 5.3 Rust tests

| Test class | Required cases |
|---|---|
| example algebra | distinct `+x` and `+x` retain two; dependent `+x` then `-x` telescopes; normalization is canonical |
| causal identity | equal repeated identity deduplicates; unequal repeated identity errors; different identities with equal content both survive |
| property algebra | additive composition is associative and permutation-independent over generated multisets |
| replay | absent legacy pair accepted only as legacy; half-witness, pre-state gap, and post-state mismatch rejected |
| cache | changing any user or system witness changes the replay payload hash |
| block indexing | exact witnesses are contiguous, finish at the block post-state, and yield per-execution deltas |
| integration | proposer and validator replay roots agree; multi-parent result is invariant under parent arrival order; restart/cache reconstruction retains exact effects |

## 6. Activation and compatibility

The protobuf additions are backward-decodable because absent byte fields decode as empty.
That does not make the consensus change rolling-upgrade-safe. New validators hash and
validate the witnesses and use a different merge projection for reachable programs. All
validators on a shard must activate the feature together, or an explicit protocol version
must select it after a finalized cut. The implementation rejects a merge that contains both
legacy and exact indices so a boundary mistake fails visibly instead of producing two roots.

Historical blocks retain the legacy replay/index path. A finalized-state import need not
replay historical intermediate roots, because blocks below the finalized merge floor are not
eligible merge effects. Recent exact blocks must either have been replayed locally or carry
intermediate roots materialized by replay before indexing.

## 7. Threat model and failure behavior

| Threat | Required response |
|---|---|
| forged execution root | deterministic replay rejection |
| one witness field omitted | deterministic replay/index rejection |
| root chain gap | deterministic replay/index rejection |
| replay cache alias with different witnesses | impossible because witnesses enter the cache key |
| repeated identity with altered delta | fail closed with merge error |
| same bytes from independent contracts | retain both multiplicities |
| integer number-channel overflow | checked combination rejects; never wraps |
| activation mixture | reject the merge epoch |

## 8. Verification commands

```bash
make -C formal/rocq/merge_algebra
rocqchk -Q formal/rocq/merge_algebra/theories MergeAlgebra \
  MergeAlgebra.MainTheorem
python3 formal/z3/merge_algebra/channel_netting_monoid.py
cargo test -p rspace_plus_plus rspace::merger::channel_change
cargo test -p casper merging::conflict_set_merger
cargo test -p casper effect_state_witness
cargo test -p casper replay_payload_hash
```

The repository-wide formal entry point remains `scripts/check-merge-algebra-ALL.sh`.

## 9. Review conclusions

The repair is principled with respect to Rholang's smart-contract semantics and independent
validators because it satisfies both requirements that the earlier model conflated:

- independent validators converge on a causally keyed, replay-authenticated set of
  executions; and
- RSpace preserves the exact multiplicity of the messages those executions create.

Formal verification did not previously prove the attribution boundary, so the failure was
missed rather than contradicted by a valid proof. The revised catalog makes attribution,
causal deduplication, content projection, and activation separate obligations. Similar
errors are searched for by tests that vary execution identity independently of serialized
content and by the explicit negative model for replicated whole-block deltas.

## 10. References

- Lucius Gregory Meredith, *Quoting is Colour-Swap: a model of the rho calculus in the
  knotted universe*, `../publications/denotational-semantics-for-rho/knot-rho.tex`, section
  “The RSpace denotation: parallel composition as keyed multiset union.” This is the
  normative semantic evidence for finite-multiset addition, absence of content-level
  idempotence, and preservation of two byte-identical independent outputs.
- L. G. Meredith, *From Turing's Machine to the Rho Calculus: An Introduction by
  Translation*, `../publications/FromTuringToRHO/rho_via_turing.tex`, sections on the
  commutative-monoid parallel fragment and RSpace as the content-addressable tuple-space
  implementation. This supports the process-level concurrency and storage correspondence.

The publications determine the Rholang/RSpace content semantics. They do not specify the
block-level causal identity, replay witness format, activation boundary, or Byzantine
validator recomputation protocol; those are the implementation and consensus obligations
formalized here.
