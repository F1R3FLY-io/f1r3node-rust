# Cutover benchmark protocol for the default RhoPath key grammar

This is a preregistration: the plan of a measurement, and the rule that turns its results into
a recommendation, both fixed before any data are collected. It was written by leaf
`rspace-key-grammar-cutover-benchmark-protocol` (14413) of pgmcp, the campaign's tracker and
experiment registry; a leaf is a tracker task without subtasks. **This document measures
nothing, and no sample exists.** It also decides nothing: once the measurement has been run,
its result is a recommendation to the campaign owner, the person who directs the campaign and
rules on its open questions.

## The question and why it is open

RSpace is the node's tuple space: it stores sent data and waiting receives per channel. Its
durable backend keeps every stored item under a byte key in a trie, a tree that stores byte
strings by sharing their common prefixes. The **key grammar** is the rule that turns a channel
into the first bytes of such a key. Two grammars exist in the source
([`rho_path.rs`](../../../rspace++/src/rspace/rho_path.rs)):

- **RhoPathV1**, the grammar in use. Its datum key carries the channel twice, as a structure
  frame and as an identity frame.
- **RhoPathV2**, a proposed grammar. Its datum key carries the channel once, as one canonical
  image of the channel.

The question is which of the two a new durable store should get by default. Experiment 487 of
pgmcp measured the keys of both grammars without running a store. Table 1 quotes its figures
for the three kinds of channel it generated.

**Table 1: figures of experiment 487 (each cell: V1, then V2, then the ratio of V2 to V1)**

| Kind of channel | Bytes of one datum prefix | Trie edges for 100,000 channels | Instructions to build one datum prefix |
| --- | --- | --- | --- |
| Unforgeable name | 232.50, 176.62, 0.7597 | 22,400,395, 14,011,708, 0.6255 | 17,100.2, 8,240.7, 0.482 |
| Tuple of an unforgeable name and a short string | 395.50, 379.63, 0.9599 | 38,500,568, 29,912,135, 0.7769 | 1,320,425.6, 13,168.6, 0.00997 |
| Large compound channel, a list of 16 triples | 6,689, 7,504, 1.122 | not measured | 63,410,521.5, 175,052.2, 0.00276 |

The byte figures of the first two rows are means over 100,000 generated channels; the third
row is one channel of that experiment's corpus. The instruction counts are that experiment's
*primary reading*: its cold reading, in which every channel is keyed for the first time, with
the larger of its two pairs of iteration counts. A *datum prefix* is the part of a datum key
that is computed from the channel, and an *instruction* is one machine instruction that the
processor executes; [Terms and notation](#terms-and-notation) and
[Secondary metrics](#secondary-metrics) define both precisely.

Three things follow. First, the effect of the grammar depends on the kind of channel: V2
prefixes are shorter for names and longer for the large compound channel. Second, the
instruction counts differ by orders of magnitude for compound channels; experiment 487
attributes 98.9 % and 99.7 % of V1's count for the tuple and for the large compound channel to
one function of the V1 grammar, `encode_trie_path` (see
[The modelled figure for the trie-path cost](#the-modelled-figure-for-the-trie-path-cost)).
Third, none of the figures is a time: the record of experiment 487 states that "Wall time is
not measured".

The pre-registered rule of experiment 487 ends with the branch "otherwise recommend making the
key grammar a parameter of the semantic profile and deciding at cutover with a workload
benchmark". The repository records that this is the experiment's outcome: the key grammar
"becomes a parameter of the store's semantic profile", and "the choice between the grammars is
made at cutover with a workload benchmark"
([Channel identity per key profile](../rspace/pathmap-bag-rspace-integration.md#channel-identity-per-key-profile)).
**This protocol is the plan of that workload benchmark.** It fixes three things before any
sample: what is measured, on which workload, and which result leads to which recommendation.

Why fix them first? A rule chosen after the data are seen can be bent, knowingly or not,
toward a hoped-for result; a rule fixed before the data cannot
([Nosek et al. 2018](#references)). The passage of the repository cited above also records an
expectation: "The owner expects RhoPathV2 to prove superior." The rule below is fixed before
any sample so that the recommendation follows from the measurements alone, whichever grammar
they favour.

## Terms and notation

Every term below is used in the rest of this document with exactly this meaning. Mathematical
symbols are defined where they are introduced. The tables are read as one glossary: a row may
use a term that another row of any of these tables defines.

**Campaign terms**

| Term | Meaning |
| --- | --- |
| pgmcp | The campaign's tracker and experiment registry. A *tracker item* is a task recorded there, cited by its number or by its public name. A *leaf* is a tracker item without subtasks. An *experiment* is a registry record that fixes a question, a hypothesis and a decision rule before the measurements it later receives; it is cited by its number. |
| campaign owner | The person who directs the campaign and rules on its open questions. |
| coordinator | The agent that assigns the campaign's leaves and decides their technical design within the campaign owner's rulings. Its decisions are not rulings of the campaign owner. |
| owner question | A question recorded for the campaign owner, which only the campaign owner can answer. Two are named in this document. **Q2** asks whether RhoPathV2 may become a persisted, selectable, non-default profile with the profile ID `RSpace.Rust.V2`. **Q5** asks whether the cost of `encode_trie_path` should be fixed, as a separate defect, before this benchmark runs. Neither is answered. |
| stop point | A tracker item at which work stops until the campaign owner has decided. Two concern the default key grammar: item 6021 (`rhopath-v2-rci2-activation-bridge`), the item for the activation of RhoPathV2, and item 7936, the cutover. |
| cutover | The step at which the durable backend would become the backend that a node uses by default (item 7936). |
| parent protocol | The preregistered comparison of two backends of RSpace, its current backend LMDB (Lightning Memory-Mapped Database, a transactional key-value store) and the durable backend: the [equal-contract LMDB versus durable DAG comparison protocol](lmdb-dag-equal-contract-comparison-protocol.md), written by leaf `rspace-dag-required-performance-control` (7949) and registered as experiment 377. DAG, directed acyclic graph, is its name for the durable backend's graph of immutable state objects. This protocol imports from it. |
| pinned commit | The commit of the repository at which this protocol was written and at which the files of the parent protocol were hashed. The manifest records it as `pinned_at_commit`. Every statement about the source in this document is a reading of the source at that commit, not the result of a run. |
| recommendation | A statement to the campaign owner of which grammar the measurements favour under the rule fixed here. It is not a decision. |
| ID, UTC, JSON | *ID* is short for identifier. *UTC* is Coordinated Universal Time, in which every time of this protocol is written. *JSON* is JavaScript Object Notation, the text format of the machine-readable files. |
| SHA-256 | The Secure Hash Algorithm with a 256-bit digest ([FIPS 180-4](#references)). The digest of a file changes when any byte of the file changes, so it identifies one exact content. |

**Tuple-space terms**

| Term | Meaning |
| --- | --- |
| RSpace | The node's tuple space: it stores data and waiting continuations per channel. |
| Rholang, Rho calculus | *Rholang* is the node's contract language. The *Rho calculus* is the process calculus it realises, in which processes communicate over names ([Meredith and Radestock 2005](#references)). |
| channel, `Par` | A *channel* is a name on which data are sent and receives wait. In the Rho calculus a name is a quoted process, so in the source a channel is a value of the process type `Par`. |
| unforgeable name | A channel that is created fresh and is identified by random bytes. Experiment 487 generates one as a `GPrivate` with a random 32-byte identifier. |
| datum | A sent value with its persistence flag. |
| waiting continuation | A receive that rests in the tuple space until data arrive: its patterns, the code to run, and its flags. |
| produce, consume | The RSpace operations that send a datum on a channel, and that install a receive on an ordered list of one or more channels. |
| COMM | One communication step of the Rho calculus, named after its reduction rule: a send meets a matching receive. In RSpace, a produce or consume call that completes a match. |
| join | A receive that waits on several channels at once. For every channel RSpace also keeps an index of the channel lists that mention it; the entries of this index are stored under *join keys*. |
| checkpoint, root | A *checkpoint* is the RSpace operation that records the current contents as a state and returns its *root*, the hash that names the state. |
| hot store | RSpace's in-memory layer holding the changes made since the last checkpoint. |
| durable backend | RSpace with its durable PathMap backend selected, as in the treatment arm of the parent protocol. PathMap is the byte-trie library that the backend is built on. Both arms of this protocol run on it. |
| trie, edge | A *trie* is a tree that stores byte strings by sharing their common prefixes. An *edge* leads from a node to a child and is labelled with one byte. |

**Key-grammar terms**

| Term | Meaning |
| --- | --- |
| key, key prefix | A *key* is the byte string under which the durable backend stores one item in its trie. There are three families: datum keys, continuation keys and join keys. The *prefix* of a key is its first part, computed from the channel alone (for a continuation key, from the ordered channel list alone). |
| key grammar | The rule that builds key prefixes from channels. |
| frame | A self-delimiting byte string inside a key ("Common framing" in the [RhoPathV2 wire grammar](../rspace/rhopath-v2-wire-grammar.md)). |
| RhoPathV1, V1 | The key grammar in use (`RhoPathV1` in [`rho_path.rs`](../../../rspace++/src/rspace/rho_path.rs)). Its datum prefix holds two frames per channel, a structure frame and an identity frame; its continuation and join prefixes hold identity frames only. |
| RhoPathV2, V2 | The proposed key grammar (`RhoPathV2` in the same file), specified in the [RhoPathV2 wire grammar](../rspace/rhopath-v2-wire-grammar.md), whose first line of text reads "Status: proposed, non-activated." It represents a channel by one frame, the channel's RCI2 image. |
| RCI2 | *Rholang Channel Image, version 2*, "the shadow logical channel-identity format" (glossary of the [consensus-change register](../../consensus/consensus-change-register.md)): one canonical byte image of a channel (`ChannelImageV2` in [`channel_image.rs`](../../../models/src/rust/channel_image.rs)). |
| `encode_trie_path` | The function that computes the structure frame of a V1 datum prefix ([`canonical_path.rs`](../../../models/src/rust/canonical_path.rs); called for a channel by `rho_path_structure` in [`rspace_path.rs`](../../../models/src/rust/rspace_path.rs)). The file that builds the V2 image does not call it. |
| semantic profile, profile ID | A *semantic profile* binds the codecs of a durable store as one inseparable set (`SemanticProfile` in [`semantic.rs`](../../../rspace++/src/rspace/persistence/semantic.rs)). Its *profile ID* is a 16-byte identifier, and that file states that "The profile ID is part of every semantic manifest". The profile ID in use is `RSpace.Rust.V1` followed by two zero bytes. `RSpace.Rust.V2` is the profile ID proposed for the same codecs with RhoPathV2 keys (owner question Q2). |
| CBR-053 | An entry of the [consensus-change register](../../consensus/consensus-change-register.md#cbr-053). CBR stands for *Consensus-Breaking Record*, the identifier prefix of every entry. CBR-053 forbids the production activation of RhoPathV2 until four conditions hold. The first, called the **channel-identity condition** here, is that "a channel-specific RhoPathV2 removes root `locally_free` from its structural identity frame"; `locally_free` is a field of `Par`, and *root* refers to the outermost `Par` of a channel. |

**Comparison terms**

| Term | Meaning |
| --- | --- |
| arm | One of the two configurations in a paired measurement. Both arms of this protocol are the real `RSpace` on the durable backend. They differ in the semantic profile of the store, through it in the key grammar, and in nothing else. |
| control, treatment | The *control* arm is the configuration in use: profile `RSpace.Rust.V1`, grammar RhoPathV1. The *treatment* arm is the proposed one: profile `RSpace.Rust.V2`, grammar RhoPathV2. |
| harness | The benchmark program that drives both arms. |
| operation list | The seeded sequence that the harness feeds an arm: produce and consume operations with their payloads, checkpoints, and resets (a reset makes the contents those of an earlier checkpoint). |
| run | One execution of one cell by one arm. |
| cell | One workload configuration that is measured, with a per-cell manifest frozen before timing: seed, operation list and settings. |
| timed, primary, secondary cell | A *timed cell* is sampled in pairs and timed. The *primary cell* is the parent protocol's `owner-primary-65536`. Every other timed cell is a *secondary cell*. |
| eligible, ineligible | An *ineligible* cell is, in the parent's words, "A cell that cannot be measured under the equal contract, or whose run instance fails preflight." It is reported as ineligible and never dropped silently. Every other cell is *eligible*. |
| workload | A family of cells. This protocol has five, W0 to W4. |
| W0 | The **deciding workload**: every timed cell of the parent protocol, with the parent's operation lists. Only W0 enters the decision rule. |
| stratum; W1, W2, W3, W4 | A *stratum* is a workload restricted to one kind of channel or of receive, so that the effect of the grammar on that kind is seen in isolation. **W1**: unforgeable-name channels. **W2**: tuple channels. **W3**: large compound channels. **W4**: receives on several channels, with their joins. The four are *diagnostic*: they are reported and enter no rule. |
| pair, paired sampling | A *pair* is one control run and one treatment run of the same cell, executed back to back in a randomized order. *Paired sampling* collects every observation in such pairs, so that a treatment value is always compared with the control value taken next to it, under the same conditions of the host. |
| witness | A check that a pair must pass before its measurements count. |
| metric, event | A *metric* is a measured quantity. Its *event* is the occurrence that it measures, for example one produce call. |
| primary metric | A metric that enters the decision rule. There are eight ([Primary metrics](#primary-metrics)). |
| secondary metric | A metric that is reported beside the result and enters no rule. There are three ([Secondary metrics](#secondary-metrics)). |
| comparison | One primary metric in one eligible timed cell of W0 in which every run performs the metric's event. The decision rule ranges over all comparisons. |
| within-pair ratio | The treatment value of a metric divided by the control value of the same pair. |
| bootstrap interval | An interval for a statistic, obtained by recomputing the statistic on many resamples of the observations, drawn with replacement ([Efron 1979](#references)). |
| equivalence margin | The largest ratio of treatment to control that is still not a material regression: 1.05. The parent protocol does not use this term; [What is imported from the parent protocol](#what-is-imported-from-the-parent-protocol) shows which number of the parent it denotes. |
| significance level | 1 percent, two-sided: the complement of the parent protocol's two-sided 99% interval. The parent protocol does not use this term either. |

## Status of the two arms

The two arms do not have the same standing, and this protocol must not blur that.

- **`RSpace.Rust.V1` is in use.** It is the profile ID of the source
  (`RSPACE_SEMANTIC_PROFILE_ID` in
  [`semantic.rs`](../../../rspace++/src/rspace/persistence/semantic.rs)), and RhoPathV1 is, in
  the words of the [RhoPathV2 wire grammar](../rspace/rhopath-v2-wire-grammar.md), "the only
  active path format".
- **`RSpace.Rust.V2` is proposed.** The identifier does not occur in the repository at the
  pinned commit. Creating durable stores under it needs the campaign owner's approval. That
  is owner question Q2, and Q2 is not answered.
- **The treatment arm cannot be built yet.** It needs three leaves that have not landed:
  `rspace-key-grammar-profile-binding`, which binds the key grammar to the semantic profile,
  and the two differential tests of the [correctness gate](#correctness-gate),
  `rspace-key-grammar-lifecycle-differential` and
  `rspace-key-grammar-durable-lifecycle-differential`.
- **Fixing the rule presumes none of these answers.** If the campaign owner does not approve
  the profile, the benchmark is not run and this protocol has no effect. Nothing in this
  document authorises creating a store under `RSpace.Rust.V2`.

## What is imported from the parent protocol

The parent protocol already fixes how two arms of the real `RSpace` are compared fairly. This
protocol does not restate that method. It **imports it by reference** from three sections of
the parent and changes only what the two arms are. Importing has two benefits: the campaign's
two benchmarks are run and analysed by one method, and no number of the method can drift
between two copies of a text.

**Table 2: the imported items**

| Imported item | Section of the parent protocol | What it fixes |
| --- | --- | --- |
| Harness and equal contract | [Equal contract and observations](lmdb-dag-equal-contract-comparison-protocol.md#equal-contract-and-observations) | Both arms are driven by one harness under one per-cell manifest frozen before timing: "Keep the same logical operation order, checkpoint frequency, retained-root policy, cache state and concurrency in both arms." |
| Operation lists | The same section, and [Required matrix and measured quantities](lmdb-dag-equal-contract-comparison-protocol.md#required-matrix-and-measured-quantities) | The cells (Tables 3 and 4 of the parent) and, for each cell, the seeded operation list and its SHA-256. |
| Durability contract | Equal contract and observations | "each arm passes the kill/reopen durability witness on its own backend before any speed comparison". |
| Retention contract | Equal contract and observations | "retention policy (keep-all in both arms)": no recorded state is ever dropped. |
| Cache contract | Equal contract and observations | "hot-store and derived-cache enablement and capacity" are frozen per cell and equal in both arms. |
| Concurrency contract | Equal contract and observations | "worker count and CPU affinity" are frozen per cell and equal in both arms. |
| Host rules | [Paired sampling and decision rule](lmdb-dag-equal-contract-comparison-protocol.md#paired-sampling-and-decision-rule) | The preparation of the measurement host: an idleness check before each pair, the same processor cores for both arms, a hard memory ceiling without swap, and "Timed paired runs have no profiler attached." |
| Paired sampling | Paired sampling and decision rule | The number of pairs, the randomized and balanced order within a pair, one retained run per pair ("never rerun a pair to select a favorable result"), and the bootstrap interval. |
| Witnesses | Equal contract and observations | The three work-equivalence witnesses and each arm's kill/reopen durability witness ([How the imported witnesses read here](#how-the-imported-witnesses-read-here)). |

**Table 3: the numbers taken from the parent, each quoted with its section**

| Number | Quotation | Section of the parent protocol |
| --- | --- | --- |
| 51 pairs in the primary cell | "For the primary cell use 51 complete pairs of separate control and treatment runs." | Paired sampling and decision rule |
| 30 pairs in every other timed cell | "Use 30 complete pairs for each predeclared secondary cell." | Paired sampling and decision rule |
| 100,000 resamples, seed 20261001, 99% interval | "Use 100,000 pair-preserving bootstrap resamples with recorded seed `20261001`, reporting a two-sided 99% interval for that median." | Paired sampling and decision rule |
| Margin 1.05 | "the lower 99% throughput-ratio bound must be at least 0.95 and the upper 99% ratio for p99 latency at most 1.05 to exclude a material regression" | Paired sampling and decision rule |
| The same 5 percent for sizes | "a greater-than-5% physical-footprint or peak-RSS regression in a critical cell is a review blocker" | Paired sampling and decision rule |
| 4,096 mutations and 64 checkpoints per run of the primary cell | "Each primary replicate runs exactly 4,096 mutations and 64 checkpoints after the same untimed initial-state construction" | Equal contract and observations |
| The configuration of the primary cell | "One worker; 128-byte payload; 64 changed paths per checkpoint; unchanged current objects 4,096/65,536/1,048,576; 16 retained roots with 95% sharing; checkpoint every 64 mutations" and "The 65,536-object cell is the single primary decision cell." | Required matrix and measured quantities (Table 3 of the parent) |

In these quotations p99 is the 99th percentile, and RSS is the resident set size, the physical
memory that a process holds.

**The equivalence margin and the significance level.** The rule of this protocol speaks of the
parent's *equivalence margin* and of its *significance level*. The parent uses neither term.
It has exactly one magnitude of margin, 5 percent (the fourth and fifth rows of Table 3), and
exactly one confidence level, the two-sided 99% of the third row. Every primary metric of this
protocol is a time or a size, for which lower is better, so the margin takes the form the
parent gives it for a latency: the upper bound of the interval for the ratio of treatment to
control must be at most 1.05. The significance level is the complement of the confidence
level: 1 percent, two-sided.

**What is not imported.**

- *The parent's own decision rule.* Its throughput improvement claim and its list of critical
  sentinels answer the parent's question. This protocol has its own rule
  ([Decision rule](#decision-rule)) and takes from that passage only the margin and the
  confidence level.
- *Settings of LMDB.* The parent freezes them per cell. They have no object here, because no
  arm of this protocol uses LMDB.
- *Cells the parent marks not applicable.* The parent's Table 5 marks a component lane, its
  garbage-collection cells and a retention-eviction case as not applicable before sampling.
  They are not timed cells of the parent, so they are not part of W0. The reasons carry over:
  the component lane drives neither arm's `RSpace`, and under the imported keep-all retention
  neither arm reclaims or evicts a state.
- *No new trace.* The parent states that "traces that contain two orders of one channel set are
  excluded from every cell, timed and untimed", because its control arm cannot run them. The
  operation lists of W0 are the parent's, and the lists of the diagnostic strata are generated
  by the parent's rules, so the exclusion is inherited as it stands. Lifting it would change
  the imported contract and is not proposed here.

### How the imported witnesses read here

The parent labels its three work-equivalence witnesses W1, W2 and W3. In this document the
letter W followed by a digit always names a workload, so the witnesses are called here by the
names that the parent's manifest gives them.

**Table 4: the imported witnesses, with two key grammars on one backend**

| Witness | Compared | Passes when |
| --- | --- | --- |
| `same_operation_list` | Across arms | Both arms executed the same operation list: the SHA-256 of the executed list is equal. |
| `own_validity` | Within each arm | A replay of the arm's own event log, and an export of the arm's state imported into a fresh store, give the arm's own results, contents and root. |
| `choice_free_contents_digest` | Across arms, in timed cells | The contents digest, which the harness computes outside the store from the values read through `RSpace`, is equal in both arms at every checkpoint position. |
| Kill/reopen durability witness | Within each arm | After the process is killed and the store is reopened, the arm recovers its own last acknowledged root and the contents recorded there. |

One phrase of the parent needs a reading. For `own_validity` the parent imports the exported
state into "a fresh store of the same backend". Both arms of this protocol run on one backend,
so the phrase is read as: a fresh store of the arm's own profile. The witness never leaves its
arm, in the parent and here.

The parent's list of what is **never compared across arms** stays in force: roots, orders,
chosen COMMs and state-sync bytes (the bytes of a state exported to another store). For roots
there is a reason of this protocol's own: the profile ID is part of every semantic manifest of
a store, and the two arms have different profile IDs and different keys, so their stored bytes
differ by design. A difference in a quantity that is never compared is not a failure. A
mismatch of a witness is, in the parent's words, "a correctness failure of the pair, not an
outlier".

### The pin

Importing by reference would be unsafe if the parent could change unnoticed. The
[control manifest](rhopath-grammar-cutover-control-manifest.json) of this protocol therefore
records two things: the SHA-256 that the three files of the parent (its protocol, its control
manifest and its negative fixtures) have at the pinned commit, and the literal headings of the
three imported sections. The checker
([Machine-readable freeze and its checker](#machine-readable-freeze-and-its-checker))
recomputes the three digests from the working tree and checks that each heading still exists
in the parent protocol. A difference is refused with its own error code: `parent_pin_mismatch`,
whose message names the file, or `parent_heading_missing`.

The digests are not repeated in this document; the manifest is their single source. From the
root of the repository they are recomputed with:

```bash
sha256sum docs/design/pathmap/lmdb-dag-equal-contract-comparison-protocol.md \
          docs/design/pathmap/lmdb-dag-equal-contract-control-manifest.json \
          docs/design/pathmap/lmdb-dag-equal-contract-negative-fixtures.json
```

## If the parent protocol is amended

This protocol keeps importing the content that is pinned, not whatever the files of the parent
hold later. When a file of the parent changes, the checker fails with `parent_pin_mismatch`
until this protocol is **re-pinned** by a recorded amendment of its own: an entry in the
[amendment record](#amendment-record) and the new digests in the manifest. While no sample
batch is recorded in the manifest, a re-pin needs no further ceremony. Once a sample batch is
recorded, this protocol offers no re-pin: changing the imported contract after samples exist
is a question for the campaign owner.

One fact is recorded here without judgement. An amendment of the parent protocol is drafted
and awaits the campaign owner; it concerns the parent's cross-backend witnesses. A re-pin may
therefore be needed before the first sample.

## Arms

**Table 5: the two arms**

| Arm | Profile ID | Key grammar | Backend | Status at the pinned commit |
| --- | --- | --- | --- | --- |
| Control | `RSpace.Rust.V1` | RhoPathV1 | Durable backend | In use |
| Treatment | `RSpace.Rust.V2` | RhoPathV2 | Durable backend | Proposed; awaits owner question Q2; cannot be built yet |

Everything else is equal in both arms: the source revision, the harness, the operation list of
each cell, the settings of the host, and the durability, retention, cache and concurrency
contract. A profile ID other than these two is not an arm of this protocol. Because the
backend is the same, a difference between the arms is a difference made by the key grammar:
the bytes of the keys, the shape of the trie that they form, and the work of building them.

## Workloads

**Table 6: the five workloads**

| Workload | Role | Channels | Receives | Source |
| --- | --- | --- | --- | --- |
| W0 | Deciding | As in the parent's operation lists | As in the parent's operation lists | Every timed cell of the parent protocol |
| W1 | Diagnostic | Unforgeable names | On one channel | Experiment 487, its set of unforgeable names |
| W2 | Diagnostic | Tuples of an unforgeable name and a short string | On one channel | Experiment 487, its set of tuples |
| W3 | Diagnostic | Large compound channels: lists of 16 triples of a fresh name, a short string and an integer | On one channel | Experiment 487, its large channel |
| W4 | Diagnostic | Unforgeable names | On two channels in one cell, on three channels in a second cell | Experiment 487, its continuation and join prefixes |

### W0, the deciding workload

W0 is the set of timed cells of the parent protocol (the column "Timed cells" of its Table 4),
each with the operation list that the parent freezes for it. The primary cell of W0 is the
parent's primary cell.

W0 decides for two reasons. Its operation lists were fixed for another question, the
comparison of two backends, so they were not chosen with either key grammar in mind. And with
one workload for both benchmarks, the two decisions that meet at the cutover, the backend and
the key grammar, rest on the same operation lists.

### W1 to W4, the diagnostic strata

W0 mixes kinds of channel, and Table 1 shows that the grammar acts differently on each kind. A
result on W0 alone would say which grammar is recommended, but not why. The strata answer the
why: each isolates one kind that experiment 487 examined, so that the report can say which
kind drives a difference seen on W0.

- **W1, W2 and W3** take the three kinds of channel of Table 1.
- **W4** takes receives on several channels. Experiment 487 found smaller differences there
  than for datum prefixes: for unforgeable names it reports, as V1 / V2 bytes, continuation
  prefixes of 199 / 187 on one channel, 390 / 358 on two and 581 / 529 on three, and join
  prefixes of 199 / 178, a ratio of 0.894. The reason is in the source: V1 builds continuation
  and join prefixes from identity frames only (`continuation_prefix` and `join_prefix` in
  [`rho_path.rs`](../../../rspace++/src/rspace/rho_path.rs)).

Each diagnostic cell is the parent's primary cell (last row of Table 3) with one change: the
population of channels for W1, W2 and W3, and the number of channels of every receive for the
two cells of W4. There are five diagnostic cells. Their operation lists are generated by the
parent's rules, each is sampled with 30 pairs, the parent's number for a secondary cell, and
each pair passes the same witnesses. A diagnostic cell that exceeds its recorded resource cap
is reported as such, as the parent requires: "A resource-capped or unsupported large cell is
a recorded failure/limit; it must not silently disappear."

The strata do not decide, because each is one kind of channel in isolation, which is not a
workload that the campaign has chosen to stand for a node. A rule over them would let one kind
overturn the result of the chosen workload. They explain; W0 decides.

## Metrics

Every value below is the value of one run.

### Primary metrics

**Table 7: the eight primary metrics (for each, lower is better)**

| Primary metric | Event | Value of one run | Unit |
| --- | --- | --- | --- |
| Produce wall time | A produce call that completes no COMM | Mean elapsed time of the events | Seconds per call |
| Consume wall time | A consume call that completes no COMM | Mean elapsed time of the events | Seconds per call |
| COMM wall time | A produce or consume call that completes a COMM | Mean elapsed time of the events | Seconds per call |
| Checkpoint publication time | An acknowledged checkpoint call | Mean elapsed time of the events | Seconds per checkpoint |
| Durable bytes written per checkpoint | An acknowledged checkpoint call | Physical bytes written in the timed interval, divided by the number of events | Bytes per checkpoint |
| Hot-store resident bytes | A checkpoint position, read immediately before the checkpoint call | Largest growth of the live heap since immediately before the store was opened | Bytes |
| Open time | Opening the store after an orderly shutdown | Mean elapsed time of the events | Seconds per open |
| Recovery time | Opening the store after the process was killed during a write | Mean elapsed time of the events | Seconds per recovery |

**Timing metrics.** Let a run perform $`k \ge 1`$ events of a timing metric, with elapsed times
$`d_1, \ldots, d_k`$ in seconds. The harness reads each from a monotonic clock immediately
before the call and immediately after it returns. The value of the run is the arithmetic mean

```math
x = \frac{1}{k} \sum_{j=1}^{k} d_j .
```

**Durable bytes written per checkpoint.** Let $`P`$ be the physical bytes written to the files
of the store during the timed interval of the run, and $`q \ge 1`$ the number of acknowledged
checkpoints in it. The value of the run is

```math
x = \frac{P}{q} .
```

$`P`$ is the quantity that the parent calls "total physical write bytes" (Required matrix and
measured quantities).

**Hot-store resident bytes.** Let $`h_0`$ be the live heap bytes of the process immediately
before the store is opened, and $`h_j`$ the live heap bytes immediately before the $`j`$-th
checkpoint call, for $`1 \le j \le q`$. The value of the run is

```math
x = \max_{1 \le j \le q} \left( h_j - h_0 \right) .
```

Live heap bytes are the bytes requested from the allocator and not yet freed. They are counted
by a counting allocator that wraps the system allocator, the instrument of experiment 487's
heap measurement ("Counting `#[global_allocator]` over `System` (requested bytes, not
allocator overhead)").

**Why these definitions.**

- *Eight metrics.* The leaf names five quantities. "Wall time per produce, consume and COMM"
  holds three events, and "open and recovery time" holds two, which gives eight.
- *Three kinds of call.* A call that completes a COMM does other work than a call that only
  stores its datum or its continuation. Mixing them would make a metric depend on the share
  of matches in an operation list. The first three metrics split the produce and consume
  calls of a run into disjoint classes.
- *The mean.* The leaf asks for the wall time per operation, which is the total time divided
  by the number of operations. The percentiles of latency that the parent records for every
  cell stay recorded; they are not primary metrics here.
- *Checkpoint publication.* The elapsed time of an acknowledged checkpoint call includes what
  the parent calls "the DAG's successful WAL/pack/catalog publication and required
  synchronization" (WAL is the write-ahead log).
- *Hot-store resident bytes.* The hot store keeps no count of its own bytes: a search of
  [`hot_store.rs`](../../../rspace++/src/rspace/hot_store.rs) for such a counter finds none.
  The metric is therefore defined by an instrument that both arms share. It is read
  immediately before a checkpoint, when the hot store holds a whole interval of changes. It
  counts everything that the store holds on the heap at that moment, of which the hot store
  is a part, so it bounds the hot store's bytes from above. The counting allocator is part of
  the harness, the same code in both arms, and not a profiler attached to a run.
- *Open and recovery.* The parent's publication and lifecycle lane has a "clean and torn
  restart". Open time is measured at a clean restart and recovery time at a torn one.

**Where a metric is defined.** A metric is *defined* in a cell when the frozen per-cell
manifest makes every run of the cell perform at least one event of the metric. Opening a store
after an orderly shutdown or after a kill is a step of a cell's procedure, not an operation of
an operation list. The set of comparisons is written down from the frozen per-cell manifests
before the first pair is collected. It is never chosen after results are seen.

### Secondary metrics

**Table 8: the three secondary metrics, by the methods of experiment 487**

| Secondary metric | What is computed | Method |
| --- | --- | --- |
| Key bytes | The mean length in bytes of the key prefixes that a grammar builds for the channels and channel lists of a cell's operation list, for each key family | Build every prefix under each grammar and take its length. |
| Distinct trie edges | The number of edges of the byte trie that holds those prefixes, for each key family | The sum $`E`$ below. |
| Instructions per key construction | The number of machine instructions executed to build one prefix, for each key family | The slope $`\hat{c}`$ below. |

**Distinct trie edges.** Sort the $`n`$ prefixes bytewise as $`k_{(1)} < \cdots < k_{(n)}`$.
Then

```math
E = \sum_{i=1}^{n} \Bigl( \lvert k_{(i)} \rvert - \operatorname{lcp}\bigl(k_{(i)}, k_{(i-1)}\bigr) \Bigr), \qquad k_{(0)} = \varepsilon ,
```

where $`\lvert k \rvert`$ is the length of $`k`$ in bytes, $`\operatorname{lcp}(a, b)`$ is the
length of the longest common prefix of $`a`$ and $`b`$, and $`\varepsilon`$ is the empty byte
string. Each prefix adds the bytes that it does not share with its sorted neighbour, so
$`E`$ is the number of edges of the uncompressed byte trie that holds the prefixes.

**Instructions per key construction.** A program builds the prefix $`N`$ times and, in a
second run, $`2N`$ times, each time under cachegrind, a tool of Valgrind
([Nethercote and Seward 2007](#references)) that counts the machine instructions a process
executes, with its cache simulation switched off. With $`I(n)`$ the total count of a run that
builds the prefix $`n`$ times,

```math
\hat{c} = \frac{I(2N) - I(N)}{N} .
```

The start of the process and its set-up are the same work in both runs, so they cancel. These
are separate diagnostic runs: the parent admits "never a profiled run as a timing replicate".

The secondary metrics are deterministic and do not depend on the load of the host. They help
to explain a difference in a primary metric, for example shorter keys behind fewer bytes
written. They are not what a node spends its time on, and they enter no rule.

## Correctness gate

A benchmark of two arms that do different work measures nothing. Before any pair is
collected, two differential tests must pass at the measured source revision:

- `rspace-key-grammar-lifecycle-differential`, and
- `rspace-key-grammar-durable-lifecycle-differential`.

Each is named after the leaf that delivers it. As the names say, each is to drive a store
under either grammar through the same lifecycle and to compare what the two return. Neither
test exists at the pinned commit, and their content is fixed by their own leaves, not here.

If the differentials record a **divergence of the V1 arm**, a case in which the two grammars
do not behave alike, that record goes to the campaign owner first. No comparison is made until
the campaign owner has decided what the divergence means for it. The reason is the same: this
protocol can say which grammar is faster or smaller, not which one is right.

## Paired sampling and the statistic

**Pairs.** Cell $`c`$ is sampled with $`n_c`$ pairs: 51 in the primary cell, and 30 in every
other timed cell of W0 and in every diagnostic cell (Table 3). Order within a pair, fresh
directories, the preparation of the host and the rule that no pair is rerun are imported
(Table 2).

**The statistic of one comparison.** Take a primary metric $`m`$ and a cell $`c`$. For pair
$`i`$, with $`1 \le i \le n_c`$, let $`x^{\mathrm{V1}}_{m,c,i}`$ be the value of the control
run and $`x^{\mathrm{V2}}_{m,c,i}`$ the value of the treatment run. Both are positive: every
metric of Table 7 is a positive time or a positive number of bytes on a run that performs its
events. The within-pair ratio is

```math
r_{m,c,i} = \frac{x^{\mathrm{V2}}_{m,c,i}}{x^{\mathrm{V1}}_{m,c,i}} .
```

Lower is better for every primary metric, so a ratio below 1 favours the treatment and a ratio
above 1 favours the control. The estimate is the median of the ratios,

```math
R_{m,c} = \operatorname{median}_{1 \le i \le n_c} \; r_{m,c,i} .
```

**The interval.** Draw $`B = 100{,}000`$ resamples with a generator seeded with 20261001.
Resample $`b`$ draws $`n_c`$ of the $`n_c`$ pairs with replacement, keeping every pair intact,
and $`R^{*b}_{m,c}`$ is the median of its ratios. With $`Q_p`$ the quantile of order $`p`$,
the bounds are

```math
L_{m,c} = Q_{0.005}\bigl(R^{*1}_{m,c}, \ldots, R^{*B}_{m,c}\bigr), \qquad
U_{m,c} = Q_{0.995}\bigl(R^{*1}_{m,c}, \ldots, R^{*B}_{m,c}\bigr) .
```

The range from $`L_{m,c}`$ to $`U_{m,c}`$ is the two-sided 99% bootstrap interval of the
median. The existing
[paired statistics helper](../../../scripts/bench/paired_benchmark_statistics.py) computes
these three numbers in its function `paired_bootstrap_median_of_ratios`.

**Why this statistic.** Pairing cancels the slow drifts of the host, because both runs of a
pair meet the same conditions. The median is not moved by a few disturbed pairs. And it is the
parent's statistic, so both benchmarks of the campaign are read in the same way.

## Decision rule

The rule is evaluated on **measured figures only**, and on **W0 only**. Let
$`\delta = 1.05`$ be the equivalence margin.

### Classification of one comparison

**Table 9: the three classes of a comparison**

| Class | Condition | Meaning |
| --- | --- | --- |
| Worse beyond the margin | $`U_{m,c} > \delta`$ | A regression of more than 5 percent is not excluded. |
| Better | $`U_{m,c} < 1`$ | The treatment is better at the significance level: the whole interval lies below 1. |
| Equivalent | $`1 \le U_{m,c} \le \delta`$ | A regression of more than 5 percent is excluded, and an improvement is not shown. |

Every comparison falls into exactly one class, because the three conditions split the possible
values of $`U_{m,c}`$ into three ranges that do not overlap and that leave no gap.

### The three outcomes

Let $`S`$ be the set of all comparisons $`(m, c)`$, and

```math
U_{\max} = \max_{(m,c) \in S} U_{m,c}, \qquad U_{\min} = \min_{(m,c) \in S} U_{m,c} .
```

The outcome is

```math
\text{outcome} =
\begin{cases}
1 & \text{if } U_{\max} > \delta, \\
2 & \text{if } U_{\max} \le \delta \text{ and } U_{\min} < 1, \\
3 & \text{if } U_{\max} \le \delta \text{ and } U_{\min} \ge 1 .
\end{cases}
```

**Table 10: the rule in words and as conditions**

| Outcome | The rule in words | Condition | Recommendation |
| --- | --- | --- | --- |
| 1 | Some primary metric of V2 is worse than V1's beyond the margin. | $`U_{\max} > \delta`$ | V1 |
| 2 | No primary metric of V2 is worse beyond the margin, and at least one is better at the significance level. | $`U_{\max} \le \delta`$ and $`U_{\min} < 1`$ | V2, on the measurement |
| 3 | All are equivalent. | $`U_{\max} \le \delta`$ and $`U_{\min} \ge 1`$ | V2, by the tie-break: it is the grammar that the channel-identity condition of CBR-053 calls for |

**Exactly one outcome applies.** If $`U_{\max} > \delta`$, outcome 1 applies and neither of
the others. Otherwise $`U_{\max} \le \delta`$, and then exactly one of $`U_{\min} < 1`$
(outcome 2) and $`U_{\min} \ge 1`$ (outcome 3) holds. In outcome 3 every $`U_{m,c}`$ lies
between $`U_{\min} \ge 1`$ and $`U_{\max} \le \delta`$, so every comparison is equivalent in
the sense of Table 9.

### When there is no recommendation

The report gives no recommendation, states the reason, and does not fall back to a weaker
rule, in three cases:

1. the correctness gate has not passed;
2. some primary metric is defined in no eligible timed cell of W0, so that the rule would
   pass over that metric in silence;
3. a pair of runs fails a witness, which is a correctness failure and not an outlier.

### Why the rule has this form

- **The burden of proof lies on the treatment.** A comparison counts as worse whenever a
  regression beyond the margin is *not excluded*, which is the parent's own form ("to exclude
  a material regression") and the usual form of a test of equivalence by the inclusion of an
  interval ([Schuirmann 1987](#references)). A wide interval, from a noisy cell or from too
  few pairs, therefore counts against the treatment. That is intended: the grammar in use
  stays the recommendation unless the proposed one is shown to be no worse.
- **Every comparison must pass, and no correction is needed for that.** V2 is recommended
  only if it is shown to be no worse in every comparison. A conclusion that requires every
  single test to succeed keeps the error level of one test
  ([Berger 1982](#references)): the chance of recommending V2 although some comparison is
  truly worse beyond the margin is at most that of one bound, about half a percent, as far as
  the bootstrap interval attains its nominal level.
- **"At least one better" is not corrected for the number of comparisons.** With many
  comparisons, one of them can show an improvement by chance. This changes only the reason
  that is reported, outcome 2 or outcome 3, and never the grammar that is recommended. The
  report states how many comparisons there are and how many are better.
- **Readings that were not chosen.** "Worse beyond the margin" could also be read as
  $`R_{m,c} > \delta`$, the estimate alone, or as $`L_{m,c} > \delta`$, a regression that is
  demonstrated. The report gives $`R_{m,c}`$, $`L_{m,c}`$ and $`U_{m,c}`$ for every
  comparison, so the campaign owner can see what either reading would have given.
- **The tie-break.** When the measurements do not separate the grammars, the rule names V2
  because of the channel-identity condition of CBR-053. The repository records, by reading and
  untested, that "RhoPathV1 datum keys separate equal names that differ in root
  `locally_free`" (row C4 of the table in
  [Evidence and gaps](../rspace/pathmap-bag-rspace-integration.md#evidence-and-gaps)). A
  recommendation is not an activation: CBR-053 keeps all four of its conditions, and this
  protocol changes none of them.

### What goes to the campaign owner

The report holds the recommendation and the number of its outcome; the set $`S`$; the values
$`R_{m,c}`$, $`L_{m,c}`$ and $`U_{m,c}`$ of every comparison; every ineligible cell with its
reason; the same three values for every primary metric in every diagnostic cell; the
secondary metrics; and the modelled figure
([The modelled figure for the trie-path cost](#the-modelled-figure-for-the-trie-path-cost)),
labelled as a model.
It goes to the campaign owner at stop points 6021 and 7936. The campaign owner decides the
default key grammar. **This protocol decides nothing.**

## The whole flow in one diagram

**Diagram 1: from the registration of the rule to the report**

```text
  ┌────────────────────────────────────────────────────────────────────────────────────────────┐
  │ BEFORE ANY SAMPLE                                                                          │
  │ · the rule is registered as an experiment in the registry, and its ID is recorded here     │
  │ · the parent protocol is pinned by SHA-256: three files, three section headings            │
  │ · the campaign owner has approved the treatment profile (owner question Q2), and the       │
  │   treatment arm is built (leaves profile-binding and the two lifecycle differentials)      │
  └───────────────────────────────┬────────────────────────────────────────────────────────────┘
                                  ▼
  ╭────────────────────────────────────────────────────────────────╮
  │ CORRECTNESS GATE                                               │
  │ Do both lifecycle differentials pass, with no recorded         ├── no ──▶ to the campaign
  │ divergence of the V1 arm?                                      │          owner first; no
  ╰───────────────────────────────┬────────────────────────────────╯          comparison is made
                                  │ yes
                                  ├────────────────────────────────────────────────┐
                                  ▼                                                ▼
  ┌────────────────────────────────────────────────────────────────┐  ┌────────────────────────┐
  │ W0, THE DECIDING WORKLOAD: the parent protocol's operation     │  │ W1 to W4, DIAGNOSTIC   │
  │ lists, run by both arms on the durable backend                 │  │ STRATA                 │
  │   control arm:   profile RSpace.Rust.V1 (grammar RhoPathV1)    │  │ W1 unforgeable names   │
  │   treatment arm: profile RSpace.Rust.V2 (grammar RhoPathV2)    │  │ W2 tuple channels      │
  │ Paired runs in random order: 51 pairs in the primary cell,     │  │ W3 large compound      │
  │ 30 pairs in every other timed cell. After every pair the       │  │    channels            │
  │ imported witnesses are checked; a failed witness stops the     │  │ W4 receives on 2 and   │
  │ comparison.                                                    │  │    3 channels (joins)  │
  └───────────────────────────────┬────────────────────────────────┘  │ 30 pairs each. Also:   │
                                  ▼                                   │ the three secondary    │
  ┌────────────────────────────────────────────────────────────────┐  │ metrics and the        │
  │ FOR EVERY PRIMARY METRIC m AND EVERY ELIGIBLE TIMED CELL c     │  │ modelled figure for    │
  │   r(i)   = value under V2 / value under V1, in pair i          │  │ encode_trie_path.      │
  │   R(m,c) = median of the r(i)                                  │  │ They enter no rule.    │
  │   L(m,c), U(m,c) = bounds of the two-sided 99% bootstrap       │  └────────────┬───────────┘
  │                    interval of R(m,c)                          │               │
  └───────────────────────────────┬────────────────────────────────┘               │
                                  ▼                                                │
  ╭────────────────────────────────────────────────────────────────╮               │
  │ Is the LARGEST upper bound above the margin?                   │               │
  │   maximum of U(m,c) over all comparisons (m,c)  >  1.05        │               │
  ╰─────────┬────────────────────────────────┬─────────────────────╯               │
        yes │                                │ no                                  │
            │           ╭────────────────────┴────────────────────╮                │
            │           │ Is the SMALLEST upper bound below 1?    │                │
            │           │   minimum of U(m,c)  <  1               │                │
            │           ╰─────────┬─────────────────────┬─────────╯                │
            │                 yes │                     │ no                       │
            ▼                     ▼                     ▼                          │
  ┌───────────────────┐ ┌───────────────────┐ ┌───────────────────┐                │
  │ OUTCOME 1         │ │ OUTCOME 2         │ │ OUTCOME 3         │                │
  │ recommend V1:     │ │ recommend V2:     │ │ recommend V2 by   │                │
  │ some metric is    │ │ none worse, at    │ │ tie-break: all    │                │
  │ worse beyond the  │ │ least one better  │ │ equivalent        │                │
  │ margin            │ │                   │ │                   │                │
  └─────────┬─────────┘ └─────────┬─────────┘ └─────────┬─────────┘                │
            └─────────────────────┼─────────────────────┘                          │
                                  ▼                                                ▼
  ┌────────────────────────────────────────────────────────────────────────────────────────────┐
  │ REPORT TO THE CAMPAIGN OWNER at stop points 6021 and 7936                                  │
  │ the recommendation and the number of its outcome · every R, L and U · every ineligible     │
  │ cell · the diagnostic strata · the modelled figure, labelled as a model                    │
  │ The campaign owner decides the default grammar. This protocol decides nothing.             │
  └────────────────────────────────────────────────────────────────────────────────────────────┘
```

Legend: square boxes are steps and rounded boxes are decisions; an arrow leads from each to
the next. `m` is a primary metric and `c` an eligible timed cell of W0. `r(i)` is the
within-pair ratio of pair `i`, `R(m,c)` its median over the pairs, and `L(m,c)` and `U(m,c)`
the lower and the upper bound of the interval
([Paired sampling and the statistic](#paired-sampling-and-the-statistic)). The number 1.05 is
the equivalence margin. The box on the right runs beside the left column and meets it only at
the report: nothing in it reaches a decision.

## The modelled figure for the trie-path cost

**The concern.** Table 1 shows that building a V1 datum prefix for a compound channel takes
far more instructions than building the V2 prefix, and experiment 487 attributes the
difference to one function, `encode_trie_path`. Owner question Q5 asks whether that cost
should be fixed, as a separate defect, before this benchmark runs. If it is not fixed, the
control arm is measured with the cost in it, and a reader of the result should be able to see
how much of a difference may be owed to a cost that a fix could remove.

**The facts at the pinned commit.** Q5 is not answered, and no fix has landed. The second
statement is a reading of [`canonical_path.rs`](../../../models/src/rust/canonical_path.rs),
not a measurement. `encode_trie_path` (line 1014) returns the first component of
`encode_trie_path_with_stability`, which creates a new encoder state on every call (lines 1032
to 1041). The two decodes that experiment 487 names are present: `entry.decode()` in
`split_carrier_list` (line 410) and `.first::<Expr>()` in `encode_segment` (line 768). A
case-insensitive search of the file for `memo`, `cache`, `OnceLock`, `OnceCell` and
`thread_local` finds two comment lines and no cache.

**What enters the rule.** Nothing of this section. The decision rule is evaluated on measured
figures only. The figure defined here is a labelled diagnostic for the campaign owner.

**The definition.** Experiment 487 measured `encode_trie_path` alone beside the whole
construction of a V1 datum prefix. For a stratum $`w`$, let $`a_w`$ be the instructions of
`encode_trie_path` alone and $`b_w`$ those of the whole construction, both from that
experiment's primary reading (the note under Table 1). The **cost share** of the stratum is

```math
s_w = \frac{a_w}{b_w} .
```

**Table 11: the cost shares of experiment 487**

| Stratum | $`a_w`$: `encode_trie_path` alone | $`b_w`$: the whole V1 construction | Share $`s_w`$ | The share as experiment 487 states it |
| --- | --- | --- | --- | --- |
| W1, unforgeable names | 6,274.0 | 17,100.2 | 0.3669 | 37 % |
| W2, tuples | 1,306,000.9 | 1,320,425.6 | 0.9891 | 98.9 % |
| W3, large compound channels | 63,238,710.5 | 63,410,521.5 | 0.9973 | 99.7 % |

Let $`\hat{c}^{\mathrm{V1}}_w`$ and $`\hat{c}^{\mathrm{V2}}_w`$ be the values of the secondary
metric *instructions per key construction* that this benchmark measures for the datum prefix
on stratum $`w`$, in the control and in the treatment arm. The **modelled control figure** is

```math
\tilde{c}^{\mathrm{V1}}_w = \hat{c}^{\mathrm{V1}}_w \, (1 - s_w) .
```

Beside the measured ratio $`\hat{c}^{\mathrm{V2}}_w / \hat{c}^{\mathrm{V1}}_w`$ the report
gives the modelled ratio $`\hat{c}^{\mathrm{V2}}_w / \tilde{c}^{\mathrm{V1}}_w`$.

**It is a model, not a measurement.** It rests on three assumptions: that the share which
experiment 487 measured on its own channels holds for the channels of the stratum and for the
measured revision; that a fix removes the whole cost of the function, which is the most that a
fix can remove, so the model is the case most favourable to V1; and that nothing else changes.
It counts instructions, and instruction counts are not wall time: the model says nothing about
a primary metric, and no primary metric is modelled.

**Where no share exists.** Experiment 487 measured the split for the datum prefixes of the
three kinds of Table 11 only. It has no share for the channels of W0. W4 adds nothing to
model: its channels are those of W1, and the continuation and join prefixes that set it apart
are built under V1 without `encode_trie_path`. For W0 and W4 the control arm is reported as
measured only, and the report says so. A share is never borrowed from another kind of channel.

**When the figure is omitted.** If a fix of the cost of `encode_trie_path` has landed in the
measured revision, the modelled figure is omitted and the report says so. The measured control
arm then already holds the fix, and applying the share would count it twice.

**An illustration.** Table 12 applies the formula to experiment 487's own figures, to make it
concrete. Its rows are arithmetic on Table 11. They are not results of this protocol, and they
are not times.

**Table 12: the formula applied to the figures of experiment 487 (instructions per datum prefix)**

| Stratum | V1 as measured | V1 as modelled | V2 as measured | Measured ratio | Modelled ratio |
| --- | --- | --- | --- | --- | --- |
| W1 | 17,100.2 | 10,826.2 | 8,240.7 | 0.482 | 0.761 |
| W2 | 1,320,425.6 | 14,424.7 | 13,168.6 | 0.00997 | 0.913 |
| W3 | 63,410,521.5 | 171,811.0 | 175,052.2 | 0.00276 | 1.019 |

## Registration

The decision rule of this protocol is registered in pgmcp as experiment 489, at `2026-11-12T19:23:19Z`, before any sample is taken.

The coordinator registers the rule. This document and the manifest both record the ID of the
experiment and the time of its registration, and the checker requires the two records to
agree. Sampling is recorded in the manifest as *sample batches*, each with its time in UTC.
The checker refuses a manifest in which a sample batch is not later than the registration
(`rule_registered_after_sample`). It compares recorded times only and never reads a clock.

## Machine-readable freeze and its checker

Three files freeze this protocol in a form that a program can check:

- the [control manifest](rhopath-grammar-cutover-control-manifest.json), which holds the arms,
  the workloads, the metrics, the correctness gate, the sample plan, the decision rule, the
  modelled figure, the registration and the pin of the parent protocol;
- the [negative fixtures](rhopath-grammar-cutover-negative-fixtures.json), eight mutations of
  the manifest, each of which must be refused;
- the [checker](../../../scripts/bench/check-rhopath-grammar-cutover-manifest.py).

The checker is run by hand from the root of the repository, without arguments:

```bash
python3 scripts/bench/check-rhopath-grammar-cutover-manifest.py
```

It uses the Python standard library only, opens no network connection, reads no clock and
writes no file. It checks the manifest against the plan described in this document,
recomputes the pin of the parent protocol, checks that this document records the manifest's
experiment ID and registration time, replays every negative fixture, and runs a list of
built-in probes for the checks that have no fixture. It exits with status 0 and ends with a
line that states how many fixtures and probes were refused. On the first failure it prints one
line with a named error code on standard error and exits with status 1.

**Table 13: the eight negative fixtures**

| Mutation of the manifest | Error code |
| --- | --- |
| An arm is dropped | `arm_dropped` |
| A workload is dropped | `workload_dropped` |
| W0 is not the deciding workload | `deciding_workload_drift` |
| A primary metric is dropped | `primary_metric_dropped` |
| The correctness gate is removed | `correctness_gate_missing` |
| A sample is recorded that is not later than the registration of the rule | `rule_registered_after_sample` |
| A profile ID other than those of the two arms | `profile_id_not_an_arm` |
| A sample plan smaller than the parent protocol's | `sample_plan_smaller_than_parent` |

A fixture passes only if the checker refuses the mutated manifest with exactly the code of its
row. A refusal with another code is itself a failure, so a check cannot be removed unnoticed
while another check happens to refuse the same mutation.

## What this document does not claim

- **No measurement and no verdict.** No benchmark was run for this document. The figures of
  experiment 487 are quoted, not measured again.
- **No approval.** It does not say that the campaign owner has approved RhoPathV2 or the
  profile `RSpace.Rust.V2`, and it answers no owner question.
- **No decision.** Its rule yields a recommendation. The default key grammar is decided by the
  campaign owner at stop points 6021 and 7936.
- **No activation.** It changes no condition of CBR-053 and no byte of any key.

## Amendment record

This is revision 0. No amendment has been made.

## References

- R. L. Berger. "Multiparameter Hypothesis Testing and Acceptance Sampling". *Technometrics*
  24(4):295–300, 1982.
  [doi:10.1080/00401706.1982.10487790](https://doi.org/10.1080/00401706.1982.10487790).
- B. Efron. "Bootstrap Methods: Another Look at the Jackknife". *The Annals of Statistics*
  7(1):1–26, 1979. [doi:10.1214/aos/1176344552](https://doi.org/10.1214/aos/1176344552).
- L. G. Meredith and M. Radestock. "A Reflective Higher-order Calculus". *Electronic Notes in
  Theoretical Computer Science* 141(5):49–67, 2005.
  [doi:10.1016/j.entcs.2005.05.016](https://doi.org/10.1016/j.entcs.2005.05.016).
- National Institute of Standards and Technology. *Secure Hash Standard (SHS)*. Federal
  Information Processing Standards Publication 180-4, August 2015.
  [doi:10.6028/NIST.FIPS.180-4](https://doi.org/10.6028/NIST.FIPS.180-4).
- N. Nethercote and J. Seward. "Valgrind: A Framework for Heavyweight Dynamic Binary
  Instrumentation". *Proceedings of the 28th ACM SIGPLAN Conference on Programming Language
  Design and Implementation*, pages 89–100, 2007.
  [doi:10.1145/1250734.1250746](https://doi.org/10.1145/1250734.1250746).
- B. A. Nosek, C. R. Ebersole, A. C. DeHaven and D. T. Mellor. "The preregistration
  revolution". *Proceedings of the National Academy of Sciences* 115(11):2600–2606, 2018.
  [doi:10.1073/pnas.1708274114](https://doi.org/10.1073/pnas.1708274114).
- D. J. Schuirmann. "A comparison of the Two One-Sided Tests Procedure and the Power Approach
  for assessing the equivalence of average bioavailability". *Journal of Pharmacokinetics and
  Biopharmaceutics* 15(6):657–680, 1987.
  [doi:10.1007/BF01068419](https://doi.org/10.1007/BF01068419).
