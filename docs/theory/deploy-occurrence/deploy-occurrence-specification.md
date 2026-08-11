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

A **source block** $`b`$ is the block whose `body.deploys` contains a processed
copy of $`d`$.

A **deploy occurrence** is the pair

```math
o = (d, b).
```

Two occurrences may have the same deploy identifier and different source
blocks. They are not the same protocol event.

A **rejection tombstone** is

```math
t = (d, b, r),
```

where $`r`$ is a reason: merge conflict, duplicate occurrence, or collateral
chain drop. A tombstone removes only the exact occurrence $`(d,b)`$.

A **canonical projection** is the deterministic reduction of the occurrences
visible from a finalized DAG view into active and rejected occurrences.

## Required invariants

### O1 — source identity

Every newly created rejection record MUST contain both deploy identifier and
source block hash. Signature-only records are accepted only as legacy wire data.
They MUST NOT be interpreted as proof that every source occurrence was rejected.

### O2 — exact rejection

For observations $`O`$ and exact tombstones $`T`$, active occurrences are

```math
A(O,T) = O \setminus \{(d,b) \mid (d,b,r) \in T\}.
```

A tombstone for $`(d,b_1)`$ cannot remove $`(d,b_2)`$ when $`b_1 \ne b_2`$.

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

Finalization status and `/api/deploy` MUST use the same canonical disposition.
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

## Reference reducer

```text
function reduce(observations, canonical_rejection_records):
    tombstoned_sources := exact source hashes from canonical rejection records
    active := observations whose source hash is not tombstoned
    active := sort active by descending height, then ascending block hash

    if active contains more than one finalized occurrence for one deploy:
        return DeployDispositionAmbiguity
    if active contains one occurrence:
        return Failed if its execution failed, otherwise Finalized
    return Expired when its lifespan elapsed, otherwise Pending
```

## Compatibility

The protobuf additions use new field numbers, so old records decode with an
empty source hash and an unspecified reason. Validation retains a legacy path
for blocks that contain only legacy records. New blocks always emit provenance.
An upgraded database can keep its old singular deploy index; new valid blocks
populate the occurrence index, and legacy lookup remains the fallback.

## Security consequences

- Invalid blocks are not added to the occurrence index.
- A peer cannot erase every copy of a deploy by supplying a signature-only
  tombstone in a new block.
- Arithmetic in merge ranking cannot wrap and reverse economic priority.
- Ambiguous canonical state fails loudly instead of allowing validators to
  report different winning blocks.
- Memory pressure is observed with in-flight block and process RSS metrics;
  the host-protection ceiling is not raised to conceal retention.
