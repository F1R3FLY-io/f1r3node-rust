# Fork-choice certified-context specification

This document is the normative contract for the Casper fork-choice estimator. The
[glossary](./fork-choice-glossary.md) defines its terms, and the
[verification dossier](./fork-choice-verification.md) maps every requirement to
executable evidence. `MUST`, `MUST NOT`, and `SHOULD` have their RFC 2119 meanings.

## 1. Scope

Fork choice selects the ranked block-DAG tips from which a proposer chooses parents.
It is an LMD-GHOST calculation over one certified finalized-floor context, not a
calculation over whichever mutable indices happen to exist at the receiving node.

The consensus-critical implementation is split across:

- `casper/src/rust/causal_equivocation.rs`, which constructs and digests the certified
  context and its distinct causal-parent and finality-vote projections;
- `casper/src/rust/estimator.rs`, which computes the LCA, frozen-stake scores, ranking,
  and parent bounds from that context;
- `casper/src/rust/engine/multi_parent_casper/snapshot.rs`, which consumes the ranked
  result when creating a proposal snapshot;
- `casper/src/rust/validate.rs`, which validates the declared parent structure; and
- `shared/src/rust/shared/list_ops.rs`, which implements the total ranking order.

## 2. Certified round input

For one incoming finalized floor `F`, a `CertifiedConsensusContext` contains:

- the hash and post-state hash of `F`;
- the exact active-validator set at `F`;
- each active validator's positive frozen stake and bond generation at `F`;
- exactly one latest-message slot for every active validator;
- inherited, generation-scoped objective-equivocation evidence;
- the causal-parent projection `C` and finality-vote projection `V` derived once from
  those fields; and
- a canonical digest committing to all of the above.

### R-CONTEXT

Every consensus consumer in one proposal or validation round MUST use the same
certified context. The estimator and finalizer MUST NOT independently derive a second
projection from receiver-local state.

The active set, stake map, generation map, and exact latest-message keys MUST agree
exactly. Fork choice MUST fail closed if any active-validator slot is absent. Extra
non-authority slots do not become eligible votes.

### R-CAUSAL-PROJECTION

An exact latest message belongs to the causal-parent projection `C` only when all of
these conditions hold:

1. its slot belongs to an active validator with positive frozen stake;
2. the cited block exists and is intrinsically valid;
3. except for the approved-genesis placeholder, its sender equals the slot validator;
4. its certified sender generation equals the validator's frozen generation;
5. that generation has no inherited objective-equivocation evidence; and
6. objective admission accepted the cited block in the certified dependency closure.

The finality-vote projection is the floor-descending subset:

```math
V = \{m \in C \mid m = F \lor F \preceq m\}
```

The two projections MUST be deterministic and MUST record a stable exclusion reason
for each rejected slot. Receiver-local invalid caches, latest-message maps, finalized
flags, ambient DAG height, iteration order, and arrival order MUST NOT change them.
The implementation MUST NOT use `V` as the proposal's complete state-dependency set:
doing so can drop an accepted sibling merely because the finalized floor advanced on
another branch.

## 3. LMD-GHOST rule

### R-LCA

The estimator MUST compute the lowest universal common ancestor of every eligible
latest message. It MUST NOT discard a certified message because a receiver has learned
an unrelated taller block. If no message is eligible, the approved genesis is the
scoring root.

There is deliberately no receiver-local `LATEST_MESSAGE_MAX_DEPTH` projection. Such a
projection makes the LCA depend on ambient DAG height and can make closure-equivalent
validators choose different roots.

### R-SCORE

For each validator represented in `V`, its frozen-floor stake `A(v)` is added exactly once to
every block in the supporting ancestry of `lm(v)` down to the LCA:

```math
score(b) = \sum_{v : b \preceq lm(v)} A(v)
```

The same `A(v)` MUST be used along the entire supporting chain. Candidate or
unfinalized bond maps MUST NOT reweight an already certified round. Each validator's
support traversal MAY execute in parallel, but contributions MUST be reduced with
checked integer addition in a deterministic order. Overflow MUST return a typed error.

### R-GHOST

Fork choice has two concurrent-preserving lanes with different purposes.

The head lane starts at the LCA and repeatedly selects the scored child that is first
under score-descending, hash-ascending order. It stops at the first block with no
scored child. This terminal block `g` is the greedy GHOST head.

The frontier lane starts at the same LCA. It may expand scored nonterminal blocks in
any order, replacing each with all of its scored children. It retains blocks with no
scored child and deduplicates shared children. On termination it MUST equal the exact
scored terminal frontier `T`; expansion order MUST NOT affect `T`.

The estimator MUST compose the two lanes as:

```math
ranked = [g] \mathbin{++} sort_{score\downarrow,hash\uparrow}(T \setminus \{g\})
```

It MUST NOT select the head by globally sorting `T`. For example, if one root child
has score `60` split between terminal descendants of scores `30` and `30`, while a
second root child and its only terminal descendant have score `40`, GHOST enters the
score-`60` subtree. The score-`40` terminal is a secondary tip, not the head.

Every traversed child edge MUST strictly increase block height. A non-advancing edge,
cycle, missing child, or greedy head absent from `T` MUST produce a typed error.

### R-TOTAL

Distinct siblings during greedy descent and distinct members of the secondary
terminal frontier MUST be ordered by score descending and then block hash ascending.
This is a total order. The ranked list's first position is reserved for `g`; only its
tail is globally score-sorted.

### R-PROPOSAL-PARENTS

The proposal parent candidates are the unique block hashes in `C`. If no candidate
equals or descends from `F`, the proposer MUST add `F` as an explicit backstop. It
MUST then remove every candidate covered by another candidate, producing the complete
reachability-maximal antichain. This compaction may remove an ancestor only when a
retained parent covers it; it may not select an arbitrary subset.

When `V` is nonempty, `g` MUST be a member of the compacted causal-parent set and MUST
remain the first declared parent. When `V` is empty, `F` is the first declared parent.
Deploy presence, pending-work policy, recovery policy, or input enumeration MUST NOT
replace this main parent. Recovery may narrow to the main parent only when it descends
from `F` and covers every live causal tip.

The evidence closure is rooted at the exact latest-message hashes and `F`, independently
of which causal tips survive proposal-parent depth expiry. Therefore expiration of an
old branch cannot erase equivocation evidence or the certified floor.

## 4. Cross-validator determinism

### R-EXTENSIONAL

For certified contexts with the same digest and DAG closures containing every cited
block, fork choice MUST be extensionally equal:

```math
C_1.digest = C_2.digest
\land closure(D_1, C_1) = closure(D_2, C_2)
\Longrightarrow FC(D_1, C_1) = FC(D_2, C_2)
```

Local blocks outside that closure and every receiver-local index are observationally
irrelevant. Parallel validator execution and message arrival may change when a round
can be evaluated, but never its result.

## 5. Parent bounds

### R-COUNT

Exactly `-1` and the estimator sentinel `i32::MAX` mean unlimited; zero and every
value below `-1` are invalid configuration. The implementation MUST NOT rely on
signed-to-unsigned wrapping.

The ranked vote frontier may cap secondary estimator results while retaining its head.
The proposal's causal-parent antichain MUST NOT be silently truncated. Let $`P`$
be the complete, depth-expired, reachability-maximal parent frontier derived from
one frozen proposal snapshot, including the finalized-floor backstop when needed.
A finite `max-number-of-parents` value $`c`$ admits that snapshot exactly when:

```math
|P| \le c
```

If $`|P| > c`$, proposal construction MUST return a typed deferred result before
block creation or signing. It MUST retain the complete evidence and pending work,
and MUST NOT truncate $`P`$ or select a receiver-local subset.

Let $`M`$ be `number-of-active-validators`. The provisioning rule
$`c \ge M + 1`$ is a sufficient worst-case bound: it reserves one distinct tip per
configured active-validator slot plus an independent floor backstop. It is not a
necessary admission condition because $`M`$ is a future committee ceiling while
the current committee may be smaller, validators may share a latest block, and
reachability compaction may cover multiple tips with one parent. Startup SHOULD
warn when a finite cap is below this worst-case bound, but MUST reject only
syntactically invalid bounds. Runtime admission is authoritative because it checks
the exact frozen frontier.

### R-DEPTH

For ranked candidates `R = [g] ++ S`, let `H` be the maximum block height in `R`.
Finite-depth filtering MUST return:

```math
F_D(R) = [g] \mathbin{++} [p \in S \mid H - height(p) \le D]
```

The selected first parent `g` is unconditional and remains first. Every secondary is
measured from the freshest original candidate. A secondary outside the horizon expires
from the current live causal frontier deterministically on every validator; it remains
in the exact evidence closure. This expiry is the liveness mechanism for a permanently
disjoint old unfinalized branch and is not an arbitrary omission.
The receiver computes the same `H` over declared parents, exempts only index zero and
the universally available approved genesis, and checks every other parent against
`D + depth_buffer`. It does not recompute fork choice.

### R-NONEMPTY

An empty ranked-tip set or an empty bounded-parent result MUST return a typed consensus
error. It MUST NOT panic and MUST NOT silently produce a parentless ordinary block.

## 6. Robustness

- Missing metadata for a context-cited message, LCA, scored ancestor, or declared
  parent MUST produce a typed error rather than silently shrinking the calculation.
- Every score addition and parent-count conversion MUST be checked.
- Parallel score traversal MUST not mutate the DAG or the certified context.
- Parallel or asynchronous frontier traversal MUST converge to the same exact,
  duplicate-free terminal set.
- A selected hash subset MUST NOT be used to rerun fork choice after parent pruning;
  pruning preserves the already selected GHOST first parent.

## 7. Validator boundary

Validators replay the block's declared parents and validate their structural and
certified-context constraints. They do not require a Byzantine proposer to have chosen
the same policy-preferred parent that the local proposer would choose. This preserves
Casper's accepted language while making the proposal-to-receiver depth-bound bridge
explicit and deterministic.

Let $`J`$ be the frozen justification set. Let $`P`$ be the declared parent set.
Let $`F`$ be the signed finalized-floor commitment.

The proposer derives its preferred frontier from $`J`$ and one frozen DAG view.
The receiver does not repeat that policy choice. The receiver evaluates these independent inputs:

- $`P`$ selects the replay state.
- $`J`$ selects votes, evidence, and authority.
- $`F`$ sets the durable state floor.

For each parent $`p`$, let $`\phi(p)`$ be its effective committed floor.
Let $`\preceq_D`$ mean DAG ancestry. Let $`\sqsubseteq_S`$ mean state containment.
The receiver-side floor check is:

```math
\begin{aligned}
\mathsf{Admit}(F,P) \equiv{}& P \ne \varnothing \\
&\land \exists p \in P.\; F \preceq_D p \\
&\land \forall p \in P.\; \phi(p) \preceq_D F
                         \land \phi(p) \sqsubseteq_S F \\
&\land \forall p,q \in P.\;
  \phi(p) \preceq_D \phi(q) \lor \phi(q) \preceq_D \phi(p).
\end{aligned}
```

An honest preferred frontier implies $`\mathsf{Admit}(F,P)`$.
The converse does not hold. A declared subset can omit a justified sibling and remain valid.
The subset remains valid only when a declared parent carries $`F`$.

This boundary preserves asynchronous Casper behavior. Exact receiver-side frontier equality would reject replay-safe blocks and reduce liveness.

## 8. Safety invariants

| ID | Forbidden state |
|---|---|
| S1 | Closure-equivalent honest validators derive different LCA, scores, ranking, or head from one certified context. |
| S2 | A missing, wrong-generation, equivocating, intrinsically invalid, sender-mismatched, non-authority, or outside-floor message contributes stake. |
| S3 | Candidate bonds or receiver-local indices change a frozen round's scores or projection. |
| S4 | Fork choice proceeds with an incomplete active-validator slot set. |
| S5 | Count or depth handling removes or replaces the selected first parent, silently truncates a live causal antichain, or expires a tip nondeterministically. |
| S6 | Missing metadata, empty output, integer overflow, or invalid conversion panics or silently changes the result. |
| S7 | An honest proposal's secondary parents fail the buffered receive-side depth predicate. |
| S8 | A globally largest terminal leaf replaces the greedy heaviest-subtree head. |
| S9 | Frontier expansion order, a multi-parent diamond, or a malformed non-advancing edge changes or prevents the ranked result. |
| S10 | A receiver accepts a parent frontier without committed-floor ancestry, or requires equality with its local preferred frontier. |
| S11 | Deploy or recovery policy replaces the GHOST main parent without proving floor ancestry and complete causal-tip coverage. |
| S12 | Evidence closure omits the captured finalized floor or an exact latest message because proposal-parent compaction or expiry removed it. |

## 9. Conformance

An implementation conforms only when every requirement above is represented in the
Rocq refinement, a TLA+ model with a falsifying negative control, Rust
example/property tests against production code, and the local verification gate. Run
`scripts/check-fork-choice-ALL.sh` and the certified-context portion of
`scripts/check-finalized-floor-ALL.sh` for the authoritative evidence set.
