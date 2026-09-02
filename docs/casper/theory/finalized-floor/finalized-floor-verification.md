# Finalized-Floor Multi-Parent Merge — Verification & Bug-Fix Dossier

> **Status:** the merge-scope cliff, finality ratchet, stateless finalizer
> starvation, certified stale-state promotion, over-constrained main-spine
> admission, single-base accepted-effect loss, and heartbeat validation-backlog defects are **found**, **fixed**
> across the production paths, and
> covered by Rocq capstones, TLA⁺ before/after models, Rust regressions, and the
> cross-checks cataloged below. Verification is **local-only** — no Rocq/TLA⁺/
> Wolfram step is wired into CI.

This document is written so the design and its verification can be reconstructed
from scratch. It records the feature, the bug's root cause, the fix, the formal
artifacts (with theorem anchors), the invariant catalog, the additional findings,
and exactly how to re-run every check.

> **Companion docs.** [`finalized-floor-specification.md`](./finalized-floor-specification.md)
> is the normative contract (the *what must hold*, RFC-2119);
> [`finalized-floor-glossary.md`](./finalized-floor-glossary.md) defines every
> symbol/term used here (before first use) and gives literate-programming pseudocode
> for the warm up-walk and the launder-free combine. Rendered PlantUML diagrams live
> in [`diagrams/`](./diagrams/) and are embedded in §8.

---

## 1. The feature

The **finalized-floor multi-parent merge** chooses the base a new block's merge
builds on. For a block `B` with parents `P₁…Pₖ`:

- **`floor(B)`** = the highest ancestor of `B`'s parents that the clique oracle
  certifies **finalized**, evaluated over `B`'s *frozen justification snapshot*
  `just(B)` (never a node-local live view). Every input is a signed/structural
  fact of `B`, so **every honest node derives the same floor** — this is the
  linear-finality analogue of RChain's per-message fringe.
- **merge base** = `floor(B).post_state`.
- **merge scope** = the unfinalized band `closure(parents) \ closure(floor)` — the
  blocks whose writes the merge must fold onto the base.

`floor(B)` is the maximum of two candidate sources (both pure functions of `B`):

1. **Inheritance** — each parent's own floor (a child's cut never drops below a
   parent's, so a race sealed at some cut is never re-litigated).
2. **Advancement** — per parent, the highest main-chain ancestor with
   `ft_witnessed > θ` over `just(B)` (genesis is finalized by definition).

Key source files:

| Concern | File |
|---|---|
| Floor derivation, frontier walk | `casper/src/rust/finality/floor.rs` |
| Clique oracle (`ft_witnessed`) | `casper/src/rust/safety/clique_oracle.rs` |
| Merge driver, scope, backstop | `casper/src/rust/util/rholang/interpreter_util.rs` (`compute_parents_post_state`) |
| Merge write-algebra | `rspace++/src/rspace/merger/merging_logic.rs`, `casper/src/rust/merging/conflict_set_merger.rs` |
| Floor / frontier cache (LMDB) | `block-storage/src/rust/dag/block_dag_key_value_storage.rs` |
| LFB candidate discovery and ordering | `casper/src/rust/finality/finalizer.rs` |
| Bounded concurrent finalizer scheduling and retry | `casper/src/rust/engine/multi_parent_casper/finalization_runner.rs` |

---

## 2. The bugs — merge loss, finalizer starvation, and state-preservation bypass

The first three defects are coupled: one loses data and the other two drive the
system into that path under load. The remaining defects cross scheduling,
activation, exact occurrence, and state-derivation boundaries and therefore need
independent invariants rather than one broad “consensus” patch.

### H1 (SAFETY) — silent lossy fallback

`compute_parents_post_state` capped the merge with
`MAX_FLOOR_DISTANCE_BLOCKS = 256` and `MAX_PARENT_MERGE_SCOPE_BLOCKS = 512`. When
the finalization lag

```
Δ = num(maxParent) − num(floor)
```

exceeded the cap, it **silently returned the single highest-numbered parent's
post-state with empty rejected-sets** — discarding every other parent's committed
writes. Deterministic (no fork), but committed writes vanished (safety **S5** /
`¬T-K1` / `¬T-NDA`), and a dropped co-parent's deploys could be simultaneously
marked non-re-proposable → **permanently stranded**.

### H2 (DRIVER) — uncached Θ(Δ²·V) floor walk

`parent_frontier` re-walked the main chain **uncached** on every merge, running
the max-clique `ft_witnessed` oracle (each an O(V) per-validator ancestry BFS) at
**every** step — Θ(Δ·V) oracle calls per parent, Θ(Δ²·V) cumulatively. As Δ grew,
propose latency grew super-linearly → finalization lagged → Δ grew: a
**positive-feedback ratchet** that pushes Δ across the 256 cliff under genuine
load (concurrency + propagation delay). `DEEP_WALK_WARN_THRESHOLD = 256` only
warned.

### H3 (COMPOUND) — unbounded ancestor scan

The merge-scope ancestor collection was bounded by a shard config
(`max_parent_depth`) that **degenerated to an unbounded O(chain) scan** at its
default (`≤0` or `i32::MAX`), and could even cut *above* the floor (dropping the
band in between). It compounded H2's per-merge cost.

### H4 (LIVENESS) — stateless repeated-prefix finalizer starvation

Every finalizer invocation rebuilt the same newest-first candidate list, examined
only the first 128 entries, stopped after an eight-second wall-clock budget, gave
each candidate one second, and was itself cancelled by a fifteen-second runner
timeout. No continuation cursor survived the restart. An older finalizable
candidate could therefore remain permanently outside the repeatedly examined
prefix, or behind a repeatedly slow candidate. Because timeout expiry depends on
node-local execution speed, equal DAG state did not imply equal candidate coverage.

The green gate missed H4 because it used candidate sets smaller than the cap and
did not model repeated stateless invocations, adversarially slow candidates, or
host-dependent timeout schedules. The TLA⁺ negative controls now reproduce each
starvation class separately.

### H5 (SAFETY/LIVENESS) — finalized receipt masked by an above-floor tombstone

The exact occurrence reducer originally computed winning signatures only from
the visible above-floor scope. A sibling tombstone could therefore hide an
effect whose winning execution was already materialized in the finalized-floor
state. Recovery then treated the signature as absent and re-proposed it. For an
`IntegerAdd` channel, the duplicate execution could materialize a second datum;
for other state it could make parent-order-dependent block validity or stall
finalization.

Three refinement seams made this possible. First, base receipt precedence was
gated by the historical floor block's protocol version instead of the active
shard protocol, so a current exact scope evaluated against historical floor
metadata lost the guard.
Second, filtering removed an individual occurrence without proving that every
dependent deploy-chain effect was removed. Third, exact effect metadata was not
required to fold back to the aggregate state change used by application.

This branch exposed the defect because it introduced exact occurrence
tombstones and additive per-effect projection. `dev` lacked that new
interaction, so its older signature-wide behavior did not create this precise
base-versus-scope masking path. Earlier formal work proved above-floor
observation-order convergence but treated the finalized base as an external
state constant; it therefore did not model activation, historical floor
versions, or the state-record projection boundary. The new models make those
three premises explicit and include unsafe controls for each omission.

### H6 (SAFETY/LIVENESS) — split protocol-version authority at genesis

The cost-accounted binary configured Casper protocol 2, but
`ApproveBlockProtocolFactory::create` hard-coded `Genesis.version = 1`.
Approvers consequently signed a protocol-1 genesis while block creation read the
protocol-2 running configuration and emitted protocol-2 blocks. Peer admission
then read a third lifecycle point: `BlockProcessor::check_if_of_interest`
compared those blocks with the approved genesis header's version 1. Honest peers
discarded honest protocol-2 proposals before validation, so validators advanced
different local branches and finality stalled.

The observed split was therefore a genuine consensus disagreement, but its
cause preceded merge ordering: the network had no single protocol-version
authority spanning ceremony, running state, proposal, and reception. It appeared
on this branch because the D3 protobuf change declared protocol 2 for the
fresh-genesis cost-accounted encoding while the ceremony retained the old
literal. `dev` still used the protocol-1 lifecycle and did not create that
cross-version combination.

Earlier formal verification missed H6 because its transition systems began
after the active protocol had already been selected. The activation model proved
how an active-version merge interprets historical floor metadata, and the
end-to-end cost model proved execution through finality under one fixed protocol;
neither modeled genesis candidate construction, approver version checks,
approved-block admission, runtime adoption, proposer headers, and receiver
interest filtering in one state machine. `ProtocolVersionLifecycle.tla` and
`ProtocolVersionLifecycle.v` now make those omitted transitions and refinement
points explicit.

### H8 (SAFETY/LIVENESS) — clique-certified state-lineage bypass

The finalizer previously treated this conjunction as sufficient for LFB
advancement: the exact clique oracle certified candidate `C`, and `C` was a
main-chain descendant of current LFB `F`. That implication is false for a
multi-parent execution DAG. `C` can name `F` in its main-parent ancestry while
its post-state was computed from an older floor because another parent conflicted
with `F`. DAG ancestry records message causality; it does not prove state
derivation.

The execution path had the matching defect. A covering-parent fast path could
return that parent's post-state before proving that the newly derived floor was
already in the parent's state lineage. If the floor had advanced to a conflicting
branch, the shortcut bypassed its finalized transition. The result was not a
different majority vote: the stale candidate could legitimately retain its clique
certificate. The failure was installing a certified state that omitted an effect
already committed by `F`.

This interaction is branch-specific. The branch combined finalized-floor
advancement, floor-bounded multi-parent replay, exact conflict disposition, and
cost-accounting state transitions. `dev` did not contain that composition. The
cost-accounting papers require atomic resource commitment and conservation but do
not specify Casper's LFB-selection algorithm. The repair is therefore a node-level
refinement derived from the papers' atomicity obligation plus the existing Casper
contract that finalized state is permanent; it is not attributed to a paper as a
verbatim rule.

Earlier verification missed H8 because its abstraction identified DAG ancestry
with state ancestry. The merge models represented parent effects as monotone sets
or began from an already selected merge base, and the finalizer model represented
candidate readiness as one predicate. None carried an explicit block-to-state-base
edge across execution, certification, and LFB installation. Tests likewise lacked
the discriminating schedule: finalize one conflicting sibling, receive a valid
merge computed below it, prove that merge is still clique-certified, and then
require a later floor rebase to preserve the finalized value. H8 closes those
omissions rather than weakening the assertion to make the observed test pass.

The first H8 repair exposed a second, strictly different counterexample. Suppose
candidate `P` preserves the current LFB, but a heavy validator's latest block is a
merge that names `P` as a DAG parent while rejecting `P`'s conflicting state
effect. The original causal oracle counts that validator because `P` is in the
merge's all-parent DAG past. The validator's latest state does **not** descend
from `P`. With weights $`7/3/3/3`$, the merge validator and `P`'s proposer contribute
causal weight $`10/16`$, enough to clear a strict $`0.1`$ threshold, while only weight
$`3/16`$ preserves `P`'s state. Current-LFB ancestry alone admits `P`; using it as a
later replay floor reintroduces the rejected transition and can apply its private
resource twice.

This is not a flaw in weighted-majority arithmetic. It is a mismatch between the
proposition certified by causal ancestry and the stronger proposition required of
a state floor. The complete repair retains the causal certificate and additionally
requires an exact **state-preserving certificate**. Its supporting validators are
those whose frozen latest messages both causally include the candidate and
state-descend from it; the node then runs the same hard-majority, maximum-clique,
and exact-threshold calculation over that restricted support. A candidate must
also state-descend from the current LFB. Thus neither certificate can stand in for
the other.

Earlier verification missed this refinement because `certified` was deliberately
abstract in the state-lineage proof and the TLA+ model assigned certification per
block. Neither model related each certificate voter's frozen latest-message state
to the candidate. The strengthened Rocq theory defines state agreement and proves
that state-preserving finalization refines causal finalization. The strengthened
TLA+ model contains a candidate that passes causal certification and current-LFB
ancestry but fails state certification; disabling only the new gate reproduces
the unsupported-floor promotion.

### H9 (LIVENESS) — state-safe multi-parent rebase rejected off the main spine

The first H8 repair kept the old main-chain-descendant condition and added state
ancestry beside it. That conjunction was too strong. In an asymmetric 60/20/15
validator schedule, the heavy validator resumed after a pause and finalized
`061ba6…` at height 6. Parent selection then correctly kept that block as a
secondary parent while promoting deploy-carrying `6adc63…`/`114ff0…` branches to
main parent. Their replay states included `061ba6…`, and nodes continued validating
and producing blocks through approximately height 59, but every later candidate
failed the main-spine prefilter. The boot node's LFB consequently remained at
height 6.

The main-parent relation is the path over which finalizer agreements are
propagated; it is not the provenance relation of a multi-parent post-state. The
necessary and sufficient admission refinement is therefore unchanged exact clique
certification plus state ancestry. Removing the redundant main-spine conjunct does
not admit the H8 stale candidate because that candidate still lacks state ancestry.
It does admit a rebased candidate whose old LFB is a secondary parent.

Earlier H8 verification missed H9 because both its concrete Rocq scenario and its
TLA⁺ rebase encoded the old LFB as a main ancestor of the repaired block. They
proved the safety gate but did not vary main ancestry independently from state
ancestry. The strengthened artifacts now make the rebased block a DAG descendant
and state descendant of the LFB while placing it on a different main-parent spine.
The exact 60/20/15 stake map and strict `FTT=0.1` arithmetic reproduce the
integration topology. A dedicated negative control turns the erroneous main-spine
conjunct back on and must fail off-main rebase eligibility; a separate fair-trace
control must violate eventual two-node rebase progress rather than merely checking
the initial eligibility predicate.

### H10 (SAFETY/LIVENESS) — exact-effect rejection expanded by block ancestry

After H8 preserved the older clique-certified state floor, the merge scope could
contain two sibling `closeBlock` effects: the one already materialized in the
floor and a stale sibling transition that consumed the same pre-floor PoS cell.
Availability filtering correctly rejected the stale exact effect. The subsequent
fallback then expanded that rejection to every chain whose **source block** was a
DAG descendant of the stale source block. That relation was too coarse for a
block containing multiple exact per-execution witnesses. It removed a merge
effect that consumed the retained floor resource and unrelated user effects even
though neither physically depended on the stale transition. The resulting empty
or incomplete merge scope caused proposal stalls and, under asymmetric delivery,
different locally observed deploy blocks.

The correct relation is between exact state transitions, not their containing
blocks. A target physically depends on a source when it removes the byte-identical
ordinary datum or continuation added by that source under the same channel key.
Mergeable number-channel materializations are excluded because their typed deltas
are combined algebraically. Rejection is the least transitive closure of the
direct rejection seeds under that relation. Independent exact effects survive;
whole-block descendant expansion remains only as a conservative fallback for
legacy indices without exact witnesses.

This branch exposed H10 because it combined three changes absent as a composition
on `dev`: exact per-execution state witnesses, partial occurrence rejection, and
the H8 state-lineage floor that retains one conflicting sibling as the merge base.
The pre-existing block-lineage fallback had been sound only for its coarse
whole-block abstraction. Earlier proofs modeled complete deploy chains as atomic,
or began after a correct base and survivor set had already been chosen. They
therefore proved closure of a modeled chain but did not relate partial exact-effect
rejection to physical RSpace resources. `EffectCausalClosure.tla` now explores
every dependency-ready classification order. Its two unsafe controls reproduce
both blanket descendant loss and the dual one-hop error. The axiom-free Rocq
development defines the rejection set as the least closed set and proves both
independent-effect survival and absence of accepted dependents of rejected state.

### H11 (SAFETY/LIVENESS) — one state base erased accepted merge provenance

The first H8 repair represented every block by one functional “direct state
base.” A covering parent was selected when it covered all parents and preserved
the floor; otherwise the block's state base collapsed to the finalized floor.
That abstraction is not a refinement of the runtime merge. A valid three-parent
merge begins at the floor but folds accepted effects from **all** non-redundant
parents. When no parent covers the other two, recording only the floor falsely
reports those accepted parent transitions as absent.

The failure compounds over two merge rounds. Three validators can each produce
a valid merge of the same source and two siblings, and the next round can merge
those three tips in any parent order. Runtime replay retains the source effect in
every tip, but the single-base relation reports only the floor. State-support
certification then withholds votes from valid latest messages, so nodes with
different delivery prefixes can temporarily select different deploy-inclusion
blocks and stop advancing their LFB. The deploy-summary disagreement, pending
deploys, negative fault-tolerance assertions, and later unavailable-Casper or
unknown-root errors are downstream symptoms of that one provenance mismatch.

This regression was introduced on this branch by the first state-lineage repair;
`dev` had neither the functional state-base gate nor this failure mode. The
formal work missed it because `StateLineageFinality.v` assumed an abstract
reflexive/transitive preservation relation, while `StateLineageFinality.tla`
assigned the relation as a scenario table. Neither artifact proved a refinement
map from the actual accepted/rejected merge effects to that relation. Tests
covered explicit rejection and rebase, but not an accepted three-way merge,
all parent permutations, a repeated merge round, and majority certification in
one scenario.

The repair makes the missing refinement data consensus-visible. Every successful
execution has identity $`(source\_block\_hash, execution\_index)`$; every block
serializes the exact canonical identities its parent merge rejected; and DAG
metadata persists successful local indices, rejected identities, and protocol
version. Active effects are then derived by the exact merge recurrence in the
normative specification. The state-preservation predicate is no longer a guessed
base chain: it is DAG ancestry plus subset inclusion between the two derived
active-effect sets.

### H12 (LIVENESS) — a dual-certified state floor was invisible off every main spine

The H8/H9 repair correctly allowed the finalizer to certify a state-preserving
candidate whose current LFB survived through a secondary parent. Per-block floor
derivation still enumerated only inherited floors and each declared parent's
main-parent frontier. Those two components therefore used different discovery
relations: LFB admission consumed complete causal evidence, while replay-floor
advancement consumed only main-spine evidence.

The retained five-node trace made the separation explicit. Validator 1 made 444
floor derivations and chose genesis every time. During the same execution, several
blocks held causal and state-preserving support of `300/300`; the deploy-bearing
state was later present in the global finalizer's certified ancestor batch. The former
deploy-support override moved each proposal's main parent away from the GHOST head,
leaving the certified state as a secondary ancestor of every selected tip. It was therefore
universal in the causal DAG but absent from every per-parent main spine. Different
delivery prefixes then observed the deploy in different unfinalized blocks, while
pending deploys, stalled LFB heights, unavailable-Casper responses, and unknown
RSpace roots accumulated downstream. The node disagreement was a real liveness
and state-progression defect; changing query assertions or majority thresholds
would only hide it.

This branch exposed H12 because state-preserving LFB admission was composed with a
non-GHOST deploy main-parent override. The override has now been removed, but complete
off-main candidate discovery remains required for arbitrary valid multi-parent DAGs and
Byzantine parent ordering. `dev` did not compose those mechanisms. Earlier formal verification proved the finalizer's
off-main admission rule and separately proved active-effect preservation, but it
left candidate enumeration in `derive_floor` outside the refinement map. The
models assumed that a certified candidate was already discoverable. Tests likewise
covered off-main LFB admission and parent-order invariance without constructing a
candidate that was secondary to **all** current parents and absent from **all**
their main spines.

The repair adds a third, block-structural candidate source. A deterministic
multi-source traversal propagates one coverage identity from each declared parent
through every causal edge in descending `(block_number, block_hash)` order. The
highest candidate covered by every parent is admitted only when the unchanged
causal certificate, unchanged state-preserving certificate, and inherited-floor
preservation all hold over the block's frozen justification snapshot. Strictly
descending edge heights prove that coverage is complete when a candidate is
examined; malformed or late coverage fails closed. The existing sound-base chooser
then handles the candidate without altering finalizer agreement propagation,
voters, weights, clique selection, or exact `FTT` arithmetic.

`CertifiedFloorPromotion.tla` exhausts every two-node delivery/derivation order:
1,051 generated states, 225 distinct states, depth 9. The safe causal-discovery
model preserves certificate separation and eventually promotes both nodes; the
main-spine-only control violates complete-evidence promotion after one node receives
all three tips. Apalache independently checks the safe invariants through bound 8
and finds the unsafe trace at step 3. The axiom-free Rocq development proves that
a dual-certified current floor preserved by every parent is both universal and
causally discoverable, and that selecting only universal candidates preserves the
current floor. Rust examples exercise the exact `FTT=0.1` three-validator topology,
all six parent permutations, and a state-rejection control; a generated-DAG test
varies side-branch and post-merge depths plus parent order.

The first complete repair was semantically correct but evaluated causal support
with one storage-backed ancestry walk for every candidate-validator pair. AMD
uProf measured approximately 18.13 seconds in raw DAG ancestry, 15.38 seconds in
the universal frontier, and 16 seconds in clique evaluation on the 132-block
regression. The exact test therefore took 63.21 seconds after state-provenance
memoization. The optimized algorithm computes the transposed relation once:
validator identities begin at the frozen latest messages and propagate through
all causal parents in descending height/hash order. For every candidate `C`, its
result is exactly $`\{v \mid C \preceq_{DAG} J(v)\}`$; the corresponding weight
map, maximum-clique input, hard-majority gate, threshold, and strictness remain
unchanged. The same regression now passes in 22.92 seconds while retaining all
132 blocks needed to cross the former 128-candidate boundary.

The scan-reuse guard is deliberately narrower than “same evidence.” A child may
reuse its parent's result only across a one-parent edge whose parent also has one
predecessor, whose inherited floor matches the cached parent floor, whose frozen
snapshot is identical, and whose latest messages are all older than the parent.
A multi-parent merge always rescans because creating the merge can make a
previously branch-local certified candidate universal even when no validator
message changed.

Rocq proves propagated coverage equivalent to pairwise reachability and proves
the linear-snapshot reuse theorem from the one-predecessor and unsupported-parent
premises. `LatestMessageCoverage.tla` executes the worklist itself rather than
assuming the result. TLC exhausts 27 generated / 16 distinct states to depth 8,
proves partial soundness, exact coverage for every processed block, exact final
coverage, absence of late propagation, and termination. Removing only the
descending scheduler violates `Inv_NoLateCoverage` after processing one shared
ancestor too early. Apalache checks the safe model through bound 8 and finds the
same unsafe trace through bound 4. Rust compares the propagated supporter set,
corresponding weight map, and final clique decision with the original pairwise
oracle across generated branch depths and every parent permutation; malformed
non-descending edges and all reuse-guard exclusions have example regressions.

### H13 (SAFETY/LIVENESS) — snapshot state support raced incomplete provenance

The six-node bonding lifecycle exposed a second closure boundary. Snapshot parent
selection captured a complete latest-message map, but it materialized finalized
floors only for the selected parent set. An off-parent latest message could carry
an exact rejected-effect record, so deciding whether it preserved the captured
LFB recursively called `state_input_blocks(latest)`. Its canonical floor was not
yet in `floor-index`. The query therefore failed with `finalized floor is not
materialized for state provenance`, even though the block and all causal metadata
were present.

The failure was timing-sensitive because block processing and the finalizer both
populate the same write-once cache. Nodes with a fortuitous finalizer interleaving
had the entry; another node could reach snapshot selection first. The failed
proposal or dependency-free block remained eligible for another attempt. In the
retained trace this produced 175 repeated processing failures on one node and 142
replays per node, amplifying CPU and RSS until the host-protection guardian fired.
This was not a clique, threshold, or committee disagreement. It was an incomplete
precondition for a deterministic state-support query.

The repair freezes one target set containing the current LFB, all latest messages,
and the selected parents, sorts and deduplicates it by block hash, and recursively
materializes every target before state-support selection. `finalized_floor`
uses the same closure for its parent and justification inputs. Cache population
is monotone and idempotent, so arbitrary finalizer/snapshot interleavings produce
the same union; any real storage or provenance error still aborts the snapshot.

`SnapshotFloorMaterialization.tla` executes snapshot and finalizer writes in every
order. TLC exhausts 18 generated / 10 distinct states to depth 5, proves the cache
is exactly the union of completed canonical closures, proves selection requires
the complete snapshot closure, and proves eventual selection. The parent-only
control reaches selection without the off-parent latest closure and violates
`Inv_SelectedSnapshotHasCompleteProvenance` at depth 3. Apalache independently
checks the safe invariants through bound 8 and finds the unsafe trace through
bound 4. Rocq proves closure completeness, preservation of existing entries,
permutation transparency, idempotence, and commutation with arbitrary finalizer
materialization; the concrete parent-only witness is incomplete. Rust examples
exercise both `finalized_floor` and snapshot fork choice without pre-seeding the
off-parent cache entry.

### H14 (LIVENESS/RESOURCE) — heartbeat production outran validation and replay

The failing multi-validator runs did not show the clique oracle selecting two
different LFBs. They showed a production/consumption instability before stable
finality: every validator repeatedly produced empty or support blocks while each
node was still validating and replaying peers' preceding blocks. A block took
approximately 6–10 seconds to validate in the retained run, while three validators
could collectively introduce several blocks within one 5–10 second interval. The
unresolved DAG widened, live replay state accumulated, finalization arrived in
50–110 second batches, API requests timed out, and the resource guardian eventually
terminated the nodes.

The pre-repair heartbeat predicate conflated three different observations:
producer-supplied LFB timestamp age, latest-frontier timestamp activity, and actual
local LFB advancement. A producer timestamp could be old while the finalizer was
making useful progress; frontier churn could remain fresh while the LFB was stuck;
and missing latest-message state allowed every validator to manufacture more empty
work. The existing unfinalized-width pressure guard applied only after this feedback
had already opened and used a strict `>` boundary. A fixed leader would reduce
production but fail liveness when that validator was offline.

The earlier formal campaign missed H14 because the finalized-floor and recovery
models ended at a stable finite candidate snapshot. They did not compose continuous
heartbeat arrival, independently advancing validator-local recovery views, a finite
validation service rate, bounded admission, and an offline first leader. Example
tests likewise checked one heartbeat decision at a time rather than an adversarial
producer/consumer schedule. The missing dimension was distributed queue dynamics,
not another arithmetic case in the clique calculation.

`HeartbeatFinalityBackpressure.tla` closes that state-space boundary. Each validator
has an independent recovery view and local round. Every produced block records an
explicit ancestry set and the producer's captured latest-message view; candidate
production, validation, view delivery, causal support, state-preserving support,
and promotion interleave. Promotion requires the unchanged exact hard-majority and
threshold formula over supporters that form a mutual causal clique and a refining
mutual state-preserving clique. Leadership and delivery do not manufacture support.

The eventual-synchrony configuration sets `DeliveryWithinRound = TRUE` and
`BoundedRecoveryScheduling = TRUE`. The first assumption drains admitted work
before the next selected layer; the second bounds online heartbeat tasks to at
most one completed local recovery step of skew once the scheduling bound applies.
These are premises only for the temporal property that recovery rotates past the
offline first leader. TLC exhausts 22,468 generated / 4,194 distinct states to
depth 30 and proves bounded backlog, per-round leader uniqueness and
leader/attempt agreement, state-support refinement, exact dual-certificate
promotion, cross-node promotion compatibility, and that liveness property. A
second safe configuration starts with a real unfinalized candidate and exhausts
22,960 generated / 4,338 distinct states to depth 30 while proving its eventual
promotion. An asymmetric `1/4/5` stake configuration, in which neither online
validator has a hard majority alone, exhausts 22,468 generated / 4,194 distinct
states to depth 30 and requires their combined `4+5` mutual clique. The
asynchronous configuration disables both assumptions, lets
validators advance local clocks and rounds independently, and checks the same
safety invariants without asserting liveness. TLC exhausts 113,968 generated /
17,766 distinct states to depth 30. Safety therefore depends on neither
within-round delivery nor bounded relative heartbeat scheduling.

`HeartbeatRecoveryCadence.tla` separately models independent validator-local
elapsed clocks, including arbitrary jumps caused by a delayed task wake. The
contract applies
$`\max(\mathtt{max\_lfb\_age},\mathtt{check\_interval})`$ once before round zero and then
opens successive rounds every `check_interval`; the task consumes the earliest
uncompleted round at or below that elapsed frontier. TLC exhausts 1,123,849
generated / 287,496 distinct states to depth 26 and proves the exact frontier, no
premature attempt, at most one attempt per local round, and that completed rounds
always form a contiguous prefix. The collapsed-timeout control reuses the stall
timeout between later rounds and violates `Inv_CadenceMatchesContract` after an
immediate elapsed-time jump, reproducing delayed post-stall recovery.

Apalache checks the composed eventual-synchrony invariants through bound 5, the
fully asynchronous safety projection through bound 4, and the cadence/prefix
invariants through bound 10. Separate bounded controls reach real promotion,
causal-only promotion without a state certificate, eager backlog overflow, and
the collapsed-cadence defect, so the bounded safe runs are not vacuous.

The eager control violates the backlog invariant, the fixed-leader control
violates temporal liveness, and the causal-only control violates exact state
certification.

The axiom-free Rocq refinement proves finalized-height-offset leader membership
and uniqueness, earliest-uncompleted round minimality and skipped-wake
preservation, node-local delivered latest messages, captured block views,
state-preserving descent, and a concrete selected-layer A→B→A construction that
yields both causal and state mutual cliques. Rust properties prove leader
permutation independence, complete rotation and cycle periodicity, exact cadence,
ordered delayed-wake catch-up, and rejection of out-of-order completion. Focused
examples prove selected-leader success, selected-leader deferred retry,
nonleader completion without proposal, zero-interval rejection, pending-deploy
admission, missing-latest suppression, and the exact unfinalized-DAG backpressure
boundary.

### H15 (SAFETY/LIVENESS/RESOURCE) — proposal intent and queue state were conflated

H14 bounded the coarse heartbeat producer, but the runtime still had two finer
authority and concurrency defects. First, an ambient `is_async` Boolean represented
both response behavior and permission to create an empty block. A peer user-deploy
observation could also authorize a support sibling. Every validator could therefore
create work from another validator's work even though no selected recovery round
authorized it. This was the runtime analogue of the already unsafe
`EagerHeartbeat = TRUE` control: arrival rate could exceed replay and validation
service rate without adding new user work or new certificate evidence.

Second, the proposer receiver awaited the only active proposal before receiving the
next request. Its semaphore-collision branch was therefore unreachable; pending
requests accumulated in FIFO order instead of exercising the intended coalescing
path. A pending-deploy wake racing proposal completion could be either duplicated
or lost if implemented with a check-then-clear Boolean. Recovery also lacked a
capability tied to the exact LFB view, so a queued request could outlive the floor
hash, height, round, or selected leader that authorized it.

The earlier formal model represented bounded admission as a counter and represented
proposal production as one transition. It did not refine three request kinds through
queue collision, proposal start, terminal outcome, and a concurrently advancing LFB.
It also treated pending deploy and recovery as alternatives, so it could not show
that ordinary work might mask the only selected recovery opportunity. The missing
state was the proposal reservation itself and the atomic dirty epoch between
admission and completion.

`PendingDeployHeartbeatComposition.tla` and
`ProposerAdmissionCoalescing.tla` close those boundaries. The former composes
admissible pending work with selected recovery in one proposal and distinguishes
stored pending work from work still eligible under attempt and occurrence bounds.
The latter models the lock-free gate as `Idle`, `Active`, or `ActiveDirty`, with
manual, pending-deploy, and finality-recovery intents. Together they require fresh
recovery authority for every empty proposal, exactly one forced non-empty follow-up
per dirty epoch, terminal evidence before pool removal, retry after nonterminal
outcomes, and rejection of a recovery permit after its captured LFB view changes.

### Why the green-gate missed it

The convergence gate `three_writers_converge_under_load = run_convergence(3,3,21)`
is ≈ **35 blocks** — far below the 256 cliff. The 400-block observation is a
soak / shard run under real concurrency, where the ratchet has room to build.

### The ratchet, quantitatively

Model the finality lag as a difference equation `Δₙ₊₁ = f(Δₙ)` where a propose
step advances the tip and a finalize step advances the floor, and finalize
throughput falls as propose cost `∝ Δ²` rises. `formal/wolfram/finalized_floor/
delta_ratchet.wl` shows — parameter-free, over the reals — that with the buggy
Θ(Δ²) advance the smooth return-map slope exceeds 1 wherever `Δ > 0`. A
transient above the service-rate tipping point therefore runs away. The fixed
**O(1)** advance has zero lag-dependent feedback, but that fact alone does not
guarantee bounded lag: `Δ` drains when service exceeds arrivals, remains flat
at equality, and grows when arrivals exceed service.

```
 Δ  ▲                              buggy: propose cost ∝ Δ²  → finalize starves
256 ┤· · · · · · · · · · · ·╱····  cliff (silent write-loss fires here)
    │                    ╱⟋   ← runaway above tipping point (smooth slope > 1)
    │              ╱⟋⟋
    │        ╱⟋⟋
  k ┤─────────────────────────────  fixed: O(1), bounded iff service ≥ arrivals
    └────────────────────────────▶ block height
```

---

## 3. The fix

### 3.1 H2 — persist a per-block frontier + incremental up-walk

Cache `F(X) := parent_frontier(X, just(X))` — the highest witnessed-finalized
block on `X`'s main spine over `X`'s **own** snapshot — in a new `frontier-index`
LMDB store mirroring `floor-index`. `F(X)` is a pure function of the block (it is
exactly the `parents[0]` advancement candidate `derive_floor` already computes),
so caching is free; `floor_of_block` persists it, `derive_floor` now returns
`(floor, F(B))`.

`parent_frontier(parent, J)` (where `J = just(child) ⊇ just(parent)`) becomes:

- **Warm** (`incremental_frontier`): read the cached pivot `F(parent)`; verify it
  still finalizes over the larger `J` (**L-SNAP** guard) and that the committee is
  constant across the band (**L-ANC** guard); then **up-walk** the spine from the
  pivot toward `parent`, advancing while each block stays finalized. The band is
  collected with cheap `main_parent` hops (no oracle calls); only the up-walk
  itself calls the oracle — `O(advance)` calls, **amortized O(1)** (advance sums
  telescope to the spine length).
- **Cold** (`cold_parent_frontier`): the original top-down walk (cache miss, guard
  trip, pivot off-spine, or genesis).

```
 spine (bottom→top):   genesis ── … ── F(parent) ── … ── parent
 flags over just(B):    true    true    TRUE(pivot)  ?…?     ?
 warm up-walk:                          └──advance──▶ stop at first non-final
 cold down-walk:        first finalized from the top ◀──────┘   (== warm, by L-ANC)
```

**Determinism linchpin (why the cache never forks):**

- **L-ANC** (ancestor-monotone): `Finalized(C,J) ∧ C' ancestor of C ⟹ Finalized(C',J)`.
  The *same* quorum that finalizes `C` finalizes every ancestor (each member has
  `C`, hence `C'`, in its past). ⟹ finalized blocks are a downward-closed prefix
  of the spine, so "highest finalized" is well-defined and the up-walk may stop at
  the first non-finalized block.
- **L-SNAP** (snapshot-monotone): `just(B) ⊇ just(P) ⟹ Finalized(C,just(B)) ≥
  Finalized(C,just(P))`. ⟹ the pivot stays valid over the child's larger snapshot.

Together: **warm-walk result == cold-walk result** (transparent cache).
Residual: a bonding event inside the band can break L-ANC's constant-committee
premise; the warm path detects this (committee comparison) and falls back to the
cold walk (`floor_incremental_guard_fallback` metric). This guard is **not merely a
runtime check** — Rocq `GuardBridge.chain_adj_AdjDC` proves that a *constant*
committee across the band **derives** `Floor.frontier_cache_transparent`'s `AdjDC`
premise from L-ANC, so the guard is exactly the condition under which warm == cold
(the "Rocq assumes what Rust enforces" seam is closed; tested by
`guard_trip_committee_change_falls_back_to_cold`).

### 3.2 H1 — deterministic backstop, never a silent lossy substitution

The over-cap path now returns `Err(CasperError…)` keyed on the **deterministic**
`floor_distance` Δ only. On **propose** the `Err` parks the round (retried once
finality advances and Δ shrinks); on **validate** an over-Δ block is
deterministically invalid — both sides compute the same Δ, so **no fork**. The
scope-size test `|visible_blocks| > 512` is **demoted to a metric** — it is *not*
node-deterministic (branch width differs across views), so it must never gate
admission. The lossy `put_cached_parents_post_state` was deleted. This also fixes
the `canonical_won_sigs` stranding.

### 3.3 H3 — floor-bounded ancestor scan

The floor is now derived **before** the ancestor scan, which is bounded at the
floor height (`meta.block_number ≥ floor_block_number`) — `O(Δ)`, and never cuts
above the floor.

### 3.4 Metrics

Renamed `MERGE_SCOPE_TOO_LARGE_FALLBACK_FIRED` → `MERGE_SCOPE_BACKSTOP_ERROR`;
added `floor_distance`, `merge_scope_size`, `floor_walk_oracle_calls`,
`floor_frontier_advance`, frontier `cache_hit`/`cache_miss`, and
`floor_incremental_guard_fallback`.

Net effect: per-merge cost **Θ(Δ²·V) → amortized O(1) oracle + O(Δ) cheap reads**;
the ratchet collapses and over-cap is safe.

### 3.5 H4 — complete deterministic scan over one frozen view

One invocation now freezes latest messages once, derives both traversal roots and
the clique snapshot from that same immutable view, traverses the complete
all-parent causal closure above the exact current LFB height, and deterministically
orders the result by descending block height and descending block hash. It evaluates
that complete sequence until the highest finalizable candidate is selected or the
finite snapshot is exhausted.

Discovery deduplicates each `(validator, block)` pair when it is enqueued, rather
than after the next breadth layer is allocated. Reconvergent parent paths therefore
cannot inflate the frontier with duplicate work: the traversal remains complete
while its scheduled work is bounded by the reachable pair set. Rocq proves that
the deduplicated schedule has exact union membership and contains no duplicates.

Missing candidate metadata or parent data is an error, not evidence that a
candidate is non-finalizable. Cooperative yields limit scheduler monopolization
without changing coverage or classification. The bounded concurrent runner no
longer cancels a correct scan on a local wall-clock deadline. Consequently, a frozen
view has one node-independent result: selected highest candidate, exhaustive
absence, or an explicit inconclusive error.

### 3.6 H5 — active-version base precedence and exact chain projection

The merger now constructs a base-committed receipt set from the finalized-floor
body and gives it precedence over every above-floor tombstone. Exact tombstones
remain causal and scope-local: they reject the complete dependent deploy chain,
not merely the named occurrence. The same selected-chain projection drives the
ordinary state fold, mergeable contributions, and disposition records.

Protocol activation is a consensus boundary rather than a runtime feature
switch. The active shard protocol selects exact semantics even when the floor is
a legacy block. Every admitted above-floor source has that active version. Each
record is decoded according to its containing block header: current records
require causal provenance and a specified reason; legacy records require the
legacy empty-provenance/unspecified representation. Mixed scope versions and
protocol-incompatible encodings fail closed.

`DeployChainIndex::validate_exact_projection` independently recomputes the
ordinary and mergeable aggregates from the exact per-effect map. It rejects a
missing effect, repeated identity, inconsistent repeated identity, noncontiguous
root chain, or aggregate mismatch before any state is selected or applied.

The legacy-floor/current-scope case is a defensive composition property of the
merge reducer. It is not the network migration path: consensus fields are added
across versions 2 through 6, so this release starts protocol 6 from a fresh
protocol-6 genesis and rejects protocols 1 through 5 as active approved
protocols. Protocol 2 remains the historical threshold for exact rejected-deploy
disposition records; protocol 3 activates exact per-execution state-effect
provenance, protocol 4 activates vault-backed quantitative byte evidence,
protocol 5 activates certified validator incarnations, and protocol 6 activates
certified admission and finalized-floor commitments with portable certificate
sidecars.

### 3.7 H6 — one fail-closed protocol-version lifecycle

Genesis construction now receives the configured protocol version and emits it
in the candidate header. Every genesis approver validates that header against
its configured version before signing. Approved-block validation and
`hash_set_casper` admit only explicitly supported protocols; this release's set
is exactly `{6}`, so protocols 1 through 5 and unknown versions fail without
mutating the shard configuration.

After admission, the approved version is adopted into the running
`CasperShardConf`, and initialization and recovery retain that adopted
configuration. Proposal construction and peer-interest filtering both read the
running version via `Casper::get_version`; the approved genesis header is no
longer an independent receiver authority. Thus ceremony, approval, adoption,
proposal, and reception form one version chain. There is no accounting feature
flag, legacy execution path, A/B mode, or height-triggered transition in the
binary.

### 3.8 H7 — concurrent rejection causes form a canonical join

An exact tombstone's causal authority is its `(deploy signature, source block)`
key. Its reason explains why that occurrence was removed, but it does not grant
additional authority. Concurrent descendants can therefore record different
valid reasons for the same key when one closure sees a direct duplicate and
another sees only the containing chain's collateral removal.

The reducer treats reasons as the four-element ordered join-semilattice

```math
r_{\bot} \prec r_{\mathrm{collateral}} \prec r_{\mathrm{merge}}
\prec r_{\mathrm{duplicate}}.
```

`Unspecified` is `$`r_{\bot}`$` and is forbidden in a current-protocol record;
it remains the fold identity. Canonicalization is the maximum under this order.
A direct duplicate therefore dominates every alternative, a direct merge
conflict dominates a merely collateral removal, and repeated evidence is
idempotent.

```text
join_reasons(records):
    reason := unspecified
    for each causally valid record in any order:
        reason := max_by_protocol_precedence(reason, record.reason)
    return reason
```

The last-writer implementation is retained as a TLA+ negative control: two
validators can observe the same `{merge_conflict, collateral_chain_drop}` set in
opposite orders and serialize different results. `RejectionReasonConfluence.tla`
exhausts all interleavings and proves equal-observation convergence for the
canonical join. `RejectionReasonConfluence.v` proves commutativity,
associativity, idempotence, direct-cause precedence, and arbitrary-list
permutation invariance without added axioms.

The finalization-status projection uses the same causal evidence scope as the
merge reducer. Every validated exact tombstone in the LFB's complete parent
closure is authoritative, including a record reached only through a secondary
parent. Restricting exact records to the main-parent spine can leave two active
sources in the API after committed state retained one. The
`FinalizedOccurrenceStatus` TLC and Apalache models retain that implementation
as a negative control, while Rocq proves causal-closure authority independent
of main-spine placement and the Rust DAG regression checks the complete
resolver path. Legacy signature-wide records retain their historical
main-chain rule; this does not weaken protocol-3 exact occurrence semantics.

### 3.9 H8 — preserve certification, constrain committed-state promotion

The H8 admission repair separates the existing causal certificate from a second
state-preserving certificate. H11 replaces its initial single-base realization
with exact state-effect provenance; the admission architecture remains valid,
but its preservation predicate now means active-effect inclusion.

Floor-frontier advancement first reduces the raw causal main-parent frontier to
the highest state-preserving candidate, then lowers it along the same main-parent
spine until it holds the state-preserving certificate. Floor selection also
rejects a candidate at or above an inherited floor when it does not preserve
every effect active at that inherited state. The execution fast path is
conditional on the same predicate;
otherwise `compute_parents_post_state` replays the floor-bounded merge. These three
sites use the same provenance rule, so proposal, replay, and finalization cannot
assign different meanings to a block's accepted effects.

The causal clique oracle is unchanged. `Finalizer::run` still discovers and orders
the complete frozen main-parent candidate set, applies the same exact
strict-majority upper-bound test, and calls the same exact maximum-clique decision.
After causal certification, it requires a second exact certificate over validators
whose frozen latest-message states preserve the candidate. It finally requires the
candidate to preserve every active current-LFB effect. A stale-state block remains
valid and causally certified but cannot replace committed state. A causally
certified rejected parent likewise cannot become a state floor merely because a
heavy validator named it as a merge parent.

The floor path applies the same state certificate to each causal frontier before
using it as an advancement candidate. Finalized-floor materialization closes over
both DAG parents and frozen justification tips, because either can be traversed by
the state-support predicate. Active-effect evaluation closes over maximal parents
and the cached floor and is memoized per `(block, effect)`; this changes no result
because the recurrence is an immutable function of consensus metadata.

The next proposal detects that its covering parent does not preserve the advanced
floor, rebases from the floor, and restores progress.

This separation avoids the unsafe alternative of retroactively invalidating the
stale block after another block finalizes. Validity remains a pure function of the
block and its ancestors. The causal and state-preserving certificates are pure
functions of a frozen snapshot; LFB admissibility adds the transition predicate
over the current committed state.

### 3.10 H11 — exact active-effect recurrence and canonical validation

For a block `B`, let `Own(B)` be its successful user and system execution
identities, `Rejected(B)` the exact set recomputed by the merge, and `Inputs(B)`
its maximal direct parents plus its finalized floor. The implementation evaluates:

```math
Active(B) = \left(Own(B) \cup
  \bigcup_{I \in Inputs(B)} Active(I)\right) \setminus Rejected(B).
```

The block creator serializes `Rejected(B)` in sorted unique order. Validation
recomputes the same merge and rejects missing, extra, duplicated, or misordered
identities. `BlockMetadata::from_block` derives `Own(B)` only from successful
executions and persists the protocol version. Protocol-2 finality fails closed
if provenance is unavailable; legacy persisted metadata is not guessed.

`is_state_preserved(A,D)` first requires DAG ancestry. It then scans the
height-bounded causal past above `A` for a deterministic superset of potentially
removed identities. For every candidate active at `A`, it evaluates the recurrence
at `D`; one missing effect rejects preservation. Unrelated rejections are harmless.
Rocq proves this complete-superset check equivalent to full set inclusion, while
the Rust regression exercises an unrelated sibling rejection.

AMD uProf identified repeated per-edge DAG-ancestry queries and metadata decoding
inside the original state-preservation scan. The repair performs one causal-past
traversal with a height bound and memoizes metadata, ancestry, active-effect, and
preservation queries for the complete floor-materialization run. A second profile
reduced state-provenance work to approximately 0.08 seconds and thereby isolated
the remaining cost in H12's causal-support calculation rather than hiding it in
state reconstruction.

### 3.11 H14 — observed-progress recovery with bounded rotating leadership

The heartbeat task records the last LFB hash it actually observed and a monotonic
`Instant` for that observation. A new hash resets recovery history. Continued
observation of the same hash first opens round zero after the one-time timeout
$`\max(\mathtt{max\_lfb\_age},\mathtt{check\_interval})`$. Subsequent rounds open every `check_interval`;
the implementation computes
$`\left\lfloor(\mathtt{stalled\_for}-\mathtt{stall\_timeout})/
\mathtt{check\_interval}\right\rfloor`$ rather than reusing the
stall timeout as the later cadence. The leader is selected from the canonical
sorted committee derived from the LFB's post-state at
$`(\mathtt{nonnegative\_lfb\_height}+\mathtt{recovery\_round}) \bmod N`$.
This is validator-local proposal policy only: it calls the ordinary serialized
proposer and cannot alter the mutual causal clique, mutual state-preserving clique,
exact threshold, or LFB.

Snapshot construction obtains that recovery committee through `floor_committee`,
the same floor-state function used by proposal preflight and receive-side
authority validation. This authority committee is distinct from
`block.body.state.bonds`, which is replay-validated against the block's own
post-state and may register new validators only after the block is accepted.
Parent order, duplicates, divergent non-finalized head views, and one parent's
transient post-state bond cache cannot change the leader, exact justification
set, sender authorization, or synchrony weights. A transition in the candidate
block cannot authorize that block; it becomes eligible only after accepted
registration and later floor promotion. The exact head-committee negative
control permits two online validators to see different singleton parent
committees and violates global one-leader-per-round authority immediately; the
floor-bound model excludes that trace. A delayed
wake retains the earliest uncompleted round instead of skipping to the elapsed
frontier. A nonleader completes that local round without proposing. The selected
leader closes it only after the serialized proposer starts or succeeds; an empty,
deferred, or failed proposal leaves the round retryable. Round rotation prevents
an offline first leader from becoming a permanent liveness dependency without
allowing skipped wakeups to omit intermediate leaders.

Ordinary work remains distinct. Pending deploys use their lag/cooldown/recovery
caps, but observing a peer's user-deploy block does not create proposal authority.
Missing self history, genesis, unreadable metadata, system-only latest messages,
and latest-message churn without local pending work no longer create blocks. When
the selected recovery round and pending work are simultaneously due, one recovery
proposal carries that pending work; pending work cannot mask the recovery round.
Idle recovery at an already-ahead validator stops at the exact configured
unfinalized-DAG boundary. The obsolete `frontier-chase-max-lag` option was removed
because retaining a setting with no production effect would misrepresent the
protocol's resource controls.

### 3.12 H15 — explicit proposal capabilities and atomic pending-work coalescing

Every caller now supplies one `ProposeRequestKind`: `Manual`, `PendingDeploy`, or
`FinalityRecovery(permit)`. A permit captures the LFB hash, LFB height, and recovery
round. Immediately before block creation, the proposer resolves the current Casper
engine and snapshot, recomputes the canonical sorted/deduplicated recovery leader,
and rejects the request if any captured field or the selected leader is stale. Empty
block creation requires all three facts: the request is `FinalityRecovery`, the
permit remains authorized, and heartbeat empty-block capability is configured.
Manual and pending-deploy requests cannot inherit empty-block authority from an
ambient asynchronous flag.

The node admits proposal intents through one atomic three-state gate:

```text
Idle --first request--> Active
Active --pending collision--> ActiveDirty
ActiveDirty --more pending collisions--> ActiveDirty
Active --finish--> Idle
ActiveDirty --finish--> Active --one forced PendingDeploy follow-up--> Idle
```

Manual and recovery collisions report busy and are retried by their owning control
loop. Pending collisions latch the dirty epoch; all later pending collisions in the
same epoch coalesce into it. Completion changes `ActiveDirty` to `Active` before it
starts exactly one forced follow-up, so no competing caller can consume or duplicate
the wake. That follow-up is always non-empty. Cancellation clears the gate when the
current engine is unavailable, and the engine is resolved immediately before every
real or forced proposal so a queued closure cannot retain a stale Casper instance.

The heartbeat scheduler closes a recovery round only after a proposal reports
`Started` or `Success`; `Empty`, `Deferred`, and `Failed` remain retryable. A
nonleader records the exact round as skipped without reserving the proposer. The
axiom-free Rocq `proposal_scheduler_end_to_end` result proves reservation
serialization, outcome-sensitive completion, selected-leader authority, pending
plus recovery composition, ordered completion, and observation-reset behavior; it
is included in `heartbeat_backpressure_end_to_end` and therefore in
`MainTheorem.finalized_floor_heartbeat_backpressure_correct`.

### H16 (SAFETY/CONCURRENCY) — finalization evaluation was not bound to its durable predecessor

Parallel finalizer workers evaluated immutable DAG snapshots correctly, but the
publication callback re-read the durable head after evaluation. It checked only
that the selected candidate DAG-descended from that newer head. That substituted
a different state-lineage obligation into an already completed certificate.

The minimal execution is `F0 -> F1 -> C`. Two workers evaluate `F1` and `C`
against `F0`; both transitions preserve `F0`. `F1` commits first and activates
effect `e`. `C` is a DAG descendant of `F1`, but rejects `e`. Therefore
`state_preserves(F0, C)` and `dag_descends(F1, C)` are both true while
`state_preserves(F1, C)` is false. Late-binding the old certificate would make
the finalized ledger contain an adjacent state regression.

The implementation now captures one coherent `FinalizationHead` and DAG
snapshot before certificate evaluation. The same revision, block hash, height,
and record digest flow through candidate selection, state-preservation checking,
manifest construction, and `try_append`. Publication revalidates ancestry and
state preservation from the exact captured block. The compare-and-append accepts
only if that complete predecessor is still current. A changed predecessor returns
typed `StaleFinalization`; the worker performs no callback or metadata effect and
restarts from a fresh coherent base. Expensive evaluation remains parallel, and
only the constant-size ledger append is linearized.

`FinalizationBoundHead.tla` exhausts the safe two-worker state space at 101
generated / 71 distinct states to depth 5. Its late-bound control reaches the
exact five-state counterexample under both TLC and Apalache; the safe Apalache
projection passes through bound 6. Rocq proves exact-predecessor preservation,
stale revision/head inertness, fresh-evaluation necessity, and closure of the
validation/commit race without assumptions. Rust reproduces the active-effect
counterexample at the storage boundary, and Loom exhausts both same-base winners
and the ordered stale-certificate interleaving.

### H16A (LIVENESS/CONCURRENCY) — snapshot capture leaked an expected finalizer race

A validation snapshot reads the finalization ledger and its projected DAG state.
A finalizer can append a round between those reads.
The ledger then reports `StaleFinalization` to prevent a torn snapshot.
That result is an optimistic-concurrency signal, not a block-validation defect.

The snapshot path previously returned this signal to the caller.
Concurrent validation could therefore fail although both stored states were valid.
The finalizer already retried the same expected race at its write boundary.
The read path did not apply the equivalent rule.

The repair retries only the coherent finalization-base capture.
It validates the durable revision before and after projected-state capture.
It yields after each stale result and then reads a new base.
It returns all other errors without retry.
Replay, block validation, and admission do not repeat.
No validator or finalizer lock spans the retry.

`FinalizationSnapshotRetry.tla` models the reader and a concurrent finalizer.
Its safe actions publish only one revision-consistent floor and certificate.
The unsafe action publishes a stale observation and violates result coherence.
Rocq proves that stale observations publish no result.
Rocq also proves that a finite stale prefix reaches the observed coherent revision.
Loom explores finalizer progress between reader phases.
The Rust unit test injects stale captures before one coherent result.

The metric `finalization.snapshot.capture.retries` counts retry attempts.
Operators can use sustained growth to identify storage or finalizer contention.
The repair preserves parallel validator work and strict writer compare-and-append.

### H17 (LIVENESS) — failed finalizer workers falsely completed request coverage

The scheduler previously advanced `completed_through` when a spawned finalizer
task exited even if the task returned an error or panicked. Because
`launched_through` already covered the same ticket, the dispatcher then observed
no pending work and became quiescent. A transient storage, replay, or task fault
could therefore suppress the only request that would materialize a certified
floor. Proposal backpressure would continue to reject state-regressive work, but
no finalizer evaluation remained scheduled to release it.

The repaired scheduler classifies worker exit as success or failure. Success
advances monotonic completion. Failure releases only the worker slot, retains the
uncovered ticket, and makes it retryable after capped exponential backoff. A
newer successful worker subsumes any older failed ticket, preventing late retries
from regressing coverage. Expensive evaluations remain parallel; no validator,
admission, replay, or candidate-selection path is serialized.

Proposal deferral is now typed at the same boundary. Only a certified candidate
context that is ahead of the materialized floor produces
`FinalizedFloorMaterializationPending` and issues an idempotent finalization
request. Missing committee slots, inactive candidate authority, and stale
recovery permits remain distinguishable and cannot create a scheduler hot loop.
The exhaustive classification test covers all eight combinations of context
equality, slot completeness, and proposer membership; the slashing merge
regressions exercise deferral followed by successful materialization and retry.

`FinalizationWorkerRetry.tla` exhausts 658 generated / 311 distinct states at
depth 13 with two workers, repeated bounded failures, retry waits, and
newer-success races. Its old failure-as-completion control violates the
non-completion invariant in four transitions under TLC and Apalache; the safe
Apalache model passes through length 12. Rocq proves the corresponding
failure-inertness, retry obligation, success certification, and newer-success
subsumption contract without assumptions. Rust schedule/runner tests and Loom
exercise the same interleavings against the implementation boundary.

`ProposalFloorReadiness.tla` then composes two independently evolving nodes
with candidate-floor advancement, local materialization, committee-slot
availability, candidate-validator activity, and recovery-permit freshness. TLC
exhausts 1,612,009 generated / 93,636 distinct states at depth 21; Apalache
checks the safe transition system through length 8. Missing a materialization
request, scheduling finalization for an authority defect, and creating through
an unready context are separate mutation controls, and both checkers reproduce
all three counterexamples. The axiom-free Rocq refinement proves readiness
necessity, exact request classification, non-materialization isolation, and the
end-to-end proposal-readiness contract. Rust exhausts the eight Boolean
readiness combinations and verifies at the injected proposer boundary that only
`FinalizedFloorMaterializationPending` schedules finalization.

### H18 (LIVENESS/CONSENSUS PROGRESS) — durable finalizer discovery remained main-parent-only

Commit `1b19efea66c31e6b9fd73c2db635fff87a284301` made complete
causal, state-certified off-main promotion an ordinary part of this branch. The
per-block floor path then used all-parent coverage, but the durable LFB finalizer
still propagated validator support only down each latest message's main-parent
spine. A candidate carried solely through a secondary parent could therefore be
the exact state-certified proposal floor while remaining permanently invisible
to the component responsible for materializing that floor. Proposals correctly
returned `FinalizedFloorMaterializationPending`; repeated finalizer runs could
never release them.

This was not a threshold, clique, or majority-voting failure. The finalizer never
submitted the missing target to those unchanged decisions. The repair shares the
same descending all-parent latest-message coverage routine used by floor
derivation. For each candidate it produces exactly
$`\{v \mid candidate \preceq_{DAG} latest(v)\}`$, after which the existing hard
majority gate, exact mutual causal clique, independent exact state-preserving
clique, and current-LFB effect-preservation predicate all run for that candidate.
The winner is the greatest eligible `(block_number, block_hash)` pair. Discovery
is broader; certification is not.

The defect escaped the earlier proof boundary because
`CertifiedFloorPromotion` proved all-parent discovery for per-block floor
derivation while `FinalizerProgress` assumed its finite candidate sequence was
already complete. No refinement theorem or regression equated the durable
finalizer's concrete enumeration with pairwise all-parent reachability. The new
artifacts close that composition seam: `FinalizerFloorMaterialization.v` proves
coverage/decision extensionality, exact target-bound dual certification, and
unique highest-candidate selection; `FinalizerFloorMaterialization.tla` composes
two independently delivered node views with proposal deferral and local
materialization. TLC exhausts 9,289 generated / 1,849 distinct states to depth 15.
Apalache checks the safe model through length 8. Main-parent-only discovery and
causal-only rejected-sibling substitution each reproduce their named violation
under both tools. Rust compares the optimized selector with a slow exhaustive
per-target oracle, covers the strict 8-of-16 boundary and rejected sibling, and
passes the complete 11-test finalizer regression group. Loom explores a frozen
target while an ambient latest message arrives and proves the publication remains
bound to the frozen target.

### H19 (TEST CONTRACT) — a fixed deadline rejected live exact finalization

The isolated bridge regression produced a valid query deploy in canonical block
`#7`. A later support block advanced the observed LFB from `#4` to `#6`, which
correctly began a new heartbeat-recovery epoch. Two further support layers made
the target occurrence terminal approximately 49 seconds after inclusion. Every
node subsequently reported the same LFB and the exact deploy as `Finalized`; the
trace contained no invalid-block, replay, state-materialization, or resource
failure. The integration client's fixed 45-second total deadline nevertheless
reported the deploy as permanently pending.

The defect was not a Casper certificate or fork-choice error. It was an invalid
test-observer assumption: partial synchrony does not imply a universal 45-second
target-finalization bound, and a real intermediate floor advance is useful
progress without being proof that this target is terminal. Changing recovery
cadence, weakening exact deploy status, or treating an LFB advance as success
would hide the symptom by changing protocol meaning.

`TargetDeployTerminality.tla` consumes the exact status produced by the node's
already-verified deploy-status resolver as an opaque observation. Quorum and
certificate correctness remain in `FinalizedOccurrenceStatus`,
`CausalFinalityProjection`, `CertifiedFloorPromotion`, and
`FinalizerFloorMaterialization`; the client model does not duplicate or weaken
those decisions. It models independent intermediate LFB advances, first-sample
baseline establishment, same-height revisions, regressions, exact terminal
status, blocking requests that advance the monotonic clock directly to a
deadline, and the two observation budgets. It checks that success requires the
target's exact `Finalized` status, only strict LFB-height progress after the
baseline renews the stall budget, finalized-history anomalies fail loudly, no
renewal extends the absolute budget, expiry precedes interpretation of a
boundary response, and lack of progress terminates. Five
independently selected negative configurations reproduce the fixed-deadline false
timeout, hidden finalized-history anomaly, inexact-success, late-terminal
deadline bypass, and first-sample renewal defects. The first model revision incorrectly permitted observer metadata
to mutate after the observer returned; TLC exposed that lifecycle error, and
terminal observer states now stutter without further mutation.

`TargetDeployTerminality.v` proves the corresponding deadline-first pure policy,
the concrete `45/43/49/135` trace, and rejection of finalized/failed/expired
responses at an expired boundary axiom-free. The pyf1r3fly regressions run the production
poller against a monotonic fake clock, including deadline-consuming RPCs,
first-sample baseline behavior, finite-positive duration enforcement, invalid
client-timeout containment, and fail-loud revision/regression controls.
System-integration unit tests pin timeout configuration and wrapper forwarding.
Positive exact-terminal workflows opt into a 45-second no-progress budget plus a
135-second absolute bound. The absolute value is operational headroom, not a
finality theorem; the proof establishes observer boundedness, not that a deploy
must finalize. No node or Casper behavior is changed by this repair.

---

## 4. Invariant catalog → artifact map

| ID | Property | Mechanized / checked in |
|---|---|---|
| **T-TERM** | spine walk terminates | Rocq `Foundation.spine_walk_terminates` |
| **T-MONO / L-ANC** | ancestor-monotone finalization (no floor regress, S2) | Rocq `CliqueOracle.L_ANC`, `L_ANC_mainparent` |
| **L-SNAP** | snapshot-monotone finalization | Rocq `CliqueOracle.L_SNAP`, `L_ANC_SNAP` |
| **C1 — θ-exact refinement** | the node's runtime θ-decision (`ft_exact_gt`) is ancestor- and snapshot-monotone, and every certificate at θ≥0 is strict-majority `Finalized`; candidate-floor and durable-finalizer predicates are identical and both reject equality | Rocq `CliqueOracle.L_ANC_ft`, `L_SNAP_ft`, `L_ANC_SNAP_ft`, **`Finalized_ft_refines_Finalized`** (only `0≤num`, `0<den`), `FinalityThresholdAlignment.candidate_floor_and_finalizer_equivalent`, the 8-of-16 boundary control, and `is_quorum_ft_mono_weight`/`Finalized_ft_enlarge` via `FtExact.ft_exact_gt_mono_q` |
| **C1′ — negative-threshold coverage (hard gate)** | for negative sentinels, the exact θ-test can be weaker than majority; the runtime decision also applies `2·agreeing > S`, which yields strict-majority `Finalized` for all numerators. T-CACHE holds directly over `Finalized_ft` for every numerator via `L_ANC_ft` | Rocq `CliqueOracle.hard_gate`, `hard_gate_iff_Finalized`, `Finalized_ft_hg`, **`Finalized_ft_hg_refines_Finalized`**, `L_ANC_ft_hg`/`L_SNAP_ft_hg`, and `GuardBridge.BridgeFt.guard_constant_committee_transparent_ft` |
| **C11 — state-support refinement** | LFB/floor promotion requires both the original causal certificate and an exact certificate whose supporting validators' frozen latest messages preserve every active candidate effect; state certification refines causal certification and cannot be inferred from DAG-parent inclusion | Rocq `CliqueOracle.state_agreement_refines_causal_agreement`, `state_finalization_refines_causal_finalization`, `StateLineageFinality.causally_certified_state_unsupported_candidate_is_ineligible`, and `StateEffectProvenance.accepted_three_way_merges_have_majority_certificate`; TLA+ `Inv_CausalMergeVoteIsNotStateSupport`, `Inv_NoUnsupportedStateFloor`, and `StateEffectProvenance.Inv_DeliveredQuorumCertifiesSource`; Rust `causal_merge_vote_cannot_certify_a_rejected_parent_state`, `finalizer_rejects_causal_certificate_without_state_support`, and `accepted_three_way_merges_retain_state_support_across_repeated_rounds` |
| **C12 — certificate-to-floor discovery** | complete causal evidence must make a dual-certified state candidate visible to per-block floor derivation even when the candidate is secondary to every parent and absent from every main spine; discovery changes neither certificate | Rocq `CertifiedFloorPromotion` and `finalized_floor_certified_promotion_correct`; TLC/Apalache `CertifiedFloorPromotion` safe model and main-spine-only negative control; Rust `derive_floor_promotes_dual_certified_universal_secondary_ancestor` plus `dual_certified_universal_floor_is_independent_of_branch_parent_and_validator_order` |
| **C13 — coverage optimization transparency** | one descending latest-message coverage pass must yield exactly the pairwise DAG-ancestry supporter set, corresponding weight map, and clique verdict; repeated universal evaluation may be omitted only across an unchanged one-predecessor linear snapshot whose parent is newer than every latest message | Rocq `propagated_coverage_exact`, `coverage_decision_transparent`, `unchanged_linear_snapshot_reuse_sound`, and the two `MainTheorem` capstones; TLC/Apalache `LatestMessageCoverage` safe model plus unordered late-propagation control; Rust generated pairwise supporter/weight/verdict equivalence, non-descending-edge rejection, and linear/multi-parent/snapshot reuse guards |
| **C14 — finalizer materialization alignment** | a proposal floor discovered through secondary-parent evidence must be discoverable by the durable finalizer; its selected target must carry its own exact causal and state certificates, preserve the current LFB, and be the deterministic greatest eligible `(block_number, block_hash)` | Rocq `FinalizerFloorMaterialization.{validated_materialization_is_exact_and_dual_certified, target_substitution_is_rejected, finalizer_discovery_matches_pairwise_certificate, highest_exact_candidate_is_unique, finalizer_floor_materialization_trace_correct}` and `MainTheorem.finalized_floor_materialization_target_alignment_correct`; TLC/Apalache `FinalizerFloorMaterialization` safe model plus main-parent-only and causal-only controls; Rust exhaustive-oracle, strict-boundary, rejected-state, secondary-parent, reconvergence, property, and full finalizer regressions; Loom frozen-target/latest-message-arrival interleaving |
| **T-CERTIFICATE-RETRIEVAL** | a protocol-6 block missing its committed certificate remains detached on a typed persistent dependency; bounded retries survive transport failure and restart; only an expected, shape-valid, digest-matching response may persist; duplicate responses converge to one resolution and one queue wake; every fetchable persistent obligation eventually queues under weak fairness | Rocq `FinalizationCertificateRetrieval` and `MainTheorem.finalized_floor_certificate_retrieval_correct`; TLC `FinalizationCertificateRetrieval` safe model (58,184 generated / 11,879 distinct states, depth 18) plus six isolated controls; Apalache through symbolic length 12 plus the same controls; Rust content-addressed sidecar, restart, parser, retriever, property, async Running-engine, and Loom duplicate-response regressions |
| **T-CERTIFICATE-PARENT-FRONTIER / S44** | receiver admission accepts replay-safe declared-parent subsets, preserves frozen non-parent justifications, and rejects any candidate whose parents omit all ancestry of the signed floor | Rocq fork-choice `GuardBridge` refinement, projection, strict-subset witness, and disconnected witness; TLC `CertifiedFloorCommitment` safe model (294,193 generated / 31,738 distinct states, depth 32) plus causal-input control; Apalache safe length 8 plus the control at length 6; Rust exact-error unit test and protocol-6 multi-validator positive and negative replay tests |
| **T-DEPENDENCY-MAINTENANCE** | one local maintenance invocation attempts every ordinary-block and certificate obligation in its frozen snapshot before returning the first dispatch error; a failed block request cannot suppress certificate progress, and parallel LFS requests are all awaited rather than cancelled on the first failure | Rocq `DependencyMaintenanceRound` and `MainTheorem.finalized_floor_dependency_maintenance_correct`; TLC safe model (348 generated / 158 distinct states, depth 7) plus abort-on-first-failure control; Apalache through symbolic length 8 plus the same control at length 3; Rust direct `MultiParentCasper::fetch_dependencies`, block-processor ordinary/stale, block-retriever mixed-maintenance, and LFS await-all regressions |
| **T-TARGET-TERMINAL** | an external wait succeeds only on the exact target's canonical `Finalized` status observed within both budgets; the first LFB sample is only a baseline, strict later height progress may renew a stall budget, revision/regression fails loudly, no progress can renew the absolute bound, and a boundary response cannot bypass an expired deadline | Rocq `TargetDeployTerminality` and `MainTheorem.finalized_floor_target_deploy_wait_correct`; TLC/Apalache `TargetDeployTerminality` safe model plus fixed-timeout, history-anomaly, inexact-success, late-terminal, and first-baseline-renewal controls; pyf1r3fly fake-clock/RPC-deadline regressions; system-integration timeout/wrapper regressions and positive exact-terminal integrations |
| **C5 — snapshot advancement** | growth modeled as latest-message ADVANCEMENT (each binding → a DAG-descendant), not just preservation; L-SNAP holds for it, and preservation ⇒ advancement so the old L-SNAP is subsumed | Rocq `CliqueOracle.snap_advances`, `agrees_snap_advance_mono`, **`L_SNAP_advance`**, `L_ANC_SNAP_advance`, `L_SNAP_advance_ft`, `snap_extends_snap_advances`, `L_SNAP_of_extends` (original L-SNAP re-derived) |
| **T-CACHE** | warm up-walk == cold walk (no fork from cache, S1) | Rocq `Floor.frontier_cache_transparent` (takes `AdjDC`) **+ `GuardBridge.chain_adj_AdjDC` / `guard_constant_committee_transparent`** — the committee-constancy guard *derives* `AdjDC` from L-ANC, so the seam is bridged, not assumed; Rust test `guard_trip_committee_change_falls_back_to_cold` |
| **T-DETMERGE / T-CONV** | merge order-independent (no fork, S6) | Rocq `Merge.merge_or_perm`, `merge_max_perm`; Rust proptests `bitmask_or_is_commutative`, `integer_add_is_commutative` (`rspace++/…/merging_logic.rs` — the fold operands commute ⇒ order-independent) + `multiple_branches_should_merge_number_channels` (`casper/tests/merging/merge_number_channel_spec.rs`, concurrent IntegerAdd branches merge deterministically) |
| **T-K1** | no mergeable write lost (the 400-block loss, S5) | Rocq `Merge.merge_or_no_lost_bit`, `merge_absorbs`; Rust proptests `bitmask_or_dominates_each_input` (the BitmaskOr result carries every bit set in either input — no mergeable bit dropped) + `bitmask_or_is_idempotent` (`rspace++/…/merging_logic.rs`) |
| **T-NDA** | recovery not double-applied | Rocq `Recovery.apply_idem`, `no_double_apply`; Rust test `recovery_effect_is_applied_at_most_once` (`casper/tests/finalized_floor/recovery_no_double_apply.rs`) — drives the production `interpreter_util::canonical_won_sigs` recovery record: an effect is not canonically-won before it is applied, is won exactly once after a block includes it, and the recovery filter `apply(apply(s)) == apply(s)` (drops the won effect, never re-proposes it) |
| **T-BASE-PRECEDENCE** | a finalized receipt cannot be masked by an above-floor tombstone or retried | Rocq `MergeRecoveryCoherence.base_committed_dominates_scope`, `base_committed_blocks_retry`; TLA⁺ `MergeRecoveryCoherence.Inv_AtMostOneEffectPerSignature`; Rust `active_protocol_preserves_finalized_receipt_against_visible_tombstone` and `exact_protocol_finalized_receipt_is_terminal_at_every_rejection_height` |
| **T-DEPLOY-IDENTITY** | recovery, tombstone, snapshot, and admission state preserve the protocol domain of every deploy lookup ID; equal payload bytes from legacy and protocol-v6 encodings cannot reject or suppress one another | Rocq `DeployIdentitySeparation.{equal_payload_cross_domain_ids_are_distinct,v6_rejection_preserves_equal_payload_legacy_identity,legacy_rejection_preserves_equal_payload_v6_identity}` and `MainTheorem.finalized_floor_deploy_identity_separation_correct`; TLC and Apalache `DeployIdentitySeparation` safe model plus raw-key unsafe control; Rust recovery-projection property test and `loom_recovery_custody::concurrent_legacy_and_v6_dispositions_do_not_alias` |
| **T-CHAIN-ATOMIC** | a tombstone or finalized-base duplicate removes the complete dependent chain and every projected effect | Rocq `tombstoned_chain_is_excluded`, `base_duplicate_chain_is_excluded`; TLA⁺ `Inv_ChainAtomic` and the partial-chain unsafe control; Rust `exact_tombstone_rejects_complete_chain_and_preserves_reason` and `base_committed_duplicate_rejects_complete_chain` |
| **T-EFFECT-CAUSAL-CLOSURE / S26–S27** | exact rejection is the least transitive physical dependency closure: every dependent is rejected and every independent exact effect survives regardless of source-block ancestry or inspection order | Rocq `EffectCausalClosure.{causal_rejected_is_least, accepted_has_no_rejected_dependency, merge_child_survives, user_effect_survives, mergeable_materialization_is_not_dependency}` and `MainTheorem.finalized_floor_effect_causal_closure_correct`; TLA⁺/TLC/Apalache `EffectCausalClosure` safe model plus block-lineage and direct-only unsafe controls; Rust datum/continuation/mergeable identity examples, indexed-versus-pairwise and value-sensitivity proptests, transitive late-rejection proptest, legacy-fallback boundary test, and the release multi-parent merge regression |
| **T-STATE-RECORD** | selected occurrence records, ordinary state, mergeable metadata, and exact causal identities describe one transition | Rocq `state_record_effect_coherence`, `committed_effect_identity_consistent`; TLA⁺ `Inv_StateRecordCoherence`, `Inv_EffectIdentityConsistency`, and the retention and identity unsafe controls; Rust exact-projection example tests in `deploy_chain_index` |
| **T-ACTIVATION** | the active protocol, not the floor version, selects base precedence; above-floor scope and record encoding match the active version | Rocq `ProtocolActivationCoherence` and capstone `finalized_floor_protocol_activation_correct`; TLA⁺ `ProtocolActivationCoherence` safe model plus floor-version, mixed-scope, and encoding unsafe controls; Rust backstop tests for legacy-floor/current-active composition, mixed scope, and malformed encoding |
| **T-PROTOCOL-LIFECYCLE** | ceremony, approval, approved-block admission, adoption, proposal, and reception use one supported version; legacy and unknown approved versions fail closed | Rocq `ProtocolVersionLifecycle` and `finalized_floor_protocol_lifecycle_correct`; TLA⁺ three safe lifecycle configurations plus five unsafe controls; Rust `approved_protocol_version_adoption_accepts_current`, `noncurrent_approved_protocol_versions_fail_without_mutation`, `supported_protocol_versions_are_exactly_the_declared_versions`, `approved_block_rejects_noncurrent_protocol_versions`, `block_approver_protocol_should_reject_mismatched_protocol_version`, and `peer_admission_uses_the_running_protocol_version` |
| **T-BOOTSTRAP-REPLAY** | approved-state reconstruction replays each historical block from the immutable context serialized by that block; a joiner's current tip and local configuration cannot alter a historical root or invalidate valid history | Rocq `BootstrapReplayContext.{consensus_block_replay_matches_declared_root, consensus_history_replay_matches_declared_roots}`; TLA⁺ `ApprovedStateReplay` safe model and current-context unsafe control; Rust `replay_block_from_consensus_data`, exact genesis/non-genesis payload regressions, and the late-checkpoint epoch-change system-integration test |
| **T-LOCAL-FAULT** | a locally inconclusive validation is neither accepted nor objectively invalid: certification preserves its exact block hash or state root; genesis-rooted and truncated histories retain their distinct local classifications; same-artifact requests deduplicate; independent requests commute; the block leaves the ready queue, remains in custody across a failed transport request, and cannot release an ordinary descendant from the wrong recovery | Axiom-free Rocq `LocalFaultDeferral` and `typed_local_validation_recovery_correct`; parallel TLA⁺ `LocalValidationRecovery` safe model (28,881 generated / 9,025 distinct states, depth 33) plus ready-retention, identity-collapse, drop, and false-invalidity controls; Apalache safe bound 8 with the same controls at bounds 2 and 9; three `loom_local_validation_recovery` race/isolation models; Rust typed round-trip, idempotent ready-path acknowledgement, exact certificate-sidecar dependency, block/state retry, and descendant-gating regressions |
| **T-FUNDING-ADMISSION** | state-bound funding is classified from the recorded block pre-state; a canonical protocol-v6 family-1 funding ground projects to the same native SystemVault as its public key without collapsing its typed accounting lane; malformed encodings and other families cannot alias that custody; underfunding becomes a terminal zero-effect record; later supply cannot resurrect it; a fundable deploy cannot be forged as rejected | Rocq `FundingAdmissionLifecycle`, `ProtocolVersionLifecycle.funding_ground_custody_projection_correct`, `terminal_funding_admission_lifecycle_correct`, and `MainTheorem.finalized_floor_funding_ground_custody_projection_correct`; TLA⁺ `FundingAdmissionLifecycle` safe model plus live-state and pending unsafe controls and `ProtocolVersionLifecycle` custody-projection control; Rust `vault_payer` examples/properties, `physical_rejection_rolls_back_before_later_state_bound_execution`, `funding_admission_rejection_roundtrips_as_terminal_non_execution`, and `repeat_deploy_validation_rejects_duplicate_signatures_within_one_block` |
| **T-ADMISSION-EFFECT-ALIGNMENT / S33 / L11** | block-body status records project to exactly effect-bearing user executions before metadata cardinality, adjacent state-witness traversal, user/system splitting, and execution-index assignment; terminal admission rejection contributes no slot, ordinary execution failure retains one, and later proposal/finalization remains live | Rocq `AdmissionEffectAlignment` and `finalized_floor_admission_effect_alignment_correct`; TLA⁺/TLC/Apalache `AdmissionEffectAlignment` safe model plus raw-status-counting control; Rust `block_index` concrete rejection/`closeBlock`, ordinary-failure, order, and 256-case generated cardinality tests |
| **T-REASON-CONFLUENCE** | equal sets of causally valid rejection explanations serialize one reason regardless of parent or arrival order | Rocq `RejectionReasonConfluence` and `finalized_floor_rejection_reason_confluence_correct`; TLA⁺ `RejectionReasonConfluence.Inv_EqualObservationConverges` plus the last-writer unsafe control; Rust `rejection_reason_join_uses_direct_cause_precedence`, the commutative/associative/idempotent proptests, and `merge_context_canonically_joins_concurrent_rejection_reasons` |
| **S5 / Inv_NoLostParentWrite** | over-Δ never drops a parent write | TLA⁺ `SpecFixed` (holds); `Spec` (violated) |
| **Δ bound (driver)** | floor distance stays ≤ cap | TLA⁺ `Inv_DeltaWithinCap` |
| **L3/L5 liveness** | chain still progresses despite the backstop | TLA⁺ `Liveness_Progress` |
| **service-rate regimes** | lag-dependent work creates positive feedback; constant overhead removes that feedback, while `service > arrivals`, `=`, and `<` respectively drain, preserve, or grow lag | Wolfram `delta_ratchet.wl`; TLA⁺ heartbeat/backpressure liveness models |
| **T-SOUND** | chosen floor is a sound base; `None` ⇒ Err correct (S4) | Rocq `Selection.select_sound`, `select_none_correct` |
| **T-LIN** | a Case-A base is a common DAG-ancestor (one chain) | Rocq `Selection.case_a_common_ancestor`; Rust test `derive_floor_case_a_floor_is_common_ancestor_of_all_parents` (the Case-A floor is `is_dag_ancestor` of every parent) |
| **T-FIN** | the chosen floor is finalized | Rocq **`GuardBridge.upgo_finalized`** (the warm up-walk's result is `Finalized` — discharges the premise unconditionally) + `Selection.select_finalized` (a floor drawn from finalized candidates is finalized); Rust test `derive_floor_result_is_finalized_over_justifications` (the result clears `CliqueOracle::ft_witnessed_exact` over the justification snapshot) |
| **T-PS** | safety for ANY parent list (unconstrained oracle) | Rocq `Selection.T_PS`; TLA⁺ `FinalizedFloorScan` (nondeterministic parent set); Rust test `derive_floor_incompatible_fork_errors` |
| **T-COMM** | authorization committee = `bonds_of(post_state(floor))`, a pure function of the floor; exact justifications, sender membership, and synchrony share it (S8) | Rocq `Selection.committee_is_floor_bonds`; Rust `Validate::floor_authority`, proposal authority preflight, and finalized-floor synchrony weights |
| **T-COMMITTEE-TRANSITION** | serialized bonds equal replayed post-state bonds; same-block transitions cannot self-authorize; only accepted caches register identities; registered transitions become authoritative only after floor promotion | Rocq `CommitteeTransition` and `MainTheorem.committee_transition_correct`; TLA⁺/TLC/Apalache `RecoveryCommitteeTransition` and five unsafe controls; Rust post-state cache, invalid-registration, transition, exact-justification, sender, head-drift, and Loom interleaving regressions |
| **T-FINALIZATION-ATOMICITY** | concurrent finalizer evaluations publish at most one immutable state-preserving successor per exact durable predecessor; a changed revision or block identity forces fresh evaluation; stale workers have no effects; request/release races cannot lose a wake; failed or panicked workers cannot complete coverage and remain retryable until a successful equal-or-newer worker subsumes them | Rocq `FinalizationAtomicity` and `MainTheorem.{finalized_floor_atomic_commit_correct,finalized_floor_worker_retry_correct,finalized_floor_bound_head_correct}`; TLA⁺/TLC/Apalache `FinalizationAtomicity`, `FinalizationWorkerRetry`, and `FinalizationBoundHead` plus split-commit, early-effect, stale-overwrite, failure-as-completion, late-bound-state-regression, regressive-publication, and lost-wake controls; Rust finalization-ledger and worker-exit races, `bound_finalization_rejects_stale_certificates_and_dropped_finalized_state`, and `loom_finalization_atomicity` |
| **T-FINALIZATION-SNAPSHOT-CAPTURE** | a reader publishes one coherent durable revision, projected floor, and certificate; a concurrent append makes the capture stale; stale capture publishes no result and retries without repeating validation or serializing validators | Rocq `FinalizationAtomicity.{stale_snapshot_capture_publishes_no_result,finite_stale_snapshot_prefix_reaches_coherent_capture,snapshot_retry_returns_only_an_observed_coherent_revision}` and `MainTheorem.finalized_floor_snapshot_capture_retry_correct`; TLA⁺/TLC/Apalache `FinalizationSnapshotRetry` and its stale-publication control; Rust `stale_finalization_capture_retries_until_coherent` and `snapshot_capture_propagates_non_stale_errors_without_retry`; Loom `proposer_capture_is_coherent_during_parallel_finalization` |
| **T-PROPOSAL-FLOOR-READINESS** | proposal creation requires an exactly materialized candidate floor, complete candidate committee slots, active candidate-floor validator authority, and a fresh required recovery permit; only missing floor materialization schedules finalization, while authority and permit defects cannot create retry traffic | Rocq `ProposalFloorReadiness` and `MainTheorem.finalized_floor_proposal_readiness_correct`; TLA⁺/TLC/Apalache `ProposalFloorReadiness` plus pending-without-request, non-floor-request, and readiness-bypass controls; Rust exhaustive classifier and injected proposer scheduling tests |
| **T-FINALIZATION-GENESIS-IDENTITY** | the pristine ledger atomically creates one immutable genesis anchor, revision-zero head, and all recovery cursors; exact retries after arbitrary advancement are write-free; conflict, partial bootstrap, corruption, and inferred backfill fail closed; append and restart preserve the anchor | Rocq `FinalizationAtomicity.rooted_genesis_identity_contract` and `MainTheorem.finalized_floor_rooted_genesis_identity_correct`; TLA⁺/TLC/Apalache `FinalizationGenesisIdentity` plus reset, conflict, split-bootstrap, and unrooted-backfill controls; Rust ledger/DAG restart and corruption tests; four rooted-ledger Loom races |
| **T-FINALIZATION-RECOVERY** | projection, effect completion, and receipt compaction advance only over durable contiguous prefixes with $`0 \le C \le E \le P \le H`$ across every crash boundary | Rocq `FinalizationAtomicity.finalization_recovery_contract` and `MainTheorem.finalized_floor_recovery_cursors_correct`; TLA⁺/TLC/Apalache `FinalizationRecovery` plus projection-gap, early-effect, and effects-gap controls; Rust restart, arbitrary-order property, and Loom cursor regressions |
| **T-LOCAL-FINALIZATION-WITNESS** | each durable finalization record binds the exact frozen eligible latest messages, supporting closure, target state, exact FTT, authority context, and immutable local predecessor; equal target state does not imply equal node-local revision or digest, and remote local-ledger identity cannot authorize publication | Rocq `DivergentFinalizationHistories.{equal_finalized_target_does_not_imply_equal_local_ledger_identity,cross_node_local_ledger_lookup_rejects_divergent_histories}`; TLA⁺/TLC `DivergentFinalizationHistories` safe model (8 generated / 6 distinct states), Apalache through length 5, and remote-ledger-authority controls under both checkers; Rust local-witness validation, same-target/different-history example test, and arbitrary local-round-history property test |
| **T-WITNESS-EQUIVALENT-CARRIER / S42 / L17** | a predecessor proof is eligible by accepted causal membership, protocol, exact floor, and exact replay state—not by receiver-local witness identity; selection preserves the carrier block and committed digest as one pair; matching admission wakes a parked finalizer without accepting a wrong state | Rocq `WitnessEquivalentCarrier` and `MainTheorem.finalized_floor_witness_equivalent_carrier_correct`; TLA⁺/TLC `WitnessEquivalentCarrier` safe model (4,155 generated / 961 distinct states, depth 9) plus four exact controls; Apalache safe bound 5 and the same controls at bound 3; Rust example/property selector regressions, storage pair validation, asymmetric two-validator lifecycle, and three Loom wake/coalescing models |
| **T-LIVE-MINORITY-FORK-RECOVERY** | stale age opens tip discovery only; all dependencies pass through ordinary admission; only the local frozen-context finalizer may publish; post-capture admission remains retryable; validator sequence, bond generation, evidence identity, concurrent proposal, other validators, and other shards remain unchanged | Rocq `MinorityForkRecovery.minority_fork_recovery_correct`; TLA⁺/TLC `LiveMinorityForkRecovery` safe model (264,205 generated / 16,984 distinct states), Apalache through length 6, and remote-head, missing-dependency, and global-pause controls under both checkers; Rust stale/fresh running-engine regressions, three-node minority-fork recovery, recovery-cadence property test, and `loom_live_minority_fork_recovery` interleavings |
| **Case-B** | Case-A fails but every other candidate is compatible ⇒ the dominating finalized tip is a sound base | Rocq `Selection.case_b_compatible`; Rust test `derive_floor_case_b_selects_dominating_finalized_tip` |
| **maximality / T-DET** | the chosen floor is the HIGHEST sound candidate, a pure function of (parents, candidates) ⇒ every node picks the same floor | Rocq `Selection.select_highest_sound`; Rust example `derive_floor_selects_highest_sound_finalized_candidate` (inheritance + advancement: lagging inherited cuts `{g@0, t@1}` lose to the newly-finalized higher candidate `c@2`) + Rust **proptest** `derive_floor_selects_highest_sound_candidate_over_chain` (`finality::floor` lib tests: on a RANDOM single-validator chain, the candidate multiset `{inherited b_i} ∪ {frontier b_k}` is all Case-A sound and `derive_floor` returns its block-number MAXIMUM — inheritance or advancement, whichever is higher) |
| **H3 coverage** | floor-bounded scan drops no parent write ≥ floor | Rocq `Selection.scope_covers_band`; TLA⁺ `FinalizedFloorScan` (`.cfg` PASS, `_bug.cfg` counterexample) |
| **T-ALG (semilattice)** | BitmaskOr / keep-one fold laws | Rocq `Merge` (`Nat.lor` / `Nat.max`); Rust proptests `bitmask_or_is_associative`, `bitmask_or_is_commutative`, `bitmask_or_is_idempotent` (`rspace++/…/merging_logic.rs` — the join semilattice laws for the shipped `combine_mergeable_value`) |
| **T-ALG (IntegerAdd c/d)** | wrapping-add group + checked-apply reject overflow/`<0` (S7) | Rocq `IntegerAdd.wadd_assoc`, `checked_apply_rejects_overflow`/`_negative`; Rust proptests `integer_add_is_commutative`, `integer_add_is_associative`, `integer_add_overflow_returns_none` (`≡ i64::checked_add`) + unit `integer_add_rejects_overflow_and_underflow` (`rspace++/…/merging_logic.rs`) |
| **IntegerAdd launder** | fail-loudly at BOTH combine **and terminal apply**; the diff (`end−prev`) stays wrapping — it is the group inverse that recovers the true delta; supply-cap bound | Rocq `IntegerAdd.launder_exhibit`/`checked_combine_sound`/`supply_cap_no_launder`; Z3 `integeradd_launder_bitvec.py`; Rust `combine_mergeable_value` (combine, `checked_add`), `calculate_number_channel_merge` (terminal apply, `checked_add`+`≥0`); tests `cal_merged_result_rejects_integer_add_true_launder_wraps_nonnegative`, `merge_integer_add_overflow_is_rejected`, `diff_integer_add_recovers_wrapped_delta` |
| **A9 exact-integer FT** | finalization decides `2·q·den ⋛ S·(den+num)` in i128 (`≥` floor / `>` LFB), not the fuzzy f32 ratio — precise + node-identical | Rocq `MainTheorem.finalized_floor_ftexact_correct` (`FtExact.v`); Z3 `ft_exact_no_overflow.py`; Sage `ft_algebra.sage`; Rust `clique_oracle.ft_decides_exact`/`ft_witnessed_exact`; test `ft_decides_exact_tests` |
| **T-CERT-SEPARATION** | state admission does not alter causal certification: a stale merge may hold both exact certificates yet fail current-LFB effect preservation, while a rejected parent may retain its causal certificate but fail the distinct state-preserving certificate | Rocq `StateLineageFinality.{eligibility_preserves_certificate, certified_stale_candidate_is_ineligible, causally_certified_state_unsupported_candidate_is_ineligible, state_lineage_end_to_end}` and `CliqueOracle.state_finalization_refines_causal_finalization`; TLA⁺/Apalache `Inv_CliqueCertificateIsUnchanged`, `Inv_StaleMergeSeparatesDagAndState`, and `Inv_CausalMergeVoteIsNotStateSupport`; Rust `finalizer_rejects_dag_descendant_without_state_lineage`, `causal_merge_vote_cannot_certify_a_rejected_parent_state`, and `finalizer_rejects_causal_certificate_without_state_support` |
| **T-STATE-PRESERVATION / S24** | every floor/LFB promotion has state-preserving support and preserves every effect active at the current LFB; stale-state and rejected-parent promotions violate distinct invariants, while an off-main-parent floor rebase restores admissibility and progress | Rocq abstract admission results in `StateLineageFinality` plus the concrete recurrence in `StateEffectProvenance` and `MainTheorem.{finalized_floor_state_lineage_correct, finalized_floor_state_effect_provenance_correct, finalized_floor_state_support_refines_causal_certificate}`; two-node asymmetric `StateLineageFinality` and three-validator `StateEffectProvenance` TLA⁺ models plus negative controls; Apalache bounded arrival-order safe/unsafe checks; Rust state-frontier/state-support proptests, stale-state rejection, rejected-parent rejection, off-main advancement, exact three-way/permutation/repeated-round regressions, and real conflicting-deploy execution-rebase regressions |
| **T-EFFECT-PROVENANCE / S28** | active effects equal the accepted multi-parent merge recurrence; parent order and redundant covered parents do not change the set; direct rejection removes only the named identity; a complete rejection-candidate superset yields exactly the subset-preservation verdict | Rocq `StateEffectProvenance.{merge_parent_order_invariant, covered_parent_elimination, merge_rejects_named_effect, repeated_three_way_merge_preserves_source, complete_rejection_candidate_check_iff_preserves}` and `MainTheorem.finalized_floor_state_effect_provenance_correct`; TLC `StateEffectProvenance` safe model (649 generated, 144 distinct states) and single-base control; Apalache `StateEffectProvenanceApalache` through bound 8 and single-base counterexample; Rust body/metadata round trips, validation tamper/canonicality checks, all six three-parent permutations, repeated majority round, unrelated-rejection scan, and merge-rebase provenance assertions |
| **T-STATE-PARENT / S29** | proposer fork choice derives base-admissible causal parents before applying floor descent; finality votes are a strict subset; every causal tip is direct or covered; the exact frozen frontier either fits in full or defers without signing; the configured committee maximum is not a live-frontier gate; the captured LFB remains an ancestor; floor-rebased replay prevents stale deltas from erasing finalized effects | Rocq `CausalFinalityProjection.{finality_projection_is_subset_of_causal_parent_projection,accepted_stale_latest_is_causal_but_cannot_vote,causal_parent_minus_vote_is_exactly_floor_stale,intrinsically_invalid_latest_message_cannot_be_causal_parent,wrong_generation_cannot_be_causal_parent,causally_equivocating_incarnation_cannot_be_causal_parent}` plus `StateEffectProvenance`, `ForkChoice.Bound` exact-fit/no-truncation theorems, and the axiom-free `MainTheorem` capstones; TLC `StatePreservingForkChoice` safe model and its causal/floor controls; TLC/Apalache `ParentFrontierCapacity` safe exact-fit and over-cap schedules plus the static-maximum control; Rust projection properties, exact-capacity properties, non-signing proposer deferral, causal-coverage bounds tests, two-validator stale-sibling merge regression, and Loom frozen-context plus exact-capacity interleavings |
| **T-RESTORE-CONTEXT / S45** | A restored node can omit the canonical genesis body without changing exact slots, frozen stake, certified projections, digest, replay state, or cost state. Each noncanonical missing latest-message body remains a typed dependency. Startup publishes only a complete reconciled index. The first validator-authored block has sequence 1. Validator sequences remain monotonic across bond generations. | Rocq `RestoreHorizonCertifiedContext` proves identity classification, exact-slot and stake preservation, projection and cost equality, stale-index elimination, materialization, support retention, first-proposal parity, the first authored sequence, and monotonic sequence. TLC exhausts `RestoreHorizonCertifiedContext` and `RestoreHorizonStartup` with eight weakened-rule controls. Apalache checks both safe models and their missing-dependency and skipped-reconciliation controls. Rust example, property, storage, restart, reporting, proposer, carrier, garbage-collection, and integration tests bind production behavior to the models. `loom_restore_horizon_startup` checks atomic publication to concurrent readers. |
| **T-STALE-SIBLING-RECOVERY / S29** | once one accepted sibling becomes the floor, another accepted sibling remains causal until an exact complete-frontier settlement names its source occurrence; every observer records the rejection, but only the source carrier owner keeps retry custody; distinct owners can recover independent sources concurrently; finalization preserves the floor effect and converges on every accepted effect | Rocq `StaleSiblingRecovery` plus `OccurrenceDisposition.{recovery_custody_authorization_unique_per_carrier,distinct_carrier_owners_recover_independently}` and `MainTheorem.finalized_floor_stale_sibling_recovery_correct`; TLC `StaleSiblingRecovery` safe model plus exact controls; bounded Apalache safe model plus the same symbolic controls; Rust `resolved_asymmetric_frontier_rehomes_excluded_local_deploy`; Loom `loom_recovery_custody` |
| **T-RETRY-FRONTIER-COVERAGE** | every valid latest message is covered by at least one selected parent; different messages can use different parents; parent and latest-message order cannot change readiness; floor authorization and owner custody remain mandatory; lease expiry bypasses only incomplete frontier coverage; ordinary leadership remains independent | Rocq `RecoveryFrontierCoverage` and `MainTheorem.{finalized_floor_collective_recovery_coverage_correct,finalized_floor_split_recovery_frontier_correct,finalized_floor_recovery_leadership_separation_correct,finalized_floor_recovery_parent_order_independent,finalized_floor_recovery_latest_order_independent}`; TLC/Apalache `RecoveryFrontierCoverage` safe model plus one-parent control; Rust split-frontier, permutation, incomplete-frontier, custody, reflexive-cover, lease, and map-cell regressions |
| **T-CERTIFIED-FLOOR-PROMOTION / S30** | a block secondary to every current tip but covered by every parent's causal past becomes the replay floor exactly when both unchanged certificates and inherited-floor preservation hold; parent arrival/order cannot change the choice | Rocq `CertifiedFloorPromotion.{dual_certified_current_floor_is_candidate, dual_certified_current_floor_is_discoverable, selected_floor_preserves_current, certified_floor_promotion_end_to_end}` and `MainTheorem.finalized_floor_certified_promotion_correct`; TLC safe model (1,051 generated / 225 distinct states, depth 9) plus main-spine-only control; Apalache safe bound 8 plus unsafe step-3 trace; Rust exact-`FTT=0.1` example over all six parent permutations, state-rejection control, and generated branch-depth/order property test |
| **T-COVERAGE-TRANSPARENCY / S31** | propagated latest-message coverage is extensionally equal to pairwise DAG ancestry, so supporter maps and exact clique decisions are unchanged; an identical-snapshot result may be reused only across a one-predecessor linear parent whose latest messages are older than it | Rocq `CertifiedFloorPromotion.{propagated_coverage_exact, coverage_decision_transparent, unchanged_linear_snapshot_reuse_sound}` and `MainTheorem.{finalized_floor_latest_message_coverage_correct, finalized_floor_linear_snapshot_reuse_correct}`; TLC safe coverage worklist (27 generated / 16 distinct, depth 8) plus unordered negative control; Apalache bound 8/bound 4; Rust generated support/weight/decision equivalence and reuse/error examples; 132-block regression at 22.92 seconds |
| **T-FINALIZER-MATERIALIZATION / S41 / L16** | complete all-parent discovery and deterministic greatest-eligible selection refine exact pairwise candidate evaluation; a deferred proposal floor cannot remain permanently invisible or be replaced by a merely causal rejected-state sibling | Rocq `FinalizerFloorMaterialization` and `MainTheorem.finalized_floor_materialization_target_alignment_correct`; TLC safe model (9,289 generated / 1,849 distinct, depth 15) and two exact controls; Apalache safe bound 8 and controls at bound 6; Rust full finalizer regression group (11 passed, 1 ignored), exhaustive selector oracle, generated coverage/selection property, and `loom_finalization_atomicity::frozen_target_cannot_mix_with_a_concurrent_latest_message_arrival` |
| **T-SNAPSHOT-MATERIALIZATION / S32** | floor derivation cannot inspect a parent, captured LFB, or off-parent latest message until its recursive canonical floor provenance is present; concurrent finalizer writes commute and cannot lose entries | Rocq `SnapshotFloorMaterialization` plus `MainTheorem.finalized_floor_snapshot_materialization_correct`; TLC safe closure/interleaving/liveness model (18 generated / 10 distinct, depth 5) plus parent-only counterexample; Apalache bound 8/bound 4; Rust `finalized_floor_materializes_off_parent_latest_message_provenance` and the merge-rebase regressions without cache pre-seeding |
| **T-HEARTBEAT-BACKPRESSURE / S34 / L13** | actual observed-LFB progress exposes one earliest-uncompleted rotating-leader recovery round at a time; proposal admission stays bounded; explicit ancestry/latest-message views require exact weighted mutual causal and state-preserving cliques; asynchronous safety assumes neither within-round delivery nor bounded task skew; under explicit eventual-synchrony delivery/scheduling bounds an offline first leader cannot halt progress | Rocq `HeartbeatFinalityBackpressure` plus `MainTheorem.finalized_floor_heartbeat_backpressure_correct`; TLC `HeartbeatFinalityBackpressure` eventual-synchrony model (22,468 generated / 4,194 distinct, depth 30), asymmetric `1/4/5` stake model (22,468 / 4,194, depth 30), existing-candidate model (22,960 / 4,338, depth 30), asynchronous safety model (113,968 / 17,766, depth 30), and eager, fixed-offline-leader, causal-only, and promotion-witness controls; Apalache safe bounds 5/4 plus non-vacuity controls; Rust selected-leader retry, nonleader completion, committee, delayed-wake, and rotation properties |
| **T-HEARTBEAT-CADENCE / S34 / L13** | round zero opens after the one-time `max(max_lfb_age, check_interval)` stall timeout; later rounds become available every `check_interval`; arbitrary delayed wakes consume every available round as a contiguous prefix | TLC `HeartbeatRecoveryCadence` safe model (1,123,849 generated / 287,496 distinct, depth 26) plus collapsed-timeout negative control; Apalache safe bound 10 and negative bound 1; Rust `recovery_round_cadence_matches_stall_timeout_then_check_interval` and `delayed_wakes_replay_missed_recovery_rounds_without_skipping_leaders` |
| **T-PENDING-RECOVERY-COMPOSITION / S35 / L14** | admissible pending work and selected recovery compose in one reserved proposal; stored-but-exhausted work cannot mask recovery; retryable outcomes do not close a round; terminal evidence precedes pool removal; exact occurrence disposition and bounded duplicate admission survive concurrent ingress; divergent non-finalized head committees cannot authorize multiple recovery validators for one floor round | Rocq `HeartbeatFinalityBackpressure.{floor_recovery_authorization_unique_across_head_views, authorized_pending_recovery_composes_one_proposal, nonstarting_results_release_without_completion, starting_results_complete_attached_round, proposal_scheduler_end_to_end}` through `MainTheorem.finalized_floor_heartbeat_backpressure_correct`; TLC `PendingDeployHeartbeatComposition` safe model (1,213,239 generated / 296,424 distinct, depth 32), ingress-safety model, and seven exact controls cover every finalized-height leader residue and the parent-committee defect; bounded Apalache checks use safe and invariant-control bounds of 6; Rust heartbeat pending/recovery outcome, floor-committee, and selected-leader tests |
| **T-PROPOSER-COALESCING / S36 / L15** | proposal intent is explicit; empty blocks require a fresh selected-recovery permit; one pending collision epoch yields exactly one forced non-empty follow-up; manual/recovery collisions do not become ambient work; an LFB change invalidates queued recovery authority while ordinary head growth does not | Rocq `HeartbeatFinalityBackpressure.proposal_scheduler_end_to_end` through the heartbeat capstone; TLC `ProposerAdmissionCoalescing` safe model (1,582 generated / 646 distinct, depth 12) plus ambient-empty, lost-wake, and stale-permit controls; bounded Apalache checks use safe and control bounds of 6; Rust example/property/Loom tests in `node::instances::proposer_coalescer` and proposal-intent tests in `casper::blocks::proposer` |
| **T-FPROGRESS / L6** | a complete finite frozen candidate scan selects the highest ready candidate, reports exhaustive absence only after full coverage, never converts interruption/error into absence, and schedules each reachable validator/block pair once | Rocq `FinalizerProgress.{scan_selected_sound, scan_exhausted_complete, complete_scan_selects_when_ready_candidate_exists, inconclusive_is_not_exhaustion, schedule_once_has_no_duplicates, schedule_once_preserves_exact_membership}` and `MainTheorem.finalizer_progress_correct`; TLA⁺ `FinalizerProgress` safe model plus cap/budget/timeout starvation controls; Rust `finalizer_examines_a_complete_frozen_candidate_set_beyond_the_old_prefix` and `finalizer_visits_each_validator_block_agreement_once_in_a_reconvergent_dag` |
| **ancestry precondition (GAP-2/GAP-4)** | `CliqueOracle.v`/`Selection.v` model DAG ancestry ABSTRACTLY (`anc_of`); the trusted realization `is_dag_ancestor` (`block_dag_key_value_storage.rs`, used by `floor.rs`) computes EXACTLY that relation. Its block-number prune is sound under strict per-edge monotonicity (`wf_dag`: `block_number = 1 + max parent`), which block validation enforces — **not** the global contiguity (`max−min==len`) that `block_metadata_store.rs` demoted to a `warn!` (GAP-4: a strictly stronger, separate diagnostic the prune never needed) | Rust property test `is_dag_ancestor_matches_reflexive_transitive_closure_over_parents` (`block-storage`, `--features test-internals`): on random well-formed DAGs, `is_dag_ancestor` (with the prune) ≡ the reflexive-transitive closure over parents |
| **capstone** | all of the above, axiom-free | Rocq `MainTheorem.{finalized_floor_merge_correct, finalized_floor_occurrence_correct, finalized_floor_recovery_admission_correct, finalized_floor_recovery_leadership_correct, finalized_floor_deploy_identity_separation_correct, finalized_floor_selection_correct, finalized_floor_arithmetic_correct, finalized_floor_phase7_correct, finalized_floor_ftexact_correct, finalized_floor_ftprovenance_correct, finalized_floor_thetaexact_advance_correct, finalizer_progress_correct, bootstrap_replay_and_local_fault_recovery_correct, terminal_funding_admission_lifecycle_correct, finalized_floor_effect_causal_closure_correct, finalized_floor_state_lineage_correct, finalized_floor_state_effect_provenance_correct, finalized_floor_rebased_parent_selection_correct, finalized_floor_state_support_refines_causal_certificate, finalized_floor_certified_promotion_correct, finalized_floor_materialization_target_alignment_correct, finalized_floor_heartbeat_backpressure_correct, finalized_floor_atomic_commit_correct, finalized_floor_worker_retry_correct, finalized_floor_bound_head_correct, finalized_floor_recovery_cursors_correct, finalized_floor_rooted_genesis_identity_correct, finalized_floor_witness_equivalent_carrier_correct, finalized_floor_collective_recovery_coverage_correct, finalized_floor_split_recovery_frontier_correct, finalized_floor_recovery_leadership_separation_correct, finalized_floor_recovery_parent_order_independent, finalized_floor_recovery_latest_order_independent}`; occurrence details are specified in [`deploy-occurrence-specification.md`](../deploy-occurrence/deploy-occurrence-specification.md) |

---

## 5. Formal artifacts

### 5.1 Rocq (`formal/rocq/finalized_floor/`) — axiom-free

Rocq/Coq 9.1.1, Stdlib-only. Every theorem is checked with `Print Assumptions`
⇒ *"Closed under the global context"* (no `Axiom`, `Parameter`, or `Admitted`),
**and independently re-verified by the trusted kernel via `coqchk`** (C3): the
`scripts/check-finalized-floor-ALL.sh` gate runs `coqchk -Q theories FinalizedFloor
FinalizedFloor.MainTheorem` ⇒ *"Modules were successfully checked"*, closing the
gap between the elaborator that *built* the `.vo` and the kernel that *checks* it —
the trust root under every capstone.

| Module | Depends on | Key results |
|---|---|---|
| `Foundation.v` | — | DAG, block numbers, main-parent spine, `walk_spine`, **T-TERM** |
| `CliqueOracle.v` | Foundation, FtExact | DAG ancestry, agreement, quorum `Finalized` with distinct validators, **L-ANC**, and **L-SNAP**; **C1 θ-exact bridge** through runtime-strict `FtExact.ft_exact_gt`, including θ=0; **C1′ negative-threshold hard gate** through `Finalized_ft_hg`; and **C5 advancement** through `snap_advances`, `L_SNAP_advance`, and `snap_extends_snap_advances` |
| `Floor.v` | CliqueOracle | **T-CACHE** (`warm_eq_cold`, `frontier_cache_transparent`) — takes `AdjDC` as a hypothesis |
| `GuardBridge.v` | Foundation, CliqueOracle, Floor | **guard ⇒ AdjDC** (`chain_adj_AdjDC`): under a *constant* committee, finalization along the spine is downward-closed, so the Rust committee-constancy guard *derives* Floor.v's `AdjDC` premise (no longer assumed); `guard_constant_committee_transparent` (warm == cold with AdjDC derived); **T-FIN** (`upgo_finalized`: the warm up-walk's result is `Finalized`). **Section `BridgeFt`** repeats the construction over θ-exact `Finalized_ft` via `L_ANC_ft`: `chain_adj_AdjDC_ft`, **`guard_constant_committee_transparent_ft`** (T-CACHE for every numerator), and `upgo_finalized_ft` |
| `Merge.v` | — | semilattice fold: **T-DETMERGE/T-CONV** (`merge_*_perm`), **T-K1** (`merge_or_no_lost_bit`) |
| `Recovery.v` | — | **T-NDA** (`apply_idem`, `no_double_apply`) |
| `DeployIdentitySeparation.v` | — | protocol-tagged deploy identity, cross-domain inequality for equal payload bytes, and symmetric rejection isolation |
| `MergeRecoveryCoherence.v` | OccurrenceDisposition | base receipt dominance, complete-chain tombstone and base-duplicate exclusion, committed-deploy uniqueness, state-record coherence, causal-effect identity, retry exclusion, and permutation-invariant numeric materialization |
| `RejectionReasonConfluence.v` | — | canonical rejection-reason join laws, direct-over-collateral precedence, and arbitrary observation-order invariance |
| `ProtocolActivationCoherence.v` | MergeRecoveryCoherence | active-version scope homogeneity, current/legacy record-encoding duality, and legacy-floor/current-protocol base dominance |
| `BootstrapReplayContext.v` | — | replay from the block's own context reconstructs its declared post-state root; list replay reconstructs every historical root; a concrete ambient-context counterexample shows why current-tip substitution is unsound |
| `LocalFaultDeferral.v` | — | local faults preserve consensus disposition, leave the ready queue, remain deferred after request failure, reopen only through recovery, and ordinary descendants require an accepted parent |
| `FundingAdmissionLifecycle.v` | — | underfunded proposals become immutable terminal zero-effect records, later supply cannot resurrect them, and a fundable deploy cannot be forged as rejected |
| `AdmissionEffectAlignment.v` | — | admission-rejection insertion transparency, ordinary execution-failure retention, permutation-invariant effect cardinality, and exact user/system metadata splitting |
| `EffectCausalClosure.v` | — | physical datum/continuation dependency, mergeable exclusion, least transitive rejection closure, no accepted dependent of rejected state, and concrete retained-base/independent-effect survival |
| `StateEffectProvenance.v` | — | exact active-effect merge algebra; input preservation, named rejection, floor restoration, parent permutation and covered-parent invariance, repeated three-way preservation, majority support, complete rejection-candidate scan equivalence, and the single-base counterexample |
| `CertifiedFloorPromotion.v` | — | universal all-parent candidate definition; preserved-parent causal coverage; dual-certified current-floor eligibility and discoverability; propagated latest-message coverage equivalence; decision transparency; sound unchanged-snapshot reuse across one linear predecessor; concrete proof that main-spine discovery misses a valid secondary candidate while causal discovery promotes it |
| `SnapshotFloorMaterialization.v` | — | complete parent/latest target materialization, preservation of cached entries, permutation transparency, idempotence, commutation with concurrent finalizer writes, and a concrete parent-only incompleteness witness |
| `CommitteeTransition.v` | Foundation, CliqueOracle | separation of replayed post-state bond serialization from finalized-floor authority; exact justifications and sender membership; accepted-only validator registration; registration-before-promotion; and delayed transition eligibility |
| `HeartbeatFinalityBackpressure.v` | — | finalized-height-offset leader membership and uniqueness; earliest-uncompleted cadence, minimality, completeness, and skipped-wake preservation; node-local delivered latest messages and captured views; selected-layer creator identity and state/causal descent; concrete A→B→A dual mutual-clique witness; serialized proposal reservation; nonstarting/starting outcome refinement; pending-plus-recovery composition; selected-leader and no-eager-support authority; ordered completion and floor-reset behavior; and the axiom-free `proposal_scheduler_end_to_end` and `heartbeat_backpressure_end_to_end` contracts |
| `FinalizationAtomicity.v` | — | one-winner compare-and-append, exact predecessor/state-lineage binding, stale revision/head inertness, fresh-evaluation necessity, validation/commit race closure, stale snapshot non-publication and finite-prefix retry, DAG-ancestry-insufficiency witness, failed-worker non-completion and retry, newer-success subsumption, immutable-record idempotence and commutation, monotonic publication, atomic rooted-genesis bootstrap, write-free exact genesis assertion, conflicting/partial genesis rejection, append-preserved root identity, crash-preserved records, ordered projection, contiguous effect completion, compaction bounds, and restart-preserved root and cursors |
| `ProposalFloorReadiness.v` | — | proposal creation requires materialized state and complete authority; only floor-materialization deferral requests finalization; incomplete slots, inactive authority, and stale permits remain non-retryable at the finalizer boundary; the combined readiness contract is axiom-free |
| `FinalizerFloorMaterialization.v` | CertifiedFloorPromotion | target-bound dual-certificate validation, target-substitution rejection, propagated-coverage decision equivalence, unique highest exact candidate, and the concrete state-certified secondary-parent witness missed by main-parent-only discovery |
| `DivergentFinalizationHistories.v` | — | same-target convergence with distinct local revisions and digests, and rejection of cross-node local-ledger identity as consensus authority |
| `MinorityForkRecovery.v` | — | write-free peer tips, dependency-first ordinary admission, local-finalizer-only monotonic publication, preserved validator identity, and independent per-validator progress |
| `StaleSiblingRecovery.v` | — | exact accepted-occurrence identity, floor-preserving complete-frontier settlement, source-bound rejection authorization, retry exclusion before buffering, elected single-owner recovery, and the end-to-end rehome theorem |
| `RecoveryFrontierCoverage.v` | — | one-parent coverage implies collective coverage; a split frontier proves the converse false; retry authorization remains independent from ordinary leadership |
| `Selection.v` | Floor, CliqueOracle | the Case-A/B sound-base pick: **T-SOUND**, **T-LIN**, **T-PS**, **T-FIN**, **T-COMM**, **H3**, **Case-B**, **maximality** (`select_sound`, `select_none_correct`, `case_a_common_ancestor`, `T_PS`, `select_finalized`, `committee_is_floor_bonds`, `scope_covers_band`, `case_b_compatible`, `select_highest_sound`) |
| `IntegerAdd.v` | — | signed-64 wrapping: **T-ALG(c)** (`wadd_assoc`), **T-ALG(d)** (`checked_apply_rejects_*`), launder `launder_exhibit`/`checked_combine_sound`/`supply_cap_no_launder` |
| `FtExact.v` | — | **A9 exact-integer FT** (`ft_exact_iff_ratio`/`_strict`, `ft_exact_mono_q`, `ft_exact_no_overflow`): the exact test `2q·den ≥ S(den+num)` IS the f32 ratio test cleared of denominators, monotone in `q`, overflow-free in i128 |
| `FinalizerProgress.v` | — | finite scan result distinguishes `Selected`, `Exhausted`, and `Inconclusive`; selected candidates are ready, exhaustive absence covers every candidate, complete scanning reaches any ready candidate, and enqueue-time deduplication preserves exact membership while prohibiting duplicate scheduled work; a fixed prefix admits a starvation witness |
| `StateLineageFinality.v` | — | abstract certification/admissibility separation; concrete stale-merge counterexample and safe off-main-spine rebase; proof that main ancestry is irrelevant to certified state-preserving admission; promotion preserves every committed state under any reflexive/transitive preservation relation |
| `MainTheorem.v` | all | capstones including exact-source occurrence disposition, recovery admission and leadership, protocol-tagged deploy identity separation, merge/recovery activation, terminal funding admission, admission/effect alignment, the C1/C5/C1′ bundle, complete finalizer progress, abstract admission safety, concrete state-effect provenance, certificate refinement, universal certified-floor promotion, latest-message coverage equivalence, linear-snapshot reuse, exact finalizer materialization alignment, exact target-deploy observation, snapshot floor-materialization closure, committee-transition safety, heartbeat recovery/backpressure refinement, atomic finalization publication, typed proposal readiness, crash-recovery cursor safety, local-witness identity separation, live minority-fork recovery, and exact stale-sibling settlement/rehome recovery |

The finalization model is a faithful monotone abstraction of `ft_witnessed`:
`Finalized c J b` := *some majority-weight sub-committee all agree on `b`* (a
clique is such a quorum). L-ANC/L-SNAP hold by the **same-quorum argument** — the
identical validators that finalize `b` finalize every ancestor of `b`, and still
do under a larger snapshot — which is exactly why they hold for the real oracle
(the pairwise-clique refinement reuses the same witnessing set verbatim).

**C1 — the strict-majority `Finalized` is no longer a proxy.** The node's runtime
finalization decision is the strict θ-exact test `FtExact.ft_exact_gt` (θ = num/den =
ppm/1e6), not the hard-coded strict-majority (θ=0) corner. `Finalized_ft` is
`Finalized` with its quorum weight-condition replaced by that exact test; because
L-ANC/L-SNAP are **quorum-opaque** (they carry the *same* `Q` through, never
inspecting its weight bound) they re-prove verbatim as `L_ANC_ft`/`L_SNAP_ft`. The
bridge `Finalized_ft_refines_Finalized` then shows every θ-finalized block at
θ≥0 is also strict-majority `Finalized`, so it
inherits T-CACHE and every downstream capstone with no re-proof — T-CACHE's no-fork
guarantee now rests on the decision the node actually runs. Strictness covers
θ=0 and makes a zero-stake certificate impossible; the proof needs only
`0≤num` and `0<den`.

**C1′ — negative thresholds are covered by the θ-independent hard gate.** For a
negative sentinel the θ-test alone can be weaker than strict majority. The node
does not finalize on that test alone: `ft_decides_exact`
(clique_oracle.rs:79-81) first applies a θ-independent **hard majority gate**
`if 2·agreeing ≤ S return false`, where `agreeing` is the TOTAL agreeing weight and
the θ-tested clique weight `q = max_clique_weight` is a sub-part (`q ≤ agreeing`;
the call sites pass them separately). `CliqueOracle.v` §9 models this as `hard_gate`
(a strict-majority *agreeing* set, provably `Finalized` — `hard_gate_iff_Finalized`)
and the node's real decision as `Finalized_ft_hg := Finalized_ft ∧ hard_gate`; the
capstone-checked **`Finalized_ft_hg_refines_Finalized`** shows the hard gate ALONE
yields strict-majority `Finalized` for **ALL num** — no `0<num`, no positive-stake
side-condition — so negative sentinels inherit every downstream result. Independently, T-CACHE
holds directly over `Finalized_ft` for all num via `GuardBridge.BridgeFt`
(`guard_constant_committee_transparent_ft`, built on `L_ANC_ft` which needs no sign
of `num`), so **cache transparency was never gated on `0<num`** either. The Z3
`ft_exact_no_overflow.py` exhibits the seam and its closure: the θ-test alone at
negative θ can finalize with `2q ≤ S` (a `sat` GAP), while the real decision (θ-test ∧
`2·agreeing>S`) always carries a strict-majority agreeing set (`unsat` refutation of
any counterexample over the full `−den ≤ num ≤ den` range). **C5 — snapshot
growth is modeled as latest-message ADVANCEMENT** (`snap_advances`: each binding
moves forward to a DAG-descendant), strictly more faithful than the preservation-
only `snap_extends`; `L_SNAP_advance` re-proves L-SNAP for it (via
`agrees_snap_advance_mono`/`anc_of_trans`), and `snap_extends_snap_advances`
(preservation ⇒ advancement, `anc_refl`) makes the original `L_SNAP` its
reflexive-descendant corollary — nothing existing is weakened.

**State-preservation and state-support proof.** `StateLineageFinality.v` keeps the
causal `certified` predicate and the additional `state_certified` predicate
abstract so LFB admission cannot conflate them.
`causally_certified_state_unsupported_candidate_is_ineligible` proves that causal
certification plus current-LFB preservation is insufficient without state support.
`certified_stale_candidate_is_ineligible` proves that a candidate lacking
current-LFB preservation is not admissible even when certified.
`certified_off_main_rebase_is_eligible` proves that main-parent ancestry is
irrelevant once both certificates and state preservation hold.
`eligible_promotion_preserves_lineage` proves that any admissible promotion
preserves every earlier committed state under a reflexive, transitive relation.
The concrete `Funding`/`Stale`/`Rebased` scenario proves that the stale
candidate remains certified, unsafe promotion loses the committed funding state,
a causally certified rejected parent is ineligible without state support, and an
off-main-spine rebase promotion preserves it. `CliqueOracle.v` separately defines
`state_agrees` and `StateFinalized_ft_hg`, then proves that state-preserving
certification refines causal certification whenever the preservation relation refines DAG
ancestry. `MainTheorem.v` exports that bridge as
`finalized_floor_state_support_refines_causal_certificate`.
`StateEffectProvenance.v` supplies the previously missing implementation
refinement: active state is a predicate set over exact effect identities, merge is
union of every state input minus direct rejections, and preservation is subset
inclusion. It proves reflexivity, transitivity, parent-order invariance, covered
parent elimination, finalized-floor restoration, repeated accepted three-way
preservation, majority certification, and equivalence of the optimized complete
rejection-candidate scan to full subset checking. Its concrete negative control
proves that the old single-base collapse loses the source effect.
`finalized_floor_state_lineage_correct` bundles the abstract admission results;
`finalized_floor_state_effect_provenance_correct` bundles the concrete recurrence
and scan refinement. Both are checked axiom-free.

Build (memory-capped, per the 32 GB envelope):

```bash
cd formal/rocq/finalized_floor && coq_makefile -f _CoqProject -o Makefile
systemd-run --user --scope -p MemoryMax=16G -p CPUQuota=1800% -p TasksMax=200 \
  make -C formal/rocq/finalized_floor -j1
```

### 5.2 TLA⁺ (`formal/tlaplus/finalized_floor/`)

`FinalizedFloor.tla` carries **both** models:

- **Pre-fix** `Spec` + `MC_FinalizedFloor_pre_fix.cfg`: unguarded propose with the
  silent single-parent fallback. TLC **discovers the counterexample** —
  `Inv_NoLostParentWrite` violated (`parentKeys = {1,2}`, `mergeKeys = {1}`) once
  Δ crosses the cap. This is the formal reproduction of the write-loss.
- **Post-fix** `SpecFixed` + `MC_FinalizedFloor.cfg`: the backstop as a park-guard
  (no lossy merge; scope-gate demoted). TLC **passes**: `Inv_NoLostParentWrite`,
  `Inv_DeltaWithinCap`, `Inv_FloorMonotone`, and the temporal `Liveness_Progress`
  all hold.

`FinalizedFloorScan.tla` models the **H3 scan + T-PS** that `FinalizedFloor.tla`
abstracts away: per-block writes collected by the floor-bounded scan over an
**unconstrained** parent set (any nonempty subset). `MC_FinalizedFloorScan.cfg`
(`BadCut = 0`, the fix) **passes** — `Inv_NoParentWriteDropped` + `Inv_BandCovered`
hold for every parent set (H3 is now model-checked, not assumed; T-PS holds);
`MC_FinalizedFloorScan_bug.cfg` (`BadCut = 1`, the old cut-above-floor bound) is
**violated** (the H3 counterexample).

`FinalizerProgress.tla` models one frozen, deterministically ordered candidate
set and repeated runner invocations. `MC_FinalizerProgress.cfg` passes selection
soundness, highest-ready selection, full-coverage exhaustion, and eventual
selection. The cap, budget-restart, and timeout-restart configurations each
violate eventual selection and therefore serve as executable negative controls
for H4.

`StateLineageFinality.tla` models two independent nodes, arbitrary delivery order
for a certified stale merge and its rebased successor, the asymmetric validator
stakes $`60/20/15`$, exact strict hard-majority plus `FTT=0.1` arithmetic, and
separate causal certification, state-preserving certification, and LFB-admission
predicates. Candidate `P` receives causal support from a merge validator and its
source proposer but state support only from the proposer. It therefore passes the
original exact certificate and current-LFB ancestry while failing the additional
state certificate. The rebase state-descends and DAG-descends from the LFB but
deliberately does not main-descend from it. TLC exhausts all 144 reachable safe
states and proves that both nodes eventually converge on the rebase while every
committed state remains in each local LFB's lineage.

One unsafe configuration disables only current-LFB state preservation and produces
“deliver stale, promote stale, lose committed funding.” A second disables only
state-support certification and produces “deliver rejected parent, promote
rejected parent.” A third enables the obsolete main-spine conjunct and immediately
violates off-main rebase eligibility. Apalache independently proves the safe
invariants through bound 8 and finds all three counterexamples. Every
configuration asserts that the causal certified set is unchanged, so the
refinement does not redefine the original majority vote.

`StateEffectProvenance.tla` closes the refinement gap left by that abstract
admission model. It derives one representative effect through accepted
three-parent merges, all modeled parent rotations, a repeated merge round, a
direct rejection, and a finalized-floor restoration. Two nodes receive three
validator tips in arbitrary orders and may promote only after the exact
state-preserving majority certificate is present. TLC exhausts 649 generated / 144
distinct states to depth 9 and proves exact recurrence, parent-order invariance,
preservation-summary equivalence, direct-rejection precision, floor restoration,
state-support refinement, exact majority arithmetic, node agreement, and eventual
two-node promotion. `UseSingleBase = TRUE` changes only the merge input rule and
violates `Inv_DeliveredQuorumCertifiesSource`, reproducing H11.

`StateEffectProvenanceApalache.tla` expresses the same concurrency boundary in a
solver-oriented transition system: each node receives effect-bearing parents
`A`, `B`, and `C` in an arbitrary order, settles only after the complete input set,
and may promote only from the settled effect set. Apalache checks exact settlement,
accepted-source preservation, promotion safety, and cross-node convergence through
bound 8. The single-base configuration produces the accepted-source-loss trace at
bound 4. The separate module avoids asking the SMT solver to inline the larger
TLC model's nested constant-function recurrence; it does not weaken the checked
transition or its negative control.

`StatePreservingForkChoice.tla` models the transition the earlier provenance
models omitted. Certificate delivery and latest-message delivery are independent,
so either node may advance from `G` to finalized state `F` while its frozen valid
tips include a preserving parent, an effect-dropping parent, or a stale parent.
The safe transition first records the base-admissible causal-parent projection,
then derives the floor-descending vote projection as its subset. A stale accepted
tip remains a causal input but cannot vote; an intrinsically invalid stale tip is
in neither set. The captured floor is inserted whenever no causal candidate
descends from it, including the all-stale nonempty case. Reachability compaction
retains the complete maximal antichain, the GHOST head remains index zero, and
recovery can narrow only when that head covers every live causal tip and descends
from the floor. Replay computes proposal state from the protected floor plus all
live causal inputs; deterministic merge may reject conflicting above-floor deltas
without deleting the floor contribution. Exact latest messages and the floor remain
evidence roots independently of parent compaction and deterministic depth expiry.

The exhaustive node-local TLC configuration proves non-empty parents, exact backstop,
vote-subset-causal, stale-tip retention without voting, intrinsic-invalidity
exclusion, maximal-antichain coverage, GHOST-head preservation, evidence-root
retention, floor-aware recovery narrowing, exact floor rebasing, funding-effect
retention, promotion, and eventual proposal from `F`. It generates 17,169 states,
finds 808 distinct states, and closes the complete graph at depth 10. A separate safe
zero-depth configuration proves that deterministic causal expiry restores liveness.
Nine controls independently reproduce finalized-effect loss without floor rebasing,
accepted stale-sibling loss from reusing the vote projection, multiply-invalid tip
admission, deploy-based GHOST-head replacement, omitted floor evidence, incomplete
antichain compaction, floor-blind recovery narrowing, finite-cap starvation, and
depth-without-expiry starvation. Apalache checks representation closure through
bound 3 and partitions the projection, evidence/state, and depth-expiry invariants
across four-transition proposal phases on one arbitrary representative node, then
symbolically checks each safety control on that same
projection. TLC exhausts `TypeOK` together with every semantic invariant and remains
authoritative for certificate-delivery interleavings and the temporal capacity and
expiry controls. The symbolic phase begins at an already certified `F` and reuses the
same latest-message, recovery, and proposal actions as the complete model; it does not
duplicate or weaken fork-choice computation. Every mutable field is indexed by
node, every transition changes exactly one node, every invariant is universally
node-local, and the shared `Active` and `Tip` functions are immutable. The axiom-free
Rocq `NodeLocalProductLifting` development proves that locally preserved invariants
lift through every finite arbitrary-node schedule, distinct-node updates commute,
adjacent independent schedule actions may be reordered, and another node's action
cannot change local enablement or erase an attained local goal. The interacting
three-validator receive/replay/support/promotion races remain exhaustively checked
by `ParallelValidatorConsensus`. This is a compositional product proof, not a
serial execution assumption or a protocol reduction. `Propose` is enabled only when its
stored snapshot would change; `[Next]_vars` retains semantic stuttering, and TLC
confirms that this removes no reachable state. Receiver validation remains
block-structural and does not depend on a receiver's possibly lagging LFB.

An earlier draft cited 1,860,017 generated and 163,216 distinct states for the
expanded two-node product. Those figures described the preceding transition system,
not the subsequently expanded one. A current-source product run remained incomplete
after 15.3 million generated transitions, so those obsolete figures are not release
evidence. The terminating local exhaustion plus the kernel-checked arbitrary-node
product theorem is the canonical evidence.

`ParentFrontierCapacity.tla` closes the capacity-refinement boundary that the
larger state-preservation model intentionally treated as a fixed bound. Two nodes
evaluate one immutable exact frontier in either order. With
`ConfiguredActiveMaximum = 10000`, `ParentCap = 101`, and four exact parents, both
nodes admit and record the complete frontier: the future validator ceiling does
not manufacture parents. With three exact parents and cap two, both nodes defer
and record no parent list. The unsafe static-maximum gate violates
`Inv_ExactFitIsAdmitted` after the first evaluation under both TLC and Apalache.
The corresponding Rocq refinement proves admission returns the original list
exactly iff its length fits, returns `None` iff it is over cap, and exhibits an
actual singleton frontier that fits even when the worst-case configured bound is
underprovisioned. The Rust bridge checks cardinality after depth expiry and
reachability compaction, emits a typed deferral before block creation or signing,
and retains the full evidence and pending deploy state for a later attempt.

The concrete defect was a refinement error, not a failure of the stated causal
model: `snapshot.rs` reused `FinalityVoteProjection::eligible_latest_messages()`
to build parents. That map intentionally excludes an accepted latest message once
a sibling floor finalizes, so the implementation silently narrowed causal merge
input to current-floor votes. The earlier model already retained every valid tip,
but no refinement obligation named separate Rust types for the two projections;
the incorrect reuse therefore satisfied the vote model and escaped type review.
The repaired implementation exposes `CausalParentProjection` and
`FinalityVoteProjection` separately, derives both from one frozen base predicate,
and binds both to the certified-context digest. The new unsafe control and Rust
reference property specifically close that refinement gap.

`ParallelValidatorConsensus.tla` removes the remaining global-phase
abstraction. Three validators independently receive either of two candidates,
capture their own floor block/root/state tuple, replay, validate, emit support,
receive each signer's support in arbitrary order, and promote. Crash and restart
are separate transitions between capture and validation. Checkpointing records
candidate roots in a shared repository and changes a mutable current-root
pointer, but capture and promotion use explicit node-local roots. The exact
weighted certificate uses stakes $`40/35/25`$, strict majority, and `FTT=0.1`.
Consequently, no validator can produce a certificate alone; every promotion trace
contains independently delivered support from at least two validators.

The bounded baseline safe configuration exhausts 12,877 generated / 3,411
distinct states to depth 9 and proves exact local replay before support, delivered-support
provenance, local-root availability, exact certification, atomic block/root/state
publication, preservation of every committed effect, compatible honest floors,
canonical deploy-origin agreement, and exclusion of the shared pointer from
authority. A second safe configuration enables crash after capture or replay
and restart from a fresh capture. A third safe configuration starts after
candidate 1 was locally accepted but before a concurrent candidate 2 became the
current floor. It exhausts 150 generated / 58 distinct states to depth 3 and
proves that the stale candidate cannot erase candidate 2's committed effect.
Eight one-defect configurations separately reproduce causal-only
acceptance, early support, promotion without local replay, shared-pointer capture,
shared-pointer publication, non-atomic publication, and stale effect-dropping
promotion, plus deletion of a replayed root during failure. Apalache checks the
baseline and crash-safe transition systems through bound 6 in the routine gate,
checks the paired stale-window safe system through bound 2, and reproduces the
stale committed-effect loss in one step after removing only the preservation
guard; the independently
completed bound-8 baseline run is the deep symbolic gate. Its independent
crash-root-deletion control reaches a
`ReplayRootsRemainLocallyRecorded` violation at state 4 through bound 5. The
routine gate allows 600 seconds for the crash-safe bound-6 run because the
measured reference run completed in 323 seconds; the symbolic depth and checked
properties are unchanged.

`ParallelValidatorConsensus.v` supplies the unbounded frame argument omitted by
finite-state search. It quantifies over arbitrary node, block, root, and effect
types. A sound local validation yields state-preserving eligibility. Replay
records its candidate root without changing the published finalized tuple;
promotion requires that exact locally recorded root and retains every prior
root; restart preserves the durable tuple and complete root set. An eligible
point update preserves every validator's consistency, updates to distinct
validators commute pointwise, and validators promoting the same candidate
publish identical root and effect values. The capstone
`finalized_floor_parallel_validator_consensus_correct` and the composed
`finalized_floor_parallel_accountable_promotion_correct` are included in the
authoritative assumption and kernel checks. The latter instantiates the parallel
promotion certificate predicate with the exact accountable clique certificate,
so parallel promotion and accountable safety cannot drift into separate rules.
The proof does not assume that
validators execute serially and introduces no global validator phase.

`NodeLocalProductLifting.v` supplies the complementary product argument for the
detailed fork-choice state machine. It quantifies over arbitrary node, local-state,
and action types. Assuming only that one local step preserves its local invariant,
it proves global preservation for every finite interleaved schedule. It additionally
proves pointwise commutation and schedule equivalence for distinct-node actions,
plus enablement and reached-goal framing. The capstone
`finalized_floor_node_local_product_lifting_correct` is included in both the
assumption audit and independent kernel check.

`CertifiedFloorPromotion.tla` models the discovery relation that connects those
certificates to each proposed block's replay floor. The certified state `F` is a
secondary ancestor of all three validator tips and is absent from all three main
spines. Two nodes receive the tips in every asynchronous order and may re-derive
their floor at any point. TLC exhausts 1,051 generated / 225 distinct states to
depth 9 and proves certificate preservation, promotion safety, complete-evidence
promotion, and eventual two-node promotion. Setting `UseCausalClosure = FALSE`
changes only discovery to the obsolete main-spine relation and violates
`Inv_CompleteEvidencePromotesCertifiedFloor` as soon as one node has all tips.
Apalache independently checks the safe invariants through bound 8 and finds the
unsafe violation at step 3.

`LatestMessageCoverage.tla` refines the optimized certificate-support query
rather than the certificate rule. Validator identities begin at their frozen
latest messages and move to every causal parent in strict descending-height
order. TLC exhausts 27 generated / 16 distinct states to depth 8 and proves
partial soundness, exact coverage for processed blocks, exact terminal coverage,
absence of late propagation, and completion. Disabling only the descending
scheduler violates `Inv_NoLateCoverage`: a shared ancestor can be processed
before all of its descendant support arrives. Apalache checks the safe worklist
through bound 8 and finds the same unsafe behavior through bound 4. The refinement
map identifies `v` in `coverage[C]` exactly with pairwise DAG reachability from
`J(v)` to `C`; it does not approximate supporter weight or the clique decision.

`PendingDeployHeartbeatComposition.tla` composes two sources that the earlier
heartbeat model kept separate: deploy-pool work and the selected recovery round.
The safe liveness configuration starts with different pending deploys at the two
online validators, permits transient nonstarting proposal outcomes, and explores
every proposal, occurrence-classification, recovery-support, floor-observation,
and terminal-evidence ordering. The initial state quantifies all three residues
of the finalized height modulo committee size, rather than silently fixing the
offline validator to one special rotation. TLC exhausts 1,213,239 generated /
296,424 distinct states to depth 32. The ingress-safety configuration additionally
permits one concurrent submission per validator/deploy pair. It omits liveness so
the ingress product does
not obscure the bounded-attempt, bounded-occurrence, deterministic-disposition,
pool-removal, recovery-reservation, and terminal-evidence safety obligations.

Its seven controls are exact and noninterchangeable. `AttemptClosesRound` violates
`Inv_RetryableOutcomeDoesNotCompleteRound`; `ClearOnStart` violates
`Inv_PoolRemovalRequiresTerminalEvidence`; disabling recovery reservation violates
`Inv_RecoveryReservationHonored`; selecting leaders from divergent head committees
violates `Inv_AtMostOneSelectedRecoveryPerRound`; disabling duplicate admission bounds violates
`Inv_DuplicateOccurrencesBounded`; allowing pending work to mask recovery violates
floor-certification liveness; and fixing leadership on the offline validator
violates the same temporal property. The aggregate gate also registers the safe
Apalache projection through bound 6 and the four invariant controls represented
by symbolic configurations through bound 6. The bounded symbolic checks
complement the exhaustive finite-state exploration rather than replacing it.

`ProposerAdmissionCoalescing.tla` refines the queue counter into the actual atomic
admission states and request capabilities. The model interleaves manual,
pending-deploy, and finality-recovery ingress with proposal completion, ordinary
non-finalized head advancement, and distinct LFB advancement. Permit freshness
requires the captured LFB identity and height to match a fresh snapshot. The
captured recovery round is reused to recompute the selected leader over the fresh
committee; it is not compared with a global current-round oracle. TLC exhausts
1,582 generated / 646 distinct states to depth 12 and
proves a pending wake is latched, one dirty epoch creates exactly one forced
non-empty follow-up, empty authority belongs only to fresh selected recovery,
recovery collisions retain a retry obligation, and finite ingress eventually
quiesces. Its three controls respectively violate
`Inv_EmptyAuthorityIsRecoveryOnly`, `Inv_PendingWakeLatched`, and
`Inv_StaleRecoveryPermitRejected`. The aggregate gate registers the safe Apalache
projection through bound 6 and all three controls through bound 6; those symbolic
results remain explicitly distinct from the completed TLC evidence. The retry
latch is the external owning heartbeat task's obligation after a busy result,
not coalescer-owned runtime state. Cancellation and engine-unavailability reset
are covered by the Rust Loom tests; persistent deploy rediscovery is covered by
the pending/recovery composition model.

The deploy-recovery model family in `formal/tlaplus/deploy_recovery/` closes the
floor-to-scope boundary. `MergeRecoveryCoherence.tla` checks finalized receipt
precedence, causal tombstone authority, tombstone and base-duplicate chain
atomicity, ordinary/mergeable state-record coherence, exact effect identity,
single-datum numeric materialization, and retry exclusion. Nine unsafe controls
each disable one obligation and must reproduce its named invariant violation.

`ProtocolActivationCoherence.tla` fixes the floor version at legacy protocol 1,
the exact rejected-deploy threshold at protocol 2, and the active shard at
protocol 3. Its safe configuration proves that the
legacy floor composes with current semantics as a defensive reducer property,
not as a supported in-place upgrade. Three unsafe controls demonstrate
that selecting receipts from the floor version duplicates an effect, admitting a
legacy above-floor block breaks version homogeneity, and accepting a legacy
encoding in a current block violates record-format integrity.

`ProtocolVersionLifecycle.tla` begins earlier, at ceremony or approved-block
recovery, and ends after peer reception. The current ceremony configuration
exhausts the protocol-3 path. The historical and unknown recovery configurations
prove fail-closed startup. Five unsafe configurations independently reproduce a
stale ceremony candidate, non-adopting nodes, proposer bypass, the exact
configured-v2/approved-v1 receiver disagreement, and unsupported approved-block
admission. The matching axiom-free Rocq development proves the same lifecycle
composition for arbitrary node lists.

`ApprovedStateReplay.tla` covers the later joiner's historical reconstruction.
The safe model exhausts the chain while deriving every replay input from the
block being reconstructed and proves root equality plus eventual transition to
running. Its unsafe configuration substitutes the approved tip's context for
older blocks and reproduces the exact failure pattern: a valid historical block
gets a different root and is added to the local invalid set.

`LocalValidationRecovery.tla` covers classification and recovery after replay
or storage cannot read an exact artifact. The finite model schedules a
genesis-rooted validator and a restored validator independently. Parent and
sibling validation both require the same block hash, while child replay
requires a distinct state root. The safe model exhausts 9,025 states and proves
that certification preserves artifact identity, same-artifact requests
deduplicate, distinct validators remain independent, one finite transport
failure cannot lose custody, and a recovered artifact releases only its own
waiters. The genesis-rooted validator classifies absence as a typed local fault;
the restored validator classifies it as a typed missing dependency. Neither
classification creates objective invalidity or releases the child before exact
parent validation. Weak fairness proves eventual validation of all three blocks
by both validators. Four controls reproduce the previously unsafe choices:
retaining an inconclusive block as ready causes immediate self-requeue,
collapsing a state root into a block dependency violates exact-artifact
recovery, dropping the block violates custody, and treating local absence as
objective invalidity creates false consensus evidence.

`LocalFaultDeferral.v` provides the unbounded refinement. It proves that block
hashes and state roots survive certified classification, block and state
deferrals cannot collapse, mismatched artifacts cannot release a waiter,
duplicate requests are pointwise idempotent, and independent requests commute.
`typed_local_validation_recovery_correct` is axiom-free and is included in the
bootstrap replay and recovery capstone.

`FundingAdmissionLifecycle.tla` covers the state-bound admission decision from
proposal through finalization. The safe model records the exact supply view and
requires validation to classify from that immutable pre-state, so an
underfunded attempt becomes a terminal zero-effect rejection and later supply
cannot resurrect it. One unsafe control revalidates from live supply and
reproduces proposer/validator disagreement after a top-up. The other omits the
rejection record and reproduces the indefinitely pending client status.

`AdmissionEffectAlignment.tla` closes the next refinement boundary: the block
body is a lifecycle-record sequence, while locally reconstructed merge metadata
is an execution-effect sequence. Three validators independently index a parent
containing one terminal funding rejection and one executed `closeBlock`, then
propose successors. The safe projection excludes only the admission rejection;
TLC exhausts every interleaving and proves that every validator proposes and a
later deploy finalizes. Apalache independently checks the state invariants
through the complete lifecycle bound. The unsafe configuration counts raw
status records, expects a nonexistent second metadata map, blocks the first
validator during parent indexing, and violates
`Inv_StatusOnlyRecordCannotBlock`. Rocq proves the unbounded list/cardinality
refinement, including the crucial distinction that an ordinary runtime failure
remains effect-bearing.

`EffectCausalClosure.tla` closes the exact-effect refinement boundary that the
complete-chain model left abstract. Its safe configuration nondeterministically
orders all dependency-ready classifications and proves disjoint disposition,
absence of accepted dependencies on rejected state, complete transitive
rejection, independent-effect survival, and eventual classification. The
block-lineage unsafe configuration reproduces independent merge/user effect
loss. The direct-only unsafe configuration reproduces an accepted transitive
dependent. TLC exhausts the finite state graph; Apalache independently checks
the safe invariants and finds both negative-control counterexamples.

`RejectionReasonConfluence.tla` isolates the metadata-convergence seam omitted
from the original occurrence model. Its safe configuration explores every
interleaving by which two validators can observe the three current rejection
causes and proves that equal observation sets imply equal canonical reasons. The
last-writer configuration must violate that invariant. This distinction is
important: the original model represented tombstones as an unlabeled set, so it
proved state suppression while leaving serialized reason refinement outside the
refinement map.

Run under the bounded envelope:

```bash
source scripts/lib/tlc-run.sh
FF=formal/tlaplus/finalized_floor
tlc_run "$(tlc_metadir ff_post)" "$FF/MC_FinalizedFloor.cfg"         "$FF/FinalizedFloor.tla"   # PASS
tlc_run "$(tlc_metadir ff_pre)"  "$FF/MC_FinalizedFloor_pre_fix.cfg" "$FF/FinalizedFloor.tla"   # counterexample (exit 12)
tlc_run "$(tlc_metadir ff_progress)" "$FF/MC_FinalizerProgress.cfg" "$FF/FinalizerProgress.tla" # PASS
tlc_run "$(tlc_metadir ff_lineage)" "$FF/MC_StateLineageFinality.cfg" "$FF/StateLineageFinality.tla" # PASS
tlc_run "$(tlc_metadir ff_provenance)" "$FF/MC_StateEffectProvenance.cfg" "$FF/StateEffectProvenance.tla" # PASS
tlc_run "$(tlc_metadir ff_promotion)" "$FF/MC_CertifiedFloorPromotion.cfg" "$FF/CertifiedFloorPromotion.tla" # PASS
tlc_run "$(tlc_metadir ff_latest_coverage)" "$FF/MC_LatestMessageCoverage.cfg" "$FF/LatestMessageCoverage.tla" # PASS
tlc_run "$(tlc_metadir ff_latest_coverage_unsafe)" "$FF/MC_LatestMessageCoverageUnsafe.cfg" "$FF/LatestMessageCoverage.tla" # counterexample (exit 12)
tlc_run "$(tlc_metadir ff_pending_recovery)" "$FF/MC_PendingDeployHeartbeatComposition.cfg" "$FF/PendingDeployHeartbeatComposition.tla" # PASS
tlc_run "$(tlc_metadir ff_pending_ingress)" "$FF/MC_PendingDeployHeartbeatComposition_ingress_safety.cfg" "$FF/PendingDeployHeartbeatComposition.tla" # PASS
tlc_run "$(tlc_metadir ff_proposer_gate)" "$FF/MC_ProposerAdmissionCoalescing.cfg" "$FF/ProposerAdmissionCoalescing.tla" # PASS
DR=formal/tlaplus/deploy_recovery
tlc_run "$(tlc_metadir ff_effects)" "$DR/MC_EffectCausalClosure.cfg" "$DR/EffectCausalClosure.tla" # PASS
```

Every `tlc_run` log records `TLC_SOURCE_HASH`, calculated from the selected
configuration and every TLA⁺ module in the model directory, plus
`TLC_RECOVERY_IDENTITY`, which additionally commits to the TLC binary,
fingerprint polynomial, fingerprint seed, and worker count. Checkpoint recovery
is fail-closed: a caller using `-recover` must set `TLC_RECOVER_IDENTITY` to the
exact value recorded by the run that created the checkpoint. A checkpoint must
never be resumed after any committed input changes; TLC does not natively bind
its serialized fingerprint table and frontier to those inputs. In particular,
recovering with another polynomial or seed makes the existing fingerprint set
incomparable and can create a duplicate, invalid state graph.
`scripts/check-tlc-source-binding.sh` regresses matching, mismatched, missing,
and changed-fingerprint recovery identities. The runner recomputes the complete
identity after TLC exits and rejects the result if any bound input or checker
identity changed during exploration, preventing a report from naming inputs
that differ from those actually checked.

### 5.3 Wolfram (`formal/wolfram/finalized_floor`)

`delta_ratchet.wl` models the Δ difference equation and proves over the reals
that lag-dependent quadratic work creates positive feedback, while constant
overhead has zero lag feedback. It separately proves the three service regimes:
service above arrivals drains lag, equality preserves it, and service below
arrivals grows it. Its exact examples reproduce both runaway and an overloaded
constant-overhead system, preventing the earlier overclaim that O(1) alone
establishes stability.

`weighted_quorum_regions.wl` reduces the exact strict-quorum region, the
independent agreeing-majority gate, and accountable certificate overlap. It
then checks 227,264 small exact-rational cases and 232,064 production-PPM cases,
including strict ties, negative thresholds, asymmetric stake, and controls that
remove strictness, the hard gate, or the accountable-overlap premise.

`repair_design_regions.wl` compares parent-admission repair families with
correctness as a hard constraint. Exhaustive bounded enumeration leaves exact-
frontier deferral as the only policy that preserves the complete frontier,
respects the configured bound, admits every actual fit, and publishes nothing
over-cap; static worst-case rejection, truncation, and unbounded publication
each fail one of those obligations in 5,984 cases. It also derives the exact
guarded-cache crossover and a robust sufficient condition with symbolic machine
constants. The asymptotic compute saving is
$`(c_{oracle} V/2)\Delta^2`$; profiling therefore needs to calibrate only oracle
cost, validator count, actual advancement, cheap-read cost, fixed cache cost,
and the compute/storage token-price ratio.

This licensed exploration tier is explicitly opt-in. Run
`RUN_WOLFRAM=1 scripts/check-finalized-floor-ALL.sh`; the default gate neither
discovers nor starts a kernel and therefore acquires no license. Rocq and
TLA⁺/Apalache remain the correctness authorities.

### 5.4 Empirical soak (`casper/tests/batch2/map_cell_convergence_spec.rs`)

`finalized_floor_400_block_soak` (`#[ignore]`) runs
`run_convergence(3, 100, 20, false, Some(8))`
422 blocks — an order of magnitude past the green-gate and well past the old
256/512 cliff. Every merge exercises the warm up-walk; a backstop `Err` would
surface as a panic. Across the full run the fix-relevant invariants held with
**zero** violations: no Δ-backstop fired, no fork (cross-node LFB + complete
finalized-map identity every round), no finalized write lost (FS-monotonicity),
single-datum cell (keep-one collapsed), and no finalized key/value corruption.
The oracle reads each node's immutable `@"m"` datum once at the selected state
root and decodes the complete map. It does not execute exploratory Rholang while
measuring consensus, so the runtime-query count is linear in rounds; each query
scans the state map exactly once and cannot hide unexpected entries. Run:

The long-chain workload cycles eight keys per validator. This bounds the
application map at 24 entries while retaining 300 unique signed deployments,
300 successful single-cell COMMs, and a real three-way whole-cell conflict in
every write round. Distinct-key recovery and terminal convergence remain the
responsibility of the three graded tests; bounding state here prevents
application-state growth from changing the asymptotic subject of the 422-block
consensus/frontier test.

Every fresh sibling and marker deployment binds `validAfterBlockNumber` to the
exact LFB on which all validators agree immediately before construction. The
proposed block must contain that exact signature with `Executed` admission,
without an interpreter failure; each sibling must additionally expose its new
key/value in its own post-state. Block propagation errors are fatal to the test.
These obligations matter beyond the 50-block deploy-lifespan horizon: an earlier
harness revision left fresh deployments at `validAfterBlockNumber=0`, so after
the earliest acceptable height became positive, the proposer correctly expired
new traffic and the remainder of the test unintentionally exercised empty
blocks. The direct persisted-state oracle exposed that defect at round 31. The
corrected gate therefore proves both that all 422 blocks were exercised and that
the workload did not silently disappear.

```bash
cargo test --release -p casper --test mod \
  batch2::map_cell_convergence_spec::finalized_floor_400_block_soak -- \
  --exact --ignored
```

---

## 6. Additional findings (investigated during verification)

### A10 — recovery throughput is bounded by the deploy-lifespan window

The soak's *terminal* full-convergence check initially failed: under **sustained**
single-cell N-writer overload the keep-one recovery backlog grows ~`(N−1)`/round
while recovery drains ~`1`/round, so old losers **expire** (deploy_lifespan)
before recovery. This is a **capacity bound**, not a merge fault — the merge held
every per-round invariant for the whole run. It is fundamental to keep-one on a
single cell (you cannot finalize `N` conflicting whole-cell writes/round when only
one survives per merge). The soak therefore asserts the fix-relevant invariants
every round and gates only the terminal full-convergence behind
`require_full_convergence` (the graded gates keep it `true`; the soak passes
`false`). *Not a consensus bug; a hotspot the application must avoid.*

### A9 — the fault-tolerance decision is now EXACT-INTEGER — **RESOLVED**

`clique_oracle.rs` previously computed `ft = (2q − S)/S` in **f32** and finalized on
`ft ≥ θ` (f32). That decision was deterministic (IEEE-754 f32 is exactly-rounded, so
every conforming node computed the identical verdict — no fork) but *imprecise*: for
stakes `> 2²⁴` the `i64→f32` cast drops mantissa bits, making the threshold fuzzy by
`O(S/2²⁴)`. The fix replaces the finalization **decision** with an **exact-integer**
test over `i128`, `θ = num/den = ppm / 1_000_000`:

- **One strict predicate:** `2·q·den > S·(den + num)` — `clique_oracle.rs`
  `ft_decides_exact` / `ft_witnessed_exact`, routed through every candidate-floor
  and durable-finalizer decision site. Equality is rejected by both. The early
  `agreeing ≤ S/2 ⇒ not finalized` gate remains exact.
  The `f32` `ft` value is kept only for display/metadata (`fault_tolerance_value`); **no
  decision is re-derived from it**. θ is threaded as the exact on-chain **ppm** (i64),
  converted once at `initializing.rs` (never the lossy f32).
- **i128 rationale:** `2·q·den ≤ ~2⁸⁴` and `S·(den+num) ≤ ~2⁸⁴` for `S ≤ i64::MAX`,
  `den = 10⁶`, both far below `2¹²⁷` — no overflow.
- **Formal (axiom-free):** Rocq `FtExact.v` — `ft_exact_iff_ratio_strict` proves the
  runtime strict comparison is the rational comparison cleared of denominators;
  `ft_exact_iff_ratio` retains the inclusive historical control.
  `ft_exact_gt_mono_q` is monotone in `q`, given `den ≥ 0` — the
  one honest side-condition, faithful since `den = 10⁶`; `ft_exact_no_overflow` proves the
  i128 envelope). Z3 `ft_exact_no_overflow.py` and Sage `ft_algebra.sage` cross-witness
  the same (i128 no-overflow, exact≡ratio for `≥` and `>`, and that the f32 residual is
  real — `2²⁴` and `2²⁴+1` collide under `i64→f32`). Tests: `ft_decides_exact_tests`
  (small-stake agreement with f32, the large-stake boundary tie, `2·agreeing = S`, and
  no overflow at i64::MAX-scale stake).
- **Activation:** the exact decision changes which blocks finalize *at the margin* — a
  consensus-observable change — so it activates **atomically with the unreleased
  `staging-into-dev-merge` branch** (all validators run the branch binary together; no
  mixed-version window), the same discipline as the IntegerAdd apply change (below). No
  on-chain activation parameter is introduced.

### IntegerAdd overflow-launder — **FIXED at both chokepoints** (Phase 6 W3 combine; Phase 7 W7.1 terminal apply)

The IntegerAdd number-channel pipeline is a **wrapping group** `ℤ/2⁶⁴`, and it has
exactly TWO points where an overflow must be caught — the *combine* (summing a
branch's diffs) and the *terminal apply* (writing `base + Σdiffs` to consensus
state). The per-deploy *diff* is deliberately **not** one of them.

**The defect.** Two silent-`wrapping_add` sites could commit an overflowed value.
These must be distinguished from the *rejection decision*: `conflict_set_merger.rs`
`cal_merged_result` already used `checked_add` + `≥0` to decide *which* branches to
drop — it is **not** a write. The two writes that still wrapped were
`rspace++/…/merging_logic.rs` `combine_mergeable_value` (folds a branch's IntegerAdd
diffs) and `rholang/…/rholang_merging_logic.rs` `calculate_number_channel_merge`
(the terminal apply that actually PERSISTS the merged value via `dag_merger`). A sum
that wraps to a **non-negative** value (e.g. `i64::MAX + i64::MAX + 2 ≡ 0 (mod 2⁶⁴)`)
would sail through the `≥0` gate carrying a wrong value.
**Reachability (corrected):** the terminal apply is a consensus-state write reached
whenever `base + Σdiffs` crosses `i64::MAX` — e.g. near-`i64::MAX` genesis vault
balances — and was a fragile *non-local* invariant (safe only because the combine
gate happens to keep sums in range), not "unreachable for realistic supply".

**Resolution** (user decision: **both** — fix *and* prove bounded):
- **Fix — combine (Phase 6 W3):** `combine_mergeable_value` returns `Option<i64>`;
  IntegerAdd uses `checked_add` (`None` on overflow), propagated through
  `EventLogIndex::combine` (Level A) and `cal_merged_result` (Level B) so a wrapping
  combine **rejects the branch**. BitmaskOr unchanged.
- **Fix — terminal apply (Phase 7 W7.1):** `calculate_number_channel_merge` IntegerAdd
  now uses `checked_add` + a `≥0` guard → `Err(HistoryError::MergeError)` on overflow
  OR a negative balance, never `wrapping_add`. A defense-in-depth backstop (the
  combine gate rejects overflow upstream, so this can only reject an already-wrong
  value: for `base ≥ 0` any positive overflow wraps *negative*, caught by `≥0`). The
  `Err` propagates through `dag_merger::merge → interpreter_util.rs`, aborting the
  merge deterministically.
- **Deliberately NOT changed — the diff `end − prev`:** `calculate_num_channel_diff`
  stays `wrapping_sub`. It is the exact **group inverse** of the wrapping add that
  language-level execution (`reduce.rs` `GInt +`, intended 64-bit semantics) used to
  produce `end`, so it faithfully **recovers the deploy's true intended delta** even
  when execution overflowed and stored a wrapped `end`. `end − prev` can legitimately
  exceed `i64` range for such a deploy; a `checked_sub` there would hard-error the
  LIVE per-block diff path and crash block processing on a deploy that must instead be
  **gracefully rejected at merge time**. (The casper `…_got_overflow` integration test
  refuted a checked-diff design; regression `diff_integer_add_recovers_wrapped_delta`
  locks the invariant in.)
- **Formal:** Rocq `IntegerAdd.v` — `launder_exhibit` confirms the defect is real,
  `checked_combine_sound` proves the fix is launder-free (an accepted combine returns
  the true sum, in range), `supply_cap_no_launder` proves the defense-in-depth bound
  (while partial sums stay ≤ Cap, wrapping = checked); the Z3 BitVec-64 witness
  `integeradd_launder_bitvec.py` confirms the same against exact machine `i64`
  (`bvadd = wrapping_add`). In the model, `checked_apply` corresponds to the terminal
  apply (Site 1) and `wadd`/`wsum` to the wrapping execution + the diff group.

**Activation.** The reject-vs-wrap change at the combine + terminal apply activates
*atomically* with the unreleased finalized-floor feature (the
`dag_merger::merge → conflict_set_merger → calculate_number_channel_merge` chain is
part of it) — no separate flag, no mixed-version window. The diff
(`calculate_num_channel_diff`) is on the already-live execution/replay path but is
left wrapping (above), so its behavior is unchanged and there is nothing to gate.

### A9 — exact-integer fault tolerance — **RESOLVED (see the A9 block above)**

The precision residual is closed: finalization now decides with the exact-integer test
`2·q·den ≥ S·(den + num)` (i128), across the floor and LFB-finalizer paths, activated
atomically with the branch. See the resolved **A9** block above for the full change, the
axiom-free `FtExact.v` + Z3 + Sage witnesses, the tests, and the activation note. The
change was scoped across the whole clique oracle (all finalization), independently
verified — the exact discipline the prior revision recommended.

---

## 7. Verification status

Run the whole suite with `scripts/check-finalized-floor-ALL.sh` (Rocq and the
state-preservation Apalache checks are authoritative; TLC runs when installed;
Z3/Sage remain availability-gated and Wolfram is licensed opt-in). A release claim requires this gate plus the
multi-node integration suite to pass for the candidate binary.

| Layer | Result |
|---|---|
| Rust build | `cargo check -p casper --all-targets` / `-p rspace_plus_plus` clean |
| Convergence green-gate | 3/3 pass; 400+-block soak holds all fix invariants (422 blocks) |
| Rust unit/regression | combine + terminal-apply launder (`checked_add`), discriminating true-launder (sum wraps non-negative), wrapping-group diff recovery, guard-trip cold-fallback, Case-B dominating-tip, incompatible-fork `Err`, backstop predicate, floor warm==cold + cache-transparent, frontier round-trip, complete finalizer scan, clique-certified stale-state rejection, causal-certificate/state-support separation, asymmetric $`60/20/15`$ off-main advancement, $`40/35/25`$ multi-voter parallel promotion, universal dual-certified floor promotion at exact `FTT=0.1`, all six parent permutations, generated branch-depth/order cases with pairwise coverage/support/weight/verdict equivalence, non-descending coverage rejection, narrow linear-reuse controls, state-rejection control, exact three-way/repeated/permutation effect preservation, unrelated-rejection scan precision, wire/metadata round trips, validation tamper rejection, state-frontier property cases, and real conflicting-deploy floor rebase — all pass |
| Rocq | full development builds `-j1`; **85 headline results axiom-free**, including witness-equivalent carrier interoperability and exact proof-pair binding, exact target-deploy observation, exact target-bound dual certification, all-parent finalizer discovery equivalence, unique highest exact selection, materialization-target alignment, local-ledger identity separation, live minority-fork recovery, objective-equivocation convergence, committee-transition separation and accepted-only registration, accountable parallel promotion, arbitrary-node parallel-validator isolation, admission/effect alignment, source-aware occurrence disposition and finalized-status scope, recovery admission/leadership, merge/recovery activation, exact-effect causal rejection closure, exact active-effect provenance and scan equivalence, floor-rebased causal parent selection, causal/state certificate refinement, universal certified-floor promotion, latest-message coverage equivalence, sound linear-snapshot reuse, snapshot provenance closure/interleaving, heartbeat recovery/backpressure, accountable safety, rejection-reason confluence, protocol activation and lifecycle, block-bound bootstrap replay, typed exact-artifact local-fault deferral with commuting requests, terminal funding admission, A9 exact FT, G2 provenance, θ-exact advancement, finalizer progress, state-preserving admission, and standalone bridge/refinement results |
| Rocq kernel (coqchk) | **independent kernel re-check** of `FinalizedFloor.MainTheorem` + all deps ⇒ "Modules were successfully checked" (C3) |
| TLA⁺ / Apalache | `SpecFixed`, `FinalizedFloorScan`, `FinalizerProgress`, the 4,155-generated / 961-distinct-state `WitnessEquivalentCarrier` model, the complete 144-state two-node asymmetric-stake `StateLineageFinality` model, the 144-state / 649-generated exact `StateEffectProvenance` model, the 808-distinct-state / 17,169-generated node-local `StatePreservingForkChoice` safety/liveness model at depth 10 plus the axiom-free arbitrary-node product lifting, the 3,411-state / 12,877-generated three-validator `ParallelValidatorConsensus` split-transition model, its 58-state / 150-generated stale-candidate concurrency window, the 225-state / 1,051-generated `CertifiedFloorPromotion` model, the 16-state / 27-generated `LatestMessageCoverage` worklist model, the 10-state / 18-generated `SnapshotFloorMaterialization` interleaving model, the 4,194-state / 22,468-generated eventual-synchrony `HeartbeatFinalityBackpressure` model and its `1/4/5` exact-weight variant, its 4,338-state / 22,960-generated existing-candidate variant, the 17,766-state / 113,968-generated asynchronous heartbeat safety model, the 287,496-state / 1,123,849-generated arbitrary-wake `HeartbeatRecoveryCadence` model, the 296,424-state / 1,885,257-generated `PendingDeployHeartbeatComposition` model, its 551,136-state / 2,892,275-generated ingress-safety projection, the 1,002-state / 2,542-generated `ProposerAdmissionCoalescing` model, the 8-generated / 6-distinct-state `DivergentFinalizationHistories` model, the 264,205-generated / 16,984-distinct-state `LiveMinorityForkRecovery` model, the 1,061,249-generated / 153,856-distinct-state `RecoveryCommitteeTransition` model at depth 18, the 58,321-generated / 11,880-distinct-state `ObjectiveEquivocation` model at depth 27, and the 769-generated / 256-distinct-state `ObjectiveEvidenceAuthorization` model at depth 17 pass under TLC. `WitnessEquivalentCarrier` also passes Apalache through length 5, while all four exact controls fail under both checkers by length 3. The recovery controls reject remote local-ledger identity, remote-head mutation, missing dependency closure, and global proposal pausing. `RecoveryCommitteeTransition` also passes Apalache through length 6 with separate replayed-post-state and serialized-cache variables; its fourteen TLC/Apalache controls cover authority, registration, root, sender-key, latest-message, positivity, cache, and legacy-index boundaries. `ObjectiveEquivocation` passes Apalache through length 8 with fourteen controls spanning objective discovery, incarnation grouping, authority, repair, unary selection, and vote projection. `ObjectiveEvidenceAuthorization` passes through length 12; its seven controls independently expose pair-before-epoch selection, cross-epoch acceptance, stale snapshot generation, stale snapshot bond, offender-wide unary suppression, pair-only activation loss, and proposer/receiver predicate drift. `ProtocolV5EndToEnd` passes its 19-invariant symbolic composition through length 5; all twelve guided defect traces reproduce their named violation under both TLC and Apalache, while the unconstrained action product is covered compositionally by the exhaustive component models and axiom-free Rocq capstone. Heartbeat Apalache checks pass at safe bounds 5, 4, and 10 respectively and reach explicit promotion/missing-state/backlog/cadence witnesses; the pending-composition and proposer-coalescing symbolic checks remain separately bounded; the other completed bounded Apalache model families and `EffectCausalClosure` pass; the node-local Apalache projection checks every `StatePreservingForkChoice` invariant through bound 10, the parallel-validator model through routine bound 6 and deep bound 8, the stale-window guard through bound 2, and the one-step unsafe stale-promotion counterexample; TLC exhausts each finite local schedule graph while Rocq lifts node-local preservation and independent-action commutation to arbitrary node products; write-loss, cut-above-floor, cap-starvation, budget-restart, timeout-restart, stale-state promotion, unsupported-state-floor promotion, erroneous main-spine admission, single-base accepted-effect loss, floor-unprotected parent replay, early support, promotion without local replay, shared-root authority/publication, non-atomic promotion, main-spine-only certified-floor starvation, unordered late coverage, parent-only incomplete snapshot provenance, eager heartbeat backlog, fixed-offline-leader starvation, causal-only promotion, collapsed heartbeat cadence, pending-masked recovery, premature round completion, preterminal pool removal, missing recovery reservation, unbounded duplicate admission, ambient empty authority, lost pending wake, stale recovery permit, blanket block-lineage rejection, direct-only rejection, pair-before-epoch authorization, split-root slash authority, pair-only activation loss, and authorization-predicate drift controls reproduce their counterexamples |
| Finalizer materialization refinement | `FinalizerFloorMaterialization` exhausts 9,289 generated / 1,849 distinct states to depth 15 with two independently delivered node views and proves strict-boundary rejection, state-rejected-sibling exclusion, complete secondary-parent discovery, exact target binding, dual certification, non-starvation, and eventual materialization. Apalache checks the safe model through length 8. Main-parent-only discovery and causal-only target substitution reproduce their exact named violations under TLC and Apalache. |
| Deploy recovery TLA⁺ | `DeployRecovery` checks both validators online, concurrent source-owner custody, bounded retry expiry, and finalization progress; its heartbeat, foreign-custody, and parallel-owner witness configurations distinguish liveness failure from valid independent recovery. `RecoveryFrontierCoverage` checks collective selected-parent coverage, bounded lease escape, owner retry, and independent ordinary progress; its one-parent control reproduces split-frontier deferral. `DeployIdentitySeparation` passes under TLC and Apalache, while the raw-key controls reproduce cross-protocol rejection aliasing. `MergeRecoveryCoherence`, `EffectCausalClosure`, `RejectionReasonConfluence`, `ProtocolActivationCoherence`, all three `ProtocolVersionLifecycle` safe configurations, `ApprovedStateReplay`, the 9,025-state concurrent `LocalValidationRecovery`, `FundingAdmissionLifecycle`, and `AdmissionEffectAlignment` pass; their targeted unsafe controls reproduce finalized-receipt masking, partial-chain retention, exact independent-effect loss, orphaned transitive-effect acceptance, state-record mismatch, identity mismatch, last-writer reason divergence, floor-version selection, mixed scope, malformed encoding, stale ceremony, version non-adoption, proposer bypass, receiver disagreement, unsupported startup, current-context historical root divergence, immediate local-fault self-requeue, block/state artifact-identity collapse, inconclusive-block drop, false objective invalidity from local absence, live-state funding disagreement, indefinitely pending underfunding, and validator proposal failure from raw status counting |
| Z3 | FT-algebra + BitVec-64 IntegerAdd launder (exists on wrap; checked-combine launder-free) + **G2 `ft_ppm_roundtrip`** (FPA Float32/64 RNE: `to_ppm` monotone/range, ½ppm round-trip, exact-decision display-invariance) |
| Sage | FT-algebra identity + finalization-margin monotonicity |
| Wolfram (optional) | with `RUN_WOLFRAM=1`, service-rate regimes, exact weighted-quorum regions, and correctness-constrained repair-design/crossover analyses pass under the licensed kernel; 227,264 small rational and 232,064 production-PPM quorum cases have no mismatch; exact-frontier deferral is the sole feasible bounded parent policy among four modeled families; the default gate acquires no license |
| Loom (concurrency) | **C10** `loom_frontier_floor_cache` — the write-once `floor_index`/`frontier_index` memoization observes no torn/regressed value on any interleaving (the concurrent realization of the sequentially-proved T-CACHE; real guarantee = idempotence + LMDB single-key MVCC). `loom_committee_transition` explores concurrent registration, promotion, and head drift: same-block post-state cannot self-authorize, no unregistered validator gains authority, and head drift cannot change authority or synchrony. `loom_objective_equivocation` exhausts six models covering concurrent successful sibling admission, opposite local-invalid classifications, interleaved old-generation/old-epoch/current siblings, proposer/receiver predicate parity, non-positive authority, and exact-key unary suppression. `loom_live_minority_fork_recovery` exhausts remote-tip/local-finalizer races, duplicate and reordered advice, post-capture admission retry, atomic local head/effect publication, and independent validator progress without a shared publication lock. `loom_local_validation_recovery` exhausts duplicate same-block waiters racing artifact arrival, block/state release isolation, and independent genesis/restored validator recovery without shared request state. `loom_finalization_carrier_wakeup` exhausts park/admit races, duplicate wake coalescing, different honest digests for one floor/state, and wrong-state non-wakeup. |
| Finalizer concurrency refinement | `loom_finalization_atomicity::frozen_target_cannot_mix_with_a_concurrent_latest_message_arrival` proves that a frozen requested/selected target and its publication cannot be retargeted by a concurrent ambient latest-message update. The concrete finalizer property compares optimized all-parent coverage, every per-target causal decision, the state/current-floor eligible set, and greatest-candidate selection with an exhaustive pairwise oracle. |
| Rust proptest | **G2** `prop_ft_ppm_provenance` (`reconcile==onchain`, real `to_ppm` round-trip/range, genesis embed↔read) plus **P1** `prop_bonds_from_floor`: seven transition properties cover exact replayed post-state serialization, no same-block self-authorization, accepted-only registration, authorization after promoted registration, head/post-state independence, cache mutation and duplicate rejection, and exact floor justifications. `carrier_selection_is_permutation_invariant_and_preserves_digest_pairing` covers 256 generated carrier identities, witness digests, and insertion orders. |

**Coverage matrix (§4).** After the Phase-7 strengthening every catalog item maps to
a concrete Rocq/TLA⁺/Z3/Sage artifact or Rust test — including the two seams the
earlier revision had *asserted* rather than mechanized (an honest correction: that
"no assumed row" claim was premature). Both are now closed: the frontier-cache guard
is **bridged in Rocq** (`GuardBridge.chain_adj_AdjDC` derives Floor.v's `AdjDC`
premise from the committee-constancy predicate the Rust guard enforces), and T-FIN is
**unconditional** (`GuardBridge.upgo_finalized`), no longer merely `Forall Fin cands →`.
The former A9 `f32` finalization precision residual is closed by the exact-integer
decision described above.
**G2 (θ_ppm provenance)** additionally closed a *latent* overflow-envelope gap: the A9
`ft_exact_no_overflow` bound assumed `0 ≤ num ≤ den`, but the node's own
`ft_decides_exact` validates `num ∈ [−den, den]` (documented negative-θ "finalize on
any majority clique" sentinels); the hypothesis is now widened to `[−den, den]`
(i128 envelope still holds), and `FtProvenance.reconcile_is_onchain` proves the θ_ppm a
node finalizes with is always the *on-chain* value (local config unconditionally
overridden), so it cannot fork the exact decision.

**Policy:** all of the above run **locally**. Do **not** add any Rocq / TLA⁺ / Z3 /
Sage / Wolfram step to `.github/workflows/*` (an earlier formal-CI workflow was
deliberately removed).

### 7.1 Scope disclosure — what the capstones prove (and what they do NOT)

The finalized-floor capstones prove **DETERMINISM** of floor derivation and the
floor-anchored merge: that every honest node derives the **same** finalized floor,
the frontier **cache** is transparent (warm up-walk == cold down-walk), the
multi-parent **merge** is order-independent, and no mergeable **write** is lost.
Concretely they establish floor/cache determinism (`frontier_cache_transparent`,
`guard_constant_committee_transparent`(`_ft`)), monotone finalization (L-ANC /
L-SNAP, and their θ-exact and advancement variants), sound selection
(`select_sound`, `select_highest_sound`), and the arithmetic hardening (A9/G2).
The state-preservation capstones additionally prove that a promoted candidate
preserves every previously committed active effect. They treat causal and state-preserving
certification as separate inputs. `CliqueOracle.v` proves the refinement bridge:
every state-preserving certificate is also a causal certificate when state
preservation implies DAG ancestry. The proof neither manufactures a causal certificate
nor treats a causal merge edge as evidence that the merged state retained the
candidate. `StateEffectProvenance.v` proves the concrete merge recurrence used by
that preservation relation; it does not assume the old functional state-base
abstraction. Its parent-selection theorem additionally proves that the causal
input set is non-empty, retains every valid latest input, falls back exactly to
the LFB when that set is empty, and preserves every non-rejected finalized effect
through floor-rebased merge. The theorem deliberately says nothing about
receiver-local LFB state:
validation remains a function of the proposed block's declared evidence.
`CertifiedFloorPromotion.v` closes the next refinement boundary: when the current
dual-certified floor is preserved by every declared parent, it is a universal
all-parent causal candidate and is discoverable from any non-empty parent list.
The theorem proves eligibility and preservation, while the TLA⁺ model covers
asynchronous arrival and liveness and the Rust property test covers the concrete
descending multi-source traversal.
`CertifiedFloorPromotion.v` additionally proves that propagated latest-message
coverage is extensionally equal to pairwise reachability and that the narrow
linear-snapshot reuse guard preserves the eligible ancestor set. The corresponding
`MainTheorem.v` capstones expose both results axiom-free. These are optimization
transparency theorems: supporter filtering, validator weights, hard-majority
gating, maximum-clique selection, and exact threshold comparison remain the
existing consensus rule.

They do **NOT** prove **CBC finalization safety** — the *quorum-intersection /
agreement* property that two conflicting blocks can never both finalize. **No such
theorem exists in this development**, and the disclosure is deliberate rather than
implied by the "no-fork" labels:

- `Finalized` here is a **faithful monotone abstraction** of the clique oracle (a
  majority-weight agreeing sub-committee), used to carry L-ANC/L-SNAP through the
  cache proof. It is **not** a proof that the finalization rule is *safe* — the
  monotonicity lemmas are quorum-**opaque** (they reuse the same witnessing set),
  which is exactly why they never need, and never establish, quorum intersection.
- The `2·cweight Q > cweight c` majority bound and the θ-exact test bound the weight
  of a *single* quorum; safety would need that **two** majority quorums must
  **share** an honest validator. That combinatorial lemma is **out of scope** here.
- **Groundwork laid (Tier-2):** the quorum abstractions now carry
  `NoDup (map fst Q)` (distinct validators, matching the code's
  `WeightMap = HashMap<V,i64>` keys). Distinct-validator quorums are precisely the
  hypothesis a future quorum-intersection lemma
  (`2·|Q₁| > S ∧ 2·|Q₂| > S ⇒ Q₁ ∩ Q₂ ≠ ∅`) would build on; it would additionally
  require committee well-formedness (`NoDup (map fst c)`) and a disjoint-weight
  bound, and is tracked as a separate effort, not asserted here.

Merge/floor determinism is a **necessary** condition for safety (a non-deterministic
floor or merge would fork outright) but not a **sufficient** one; CBC agreement lives
in the finalization rule and is stated here as an explicit **non-goal**. The same
disclosure applies to the merge-algebra dossier (`merge-algebra-verification.md`
§7.1), whose capstones likewise prove determinism, not quorum intersection.

---

## 8. Diagrams

Twelve PlantUML diagrams (sources + rendered SVGs in [`diagrams/`](./diagrams/); render
with `plantuml -tsvg`, checked by `scripts/check-finalized-floor-ALL.sh` step
**[6/8]**). Each is fully coloured with an in-diagram legend. Click any figure for the
full-resolution SVG.

### 8.1 Component correspondence — spec ↔ Rocq ↔ TLA⁺ ↔ Z3/Sage ↔ Rust

[![Diagram 1 — every finalized-floor component (floor derivation, clique oracle, merge write-algebra, merge driver/backstop, recovery, LMDB cache) annotated with its spec concern, Rocq module, TLA⁺ model, Z3/Sage witness, and Rust file, with the axiom-free MainTheorem capstone on top](./diagrams/01-component-correspondence.svg)](./diagrams/01-component-correspondence.svg)

*Provenance: the §4 catalog ↔ §5 artifact map, made visual.*

### 8.2 Warm up-walk vs cold down-walk (T-CACHE)

[![Diagram 2 — sequence: the warm incremental_frontier (read cached pivot → committee-constancy + L-SNAP guards → O(advance) up-walk) versus the cold top-down walk, with the L-ANC note that makes the two results identical](./diagrams/02-seq-warm-vs-cold-walk.svg)](./diagrams/02-seq-warm-vs-cold-walk.svg)

*Provenance: §3.1; Rocq `Floor.frontier_cache_transparent` + `GuardBridge`.*

### 8.3 The Δ-ratchet — buggy runaway vs constant-overhead service regimes

[![Diagram 3 — the positive-feedback ratchet: the buggy Θ(Δ²·V) floor walk can starve finalization and drive Δ across the 256 cliff into silent write-loss; the constant-overhead up-walk removes lag feedback, and keeps Δ bounded only when service meets or exceeds arrivals](./diagrams/03-delta-ratchet.svg)](./diagrams/03-delta-ratchet.svg)

*Provenance: §2; Wolfram `delta_ratchet.wl` (positive lag feedback before the fix; zero lag feedback plus explicit over/equal/under-provisioned service regimes after it).*

### 8.4 IntegerAdd overflow-launder and the fail-loudly fix

[![Diagram 4 — the launder (a combine that wraps to a non-negative value passes the ≥0 apply gate carrying a wrong value) versus the fix (checked_add at both the combine and the terminal apply; the per-deploy diff stays the wrapping group inverse that recovers the true delta)](./diagrams/04-integeradd-launder-and-fix.svg)](./diagrams/04-integeradd-launder-and-fix.svg)

*Provenance: §6; Rocq `IntegerAdd.v`; Z3 `integeradd_launder_bitvec.py`.*

### 8.5 Merge flow — floor derive → floor-bounded scan → Δ-backstop

[![Diagram 5 — activity: compute_parents_post_state derives the floor, computes Δ, and either parks (propose) / rejects (validate) deterministically when over the cap, or runs the H3 floor-bounded scan and the checked merge fold to produce the post-state](./diagrams/05-activity-merge-flow.svg)](./diagrams/05-activity-merge-flow.svg)

*Provenance: §3.2/§3.3; Rocq `Selection.scope_covers_band`; TLA⁺ `FinalizedFloorScan`.*

### 8.6 Finalization downward-closure + the warm-path guards

[![Diagram 6 — state: the warm up-walk advancing over a downward-closed finalized prefix (L-ANC), with the committee-constancy and L-SNAP guard transitions that divert to the cold-walk fallback yielding the identical frontier](./diagrams/06-state-finalization-guards.svg)](./diagrams/06-state-finalization-guards.svg)

*Provenance: §3.1; Rocq `GuardBridge.chain_adj_AdjDC` (guard ⇒ AdjDC).*

---

### 8.7 A9 — exact-integer fault-tolerance decision vs the f32 fuzzy threshold

[![Diagram 7 — the legacy f32 path casts q, S to f32 (lossy above 2²⁴) and compares the fuzzy ratio, versus the exact-integer i128 cut 2·q·den ⋛ S·(den+num) that is bit-identical on every node](./diagrams/07-a9-exact-vs-f32-decision.svg)](./diagrams/07-a9-exact-vs-f32-decision.svg)

*Provenance: §6.A9; Rocq `FtExact.v`; Z3 `ft_exact_no_overflow.py`; Sage `ft_algebra.sage`.*

### 8.8 Dual-certificate state-preservation admission

[![Diagram 8 — sequence: the original exact causal certificate accepts a rejected-parent candidate, the second exact certificate rejects it because the apparent merge support did not preserve its effects, and both certificates plus current-LFB effect preservation admit the rebased successor](./diagrams/08-state-lineage-admission.svg)](./diagrams/08-state-lineage-admission.svg)

*Provenance: §§3.9–3.10; Rocq `CliqueOracle.v`, `StateLineageFinality.v`, and `StateEffectProvenance.v`; TLA⁺/Apalache `StateLineageFinality.tla`, `StateEffectProvenance.tla`, and `StateEffectProvenanceApalache.tla`; Rust stale-state, exact-effect recurrence, parent-permutation, state-support, asymmetric off-main advancement, and execution-rebase regressions.*

### 8.9 Causal parent retention with floor-rebased replay

[![Diagram 9 — sequence: causal and vote projections diverge safely, the LFB backstop covers all-stale snapshots, GHOST remains the main parent, deterministic depth expiry preserves evidence roots, and replay protects finalized state](./diagrams/09-state-preserving-fork-choice.svg)](./diagrams/09-state-preserving-fork-choice.svg)

*Provenance: specification R-PARENT-CAUSALITY/R-PARENT-STATE/R-PARENT-EVIDENCE; Rocq `StateEffectProvenance.v`; TLA⁺/Apalache `StatePreservingForkChoice.tla`; Rust `snapshot::fallback_to_finalized_parent` and merge-rebase regressions.*

### 8.10 Atomic finalization publication and restart recovery

[![Diagram 10 — parallel immutable finalizer evaluations converge at one compare-and-append ledger transaction; the winning immutable manifest is projected in order before idempotent receipted effects, while stale workers are inert and durable cursors resume unfinished work after restart](./diagrams/10-finalization-atomicity-recovery.svg)](./diagrams/10-finalization-atomicity-recovery.svg)

*Provenance: specification R-FINALIZATION-APPEND through R-FINALIZATION-SCHEDULER; Rocq `FinalizationAtomicity.v` and `ProposalFloorReadiness.v`; TLA⁺/Apalache `FinalizationAtomicity.tla`, `FinalizationWorkerRetry.tla`, `ProposalFloorReadiness.tla`, `FinalizationBoundHead.tla`, `FinalizationRecovery.tla`, and `FinalizationGenesisIdentity.tla`; Rust finalization-ledger unit/property/thread tests, typed proposal-deferral regressions, the exact state-regression and rooted-restart storage tests, and `loom_finalization_atomicity`.*

### 8.11 Exact target-deploy observation across intermediate LFB progress

[![Diagram 11 — sequence: an exact target remains pending across a genuine intermediate LFB-height advance, the observer renews only its stall budget, later parallel-validator support makes the exact target status Finalized, and only then does the observer succeed](./diagrams/11-target-deploy-terminality.svg)](./diagrams/11-target-deploy-terminality.svg)

*Provenance: specification R-TARGET-EXACT through R-TARGET-CLOCK; Rocq/TLA⁺ `TargetDeployTerminality`; pyf1r3fly monotonic fake-clock regressions; system-integration bridge real-deploy regression.*

### 8.12 Exact stale-sibling settlement and recovery

**Diagram 12 — exact stale-sibling settlement and recovery.** Validators admit
siblings concurrently and may observe them in different orders. Finalizing `B`
changes `A` from a vote to a causal-only input; it does not discard `A`. Exact
frontier settlement creates the source-bound rejection that authorizes its
carrier owner, after which every validator finalizes the same effects.

[![Diagram 12 — sequence: three validators independently admit siblings A and B, finalize B while retaining A as a causal input, settle the complete frontier with an exact source tombstone, give carrier A custody to its owner, deny non-owner retries, and converge on one recovery containing A, B, and fresh work](./diagrams/12-stale-sibling-settlement-recovery.svg)](./diagrams/12-stale-sibling-settlement-recovery.svg)

*Provenance: specification R-PARENT-CAUSALITY/R-PARENT-STATE and exact occurrence disposition; Rocq `StaleSiblingRecovery.v` and `OccurrenceDisposition.v`; TLA⁺/Apalache `StaleSiblingRecovery.tla`; Rust `resolved_asymmetric_frontier_rehomes_excluded_local_deploy`; Loom `loom_recovery_custody`.*

---

## 9. References

Foundational literature for the consensus, finality, arithmetic, and
formal-methods claims in this dossier. DOIs are given where they exist and have
been verified; whitepapers without a DOI carry a stable identifier.

1. L. Lamport. **The Part-Time Parliament.** *ACM Trans. Comput. Syst.* 16(2),
   1998. DOI [10.1145/279227.279229](https://doi.org/10.1145/279227.279229).
   *(Quorum-based agreement — the majority-weight sub-committee underlying the
   clique oracle's `Finalized`.)*
2. M. Castro, B. Liskov. **Practical Byzantine Fault Tolerance and Proactive
   Recovery.** *ACM Trans. Comput. Syst.* 20(4), 2002.
   DOI [10.1145/571637.571640](https://doi.org/10.1145/571637.571640).
   *(BFT quorum intersection — why a finalized cut is stable, §1, L-ANC.)*
3. M. J. Fischer, N. A. Lynch, M. S. Paterson. **Impossibility of Distributed
   Consensus with One Faulty Process.** *J. ACM* 32(2), 1985.
   DOI [10.1145/3149.214121](https://doi.org/10.1145/3149.214121).
   *(Why liveness is guarded, not assumed — §2's ratchet is a liveness failure.)*
4. C. Dwork, N. Lynch, L. Stockmeyer. **Consensus in the Presence of Partial
   Synchrony.** *J. ACM* 35(2), 1988.
   DOI [10.1145/42282.42283](https://doi.org/10.1145/42282.42283).
   *(Partial-synchrony model behind the propose/finalize progress assumption,
   TLA⁺ `Liveness_Progress`.)*
5. L. Lamport. **The Temporal Logic of Actions.** *ACM Trans. Program. Lang.
   Syst.* 16(3), 1994.
   DOI [10.1145/177492.177726](https://doi.org/10.1145/177492.177726).
   *(TLA — the basis of the `FinalizedFloor.tla` / `FinalizedFloorScan.tla` models.)*
6. Y. Bertot, P. Castéran. **Interactive Theorem Proving and Program Development:
   Coq'Art.** Springer, 2004.
   DOI [10.1007/978-3-662-07964-5](https://doi.org/10.1007/978-3-662-07964-5).
   *(The Coq/Rocq calculus in which the axiom-free capstones are mechanized, §5.1.)*
7. IEEE. **IEEE Standard for Floating-Point Arithmetic (IEEE 754-2019).** 2019.
   DOI [10.1109/IEEESTD.2019.8766229](https://doi.org/10.1109/IEEESTD.2019.8766229).
   *(Exactly-rounded, deterministic `f32` — the A9 determinism argument, §6.A9.)*
8. V. Zamfir et al. **Introducing the Minimal CBC Casper Family of Consensus
   Protocols.** CBC Casper whitepaper, 2018. (No DOI; stable source:
   `https://github.com/cbc-casper/cbc-casper-paper`.)
   *(Correct-by-construction safety oracle and the linear-finality fringe that
   `floor(B)` specializes, §1.)*
9. L. G. Meredith. **Cost-Accounted Rho Calculus: A Spectral Decomposition of
   Phlogiston.** F1R3FLY.io, May 2026. Source checkout:
   `../publications/cost-accounting/cost-accounted-rho.tex`. No DOI.
   *(Atomic resource transactions and conservation obligations composed with
   finalized-state permanence in H8; it does not specify LFB voting.)*
10. L. G. Meredith. **Continued Interactive GSLTs and the Cost Endofunctor.**
    F1R3FLY.io, May 2026. Source checkout:
    `../publications/cost-accounting-as-monad/continued-gslt-cost-v2.tex`. No DOI.
    *(Generic wrapping/cost semantics and interaction-cut atomicity; it does not
    specify Casper finalizer selection.)*
