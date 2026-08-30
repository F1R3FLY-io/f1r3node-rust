# Deploy occurrence consensus specification

## Status and scope

This document is normative for duplicate deploy handling across independently
operated validators. It specifies the protocol layer between Rholang execution
and DAG consensus. It does not change Rholang reduction semantics and does not
claim that the rho calculus selects a blockchain block.

The rho calculus gives Rholang its reflective process model. Parallel
composition is associative and commutative, while communication can still
contain genuine scheduling choice. Meredith and Radestock's foundational paper
is *A Reflective Higher-Order Calculus*, DOI
[10.1016/j.entcs.2005.05.016](https://doi.org/10.1016/j.entcs.2005.05.016).
The repository's [cost-accounted rho calculus publication](https://github.com/F1R3FLY-io/publications/blob/main/cost-accounting/cost-accounted-rho.tex)
requires same-signer deployments to compete for one linear token supply and
describes the deployment as the financial atomicity boundary. The
[RSpace denotational account](https://github.com/F1R3FLY-io/publications/blob/main/denotational-semantics-for-rho/knot-rho.tex)
separately identifies communication pairing as a source of nondeterminism.

The consequence is precise: independent validators may observe and execute
conflicting deploys in different orders, but validators that have the same
validated DAG must compute the same canonical projection. Consensus supplies
that projection; it must not pretend that all Rholang races are confluent.

## Definitions

A **deploy identifier** $`d`$ is the signed deploy's protocol identifier.

For protocol v6, $`d`$ is a typed `DeployIdV6` envelope commitment. It is not a primary-signature identifier.

A **source block** $`b`$ is the block whose `body.deploys` contains a processed
copy of $`d`$.

A **deploy occurrence** is the pair

```math
o = (d, b).
```

Two occurrences may have the same deploy identifier and different source
blocks. They are not the same protocol event.

[![Deploy recovery state machine: an exact-source disposition gates the source carrier owner, while independent owners keep separate custody and lifespan expiry closes admission.](diagrams/01-state-recovery-protocol.svg)](diagrams/01-state-recovery-protocol.svg)

A **rejection tombstone** is

```math
t = (d, b, r),
```

where $`r`$ is a reason: merge conflict, duplicate occurrence, or collateral
chain drop. A tombstone removes only the exact occurrence $`(d,b)`$.

A **canonical projection** is the deterministic reduction of every occurrence
and validated exact tombstone in the LFB's complete causal closure into active
and rejected occurrences. “Canonical” describes the deterministic result; it
does not restrict evidence to the LFB's main-parent spine.

## Required invariants

### O1 — source identity

Every newly created rejection record MUST contain both deploy identifier and
source block hash. Signature-only records are accepted only as legacy wire data.
They MUST NOT be interpreted as proof that every source occurrence was rejected.

### O2 — exact rejection

For observations $`O`$ and exact tombstones $`T`$ from the complete finalized
causal closure, active occurrences are

```math
A(O,T) = O \setminus \{(d,b) \mid (d,b,r) \in T\}.
```

A tombstone for $`(d,b_1)`$ cannot remove $`(d,b_2)`$ when $`b_1 \ne b_2`$.
An exact tombstone recorded by a secondary-parent ancestor has the same
authority as one recorded on the main-parent spine: both records are validated
consensus inputs to the LFB state. Main-spine placement is relevant only to the
legacy signature-wide compatibility reducer.

### O3 — deterministic keep-one

When mutually conflicting chains contain occurrences of the same deploy, every
validator applies the same total ordering. The current ordering compares, in
order:

1. descending total cost using non-wrapping arithmetic;
2. descending maximum individual deploy cost;
3. lexicographically smallest deploy identifier;
4. post-state hash;
5. the canonical deploy set;
6. source block hash.

The source hash is the final injective tie-break. Distinct chains MUST NOT
compare equal. The policy is a protocol decision; the calculus does not choose
these economic priorities.

### O4 — observation-order independence

Let $`P(O)`$ be the canonical projection. For validators $`v`$ and $`w`$,

```math
O_v = O_w \Longrightarrow P(O_v) = P(O_w).
```

Arrival order may change a validator's temporary pending view. It cannot change
the result after both validators have the same validated observation set.

### O5 — one active finalized occurrence

For every deploy identifier $`d`$ in a source-aware finalized projection,

```math
\left|\{(d',b) \in A(O,T) \mid d'=d\}\right| \le 1.
```

If the reducer finds more than one active finalized occurrence, it MUST return a
typed ambiguity error. It MUST NOT choose by map iteration, arrival order, or a
local node preference.

### O6 — one semantic reducer

Finalization status and `/api/deploy` MUST use the same all-parent canonical
disposition as state merge. An exact tombstone MUST NOT be ignored because its
recording block is outside the LFB's main-parent spine.
A secondary deploy index may accelerate lookup, but it cannot define consensus.
The occurrence index stores every valid source block; its compatibility
representative is deterministic by block height and hash.

### O7 — monotone finalization

The finalizer may advance only to the current LFB itself or a main-chain
descendant of the exact current LFB hash. Height alone is insufficient because
sibling blocks can have the same or greater height.

### O8 — recovery eligibility

Historical rejection records do not create recovery work. Parent narrowing is
permitted only when the local rejected-deploy buffer contains a currently
selectable occurrence that has not already won canonically.

### O9 — readiness is not absence

If a read-only node has received a block body but has not admitted it into its
DAG, the HTTP API returns `409 block_pending_admission`. It does not report the
block as absent and does not convert a synchronization race into an internal
error.

### O10 — canonical retry authorization

For deploy identifier $`d`$, let $`O_d`$ be every source occurrence visible in
the selected parent closure and let $`T_d`$ be the exact-source tombstones in
that same closure. Recovery is authorized only when the active-source set is
empty:

```math
A_d = O_d \setminus T_d = \varnothing.
```

A signature appearing in any rejection record is not sufficient. If one exact
source is rejected while another survives, $`A_d \neq \varnothing`$ and the
buffered deploy remains suppressed.

### O11 — strict recovery lifespan

Let $`v_d`$ be `valid_after_block_number`, $`n`$ the proposed block number, and
$`L`$ the shard deploy lifespan. Ordinary admission and recovery use the same
strict interval:

```math
v_d < n < v_d + L.
```

At $`n = v_d + L`$, the deploy is expired. Rejection history cannot extend the
interval. Expired entries are removed from both deploy storage and the local
rejected-deploy buffer.

### O12 — carrier-owner retry custody

Let $`c`$ be a rejected source carrier. Let $`owner(c)`$ be the validator that
signed $`c`$. Only that validator can hold and retry the rejected deploy:

```math
\operatorname{retry}(v,c) \Longleftrightarrow v = \operatorname{owner}(c).
```

A validator receives the same merge as its peers. During validation, the
validator buffers a rejected deploy only when it owns the named carrier.
Arrival order and merge proposer identity cannot transfer custody.

One carrier therefore has one retry proposer. Different carrier owners can
retry different work in parallel without a shard-wide lock.

Ordinary deploy inclusion still uses a deterministic finalized-view leader.
That leader limits duplicate ordinary proposals. It does not override
source-specific retry custody.

Recovery authorization is preserved through every downstream packaging filter.
The relevant duplicate scope is the selected candidate's parent closure, not
the validator's entire historical self-chain. Let $`H_v(d)`$ mean that validator
$`v`$ previously proposed deploy $`d`$, and let $`A_P(d)`$ mean that an active
occurrence of $`d`$ is reachable from candidate parent set $`P`$. Packaging
suppresses $`d`$ exactly when $`H_v(d) \land A_P(d)`$ and the current candidate
did not select $`d`$ for exact-source recovery. If $`H_v(d)`$ is true but
$`A_P(d)`$ is false because the old source lies on an excluded branch, the
candidate rehomes the still-authorized deploy. Treating every historical
self-chain occurrence as active would make admission and packaging consult
different scopes and could leave a valid deploy pending forever.

The candidate captures $`P`$, $`A_P`$, and its selected recovery set once.
Concurrent floor advancement may authorize a later candidate, but it cannot
change the decision already being packaged. This preserves validator
parallelism: each validator classifies against its own immutable candidate
context, without a shard-wide proposer lock.

### O13 — retry custody cannot suppress finality

An unavailable carrier owner can delay only its owned retry. Other validators
continue heartbeat and finality-support proposals.

Under eventual delivery, an online carrier owner eventually observes the
settled rejection. The owner then retries or reaches the strict expiry
boundary.

### O14 — fail-closed consensus inputs

Canonical disposition reduction, finalized-ancestry scans, visible-source
checks, and leader selection require the block bodies and finalized metadata
named by the snapshot. Missing committed inputs are a typed local
storage/dependency failure. The node does not substitute an empty closure,
height zero, a main-parent-only view, or any node-local fallback.

## Reference reducer

```text
function recovery_decision(snapshot, deploy, rejected_carrier, validator):
    observations := complete parent-closure occurrences of deploy
    tombstones := exact source tombstones in the same closure
    active := observations minus tombstones

    if any required block body or finalized metadata is missing:
        return LocalDependencyError
    if active is not empty:
        return Suppressed

    next_block := snapshot.maximum_block_number + 1
    if not (deploy.valid_after < next_block
            and next_block < deploy.valid_after + snapshot.deploy_lifespan):
        purge deploy from both local stores
        return Expired

    if validator != owner(rejected_carrier):
        return NotCustodian
    selected_recoveries := {deploy}

    historical := deploy occurs on validator's self-chain
    active_in_candidate := deploy occurs in selected parent closure
    if historical and active_in_candidate and deploy not in selected_recoveries:
        return Suppressed
    return RetryOnce
```

## Compatibility

Protocol v6 requires a fresh genesis and empty occurrence storage. A v6 node rejects legacy or partial occurrence state at activation.

Historical pre-v6 decoding uses explicit legacy identity types. Protocol-v6 lookup never falls back to a primary signature or byte-length inference.

The [deploy occurrence storage specification](deploy-occurrence-storage.md) defines the activation marker, tagged key space, archive, compaction, and crash recovery.

## Security consequences

- Invalid blocks are not added to the occurrence index.
- A peer cannot erase every copy of a deploy by supplying a signature-only
  tombstone in a new block.
- Arithmetic in merge ranking cannot wrap and reverse economic priority.
- Ambiguous canonical state fails loudly instead of allowing validators to
  report different winning blocks.
- A historical self-chain occurrence outside the selected-parent closure cannot
  mask a candidate-authorized rehome.
- Memory pressure is observed with in-flight block and process RSS metrics;
  the host-protection ceiling is not raised to conceal retention.
