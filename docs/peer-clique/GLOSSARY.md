# RGB Peer Clique Glossary

> This glossary is **load-bearing** for the RGB peer-clique replication
> mechanism and its Bitcoin and Lightning anchoring. Design records and
> reviews cite its anchors. The vocabulary travels with the mechanism if
> the mechanism moves to its own repository.

The source documents are in the Rholang-RGB repository: `docs/sow-2.md`
(workstream WS5), `docs/design/architecture.md`, `docs/design/peer-clique-consensus.md`,
and `docs/glossary.md`. Those documents are drafts. This glossary follows
them and changes when they change.

Boundary terms such as [replication medium](../Glossary.md#replication-medium),
[commit](../Glossary.md#commit), and [finality event](../Glossary.md#finality-event)
stay in [docs/Glossary.md](../Glossary.md). Each entry below pins one
canonical name to a **Preferred usage** statement.

## Canonical Terms

### Peer clique

A peer clique is a small Byzantine fault tolerant group of peers, typically
three to seven, that replicates the state of one shard. The members execute
the same transactions with Rholang and RSpace++ and agree on each commit.

**Preferred usage.** Use for the replication group of an RGB shard.
*Distinguish from* the CBC Casper [clique oracle](../casper/GLOSSARY.md),
which is a finality rule, not a group of peers.
*Avoid*: "clique" alone in documents that also discuss CBC Casper.

### Peer-clique consensus

Peer-clique consensus is the replication mechanism of WS5. It orders one
transaction per commit inside a peer clique and finalizes each commit with a
quorum certificate. It replaces the legacy RGB partially synchronized state
machine inside a shard.

**Preferred usage.** Use as the mechanism name. Its
[mechanism identifier](../Glossary.md#mechanism-identifier) is `peer-clique`.
*Avoid*: "RGB consensus", because RGB anchoring is a separate step.

### Per-transaction commit

A per-transaction commit applies exactly one transaction. The replicated log
is a sequence of these commits, from height 0 upward. Conflicting
transactions resolve by order, because only one transaction commits at each
height.

**Preferred usage.** Use when the commit unit matters.
*Distinguish from* a CBC Casper block, which applies many deploys and can
cite many parents.

### Height

The height is the position of a commit in the peer-clique log. Height 0 holds
the starting state. Each later commit increments the height by one.

**Preferred usage.** Use for the peer-clique log position.
*Avoid*: "block number" for the peer clique.

### Round

A round is one attempt to finalize a commit at a given height. A round ends
with a quorum certificate or with a timeout that starts the next round.

**Preferred usage.** Use with a height, for example "height 12, round 2".
*Distinguish from* a [Cordial Miners round](../cordial-miners/GLOSSARY.md#round)
and a [Casanova round](../casanova/GLOSSARY.md#round), which mean other things.

### Epoch

An epoch is a period with a fixed committee. Membership changes only at an
epoch boundary.

**Preferred usage.** Use for the committee period of the peer clique.
*Distinguish from* a CBC Casper epoch, which is a bond and reward period
defined in `PoS.rhox`.

### Epoch start

An epoch start (`EPOCH_START`) is a finalized commit that binds the
verification context for the next epoch. The context holds the issuer, the
shard, the epoch identifier, the committee root, and the starting state root. The issuer
proposes it, and a threshold of the committee must finalize it.

**Preferred usage.** Use for the commit that opens an epoch.
*Avoid*: "reconfiguration". Membership changes inside one mechanism. The
mechanism of a shard never changes.

### Committee root

The committee root (`committee_root`) identifies the members that are
eligible to sign during an epoch. An example is a Merkle root of the sorted
member public keys.

**Preferred usage.** Use for the epoch membership identifier.

### Engaged root

The engaged root (`engaged_root`) identifies the subset of the committee that
is eligible to sign for one transaction. The quorum certificate is checked
against this subset.

**Preferred usage.** Use when the signing set is narrower than the committee.

### Issuer

The issuer, also called the initiator, is the Smart Asset identity that
maintains the canonical state tree of the asset and starts each epoch.

**Preferred usage.** Use "issuer". Write "issuer (initiator)" at first use.

### RGB state root

The RGB state root (`rgb_state_root`) is the root of the issuer's canonical
state tree after a commit. Catch-up, parent pointers
(`prev_rgb_state_root`), and state comparisons use it.

**Preferred usage.** Use for the asset state identifier.
*Distinguish from* the RSpace post-state hash, which is the execution
agreement object of the [replication boundary](../designs/replication-boundary.md).
How the two identifiers relate is an open question of that design.

### Commit hash

The commit hash (`commit_hash`) is a deterministic identifier of a
transition. It binds the shard, the issuer, the epoch, the height, and the
round. It also binds the committee and engaged roots, the pre-state and
post-state roots, the transaction, and the hashes of the validity artifacts.

**Preferred usage.** Use for the value that the clique signs.

### Quorum certificate

A quorum certificate (QC) is a threshold of member signatures over a commit
hash. It is the finality evidence of a peer-clique commit.

**Preferred usage.** Write "quorum certificate (QC)" at first use.
*Distinguish from* the CBC Casper fault tolerance value, which is a different
form of finality evidence.

### Anchor commitment

An anchor commitment is an immutable commitment that is published on Bitcoin
layer 1 or in a Lightning channel state to prove a state transition. In this
mechanism an anchor commitment follows a finalized commit. The
[anchor port](../Glossary.md#anchor-port) publishes it.

**Preferred usage.** Use "anchor commitment" for the published commitment and
"anchor" as a verb.
*Distinguish from* the repository term [Anchor](../Glossary.md#anchor), which
is a test-net node role.
*Avoid*: using "anchor" for finality. A commit is final before its anchor
commitment exists.

### Single-use seal

A single-use seal is a commitment that can be closed exactly once. RGB binds a
seal to a Bitcoin unspent transaction output (UTXO). The spend of that output
closes the seal over one message.

**Preferred usage.** Use "single-use seal" and "seal close".

### Witness transaction

A witness transaction is the Bitcoin transaction that spends a sealed output
and so proves the seal close.

**Preferred usage.** Use for the closing transaction.

### Deterministic Bitcoin Commitment

A Deterministic Bitcoin Commitment (DBC) embeds a commitment in a Bitcoin
transaction so that the same inputs always give the same output. RGB uses two
forms: `opret`, an output with `OP_RETURN` data, and `tapret`, a commitment in
a Taproot script path.

**Preferred usage.** Write "Deterministic Bitcoin Commitment (DBC)" at first
use. Name the form, `opret` or `tapret`, when it matters.

### Lightning channel anchor

A Lightning channel anchor puts the commitment in a Lightning channel state
update instead of a layer 1 transaction. The channel state update closes the
previous seal and defines the next one.

**Preferred usage.** Use for anchoring through a Lightning state channel.
*Distinguish from* a layer 1 anchor, which needs a confirmed witness
transaction.

## Coalition Structure Mapping

The [semitopology glossary](../semitopology/GLOSSARY.md) states the
coalition vocabulary of the boundary. Section 16 of the
[replication boundary design](../designs/replication-boundary.md) proposes
the `CoalitionStructure` type.

| Semitopology term | Peer-clique term |
|---|---|
| Point | Member of the epoch committee ([committee root](#committee-root)) |
| [Actionable coalition](../Glossary.md#actionable-coalition) | The signer set of a [quorum certificate](#quorum-certificate) inside the [engaged root](#engaged-root) of one transaction |
| [Witness set](../semitopology/GLOSSARY.md#witness-function) | One signer set that the threshold of an engaged subset accepts |
| [Intertwined](../semitopology/GLOSSARY.md#intertwined) | Not yet shown. Question 5 in section 17 of the design asks for the threshold and the engaged-subset rule |
| `CoalitionStructure` form | `Witness`, because the engaged subset changes for each transaction |
