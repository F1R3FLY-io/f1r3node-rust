# Cordial Miners Glossary

> This glossary is **load-bearing** for the Cordial Miners replication
> mechanism. Design records and reviews cite its anchors. The vocabulary
> travels with the mechanism if the mechanism moves to its own repository.

The source is the paper by Idit Keidar, Oded Naor, Ouri Poupko, and Ehud
Shapiro, "Cordial Miners: Fast and Efficient Consensus for Every Eventuality",
[arXiv:2205.09174](https://arxiv.org/abs/2205.09174), version 6 of
22 September 2023. Section and definition numbers below refer to that version.
Cordial Miners is cited only and not implemented in this repository.

Boundary terms stay in [docs/Glossary.md](../Glossary.md). Each entry below
pins one canonical name to a **Preferred usage** statement.

## Canonical Terms

### Cordial Miners

Cordial Miners is a family of Byzantine Atomic Broadcast protocols, with
instances for asynchrony and for eventual synchrony. The protocols do not use
Reliable Broadcast. They use the blocklace for dissemination, equivocation
exclusion, and ordering.

**Preferred usage.** Use as the mechanism name. Its
[mechanism identifier](../Glossary.md#mechanism-identifier) is `cordial`.
*Avoid*: "F1r3fly consensus" for Cordial Miners until an adapter exists.

### Miner

A miner is a participant of the protocol. There are `n >= 3` miners, and at
most `f < n/3` of them are faulty (section 2).

**Preferred usage.** Use for a Cordial Miners participant.
*Distinguish from* a CBC Casper validator, which has stake.

### Blocklace

A blocklace is a partially ordered counterpart of the blockchain. Each block
can hold a finite set of hash pointers to earlier blocks (section 4). A
blocklace is a set of blocks and induces a DAG.

**Preferred usage.** Use "blocklace" for the Cordial Miners structure.
*Avoid*: "block DAG" when the blocklace is meant.

### Block

A block holds a payload, a set of hash pointers to earlier blocks, and the
signature of its creator (section 4.1, Definition 14). A block of miner `p` is
a p-block.

**Preferred usage.** Use for the Cordial Miners commit unit.

### Observes

Block `b` observes block `b'` if a path of pointers leads from `b` to `b'`
(Definition 16). "Observes" is the transitive closure of "acknowledges".

**Preferred usage.** Use "observes" for reachability in the blocklace.

### Equivocation

Two blocks of the same miner equivocate if neither observes the other
(Definition 17). The miner is then an equivocator. Equivocation is a fault
for which the miner can be held accountable.

**Preferred usage.** Use with this exact rule.
*Distinguish from* CBC Casper and Casanova equivocation, which their
glossaries define.

### Approves

Block `b` approves block `b'` if `b` observes `b'` and observes no block that
equivocates with `b'` (Definition 18). Approval is not transitive.

**Preferred usage.** Use "approves" only in this sense.

### Depth

The depth of a block is the length of the longest path from the block
(Definition 20). Depth is also called the round of the block.

**Preferred usage.** Use "depth" in definitions and "round" for groups of
blocks.

### Round

A round of a blocklace is the set of blocks with the same depth (section 3).
Each correct miner creates one block in each round.

**Preferred usage.** Use with a number, for example "round r".
*Distinguish from* a [peer-clique round](../peer-clique/GLOSSARY.md#round) and
a [Casanova round](../casanova/GLOSSARY.md#round).

### Supermajority

A supermajority is a set of blocks from more than `(n + f) / 2` miners
(section 4.2, Definition 21). When `f = 0`, a supermajority is a simple
majority.

**Preferred usage.** Use with the `(n + f) / 2` threshold.

### Cordial dissemination

Cordial dissemination follows one principle: send to others the blocks that
you know and think they need (section 3). Each new block acts as an
acknowledgement of the blocks it observes and, by omission, as a request for
the blocks it does not observe.

**Preferred usage.** Use for the Cordial Miners dissemination rule.

### Cordial block

A cordial block acknowledges blocks from at least a supermajority of miners
(Definition 25). A cordial blocklace holds only cordial blocks. A correct
miner waits for round `r` to reach a supermajority before it creates a block
in round `r + 1`.

**Preferred usage.** Use "cordial block" and "cordial round".

### Ratifies

Block `b` ratifies block `b'` if the closure of `b` holds a supermajority of
blocks that approve `b'`. A set of blocks super-ratifies `b'` if it holds a
supermajority of blocks that ratify `b'` (Definition 22).

**Preferred usage.** Use "ratifies" and "super-ratifies" only in these senses.

### Wave

A wave is a fixed number of consecutive rounds, called the wavelength
(Definition 23). The eventual synchrony instance uses 3 rounds in a wave. The
asynchronous instance uses 5.

**Preferred usage.** Use with the instance, for example "a wave of the
asynchronous instance".

### Leader block

A leader selection function chooses one miner as the leader of each wave. A
leader block is a block of the leader in the first round of the wave
(section 4.2).

**Preferred usage.** Use "leader block" for that block.

### Final leader block

A leader block is final if the blocklace prefix up to the last round of its
wave super-ratifies it (Definition 24). A final leader block anchors the
ordering function τ.

**Preferred usage.** Use "final leader block". A final leader block gives the
[finality event](../Glossary.md#finality-event) of the blocks that τ orders
from it.
*Avoid*: "anchor" for this role in F1r3fly documents, because "anchor" means a
Bitcoin or Lightning commitment there.

### Tau ordering

The ordering function τ is deterministic. It uses final leader blocks to sort
the blocklace topologically into a sequence and excludes equivocations
(section 5).

**Preferred usage.** Write "the τ ordering" or "τ". Map τ onto content
ordering and merge, never onto RSpace reduction.

### Eventual synchrony

In eventual synchrony (ES), a Global Stabilization Time (GST) exists. After
GST, messages between correct miners arrive within a known bound (section 2).

**Preferred usage.** Write "eventual synchrony (ES)" and "Global
Stabilization Time (GST)" at first use.
