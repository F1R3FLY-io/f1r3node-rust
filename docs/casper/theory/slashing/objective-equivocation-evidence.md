# Objective equivocation evidence

This document specifies how independent validators represent and authorize a
slash for two signed blocks from one validator at one sequence number. It
complements the general [slashing specification](slashing-specification.md) and
the [Casper consensus protocol](../../CONSENSUS_PROTOCOL.md).

## Terms

- A **replica** is one node's local view of the block directed acyclic graph
  (DAG).
- A **sibling pair** consists of two distinct blocks with the same sender and
  sequence number.
- A **local invalid flag** records the result of validating one block against
  one replica's then-current view. It is not portable proof.
- **Objective equivocation evidence** is a canonical sibling pair whose facts
  can be checked from immutable block metadata at every replica.
- A **canonical pair** orders its two block hashes lexicographically.

For blocks `a` and `b`, validator `v`, and sequence number `n`, the objective
relation is:

```math
\operatorname{EquivPair}(a,b,v,n) \iff
a \ne b \land
\operatorname{sender}(a)=v \land
\operatorname{sender}(b)=v \land
\operatorname{seq}(a)=n \land
\operatorname{seq}(b)=n \land n \ge 0.
```

The wire representation is the ordered pair:

```math
(\min(\operatorname{hash}(a),\operatorname{hash}(b)),
\max(\operatorname{hash}(a),\operatorname{hash}(b))).
```

## Why a local invalid hash is insufficient

Suppose replica `R1` receives `a` and then `b`, while replica `R2` receives
`b` and then `a`. A validator may legitimately classify the first arrival as
valid and the second as an equivocation. Consequently, `R1` marks `b` invalid
while `R2` marks `a` invalid. If `R1` proposes a unary slash naming only `b`,
`R2` sees the named block as locally valid and rejects the proposal. The
result is an honest-node disagreement caused by arrival order.

The repair does not select a globally invalid sibling. Both signed siblings
remain immutable historical blocks. Instead, every replica derives the same
ordered pair and validates the relation above. This preserves already accepted
descendants and finalized history while making the equivocation proof
view-independent.

## Durable discovery and restart repair

Discovery runs inside the DAG insertion critical section for every block,
including a block that completed validation before a concurrently arriving
sibling became visible. Restricting discovery to `InsertMode::Invalid` would
miss the execution in which both siblings validated against stale snapshots.

The durable index is keyed by
`(validator, bond_generation, sequence_number)` and stores the ordered set of
observed hashes. A bond generation is a validator-key incarnation: it changes
only after a completed withdrawal followed by a fresh bond, not at an ordinary
epoch boundary. Authorization first selects the canonical merged-pre-state
generation, then filters that generation's hashes to the proposed block's
activation epoch, and only then chooses the first two lexicographic hashes.
Startup reconciliation rebuilds the index from immutable certified DAG
metadata, so a process failure between metadata and secondary-index writes
cannot permanently change the proof.

The algorithm is:

```text
insert(block):
    persist immutable block metadata
    if block.sequence is nonnegative:
        add its hash to the ordered set keyed by sender, bond generation, and sequence
    update the sender's latest-message slot by (highest sequence, lowest hash)

reconcile():
    group eligible immutable metadata by (sender, bond generation, sequence)
    replace the observed-hash index with every non-empty group
    rebuild registered latest-message slots deterministically
```

Recording singleton groups makes steady-state insertion independent of DAG
size. A group becomes evidence when its second distinct hash is inserted; the
public snapshot does not expose singleton groups.

## Consensus projection

Once a validator generation has an objective pair, its latest message is
excluded from the vote projection used by fork choice and finalization while
that generation remains the active authority. A later descendant in that
generation does not restore voting weight. A later generation of the same key
is not permanently retired by old evidence; its latest message is eligible
only under the ordinary bond and validity rules. Activation-epoch filtering is
an additional slash-eligibility boundary, not the validator's identity. This
follows the generation-scoped identity decision in
[`design/15-decision-records.md`](design/15-decision-records.md).

The implementation does **not** retroactively mark both blocks invalid. Such a
rewrite could invalidate accepted descendants or finalized state. Vote
exclusion and slash evidence are monotone secondary facts over immutable block
history.

An attributable block rejected for a negative sequence is a different case.
Its certified rejection remains in DAG metadata and the invalid-block index so
the unary invalid-sequence fault is auditable and slashable. It is not inserted
into the objective-equivocation evidence index because that index is keyed by a
valid nonnegative sequence and exists to group individually admissible sibling
claims. Duplicate insertion and restart reconciliation apply the same
eligibility predicate, so a negative sequence can neither abort durable invalid
admission nor poison a sibling group.

## Proposal and receive rules

The proposer selects at most one canonical pair per offender. A pair is
eligible only when:

1. both blocks are locally available as admitted DAG metadata;
2. the objective relation holds;
3. both evidence blocks carry the active bond generation from the canonical
   merged pre-state;
4. both evidence blocks belong to the proposed block's current activation
   epoch;
5. the offender has a positive bond in that same canonical merged pre-state;
   and
6. the pair is the lexicographically least eligible pair for that offender.

The current activation epoch is computed from the actual proposed block number,
not inferred from a mutable snapshot maximum. The bond and generation maps are
loaded together from the exact merged pre-state root used by replay. A snapshot
head, LFB cache, or post-state projection cannot substitute for that authority.

Once a structural sibling group exists, a proposer never falls back to a local
unary invalid hash from that `(validator, sequence)` group. This remains true
when no same-epoch pair is eligible. An independent deterministic unary fault
at another sequence remains eligible unless an objective pair for the offender
already takes the block's one target slot. Canonicalization occurs after
grouping hashes by epoch, so a three-hash group can still yield the
lexicographically first two hashes within the current lifetime even when the
globally first hash is old. Cross-epoch siblings cannot be relabeled as
evidence for a new validator lifetime and do not permanently exclude that
later lifetime from voting.

A receiver independently verifies the following before replay:

1. the slash issuer equals the containing block's sender;
2. the target activation epoch equals the containing block's current epoch;
3. both evidence hashes are dependencies and resolve to DAG metadata;
4. the hashes are distinct and canonically ordered;
5. both blocks have the same non-empty sender and nonnegative sequence;
6. both blocks carry the deploy's target bond generation;
7. that generation equals the canonical merged-pre-state generation;
8. both evidence epochs equal the target activation epoch;
9. the offender has a positive bond at the same canonical pre-state root; and
10. the block contains no second slash for the same offender and generation.

The fifth check is enforced when evidence is indexed as well as when a receiver
validates a pair. `ObjectiveEvidenceSequenceEligibility.v` proves that a
negative certified rejection persists without pair evidence and that every
indexed item is attributable with a nonnegative sequence.

The pair is not accepted merely because it exists in an equivocation tracker.
The receiver waits for both blocks, then checks their immutable metadata. The
runtime still passes the first hash to the Proof-of-Stake contract as the
offender lookup key; both hashes identify the same sender. Replay derives the
system-deploy random seed from the canonical pair, making the seed independent
of arrival order.

## Certified causal admission context

Objective evidence is part of the immutable incoming consensus context used to
admit a candidate; it is not read from the receiver's mutable tracker. Context
construction traverses parents and exact justifications through both accepted
and rejected wrappers. Only accepted blocks propagate their stored evidence
deltas. The block hashes named by a sound proof are checked as leaf facts and do
not recursively contribute their own stored contexts.

Evidence is normalized by validator bond generation. If the closure contains
proofs at several sequence numbers for one generation, the stable evidence
ordering selects one canonical proof because one slash consumes that
generation's authority. The normalization is a commutative, associative, and
idempotent join, so opposite arrival and traversal orders produce the same
context and candidate delta.

A candidate is ready for this decision only after admitted DAG metadata exists
for the complete structural and proof dependency closure. The equivocation
tracker and derived invalid-block index cannot make a missing parent,
justification, unary evidence block, objective-pair member, or header-proof
member ready. Accepted and rejected dependencies satisfy readiness through the
same certified DAG metadata lookup.

The dependency projection and readiness decision are one shared algorithm for
direct ingress and buffered resumption:

```text
required_dependencies(block):
  dependencies := parents(block) union justifications(block)
  for each successful historical unary slash in block:
    dependencies := dependencies union {slash.invalid_block_hash}
  for each successful objective slash in block:
    dependencies := dependencies union {
      slash.first_block_hash,
      slash.second_block_hash
    }
  for each header-certified objective proof in block:
    dependencies := dependencies union {
      proof.first_block_hash,
      proof.second_block_hash
    }
  return dependencies in ascending byte order

partition_by_admitted_metadata(block, dag_snapshot):
  admitted := empty list
  missing := empty list
  for each hash in required_dependencies(block):
    if dag_snapshot.lookup(hash) returns certified metadata:
      append hash to admitted
    else:
      append hash to missing
  return (admitted, missing)

ready(block, dag_snapshot):
  (_, missing) := partition_by_admitted_metadata(block, dag_snapshot)
  return missing is empty
```

Ascending byte order is not a consensus preference; it is the canonical
iteration order of the mathematical set. It makes request scheduling and tests
reproducible without serializing validation across validators. A snapshot that
predates a concurrent metadata insertion may conservatively leave a block
buffered for one additional resolver pass, but it cannot release the block
early or disagree about which dependency identities are required.

The persisted admission outcome binds the resulting context and authority to
the block hash, protocol version, admission schema, and compiled ruleset. A
retry may reproduce only the exact same outcome. This makes the admission
decision durable without converting a node-local observation into consensus
evidence.

## Legacy unary evidence

The optional second protobuf hash is absent for unary evidence. Its empty
encoding preserves the historical byte representation and seed derivation.
Unary evidence remains valid for slashable faults that do not supply an
objective sibling pair, and it continues to require the named block's local
invalid flag. An objective pair always takes precedence, preventing the two
evidence forms from competing for one offender.

## Verification and executable conformance

The verification obligations are split by purpose:

- TLA+ and Apalache explore opposite arrival orders, concurrent successful
  admission, rejected-wrapper traversal, accepted-only propagation, proof-leaf
  isolation, per-generation canonicalization, complete dependency gating,
  ambient-tracker noninterference, canonical evidence convergence, vote
  exclusion, epoch-before-pair selection, one-root bond/generation authority,
  pair-only activation, scoped unary suppression, proposer/receiver predicate
  parity, signed-sequence evidence eligibility across two replicas, and unsafe
  controls for every omitted boundary.
- Rocq proves pair symmetry, canonicalization, authorization sufficiency,
  local-flag independence, context-join laws, exact delta classification,
  outcome identity binding, signed-sequence admission/evidence separation, and
  preservation of immutable block validity.
- Loom explores concurrent sibling insertion and opposite local
  classifications, simultaneous old-generation, old-epoch, and current-epoch
  delivery, sequence-keyed pairing, scoped unary suppression, opposite
  dependency delivery, ambient tracker races, and exact-versus-tampered outcome
  publication. It also explores metadata persistence, duplicate repair, and
  reconciliation racing at the negative-sequence boundary.
- Rust example, property, and differential fuzz tests cover storage ordering, restart
  reconciliation, protobuf round trips, dependency extraction, seed
  determinism, malformed pairs, epoch and generation boundaries, proposer
  selection, causal evidence-delta classification, the full signed `i32`
  sequence domain, and receive-side parity.

The formal artifact catalog is maintained in
[`formal/README.md`](../../../../formal/README.md), and the slashing-specific
commands are maintained in
[`slashing-verification.md`](slashing-verification.md).

## Security consequences

- Arrival order cannot make honest replicas disagree about slash authority.
- A single hash plus a local flag cannot masquerade as objective equivocation
  evidence.
- A missing evidence block causes dependency buffering, not a Byzantine-fault
  classification.
- Equal-sequence siblings discovered after parallel validation cannot retain
  voting weight in the affected validator lifetime.
- Old structural evidence cannot permanently retire a later same-key lifetime.
- Restart repairs derive from immutable metadata rather than node-local
  arrival history.
- Existing descendants and finalized state are never rewritten to manufacture
  a canonical loser.
