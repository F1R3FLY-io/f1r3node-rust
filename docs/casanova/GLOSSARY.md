# Casanova Glossary

> This glossary is **load-bearing** for the Casanova replication
> mechanism. Design records and reviews cite its anchors. The vocabulary
> travels with the mechanism if the mechanism moves to its own repository.

The source is the paper by Kyle Butt, Derek Sorensen, and Michael Stay,
"Casanova", [arXiv:1812.02232](https://arxiv.org/abs/1812.02232), version 2
of 30 April 2019. Section numbers below refer to that version. Casanova is
specified and not implemented in this repository.

Boundary terms stay in [docs/Glossary.md](../Glossary.md). Each entry below
pins one canonical name to a **Preferred usage** statement.

## Canonical Terms

### Casanova

Casanova is a leaderless optimistic consensus protocol for a permissioned
blockchain. It produces blocks in a directed acyclic graph (DAG) and combines
voting rounds with block production. It votes only on conflicting
transactions.

**Preferred usage.** Use as the mechanism name. Its
[mechanism identifier](../Glossary.md#mechanism-identifier) is `casanova`.

### Validator

A validator is a network node that runs the protocol (section 2.3). A correct
validator is not Byzantine (section 2.6).

**Preferred usage.** Use for a Casanova participant.
*Distinguish from* a block, which is a node of the DAG.

### Block

A block is a node of the Casanova DAG. Each block refers to one or more parent
blocks and holds transactions. In the full protocol a block also holds votes
(Algorithm 4).

**Preferred usage.** Use for the Casanova commit unit.

### Equivocation

A validator equivocates when it produces a block that is not a descendant of
its own most recent block (section 2.3). Equivocation is a fault.

**Preferred usage.** Use with this exact rule.
*Distinguish from* CBC Casper equivocation, which the
[Casper glossary](../casper/GLOSSARY.md#equivocation) defines.

### Conflict domain

The set of possible events is partitioned into mutually exclusive
alternatives `E_i` (section 2.2). Each alternative set is a conflict domain.
At most one event of a conflict domain can occur.

**Preferred usage.** Use for the set that a decision chooses from.

### Conflicting transaction

Events in a conflict domain that holds more than one event are transactions.
Two transactions conflict when they belong to the same conflict domain, for
example a double spend.

**Preferred usage.** Use "conflicting transaction".

### Non-Faulty Majority

The Non-Faulty Majority (NFM) is `ceil((N - f + 1) / 2)` validators, where `N`
is the number of validators and `f` is the number of Byzantine validators
(section 2.6).

**Preferred usage.** Write "Non-Faulty Majority (NFM)" at first use.

### Fault Tolerant Majority

The Fault Tolerant Majority (FTM) is `ceil((N + f + 1) / 2)`, which equals
`NFM + f` (section 2.6). Any FTM contains at least one validator of the NFM.

**Preferred usage.** Write "Fault Tolerant Majority (FTM)" at first use.
*Avoid*: other expansions of "FTM".

### Score

The score of a block, as computed by validator `V`, is the set of validators
whose blocks are the block or descendants of it (section 4.1). With a weight
function, the score becomes a weight.

**Preferred usage.** Use with the computing validator, for example "the score
as seen by V".

### k-observed set

A k-observed set for a block is a set of validators `S` with two properties
(section 4.2). The observed score of each member for the block is at least
`k`, and the total weight of `S` is at least `k`. An FTM-observed set is the case `k = FTM`.

**Preferred usage.** Use "FTM-observed set" for the decision condition.

### Round

A round is a fixed number of blocks in the conflict resolution of one conflict
domain (section 5.2). Round 0 starts when a validator meets the conflict.
Round `r` starts `r(r+3)/2` blocks after round 0.

**Preferred usage.** Use with the conflict domain.
*Distinguish from* a [peer-clique round](../peer-clique/GLOSSARY.md#round) and
a [Cordial Miners round](../cordial-miners/GLOSSARY.md#round).

### Lock

A lock is internal validator state on a value, with an associated round
(section 5.2). A validator can release a lock only to take a lock with a later
round.

**Preferred usage.** Use "lock on a transaction in round r".

### Conflict exclusion protocol

The conflict exclusion protocol (Casanova-conflict-exclude, section 5.1) lets
attestations decide a conflict when an FTM-observed set exists, so that no
separate consensus run is necessary. Conflicting transactions do not delay
non-conflicting transactions in the same block.

**Preferred usage.** Use the paper term.
*Avoid*: "line-item veto" as a term. The architecture note uses that phrase
as a description. The paper does not use it.

### Decision

Validator `V` decides on transaction `e_i` when its DAG contains an
FTM-observed set that voted for `e_i` in some round (section 5.3).

**Preferred usage.** Use "decides on" for Casanova finality. A decision is the
[finality event](../Glossary.md#finality-event) of the conflict domain.

### Partial synchrony

Liveness needs partial synchrony: the network delivers each message within a
finite bound that nobody needs to know. Safety holds in a fully asynchronous
network (section 2.1).

**Preferred usage.** Use when you state the Casanova network model.
