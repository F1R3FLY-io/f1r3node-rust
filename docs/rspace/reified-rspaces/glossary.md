# Reified RSpaces glossary

This glossary defines the terms of the Reified RSpaces campaign.
Each term has a stable anchor, so other documents link a term as `glossary.md#<anchor>`.
The anchor is the GitHub slug of the term heading: the heading in lowercase, with each space replaced by a hyphen.

The entries appear in dependency order.
Each definition uses only common words and the terms of earlier entries.
[Part 1](#part-1-background-terms) defines the background terms of Rholang, RSpace, and the verification tools.
[Part 2](#part-2-campaign-terms) defines the terms that the campaign introduces.
The repository glossary, [docs/Glossary.md](../../Glossary.md), defines the general project terms.
The [decision register](decision-register.md) records the decisions that fix the meaning of the campaign terms.

## Index

| Term | Part | Term | Part |
|---|---|---|---|
| [Acceptance category](#acceptance-category) | 2 | [Native backend](#native-backend) | 2 |
| [Accounting observer](#accounting-observer) | 1 | [Native play](#native-play) | 2 |
| [Admission verdict](#admission-verdict) | 2 | [Native replay](#native-replay) | 2 |
| [Advisory query](#advisory-query) | 2 | [Native space](#native-space) | 2 |
| [Agent](#agent) | 1 | [Negative control](#negative-control) | 2 |
| [Allocation job](#allocation-job) | 2 | [Net write](#net-write) | 2 |
| [Allocator](#allocator) | 2 | [Op channel](#op-channel) | 2 |
| [Binding](#binding) | 2 | [Operation order](#operation-order) | 2 |
| [Block](#block) | 1 | [Outcome](#outcome) | 2 |
| [Body channel](#body-channel) | 2 | [Outer store](#outer-store) | 2 |
| [Bundle](#bundle) | 1 | [Pairing](#pairing) | 2 |
| [Canonical form](#canonical-form) | 2 | [Par](#par) | 1 |
| [Canonical group](#canonical-group) | 2 | [Param rule](#param-rule) | 2 |
| [Canonical order](#canonical-order) | 2 | [Path](#path) | 2 |
| [Capacity](#capacity) | 2 | [Path marker](#path-marker) | 2 |
| [Cascade](#cascade) | 2 | [Pattern](#pattern) | 1 |
| [Cell](#cell) | 2 | [Peek](#peek) | 1 |
| [Channel](#channel) | 1 | [Persistent](#persistent) | 1 |
| [Checkpoint](#checkpoint) | 1 | [pgmcp](#pgmcp) | 1 |
| [Class fire](#class-fire) | 2 | [Phlo](#phlo) | 1 |
| [Class key](#class-key) | 2 | [Play](#play) | 1 |
| [Collection](#collection) | 2 | [Prefix atomicity](#prefix-atomicity) | 2 |
| [Collection counter](#collection-counter) | 2 | [Prefix source](#prefix-source) | 2 |
| [Collection key](#collection-key) | 2 | [Process](#process) | 1 |
| [COMM event](#comm-event) | 1 | [Produce counter](#produce-counter) | 2 |
| [Config record](#config-record) | 2 | [Property-based test](#property-based-test) | 1 |
| [Consensus-preservation checklist](#consensus-preservation-checklist) | 2 | [Qualifier](#qualifier) | 2 |
| [Conservative extension](#conservative-extension) | 2 | [Random state](#random-state) | 1 |
| [Constructor schema](#constructor-schema) | 2 | [Re-issue](#re-issue) | 2 |
| [Continuation](#continuation) | 1 | [Re-offer](#re-offer) | 2 |
| [Counter meta-channel](#counter-meta-channel) | 2 | [Re-rooting](#re-rooting) | 2 |
| [Crossed stacks](#crossed-stacks) | 2 | [Re-store](#re-store) | 2 |
| [Datum](#datum) | 1 | [Receive](#receive) | 1 |
| [Deploy](#deploy) | 1 | [Recognized position](#recognized-position) | 2 |
| [Dirty collection](#dirty-collection) | 2 | [Reconciler](#reconciler) | 2 |
| [Dyadic number](#dyadic-number) | 2 | [Record envelope](#record-envelope) | 2 |
| [Eligible set](#eligible-set) | 2 | [Record id](#record-id) | 2 |
| [Embedding](#embedding) | 2 | [Recorded process](#recorded-process) | 2 |
| [Epoch closure](#epoch-closure) | 2 | [Reduction participant](#reduction-participant) | 2 |
| [Evaluation scope](#evaluation-scope) | 2 | [Registered family](#registered-family) | 2 |
| [Event log](#event-log) | 1 | [Regression corpus](#regression-corpus) | 2 |
| [Exact cut](#exact-cut) | 2 | [Reified RSpace](#reified-rspace) | 2 |
| [Exact value](#exact-value) | 2 | [Replay](#replay) | 1 |
| [F1r3lang](#f1r3lang) | 1 | [Reporting](#reporting) | 1 |
| [Factory](#factory) | 2 | [Representative](#representative) | 2 |
| [Factory capability](#factory-capability) | 2 | [Retention](#retention) | 2 |
| [Factory row](#factory-row) | 2 | [Rholang](#rholang) | 1 |
| [FIPS](#fips) | 1 | [Rocq](#rocq) | 1 |
| [Fire count](#fire-count) | 2 | [Rounding to nearest](#rounding-to-nearest) | 2 |
| [Fire round](#fire-round) | 2 | [Routing class](#routing-class) | 2 |
| [First admitting waiter](#first-admitting-waiter) | 2 | [RSpace](#rspace) | 1 |
| [Footprint policy](#footprint-policy) | 2 | [Scan discipline](#scan-discipline) | 2 |
| [Frontend](#frontend) | 1 | [Scan driver](#scan-driver) | 2 |
| [Gate](#gate) | 2 | [Second send](#second-send) | 2 |
| [getSpaceAgent](#getspaceagent) | 2 | [Selection order](#selection-order) | 2 |
| [Handle](#handle) | 2 | [Send](#send) | 1 |
| [Handle capability](#handle-capability) | 2 | [Seq claim](#seq-claim) | 2 |
| [Head record](#head-record) | 2 | [Settle](#settle) | 2 |
| [Head waiter](#head-waiter) | 2 | [Space](#space) | 2 |
| [Head-only matching](#head-only-matching) | 2 | [Space agent](#space-agent) | 2 |
| [Headline](#headline) | 2 | [Space counter](#space-counter) | 2 |
| [History trie](#history-trie) | 1 | [Space family](#space-family) | 2 |
| [Host operation](#host-operation) | 2 | [Space id](#space-id) | 2 |
| [Host work](#host-work) | 1 | [Space intent](#space-intent) | 2 |
| [Hot store](#hot-store) | 1 | [Space name](#space-name) | 2 |
| [Index channel](#index-channel) | 2 | [Space reference](#space-reference) | 2 |
| [Index name](#index-name) | 2 | [Space transaction](#space-transaction) | 2 |
| [Inductive invariant](#inductive-invariant) | 1 | [Staged view](#staged-view) | 2 |
| [Inert](#inert) | 2 | [Storage channel](#storage-channel) | 2 |
| [Inner collection](#inner-collection) | 2 | [Store identity](#store-identity) | 2 |
| [Internal operation](#internal-operation) | 2 | [Sub-order](#sub-order) | 2 |
| [Invariant map](#invariant-map) | 2 | [Sweep](#sweep) | 2 |
| [Join](#join) | 1 | [System process](#system-process) | 1 |
| [Join engine](#join-engine) | 2 | [Tested prefix](#tested-prefix) | 2 |
| [Join scope](#join-scope) | 2 | [Theory](#theory) | 2 |
| [Join slot](#join-slot) | 2 | [Theory identity](#theory-identity) | 2 |
| [Journal row](#journal-row) | 2 | [Ticket](#ticket) | 2 |
| [KIND](#kind) | 2 | [Times-repeated](#times-repeated) | 2 |
| [Lane](#lane) | 2 | [TLA+](#tla) | 1 |
| [Legacy charge](#legacy-charge) | 2 | [Token](#token) | 2 |
| [Level](#level) | 2 | [Trigger event](#trigger-event) | 2 |
| [LFS](#lfs) | 2 | [Tuple space](#tuple-space) | 1 |
| [Lift](#lift) | 2 | [Typed reply](#typed-reply) | 2 |
| [List mode](#list-mode) | 2 | [Unforgeable name](#unforgeable-name) | 1 |
| [List order](#list-order) | 2 | [Unit](#unit) | 2 |
| [LIST-CASCADE](#list-cascade) | 2 | [Unpaired fire](#unpaired-fire) | 2 |
| [Lock coverage](#lock-coverage) | 2 | [URN](#urn) | 1 |
| [Lock stripe](#lock-stripe) | 1 | [Use context](#use-context) | 2 |
| [Marker](#marker) | 2 | [User agent-backed space](#user-agent-backed-space) | 2 |
| [Marker tuple](#marker-tuple) | 2 | [VectorDB collection](#vectordb-collection) | 2 |
| [Matcher](#matcher) | 2 | [Vehicle](#vehicle) | 2 |
| [Merge](#merge) | 1 | [Verification recipe](#verification-recipe) | 2 |
| [Metered view read](#metered-view-read) | 2 | [Volatile channel](#volatile-channel) | 2 |
| [Mirror](#mirror) | 2 | [Waiter](#waiter) | 2 |
| [Mirror journal](#mirror-journal) | 2 | [Watermark](#watermark) | 2 |
| [Mirror writer](#mirror-writer) | 2 | [Widest kind](#widest-kind) | 2 |
| [Model map](#model-map) | 2 | [Witness test](#witness-test) | 2 |
| [Name](#name) | 1 | [Write skew](#write-skew) | 2 |
| [Namespace tag](#namespace-tag) | 2 |  |  |

## Part 1: background terms

### Rholang

Rholang is the smart-contract language of the node.
It is a concurrent language, and the reflective higher-order calculus is its formal basis [[MR05](#ref-mr05)].
The [Rholang language overview](../../rholang/01-language-overview.md) introduces the language.

### Process

A process is a Rholang term that runs.
Several processes can run at the same time.
The term `P | Q` runs the two processes `P` and `Q` in parallel.

### Name

A name is a quoted process.
Rholang writes the name of the process `P` as `@P`, and `*x` runs the process that the name `x` quotes [[MR05](#ref-mr05)].
Processes send data on names, and they receive data from names.

### Channel

A channel is a name that a process uses to send or to receive.
The Reifying RSpaces proposal uses "channel" and "name" as synonyms [[FIPS-RRS](#ref-fips-rrs)].

**Preferred usage.** Use "channel" when the text is about stored items at a name.
Use "name" when the text is about a value that a process passes or quotes.

### Unforgeable name

An unforgeable name is a name that no Rholang source text can write.
Only the `new` construct creates one.
The node represents an unforgeable name as a `GPrivate` value that holds an id, which is a byte string.
An ordinary `new` name has a 32-byte id.

### Par

A Par is the normalized form of a process inside the node.
A Par is a Protocol Buffers message, which is a binary record format [[PB](#ref-pb)].
Its byte encoding is called its prost bytes, after the Rust library that writes them.
The node computes channel hashes and state hashes from Par values.
So a new field in Par would change every hash ([decision 18](decision-register.md#decision-18)).

### Send

A send `x!(d)` offers the data `d` on the channel `x`.
The [send section of the Rholang guide](../../rholang/08-channels-and-concurrency.md#send) describes the send forms.

### Receive

A receive `for (y <- x) { P }` waits for data on the channel `x`, binds the data to `y`, and then runs `P`.
The arrow `<-` makes a linear receive, which takes the data once. Later entries define the arrows `<=` and `<<-`.
The [receive section of the Rholang guide](../../rholang/08-channels-and-concurrency.md#receive) describes the receive forms.

### Pattern

A pattern is the left side of a receive bind or of a match case.
A pattern matches data when values for its free variables make the pattern equal to the data.
The [pattern chapter](../../rholang/05-pattern-matching.md) describes the patterns.

### Tuple space

A tuple space is a shared store of data items.
Processes coordinate through it: they add items, and they wait for items that match a pattern [[Gel85](#ref-gel85)].

### RSpace

RSpace is the tuple space of the node.
RSpace calls a send a produce, and it calls a receive a consume.
It maps each channel to its stored data and to its stored receives, which wait on the channel.
After each operation, no stored data matches a stored receive, because each operation fires every match that it finds [[FIPS-RRS](#ref-fips-rrs)].
This glossary calls that property the no-pending-match invariant.

### Datum

A datum is one stored send: the data of a produce that found no matching receive.
RSpace keeps a datum at its channel until a receive consumes it.

### Continuation

A continuation is one stored receive: the patterns and the body of a consume that found no matching data.
RSpace keeps a continuation at its channels until matching data arrives.

### COMM event

A COMM event records one synchronization of a consume with one or more produces.
After the COMM, the body of the consume runs with the bound values.

### Join

A join is a receive on more than one channel, such as `for (a <- x & b <- y) { P }`.
A join fires only when each of its channels has matching data.
The [join section of the Rholang guide](../../rholang/08-channels-and-concurrency.md#joins) describes joins.

### Persistent

A persistent item stays after it takes part in a COMM event.
A persistent send uses `!!`, and a persistent receive uses the arrow `<=`.

### Peek

A peek is a receive with the arrow `<<-`.
It reads matching data, and the data stays available to other receives.

### History trie

The history trie is the persistent store of RSpace state.
It is a hash tree: each node holds the hashes of its children [[Mer88](#ref-mer88)].
The root hash, called the state root, identifies the whole state.
Two nodes with equal state roots hold equal state.

### Hot store

The hot store is the in-memory layer of RSpace.
It holds the changes of the current execution on top of the history trie.

### Lock stripe

A lock stripe is one of the mutexes that keep RSpace operations on a channel apart.
RSpace keeps two arrays of 4096 stripes, called phase A and phase B.
A channel uses the stripe at the index $`h \bmod 4096`$ in each array, where $`h`$ is the 64-bit hash of the channel.

A produce locks its channel in phase A, and each channel of the joins of its channel in phase B.
A consume locks each of its channels in both phases.
An operation takes its phase-A stripes in ascending index order, and then its phase-B stripes in ascending index order.

### Deploy

A deploy is a signed Rholang program that a user submits to the network.
The node runs each deploy as one unit.

### Checkpoint

A hard checkpoint writes the hot store into the history trie and returns the new state root.
A soft checkpoint is a snapshot of the hot store in memory.
The node reverts a failed deploy to its soft checkpoint.

### Event log

The event log is the ordered list of the produce, consume, and COMM events of an execution.
An event holds hashes of its channels and data, not the data itself.

### Block

A block is the unit of consensus.
It holds deploys, the event log of their execution, and the state root after them.

### Play

Play is the first execution of the deploys of a block.
The validator that proposes the block plays it, and play records the event log.
A validator is a node that takes part in consensus.

### Replay

Replay is the second execution of the same deploys, by the validators that receive the block.
Replay checks each operation against the recorded event log, and it must reach the same state root.
A difference makes the block invalid.

### Reporting

Reporting is a replay that also records each produce, consume, and COMM event in a report.
The node uses the report to show the events of the deploys of a block.
The reporting space `ReportingRspace` holds a replay space and gives it a logger that collects these events.

### Merge

A merge combines the state changes of blocks from concurrent branches.
The merger rejects a set of changes when one of its conflict checks fires.
This campaign relies on three existing checks of `rspace++/src/rspace/merger/merging_logic.rs`:

- Check #1 is a race: both branches destroy the same non-persistent produce or consume in a COMM event.
- Check #2 is a potential COMM: one branch leaves a new produce, and the other branch leaves a new consume, on the same channel.
- Check #3 is a join contact: a produce touches a channel that has a join of two or more channels in the pre-state.

### Random state

The random state is the deterministic random generator that each process carries.
It is a `Blake2b512Random` value, built on the BLAKE2b hash function [[ANWW13](#ref-anww13)].
A split derives a child state from a parent state and a small number.
Equal parents and equal numbers give equal children, so play and replay derive the same values.

### System process

A system process is a process that the node implements natively.
It listens on a fixed channel, which has a short unforgeable id.
A byte name is a fixed channel with a 1-byte id.
The [system channels chapter](../../rholang/10-system-channels.md) lists the system processes.

### URN

A URN is a `rho:` string, such as `rho:io:stdout`, that names a system process.
The construct ``new x(`rho:io:stdout`) in { P }`` binds `x` to the fixed channel of that system process.

### Bundle

A bundle wraps a name and limits what a holder of the bundle can do with it.
The write-only bundle `bundle+{*x}` lets its holder send on `x` but not receive on `x`.

### FIPS

A FIPS is an F1r3fly Improvement Proposal, which is a design document for the platform.
The campaign implements the FIPS "Reifying RSpaces" [[FIPS-RRS](#ref-fips-rrs)].
It also uses the FIPS "Agents" [[FIPS-AGENTS](#ref-fips-agents)].

### Agent

An agent is a Rholang process that acts as an object with state [[FIPS-AGENTS](#ref-fips-agents)].
A constructor creates an instance and returns a write-only bundle of the instance channel `this`.
Each call is a send on `this` with a reply channel, a method name, and arguments.
The parser already turns the agent syntax into ordinary Rholang.

### Phlo

Phlo is the unit of execution cost that the node charges to a deploy.
At the campaign baseline, phlo comes only from observed RSpace operations: COMM events, produces, and consumes ([decision M-3](decision-register.md#m-3)).

### Accounting observer

The accounting observer is the interface through which RSpace reports each operation for charging.
For one operation, RSpace calls it in a fixed order: start, then produce or consume, then COMM when the operation fires, and then finish.
The finish call reports the result Stored, Matched, or Rejected.
An observer call can reject the operation, for example when the deploy has no phlo left.
A rejected operation logs no event and changes no stored item.
In replay, the observer can also return a directive, which asks RSpace to run the operation in the native replay session.

### Host work

Host work is node work that protects liveness, such as matching work.
The repository glossary defines the [host-work unit](../../Glossary.md#host-work-unit) and the [host-work budget](../../Glossary.md#host-work-budget).
A space job reserves its host work before it does the work ([decision 42](decision-register.md#decision-42)).
A reservation has three amounts: operations, scanned bytes, and allocated bytes.
They charge the dimensions VerificationOperations, VerificationBytes, and SearchStateBytes, as on `feature/cost-accounted-rho`.

### Frontend

A frontend turns Rholang source text into a Par.
The current frontend is the Tree-Sitter parser with the node normalizers.
The campaign keeps the Par encoding of its syntax independent of the frontend ([decision 9](decision-register.md#decision-9)).

### F1r3lang

F1r3lang is Rholang 1.4 as MeTTaIL defines it.
MeTTaIL is a framework for language definitions.
Another agent owns F1r3lang ([decision 3](decision-register.md#decision-3)).
After the F1r3lang merge, the F1r3lang frontend replaces the current frontend ([decision 9](decision-register.md#decision-9)).

### Rocq

Rocq is a proof assistant: a program that checks mathematical proofs [[Roc25](#ref-roc25)].
Its former name is Coq, and its logic extends the calculus of constructions [[CH88](#ref-ch88)].
A Rocq model is a file of definitions and theorems.
The Rocq kernel checks each proof, and the separate checker `rocqchk` checks the compiled file again.
The command `Print Assumptions` lists the axioms that a theorem depends on.

### TLA+

TLA+ is a language that specifies a system as a state machine [[Lam94](#ref-lam94)].
An invariant of a TLA+ model is a state property that must hold in every reachable state.
The model checker TLC visits every reachable state of a finite configuration [[YML99](#ref-yml99)].
The model checker Apalache checks a formula symbolically, with a constraint solver [[KKT19](#ref-kkt19)].

### Inductive invariant

An inductive invariant is a state property that holds in every initial state and that every step keeps.
The BASE check shows the first part, and the STEP check shows the second part.
Together they show that the property holds in every reachable state.
A TLA+ model often strengthens an invariant until it is inductive, so that Apalache can check it at every depth.

### Property-based test

A property-based test states a property of the code and checks it on many generated inputs [[CH00](#ref-ch00)].
The node uses the Rust library proptest.
When an input breaks the property, proptest shrinks the input and prints a small failing case.
The campaign fixes the number of cases and the seed, so each run checks the same inputs.

### pgmcp

The tool pgmcp tracks the work items of the campaign and runs their local verification.
A leaf is a work item without children.
The verifier of pgmcp runs the checks of a leaf in a sandbox, on a read-only snapshot of the worktrees.
A leaf is complete only when the verifier confirms each of its criteria.

## Part 2: campaign terms

### Reified RSpace

A Reified RSpace is a tuple space that a Rholang program creates and uses as a value.
The FIPS "Reifying RSpaces" generalizes RSpace this way: different data structures can act as tuple spaces [[FIPS-RRS](#ref-fips-rrs)].
A program chooses the data structures of each instance.

### Space

A space is one Reified RSpace instance.
It stores data and continuations at its channels, and it keeps the no-pending-match invariant of RSpace.
All spaces persist as ordinary channels inside the one RSpace of the node ([decision M-2](decision-register.md#m-2)).
The default space is the RSpace of ordinary Rholang.

**Preferred usage.** Use "Reified RSpaces" for the feature and the campaign, and use "space" for one instance.
*Distinguish from* RSpace, which is the one tuple space of the node.

### Host operation

The host is the RSpace of the node, in play or in replay.
A host operation is a produce, a consume, or a COMM event that the host performs for ordinary Rholang.
Space operations create the same events as the host operations of an equivalent Rholang program.

### Qualifier

A qualifier sets the persistence and the access rules of a space.
The FIPS qualifiers are default, temp, and seq [[FIPS-RRS](#ref-fips-rrs)].

- A default space is persistent and allows concurrent access.
- A temp space allows concurrent access, but its data never reaches the history trie.
- A seq space is a temp space with more limits: no process can send its names, and only one sequential process can use them.

In this campaign, temp and seq spaces live for one deploy ([decision I-4](decision-register.md#i-4)).
A seq space runs only in native execution, because its [seq claims](#seq-claim) need the job order of the deterministic scheduler.

### Outer store

The outer store of a space maps its channels to their stored items.
The FIPS outer stores are HashMap, PathMap, Array, Vector, and HashSet ([decision 1](decision-register.md#decision-1)).

- HashMap maps each channel to its own items.
- PathMap treats each channel as a sequence of values, and a send on a sequence also reaches each prefix of the sequence.
- Array gives out a fixed number of names. The maximum form aborts with `OutOfNames` after the last name, and the cyclic form reuses its names.
- Vector gives out an unbounded number of names.
- HashSet stores [units](#unit) on its channels: a send carries no value, and a receive binds no value. It requires the seq qualifier.

### Unit

A unit is a record whose payload is empty: the message of `x!()`.
The collections of a HashSet space hold units only ([decision 73](decision-register.md#decision-73)).
A send with a value aborts with `PayloadNotEmpty`, and a receive with a pattern, a binder or a remainder aborts with `PatternNotEmpty`.
An empty receive `for ( <- x)` takes one unit or waits, as a receive of the default space does.

### Path

A path is a sequence of Par values that a PathMap space uses as a channel.
A PathMap space takes the path of a list channel from the elements of the list.
The path `q` is a prefix of the path `p`, written $`q \preceq p`$, when $`p = q \mathbin{+\!\!+} s`$ for some sequence $`s`$.
The operator $`\mathbin{+\!\!+}`$ concatenates two sequences.
The suffix of `p` after `q` is $`p \setminus q = s`$.

### Inner collection

The inner collection at a channel decides the order and the multiplicity of the stored items.
The FIPS inner collections are bag, set, cell, queue, priority queue, stack, and bag with VectorDB ([decision 1](decision-register.md#decision-1)).
The code calls an inner collection a discipline.

- A bag stores items with repeats and no order.
- A set stores each item at most once.
- A cell stores at most one datum. A second produce aborts with `CellOccupied`.
- A queue delivers first-in, first-out, and a stack delivers last-in, first-out.
- A priority queue keeps one queue for each priority level.
- A bag with VectorDB delivers data by vector similarity.

### Collection

A collection is the part of a space at one channel of the space.
It holds the data and the continuations that the space stores at that channel.
The inner collection of the space decides how the items of one collection interact.
Items of two different collections never interact, except through a join.

### Theory

A theory describes the data that a space admits ([decision 9](decision-register.md#decision-9)).
The FIPS describes it as a MeTTaIL theory of the data that processes can send on the channels of a space [[FIPS-RRS](#ref-fips-rrs)].
A program names a theory with the syntax `free Th(args)`.
Before the F1r3lang merge, the builtin theories are `Nat`, `Int`, `String`, `Bool`, and `Any`, and they check only the structure of a value.
P7 step 4 implements them ([specification 5](specification.md#5-theories)), and the [theory seam](theory-seam.md) explains the design.

### Theory identity

A theory identity is the content that names a theory ([decision 9](decision-register.md#decision-9)).
It holds a version, a language fingerprint, a category, a category name, a theorem id, a checker ABI, a limit profile, and the arguments.
It never holds process-local state, such as a reference to an installed theory, a token, or an epoch.
The config record of a space stores the theory identity as one `GByteArray` of its canonical bytes ([decision 74](decision-register.md#decision-74)).
The `config` reply of a handle shows the identity as a map.
So the first use of the space in a later deploy, and replay, can check data again deterministically.

### Admission verdict

An admission verdict is the result of a theory check: `Proven`, `Refuted`, or `Undetermined` ([decision 9](decision-register.md#decision-9)).
A produce stores its data only when each argument is `Proven`.
A COMM uses a stored datum only when its check is `Proven`.
`Undetermined` fails closed: it has the same effect as `Refuted`.
At a send, a verdict other than `Proven` aborts the deploy ([specification 5](specification.md#5-theories), rule TH-5).
At a match, it is a guard refusal (rule TH-6).

### Namespace tag

The namespace tag is the byte `0xE0`.
Every unforgeable id that belongs to Reified RSpaces starts with it ([decision 7](decision-register.md#decision-7)).
The tag replaces the earlier tag `0xF3`, which collides with the MeTTaIL tag band `0xF0` to `0xF9`.
An id with the namespace tag has either two bytes, $`[\mathtt{0xE0}, \mathit{idx}]`$, or more than two bytes, $`\mathtt{0xE0} \mathbin{\Vert} \mathit{KIND} \mathbin{\Vert} \mathit{body}`$.
Here $`\mathbin{\Vert}`$ is byte concatenation.

### KIND

The KIND is the second byte of a namespaced id with more than two bytes.
It is an ASCII letter that selects the layout of the rest of the id.
Spaces with the default qualifier use the uppercase KINDs, and temp and seq spaces use the lowercase forms.

| KIND | Byte | Meaning | Persistence |
|---|---|---|---|
| D | `0x44` | data of a space channel | history trie |
| C | `0x43` | the config record of a space | history trie |
| I | `0x49` | the path index of a PathMap space | history trie |
| A | `0x41` | the name allocator of an Array or Vector space | history trie |
| S | `0x53` | a sequence counter | history trie |
| H | `0x48` | the channel `this` of a space, with its forwarding receive | history trie |
| N | `0x4E` | a name of a space | none: the id is a routing stamp |
| X | `0x58` | a receive that tests one pattern for an agent space | history trie |
| B | `0x42` | a receive that holds the body of a receive on an agent space | history trie |
| d c i a s h n | lowercase letters | the same layouts for temp and seq spaces | hot store only |
| M | `0x4D` | reserved for engine-owned state | held |
| T | `0x54` | reserved for canonical theory and space-spec content | reserved |

**Preferred usage.** Write the KIND as its uppercase letter, for example "KIND D".
Write "the lowercase form" for the ephemeral variant of a KIND.

### Space id

The space id, written `sid`, is the 32-byte identity of a space ([decision I-19](decision-register.md#i-19)).
The call that creates a space derives the space id from its random state:

```math
\mathit{sid} = \mathrm{next}\left(\mathrm{split}_{\mathtt{SPACE\_ID}}\left(\mathrm{split}_{\mathtt{0xE0}}\left(r_{\mathit{call}}\right)\right)\right)
```

Here $`r_{\mathit{call}}`$ is the random state of the call, each $`\mathrm{split}`$ derives a child state, and $`\mathrm{next}`$ draws 32 bytes.
The `sid` is unique for each creating message.
A second creation with the same `sid` aborts with `SpaceAlreadyExists`.

### Storage channel

A storage channel is a host channel that holds the records of a space ([decision 7](decision-register.md#decision-7)).
Its unforgeable id has the layout

```math
\mathtt{0xE0} \mathbin{\Vert} \mathit{KIND} \mathbin{\Vert} \mathit{sid} \; [\mathbin{\Vert} \mathit{tail}]
```

where the bracketed tail is optional.
The KIND decides the tail.
For KIND D, the tail is $`\mathrm{canon}(\mathit{inner})`$, the canonical bytes of the channel inside the space.
Ordinary Rholang cannot reach a storage channel, because it cannot write an unforgeable id.

**Preferred usage.** Use "storage channel" for every channel with this layout, as decision 7 does.
*Distinguish from* the routing class `Storage`, which holds only the KINDs D, I, A, and S. Routing treats the KINDs H and C as default-space objects.

### Space transaction

A space transaction is one batch of space operations that the node applies to the host as a unit.
The Rust trait `SpaceTxn` represents it, and its `commit` method is the single place where a space changes state ([decision 19](decision-register.md#decision-19)).
Each operation creates the same events, in the same order, as the equivalent host operation.

### Staged view

The staged view is what a space transaction reads.
It shows the hot store with the writes of the transaction applied, and it reads through to the history trie for older state.

### Legacy charge

A legacy charge is a phlo charge that `ChargingRSpace` makes for a host operation in a legacy deploy.
Before the operation it charges the storage of the record or the continuation.
After the operation it refunds the storage that a fire removes and charges the event fees.
Each staged operation of a space transaction pays the legacy charges of its equivalent host operation ([decision 45](decision-register.md#decision-45), Option P).
The pre-charge runs in the callback of the operation, and the post-charges run in the callback of the next operation or after the commit.
*Distinguish from* the charges of the [accounting observer](#accounting-observer), which every deploy makes.

### Internal operation

An internal operation is a system process with a two-byte id from $`[\mathtt{0xE0}, \mathtt{0x30}]`$ to $`[\mathtt{0xE0}, \mathtt{0x32}]`$.
The three internal operations are `HANDLE_OP`, `MATCHER_OP`, and `SINK_OP`.
The forwarding receives and the translations that the campaign synthesizes call them.
The node binds `HANDLE_OP` to the URN `rho:reified:internal:handleOp`, because it binds every system definition to a URN ([decision 32](decision-register.md#decision-32)).
`HANDLE_OP` accepts a call only with the unforgeable channel `this` of an existing space, so a program that names the URN gains no capability.

### Factory

A factory is a system process that creates a space ([decision 8](decision-register.md#decision-8)).
Each factory has a `rho:space:` URN and a two-byte id $`[\mathtt{0xE0}, \mathit{idx}]`$ with an index from `0x00` to `0x1F`.
A factory call takes an optional qualifier, an optional theory, and named parameters.

In one space transaction, the factory stores three records.
They are the config record (KIND C), a counter (KIND S), and a forwarding receive on the new channel `this` (KIND H).
It then replies with the write-only bundle `bundle+{*this}`.
A space that a factory creates is a native space.

### Op channel

An op channel is a channel whose unforgeable id has the two bytes $`[\mathtt{0xE0}, \mathit{op}]`$.
The byte $`\mathit{op}`$ selects a factory (`0x00` to `0x1F`), a system process (`0x21` to `0x23`), or an internal operation (`0x30` to `0x32`).
The byte `0x20` is reserved for the factories of native engines, which are held.

The node finds the native code of the process of an op channel by the number $`200 + \mathit{op}`$, its body reference. The baseline uses the body references 0 to 36 and 101 to 108.
A persistent send `!!` to an op channel that takes calls aborts the deploy ([decision 32](decision-register.md#decision-32), item A21).
The syntax markers and `AGENT_TAG` have the same id layout, but they take no calls.

### Space family

A space family is a pair of an outer store and an inner collection, such as `hashmap:queue`.
The 32 supported pairs are the families, and family $`i`$ has its factory on the op channel $`[\mathtt{0xE0}, i]`$.
The factory URN names the family outer first, as `rho:space:<outer>:<inner>`.
The FIPS aliases `rho:space:HashMapBagSpace` and `rho:space:PathMapSpace` name the families `hashmap:bag` and `pathmap:bag`.
Phase P4 registers only the family `hashmap:queue`. Phases P5 and P6 register the other families.

### Factory capability

The factory capability of a space family is the write bundle `bundle+{[0xE0, i]}` of the op channel of its factory.
The URN of the family binds this value in the URN map of the node.
A program can call the factory with the capability, but it cannot receive on the op channel.

### Handle

A handle is the write-only bundle `bundle+{*this}` that a factory returns for a native space ([decision I-2](decision-register.md#i-2)).
The channel `this` has the unforgeable id $`\mathtt{0xE0} \mathbin{\Vert} \mathtt{H} \mathbin{\Vert} \mathit{sid}`$.
A persistent single-channel receive on `this` forwards each call to the internal operation `HANDLE_OP`.
The methods are `gensym`, `produce`, `consume`, `qualifier`, and `config`.
A dangling handle is a handle whose config record does not exist, and a call on it aborts with `DanglingHandle`.

**Preferred usage.** Use "handle" for the bundle that a program holds.
*Distinguish from* the channel `this`, which only the forwarding receive of the space reads.

### Native space

A native space is a space that a factory creates ([decision 8](decision-register.md#decision-8)).
Its space agent is Rust code of the node, and it keeps its state in storage channels under its space id.
A program reaches it through its handle, through the names of the space, and through a `use` block or a `new` annotation with its handle.
The family of the space, its qualifier, and its parameters never change after the creation.

### Handle capability

The handle capability of a native space is the `Par` value `bundle+{H(sid)}` that its factory replies, where $`H(\mathit{sid})`$ is the channel `this` of the space.
The bundle has the write flag and no read flag. So its holder can send calls to the space, but it cannot receive from the channel `this`.
The reducer removes the write bundle of a send, so a call on the handle capability reaches the channel `this`.

The internal operation `HANDLE_OP` accepts a call only with the bare id of `this` ([decision 32](decision-register.md#decision-32), item A6). Only the forwarding receive of the space passes that id.
So a program that names the URN of `HANDLE_OP` gains no capability.

### Space name

A space name is a name with KIND N: $`\mathtt{0xE0} \mathbin{\Vert} \mathtt{N} \mathbin{\Vert} \mathit{sid} \mathbin{\Vert} \mathit{tail}`$ ([decision I-3](decision-register.md#i-3)).
The tail has 32 random bytes, or the 8-byte index of an Array or Vector space.
A native space mints space names: the handle method `gensym`, the annotation `new x : s`, and a `new` inside a `use` block make them.
A space name routes to its own space in every context, so it keeps its meaning across blocks and after a restart.
No record exists for a space name itself, because the name describes itself.

### getSpaceAgent

`getSpaceAgent` is the system process with the id $`[\mathtt{0xE0}, \mathtt{0x21}]`$ and the URN `rho:space:getSpaceAgent`.
The call `getSpaceAgent!?(s)` replies with the [factory capability](#factory-capability) of the native space `s` ([decision 9.4](decision-register.md#decision-9-4)).
The FIPS aliases of a factory bind the same capability, so `getSpaceAgent(space_1) == HMB` holds for a space that `HMB` created.
The system ids $`[\mathtt{0xE0}, \mathtt{0x22}]`$ and $`[\mathtt{0xE0}, \mathtt{0x23}]`$ belong to `rho:assert:assertEqual` and `rho:lang:vector`.

### Space agent

A space agent implements the operations of a space: `gensym`, `produce`, and `consume`.
The FIPS defines this behavior as the RSpace agent: any agent with the correct behavioral type can act as an RSpace [[FIPS-RRS](#ref-fips-rrs)].
In the node, a native space agent is the Rust trait `SpaceAgent`, which prepares a produce or a consume as one space transaction.

### User agent-backed space

A user agent-backed space is a space whose space agent is an ordinary Rholang agent, not a native handle ([decision 8](decision-register.md#decision-8)).
The program selects it with a `use` block or with a `new x : a` annotation whose value is not a handle.
The reducer, which is the evaluator of Rholang in the node, then turns `new` into a `gensym` call.
It also turns a send into a `produce` call and a receive into a `consume` call.

The agent author must keep three soundness obligations, ASO-1 to ASO-3. For example, ASO-3 requires that `gensym` replies exactly once.
A violation breaks the FIPS semantics but never consensus, because the node guarantees determinism for every agent.

**Preferred usage.** Use "native space" for a space that a factory creates, and "user agent-backed space" for a space that an ordinary agent implements.

### Space reference

A space reference is the value that selects a space: the `s` of `use s { P }`, of `new x : s`, and of `getSpaceAgent!?(s)`.
The function `classify_space_ref` gives each value exactly one meaning, by five rules in order:

1. The empty `Par` is the error `EmptySpaceRef`.
2. The function removes at most one bundle, by the rule of a send.
3. A handle id, bare or under a bundle with the write flag, is its native space. Under a bundle without the write flag, it is the error `HandleNotWritable`.
4. Any other Reified id is the error `NotASpaceRef`, so a capability or a storage id is never a space.
5. Every other value is a user agent.

**Preferred usage.** *Distinguish from* a [space name](#space-name), which is a channel of one space.

### Typed reply

A typed reply is the reply of a Reified system process to a call with a protocol error ([decision 34](decision-register.md#decision-34), item A23).
It is one datum: the tuple `("rho:space:error", code, detail)`, where `code` is a string and `detail` is a map.
The call does not abort, and the deploy continues.

`HANDLE_OP` replies only when the message has a reply name and a method name.
A reply name is one unforgeable name, or one bundle with the write flag.
`HANDLE_OP` drops every other message without a reply.
The [agent API](agent-api.md#7-typed-replies) lists every code and its detail map.

### Matcher

A matcher is a channel with KIND X that tests one receive pattern for a user agent-backed space ([decision I-15](decision-register.md#i-15)).
Its id holds the canonical state of the pattern, so the matcher describes itself.
A persistent single-channel receive on the matcher forwards each test to the internal operation `MATCHER_OP`.
`MATCHER_OP` runs the same metered matcher as a native receive.

### Body channel

A body channel is a channel with KIND B that holds the original continuation of a receive on a user agent-backed space ([decision I-15](decision-register.md#i-15)).
A single-channel receive with a map pattern waits on it.
When the agent sends a map of bound values to the body channel, the original process runs with those values.

### Marker

A marker is a public two-byte unforgeable name under the namespace tag that the node never installs as a channel.
The syntax markers are `USE`, `NEWSPACE`, `PARAMS`, `QUERY`, and `FREE`, with the indices `0xD0` to `0xD4` ([decision 18](decision-register.md#decision-18)).
The marker `AGENT_TAG`, with the index `0xFF`, tags the names that a user agent returns.

The figure shows every id layout under the namespace tag, with the markers.

![Namespace bytes: an id that starts with 0xE0 is either a two-byte id [0xE0, idx] or a three-part id 0xE0 ‖ KIND ‖ body. The left table lists the KINDs D, C, I, A, S (durable storage), H, X, B (default-space objects), N (names), the lowercase ephemeral forms, and the reserved M and T. The right table lists the factory ids 0x00 to 0x1F, the reserved 0x20, the system ids 0x21 to 0x23, the internal operations 0x30 to 0x32, the syntax markers 0xD0 to 0xD4, and AGENT_TAG 0xFF.](diagrams/02-namespace-bytes.svg)

*Source: [diagrams/02-namespace-bytes.puml](diagrams/02-namespace-bytes.puml), rendered by [diagrams/render.sh](diagrams/render.sh).*

### Marker tuple

A marker tuple is a tuple expression whose first element is a syntax marker.
It carries one Reified construct inside the existing fields of a Par ([decision 18](decision-register.md#decision-18)).
So the encoding needs no new Protocol Buffers field, and ordinary terms keep their bytes and hashes.

| Source form | Marker tuple |
|---|---|
| `use s { P }` | `(USE, s, P)` |
| `new x : s in { P }` | `(NEWSPACE, [annotations], new …)` |
| `x!(d; k=v)` | the last data element `(PARAMS, [positional], {k: v})` |
| `for (y <- ch ~ (q; k=v)) { P }` | the receive source `(QUERY, ch, [q], {k: v})` |
| `free Th()` | the value `(FREE, "Th", [])` |

### Recognized position

The recognized position of a marker tuple is the one place where the runtime gives it meaning ([decision 18](decision-register.md#decision-18)).
`USE` and `NEWSPACE` tuples act in process position.
A `PARAMS` tuple acts as the last evaluated data element of a send.
A `QUERY` tuple acts as the evaluated source of a receive bind.
A `FREE` tuple acts as an argument of a factory call.

### Inert

An inert value has no effect when the runtime evaluates it.
A marker tuple outside its recognized position is inert data.
There is one exception.
A send whose last data element is a `PARAMS` tuple aborts with `ParamsUnsupported` when its channel takes no parameters.

### Routing class

The routing class of a channel decides where the sends and the receives on that channel go ([decision 8](decision-register.md#decision-8)).
The reducer computes the class after it removes the write or read bundle of the channel.
The function `classify_channel` gives each channel exactly one class:

| Class | Channel |
|---|---|
| `System` | a `GPrivate` with an id of 1 or 2 bytes: a byte name, an op channel, or a marker |
| `DefaultObject` | an id of KIND C, H, X, or B, or of a lowercase form |
| `Storage` | an id of KIND D, I, A, or S, or of a lowercase form |
| `SpaceName` | a space name: KIND N or n |
| `AgentTagged` | the tuple `(AGENT_TAG, A, raw)` of a name that a user agent minted |
| `Unforgeable` | every other unforgeable name, an ordinary `new` name included |
| `Forged` | every other channel, such as `@"x"`, and a bundle that the R7 backend sees |

The routing rules R1 to R7 of the [design](design.md#n5-routing-and-scope) send each class to the default space, to a native space, or to a user agent.
A program that uses no Reified syntax and holds no space name routes every channel to the default space, so ordinary execution does not change.

### Use context

The use context of the reducer is the native space that the innermost `use` block of the evaluated process selects.
Outside every `use` block, the reducer has no use context.
A channel of the class `Forged` routes to a collection of the space of the use context. Without a use context, it routes to the default space.

A body that the reducer stores for later evaluation under a use context gets a USE tuple of that context around it. So the context returns when the body runs, as in a lexical closure.
The reducer holds the use context in its field `space_ctx`, and it copies the field into parallel branches.

### Canonical form

The canonical form of a value is the one byte form that the encoder produces for it.
Equal values have equal canonical forms, so hashes and comparisons do not depend on the source text.
The existing sorters put marker tuples in canonical order, and the decoders accept only canonical forms ([decision 21](decision-register.md#decision-21)).

### Record envelope

A record envelope is the metadata that a space stores inside each of its records ([decision M-2](decision-register.md#m-2)).
A datum record holds the envelope `[tag, ord, priority, embedding]` before the user payload.
Here `ord` is the sequence number of the record, and `priority` and `embedding` serve the priority queue and VectorDB disciplines.
A parked continuation holds the envelope `(ord, query policy)` around its body.
Counters, allocators, and the PathMap index records carry envelopes too, so no Reified datum is a single number.
The host data structures do not change, and only space agents read envelopes.

A join waiter holds version 2 of the continuation envelope, with one [join slot](#join-slot) for each position of its group.

### Config record

The config record of a native space is the one datum on its config channel $`C(\mathit{sid}) = \mathtt{0xE0} \mathbin{\Vert} \mathtt{C} \mathbin{\Vert} \mathit{sid}`$ ([decision I-17](decision-register.md#i-17)).
After its envelope, it holds the canonical map with the keys `spaceId`, `outer`, `inner`, `qualifier`, `params`, and `theory`.
The factory stores it once, as a persistent datum without a cost authority, and nothing changes it later.
Each operation of the space reads it first.
Without the config record, a collection operation fails with `DanglingName` on a space name and with `DanglingHandle` otherwise.

### Counter meta-channel

A counter meta-channel is a storage channel of KIND S that holds one envelope-wrapped counter ([decision M-1](decision-register.md#m-1)).
Each ordered collection, which is a queue, a stack, a cell, or one whole priority queue, has one counter.
Each PathMap space has one token on the same KIND.
Each space also has one space counter, and the first operation on a collection creates the counter of that collection from it.

Every operation on such a collection consumes the counter and produces it again with the next value.
This rule also covers a peek and a fire that changes no state ([decision 31](decision-register.md#decision-31)).
So two branches that use the same collection consume the same counter datum, and merge check #1 reports the race.

### Space counter

The space counter of a native space is the counter record on $`S(\mathit{sid}) = \mathtt{0xE0} \mathbin{\Vert} \mathtt{S} \mathbin{\Vert} \mathit{sid}`$ ([decision M-1](decision-register.md#m-1)).
The factory creates it with the value 0.
The first operation on a collection consumes it, produces it again with the next value, and opens the collection counter.
So two branches whose first operations open collections of one space race on the space counter ([decision 34](decision-register.md#decision-34), item A25).

### Collection counter

The collection counter of a collection is the counter record on $`S(\mathit{sid}, k) = \mathtt{0xE0} \mathbin{\Vert} \mathtt{S} \mathbin{\Vert} \mathit{sid} \mathbin{\Vert} k`$, where $`k`$ holds the canonical bytes of the channel of the collection.
The first operation on the collection opens it with the value 1 and takes the order 0.
Every later operation consumes the counter record with a value $`v`$, takes $`v`$ as its order, and produces the value $`v + 1`$ ([decision 31](decision-register.md#decision-31)).
A record or a continuation that the operation stores carries this order as its `ord`.
So the entries of a collection have distinct orders, and these orders give the arrival order.

### Write skew

A write skew is an anomaly of concurrent transactions [[BBG95](#ref-bbg95)].
Two transactions read overlapping data, each one changes data that the other one read, and their changes do not overlap.
A check that compares only the changed data then finds no conflict, but no serial order of the two transactions gives their result.
Two branches can make a write skew on two queues: each branch peeks one queue and dequeues the other queue.
Every operation on a queue therefore bumps its counter, and merge check #1 sees the race ([decision 31](decision-register.md#decision-31)).

### Crossed stacks

Crossed stacks are two branches that each push onto one stack and pop the other stack.
Without a counter bump on the pops, merge check #1 sees no race, but no serial order of the two branches gives both pops.
So every removal bumps the counter of its collection, as every other operation does ([decision 31](decision-register.md#decision-31)).
The Rocq headline `crossed_stacks_without_removal_counters_fabricate` computes the counterexample, and [section 12.11 of the dossier](verification.md#1211-the-merge-of-the-p5-families) describes it.

### Head-only matching

Head-only matching is the matching rule of queues, stacks, and priority queues ([decision 11](decision-register.md#decision-11)).
Only the head datum and the head continuation can interact.
The head of a queue has the smallest `ord`, and the head of a stack has the largest `ord`.
A head that does not match blocks the items behind it.
A priority queue checks its levels in ascending priority order, with head-only matching in each level.

### Store identity

The store identity of a channel $`c`$ is the 32-byte hash $`\mathrm{Blake2b256}(\mathrm{bincode}(c))`$, which `stable_hash_provider::hash` computes [[ANWW13](#ref-anww13)].
The history trie keys the items of a channel by this hash.
The hot store keys them by channel equality, which agrees with the hash.
A Rust ordering of the channel type can still tell two such channels apart, as the field `locally_free` of `Par` does.
So the substrate keys every map and set by the store identity, never by that ordering.
The store identity of a list of channels is the list of their store identities, in list order.

### Collection key

The collection key of a channel $`c`$ is the byte string $`\mathrm{canon}(c) = \mathrm{bincode}(\mathrm{sort}(c))`$, which `canonical_inner_channel_bytes` computes ([decision 32](decision-register.md#decision-32), item A5).
It is the preimage of the store identity of the sorted channel.
Serde writes the field `locally_free` as empty bytes, so channels that are equal as `Par` values have one key.
The sort fixes the order of the parallel parts of a channel, and the order of the elements of a list stays significant.
The HashMap outer store keeps one collection for each key.

### Canonical group

A channel group is the channel list of a join or of a stored continuation.
A group is canonical when the store identities of its channels are non-decreasing, compared as byte strings ([decision 28](decision-register.md#decision-28)).
So a canonical group can name one channel more than once, and a group of one channel is always canonical.
Each list of channels has exactly one canonical order: the order that sorts the store identities.
The substrate refuses every other order with `NonCanonicalChannelOrder`, so the hot store never holds one channel multiset under two keys.

### Outcome

An outcome is the result of a host operation, which a space agent decides before it stages the operation ([decision 19](decision-register.md#decision-19)).
A produce has the outcome Store or Fire, and a consume has the outcome Park or Fire.
Store keeps the datum in the hot store, and Park keeps the continuation there.
Fire names the continuation or the stored data that the operation meets in a COMM event.
A produce-side fire is a produce with the outcome Fire, and a consume-side fire is a consume with the outcome Fire.
The substrate checks each outcome against the staged view.

### Lock coverage

Lock coverage is the rule that a space transaction holds the lock stripe of each channel that it touches.
The transaction must hold the stripe in each phase that the equivalent host operation locks for that channel.
The transaction takes its stripes when it begins, and it keeps them until it commits, closes after a failed view change, or is dropped.
Staging refuses an operation on a channel without coverage, with `UnlockedChannel`.
So no host operation can change a touched channel between the staging and the commit of a transaction.

### Produce counter

The produce counter $`\kappa(p)`$ counts the non-persistent produces with the identity $`p`$ since the node last took the event log.
Equal channels, equal data, and equal persistence give one produce identity, so replay uses the counter to tell identical produces apart.
The campaign writes $`\kappa_{\mathrm{host}}(p)`$ for the counter of the host, and $`\kappa_v(p)`$ for the counter that a space transaction sees.

*Distinguish from* the [counter meta-channel](#counter-meta-channel), which is a stored record of a space.

### Times-repeated

The times-repeated map of a COMM event records the value of $`\kappa(p)`$ at the COMM event, for each of its produces $`p`$.
In play, the counter of a non-persistent produce that fires already counts that produce.
Replay accepts a stored non-persistent datum for a recorded COMM event only when the counter of the datum equals the recorded value.

### Reduction participant

A reduction participant is one sequential strand of the evaluation of a deploy in the deterministic scheduler.
It makes its operations one after another, and it numbers them with its steps $`0, 1, 2, \ldots`$.
A split replaces a participant with one child participant for each parallel branch.

The reducer of `feature/cost-accounted-rho`, which the campaign merged at `4d67b5ccc`, evaluates a fired body in a detached task.
So a dispatch splits one child from the participant that makes it, and the child runs the body.
After the dispatch, that participant makes no operation, so work that follows a space operation runs in a branch of its own ([design N6](design.md#n6-scheduling)).

### Seq claim

A seq claim is the participant path of the last use of one channel of a seq space ([decision 73](decision-register.md#decision-73)).
The book of the root evaluation holds one claim for each used channel.
An operation may use a channel only when the channel has no claim or the operation descends from its last use, as a [reduction participant](#reduction-participant).
Otherwise the deploy aborts with `SeqConcurrent`.
An operation stores its claims only after its commit succeeded, and internal jobs, such as re-issues and settle jobs, claim nothing.

### Operation order

The operation order is the ordering key that the deterministic scheduler gives to each RSpace operation.
It is a pair of a session id and a path, which is a list of segments $`(n, t)`$.
An operation of a [reduction participant](#reduction-participant) appends the segment $`(k, 0)`$, where $`k`$ is the next step of the participant.
Since the merge of `4d67b5ccc`, child $`c`$ of a split at step $`k`$ appends the one segment $`(k, c + 1)`$ (DR-84 of `feature/cost-accounted-rho`).
Before the merge, a split appended the two segments $`(k, 1)`$ and $`(c, 0)`$.

Orders compare by session id first, and then lexicographically by path.
Play and replay give each operation the same order, and RSpace logs the events of a deploy in groups sorted by order.

### Sub-order

A sub-order is the ordering key of one operation inside a space transaction ([decision 19](decision-register.md#decision-19)).
Operation $`i`$ of a transaction that commits under the [operation order](#operation-order) $`\mathit{parent}`$ runs under $`\mathit{parent} \mathbin{+\!\!+} [(i, 2)]`$.
The value 2 is `SUB_OPERATION_TAG`.
A scheduler order ends with an operation segment $`(k, 0)`$, and a sub-order ends with $`(i, 2)`$, so no scheduler order equals a sub-order.
A split segment can have the second component 2, so the argument uses the final segment ([Rocq headline H8](verification.md#42-headlines-of-eventcorrelationv)).
Native phlo recording rejects two operations under one order, so each operation needs its own sub-order.

### Space intent

A space intent is the unit of work that the deterministic scheduler runs for one space operation: `Intent::Space` ([decision I-11](decision-register.md#i-11)).
Its footprint holds the key $`[2] \mathbin{\Vert} \mathit{sid}`$ of its space. So the scheduler orders the intents of one space, and it runs the intents of other spaces in parallel.
The scheduler gives the intent the next operation order of its participant, and the transaction of the intent commits under that order.
The job of a space intent runs in its own task, because the substrate admits one open transaction in each task.

### Evaluation scope

An evaluation scope is one root evaluation of a deploy by the reducer, from its entry to its end.
The parent order of a transaction is the operation order of the reducer step that commits it.
In one scope, the `SpaceStoreHandle` of a host admits one committed transaction under each parent order.
It refuses a second transaction under the same parent order with `ParentOrderReused`.
The handle forgets the committed parent orders at the start of each scope, when the hot store changes, and when the session changes.
So a deploy that runs again, for example after a revert, commits under the same orders.

### Prefix atomicity

Prefix atomicity is the failure rule of a commit ([decision 19](decision-register.md#decision-19), D6).
When operation $`i`$ of a transaction fails at commit, operations $`0`$ to $`i - 1`$ stay applied, as in the equivalent host program.
The deploy then fails, and the soft checkpoint of the deploy removes the applied operations.
Replay reproduces the same prefix ([decision 36](decision-register.md#decision-36)).
Replay stages the operations of play, and its commit makes the observer calls of play in the same order.
So a call that the observer rejects stops replay at the operation where it stopped play, with the error and the cost of play.

### Trigger event

An IO event is a produce event or a consume event of the event log.
The trigger event of a fire is the IO event of the operation that fires.
It is the produce of a produce-side fire, and the consume of a consume-side fire.
Its entry in the replay table is the trigger key of the fire.

### Binding

A binding links an IO event of the play log to a COMM event of that log, in the replay table.
Before replay, the node binds each recorded COMM event under each of its IO events that the log also holds.
These IO events are its consume and each produce of its produce list.
A produce that occurs twice in the produce list of the COMM event gives two bindings.
So the binding multiplicity of a key is 1 for the consume, and the number of occurrences for a produce.
When replay reproduces the COMM event, it removes all of its bindings at once.

### Pairing

Pairing is the replay step that matches a fire with one recorded occurrence of its COMM event ([decision 19](decision-register.md#decision-19)).
The substrate pairs a fire when it stages the fire, and it looks the COMM event up only under the trigger key of the fire.
An occurrence is free while the paired copies are fewer than the bindings under the key, divided by the binding multiplicity.
A fire without a free occurrence does not pair, but its staging does not fail ([decision 36](decision-register.md#decision-36)).
The next entry defines such a fire.
Space operations create ordinary events, so the replay table of the host holds their COMM events.

### Unpaired fire

An unpaired fire is a fire that replay or reporting stages without a pairing ([decision 36](decision-register.md#decision-36)).
The log holds no free occurrence of its COMM event under its trigger key.
Staging accepts the fire as play stages it: the fire changes the staged view, and it adds no paired copy.
So replay stages exactly the operations of play, for every log.

At commit, the unpaired fire makes the observer calls of play, in the same order, up to and including its COMM call.
A call that the observer rejects fails the operation with the error of play, for example when the phlo of the deploy runs out.
When every call accepts, the commit fails with `ReplayDivergence`.
In both cases the fire logs no event, changes no store entry, and keeps its bindings.
The operations before it stay applied by [prefix atomicity](#prefix-atomicity).

### Retention

*Superseded by [decision 45](decision-register.md#decision-45) (Rule E).* The term names the earlier rule of the substrate, which the formal history and the older decisions cite.
Retention kept a stored record in place after a peek bound it in a COMM event ([decision 19](decision-register.md#decision-19)).
The host never retains a peeked datum: it removes the datum, and the reducer sends the data again.
A space retained each stored record that a peek fire bound, and the receipt reported such a record as persistent.
Since decision 45, a peek fire removes its stored non-persistent records as the host does, and the agent makes a [re-store](#re-store).

### Volatile channel

A volatile channel is a storage channel with a lowercase KIND.
It belongs to a temp or a seq space, and it exists only in the hot store during one deploy ([decision I-4](decision-register.md#i-4)).
Its operations still create events and accounting observations, so they are charged and replay pairs them.

### Sweep

The sweep removes the volatile channels of a deploy at the end of that deploy ([decision I-4](decision-register.md#i-4)).
It removes their data, their continuations, and their join entries, and it emits no event and makes no observer call.
The sweep never touches an ordinary channel.
It refuses a join that also names a channel that it keeps, and a channel with items in the history trie.

Each root evaluation keeps a book of the temp spaces that it created and of the volatile channels that its transactions locked.
Play and replay sweep the book at the same point, after every detached task of the root evaluation ([decision 19](decision-register.md#decision-19)).
A nested call of the reducer neither clears the book nor sweeps.

### Re-rooting

Re-rooting delivers PathMap data relative to the prefix that a receive listens on ([decision 9.3](decision-register.md#decision-9-3)).
A send on path `p` that a receive on prefix `q` takes delivers its value re-rooted by the suffix $`s = p \setminus q`$.
The function $`R(s, v)`$ re-roots the value $`v`$, where a PathMap value is a set of paths written `{|…|}`:

- $`R(\varepsilon, v) = v`$ for the empty suffix $`\varepsilon`$.
- For a non-empty suffix, an empty PathMap value becomes the PathMap value that holds only $`s`$.
- A PathMap value with entries prepends $`s`$ to the path of each entry.
- Any other value becomes the list $`s \mathbin{+\!\!+} [v]`$.

For example, `@[0,1,2]!({|"hi"|})` received at `@[0,1]` binds `{|[2,"hi"]|}`, and `@[0,1,2]!(42)` binds `[2, 42]`.

### Waiter

A waiter is a continuation that a space parks while it waits for matching data.
In a PathMap space, a waiter on the prefix `q` waits at the path-index channel of `q`.
A send on path `p` checks the waiters of each prefix of `p`, most specific prefix first.

### Head record

The head record of a collection is the stored datum that a receive meets next.
For a queue, it is the record with the least `ord`, and the bincode bytes of two records with one `ord` decide between them.
The list order of the hot store does not count. The hot store inserts at the front, and a restart can load a list in another order.
A head record that does not match a receive blocks every record behind it.

### Head waiter

The head waiter of a collection is the waiter that a send meets next.
For a queue, it is the waiter with the least `ord`, and the bincode bytes decide a tie.
A head waiter that does not match a sent datum blocks every waiter behind it, and the datum joins the records.
So only the head record and the head waiter can interact, which is [head-only matching](#head-only-matching).

### Fire round

A fire round is one step of the fire loop of a collection operation.
In a round, the incoming datum or continuation meets the head of the other side, and one COMM fires.
After the round, the operation ends, or the incoming side meets the next head, or the operation re-issues.
One space transaction runs at most `FIRE_ROUND_CAP` rounds, which is 64 ([decision I-21](decision-register.md#i-21)).

### Re-offer

A re-offer is the second produce of a datum that a peek fire did not store ([decision 30](decision-register.md#decision-30)).
A non-persistent send can fire a waiter that peeks and is not persistent.
Then the space agent stages the same produce again, as the next operation of the transaction.
The re-offer has the identity, the observer calls, the counter step, and the authority of the re-produce of the host reducer.
It fires the next matching waiter, or it stores the datum when no waiter matches.

### Re-store

A re-store is the second store of a stored record that a peek fire removed ([decision 45](decision-register.md#decision-45), Rule E).
A peek fire removes each stored non-persistent record that it binds, on the consume side and in a produce-side join, as a host COMM does.
The space agent then stages a produce of the same record bytes in the same transaction.
So the re-store has the produce identity of the first send, and the record keeps its envelope and its order.
It registers the cost authority of the record, as the host reducer's `produce_peeks` does, and it does not bump a counter.
*Distinguish from* a [re-offer](#re-offer), which sends the incoming datum of a produce-side peek fire again.

### Re-issue

A re-issue runs the rest of a space operation as a new space intent ([decision 34](decision-register.md#decision-34), item A10, and [I-21](decision-register.md#i-21)).
The fire loop of an operation re-issues in two cases, after it commits the staged prefix of its transaction.
In the first case, a divergent pair fired: the incoming side survives, and its partner stays in the store. An example is a persistent send `!!` that fires a persistent receive `<=`.
In the second case, the loop reached `FIRE_ROUND_CAP`, which is 64 fires.

A re-issue repeats the operation as the program issued it.
So a divergent pair fires once in each new space intent, until the phlo of the deploy runs out.

A persistent operation also keeps the `ord` of its first round ([decision 43](decision-register.md#decision-43)).
So it keeps its record, its identity, and its place in the collection.
Between two rounds the operation is in flight, and other operations can run.
When it re-enters, it fires only if its side is empty or its `ord` is less than the `ord` of the head.
Otherwise the space stores or parks it at its place.
*Distinguish from* a [re-offer](#re-offer), which offers a surviving datum again inside the same transaction.

### Path marker

A path marker is a datum that records a stored PathMap datum at one of its prefixes.
For a datum at path `p`, the space stores one path marker at the path-index channel of each prefix $`q \preceq p`$, including $`q = p`$.
A COMM event that removes the datum also removes all of its path markers.
A peek removes the marker of its COMM, and its second send stores that marker again or spends the datum ([decision 45](decision-register.md#decision-45), Rule E).
Path markers make a send and a waiter on related paths touch a shared channel, so merge check #2 sees the contact.

### Index channel

The index channel $`I(\mathit{sid}, \mathrm{prefix\_key}(q))`$ of a PathMap space holds the waiters on the prefix $`q`$.
It also holds one [path marker](#path-marker) for each stored datum under $`q`$.

### Vehicle

The vehicle of a PathMap datum on a prefix $`q`$ is the [path marker](#path-marker) of the datum on $`q`$, followed by its payload.
It is the incoming record of a delivery to a waiter on $`q`$, and no store keeps it ([decision 40](decision-register.md#decision-40), Q3).

### Token

The token of a PathMap space is its [space counter](#space-counter) $`S(\mathit{sid})`$.
Every operation of an ordered discipline bumps it, and so does every change of state of a bag or a set ([decision 51](decision-register.md#decision-51)).

### Prefix source

The prefix source is the store seam `JoinSource` of the [join engine](#join-engine) for a PathMap space ([design N13](design.md#n13-wave-w3-of-p5-the-pathmap-engine)).
Each collection of its [join scope](#join-scope) is a prefix of a path, with the [index channel](#index-channel) of that prefix.
The window of a bind on a prefix holds the datums of each path under the prefix, re-rooted under the suffix of the path.
A delivery uses the [vehicle](#vehicle) of the datum, and a removal retires the datum and its other [path markers](#path-marker).

### LFS

LFS means Last Finalized State.
A node that joins a network downloads the last finalized state and the recent blocks around it, and then replays only above that horizon.
The [joining guide](../../node/joining-a-network.md#what-a-joiner-needs) describes the process.

### Conservative extension

A conservative extension adds new programs and keeps the meaning of every old program.
The Reified parser is a conservative extension of the baseline parser ([decision 23](decision-register.md#decision-23), [decision 24](decision-register.md#decision-24)).
Every program that parses at the baseline parses and normalizes to identical bytes with the new parser.

### Headline

A headline is a main result that a phase names and that a formal model proves or checks.
A Rocq headline is a theorem with an id such as H1, and its `Print Assumptions` line must report no axiom.
A TLA+ headline is an invariant with an id such as T1, and TLC checks it on every reachable state of a configuration.
The [verification dossier](verification.md) lists the headlines of each phase.

### Negative control

A negative control is a check that must fail.
It changes one rule of a model, a test, or an input, and it requires the expected failure.
So it shows that a passing check depends on that rule.
Examples are a mutant of a Rocq module, an Unsafe configuration of a TLA+ model, and a replay with another seed.

### Model map

The model map is the file `formal/reified-rspaces-model-map.json`.
It links each new Rust item of a phase to the Rocq definitions and the TLA+ operators that model it.
It also links each item to the headlines that it covers.
The checker `check-model-map.py` fails on an item that no model covers and that names no reason.

### Invariant map

The invariant map is the file `formal/reified-rspaces-invariant-map.json`.
It links each headline and each invariant to the property-based tests that check it, in one section for each phase.
The checker `check-invariant-map.py` fails on an invariant without a test, and on a property test that the map does not name.

### Witness test

A witness test is a deterministic test that shows one case of a headline or of a fix rule.
The invariant map lists the witness tests of a phase beside its property-based tests.

### Exact cut

An exact cut is a [host work](#host-work) budget that ends exactly at the first read of one step of an operation.
The determinism tests use exact cuts to make an operation fail at a chosen step.
They then check that the failed operation changes no row, no event, no charge and no root.

### Consensus-preservation checklist

The consensus-preservation checklist is the list of 22 properties of ordinary execution that each phase of the campaign must keep.
Examples are an unchanged COMM produce sort and an unchanged replay acceptance.
The file `scripts/verify-reified/consensus-checklist.json` binds each item to the checks that prove it.

### Footprint policy

A footprint policy lists the files that a phase must not change and the exact edits that it may make.
The checker `check-footprint.py` compares each file with its SHA-256 digest at the base revision.
The base revision is `4d67b5ccc`, the tip of `feature/cost-accounted-rho` that the campaign merged on 2026-10-05.
Before the merge, it was `6f7a80da3`, the commit that the campaign starts from.
So the check needs no git history, and it also runs in the sandbox of pgmcp.

### Regression corpus

A regression corpus is a file of expected outputs that a capture run recorded from the code of HEAD.
A regression test runs the same programs and compares its outputs with the corpus, byte for byte.

### Gate

A gate is a script that runs a group of checks and fails when one check fails.
A missing tool counts as a failure, never as a skip.

### Verification recipe

A verification recipe is a pgmcp record of the commands of a check, with their pinned inputs and limits.
Each command is a step with a key, such as `p1-props`.
A criterion of a leaf binds to one step of a recipe.

### Acceptance category

An acceptance category is one of the six kinds of evidence that each leaf needs.
They are: formally modeled, formal verification passes, every invariant extracted as a property-based test, implementation complete, thoroughly tested, and documented.
A category that does not apply to a leaf has no criterion, and the leaf states the reason.

### Allocation job

An allocation job allocates the index names of an Array or a Vector space ([specification AL-2](specification.md#24-the-allocator-of-array-and-vector-spaces)).
It takes the [allocator](#allocator) record once and puts the next value once, or it fails with `OutOfNames` and stages nothing.
A store without an allocator answers `Baseline`, and the job then commits nothing.
The job takes the cost authority of the process that asks for the names.

### Allocator

The allocator of an Array or a Vector space is its record $`A(\mathit{sid})`$, which holds the next index.
The creation of the space stages the record at 0, so two branches that allocate from one space race on it.

### Canonical order

The canonical order is the order of DR-75 over the entries of a side of a [collection](#collection) ([decision 41](decision-register.md#decision-41)).
It sorts the entries by the hash of their source operation, then by digest inside a run of equal hashes, then by index.
The order depends on the entry set only.

### List order

The list order is the order in which the [hot store](#hot-store) lists the entries of one channel.
A new entry goes to the front of the list, and a checkpoint sorts the list by the encoded bytes of its entries.
No choice of a space agent depends on the list order, because the agents use the [canonical order](#canonical-order).
The host work of a canonical order still depends on the list order, because its sort charges each comparison that it makes.

### Capacity

The capacity of a side of a [collection](#collection) is the most entries that the side holds (`Discipline::CAPACITY`).
A cell has a capacity of one record and one waiter. A store or a park on a full side fails with `CellOccupied`, and the failed operation applies nothing.

### Cell

A cell is a [collection](#collection) with one record slot and one waiter slot (F15).
A send and a receive always meet the other side, and only an element that does not fire needs a free slot.

### Class key

The class key of a payload groups equal payloads of a set (`ScanDiscipline::class_key`).
Records with equal keys form one class, and a consume of a member retires the class. A bag has no class key.

### Representative

The representative of a class of a set is its first member in [canonical order](#canonical-order).
A receive binds the representative first, and a peek binds only the representative ([specification ST-4](specification.md#35-set)).

### Class fire

A class fire is the consume over $`[D; n]`$ that a receive of a set makes on every stored member of a class ([specification ST-5](specification.md#35-set)).
It dispatches only the result of the [representative](#representative), and it removes every member that is not persistent.

### Constructor schema

The constructor schema of a [space family](#space-family) lists the named params that its [factory](#factory) accepts and the params that it requires ([specification FA-4](specification.md#15-families)).

### Factory row

A factory row pairs a [space family](#space-family) with its factory URN and its [op channel](#op-channel) $`[\mathtt{0xE0}, i]`$.
A row is active only for a [registered family](#registered-family).

### Fire count

The fire count of a [space transaction](#space-transaction) counts its fires, the fires of its settles included ([decision 49](decision-register.md#decision-49)).
A transaction makes at most `FIRE_ROUND_CAP` fires. A plan whose transaction is full stages nothing and re-issues its operation.

### First admitting waiter

The first admitting waiter of a datum is the first [waiter](#waiter) in [canonical order](#canonical-order) whose pattern binds the datum and whose guard admits it.
A send to a bag fires that waiter.

### Index name

An index name is a name $`N(\mathit{sid} \Vert \mathrm{u64\_be}(i))`$ of an Array or a Vector space, with the index $`i`$ as its 8-byte tail.
Only the [allocator](#allocator) of the space makes index names, so a program never forges one.

### Lift

A lift takes a stored record out of its [collection](#collection) during a [settle](#settle), with a consume of the continuation `K_lift`.
The settle then offers the record again with its own identity.

### Param rule

A param rule accepts the value of one constructor param and gives its canonical form ([specification FA-5](specification.md#15-families)).

### Registered family

A registered family is a [space family](#space-family) with a [factory row](#factory-row) and a dispatch arm.
Only a registered family has a URN binding and an installed factory.

### Scan discipline

A scan discipline lets every entry of a side meet an incoming element. The bag and the set are scan disciplines, and their plans are the [scan driver](#scan-driver).

### Scan driver

The scan driver holds the plans OFFER-SCAN, RECEIVE-SCAN, RESTORE-SCAN and RETIRE-CLASS of the [scan disciplines](#scan-discipline) ([design N11.2](design.md#n112-the-scan-driver)).

### Second send

The second send of a peek sends the peeked non-persistent record again, as the reducer of the host does ([decision 45](decision-register.md#decision-45), Rule E).
It has the identity and the cost authority of the record, and it is offered to the waiters before it is stored.

### Level

A level of a priority queue is the FIFO queue of the records with one priority ([decision 38](decision-register.md#decision-38)).
The levels ascend by priority, and the unprioritized level comes last. A record holds its level in the priority slot of its envelope.

### Lane

A lane of a priority queue is the FIFO queue of the waiters with one priority.
The waiters without a priority wait in the unrestricted lane.
The lane of a priority sees the head of its [level](#level), and the unrestricted lane sees the heads of all levels.

### Settle

A settle fires the pairs of entries that a fire exposes below the heads of a stack or a priority queue ([specification SK-3](specification.md#34-stack), [PQ-8](specification.md#36-priority-queue)).
A collection settles before and after each plan, and in a settle job, which is a space operation of its own.
The [join engine](#join-engine) settles the collections of a [join scope](#join-scope) with its [cascade](#cascade).

### Tested prefix

The tested prefix of a plan holds the candidates that the element in flight failed. The plan does not test them again.

### Join scope

The join scope of an operation is the set of collections that the join groups of its space connect to its own collections ([specification JN-2](specification.md#38-joins)).
The scope also holds the lock keys of these collections. The [join engine](#join-engine) locks the whole scope before it stages anything.

### Join slot

A join slot is the part of the record of a join waiter for one position of its [canonical group](#canonical-group).
It holds the order of the waiter in the collection of the position and the index of the bind in the receive.
It also holds the [lane](#lane) of a bind on a priority queue.
Version 2 of the continuation envelope holds one slot for each position. So a fire can put its results back into the bind order of the receive.

### Join engine

The join engine runs every space operation that a [join](#join) can reach ([decision 29](decision-register.md#decision-29)).
These are the receives with two or more binds on one space and the operations on a collection with a join group of other channels.
The engine searches the windows of every collection of a group.
It stages the fires, the [second sends](#second-send), and the [cascade](#cascade) on one [fire count](#fire-count).

### Cascade

A cascade fires each waiter that a fire exposes in a [join scope](#join-scope), in the same transaction ([specification JN-10](specification.md#38-joins)).
It takes the first non-persistent record of the waiter out of its collection with a [lift](#lift), and it offers that record to the waiter again.
The head-only disciplines and the set cascade ([decision 50](decision-register.md#decision-50)), and a bag needs no cascade.
At the fire cap, the transaction re-issues a settle job that carries the rest of the cascade.

### Metered view read

A metered view read loads a part of the [staged view](#staged-view) of a [space transaction](#space-transaction), and it reserves the [host work](#host-work) of the load first.
It charges the key of the load and the loaded value, the same on a cache hit and on a cache miss ([decision 47](decision-register.md#decision-47)).
So a warm load and a cold load reserve the same work.

### Journal row

A journal row records one operation of a native deploy: its source, the footprint that it locks, its predecessors and its outcome.
The outcome is Stored, Matched with the COMM of its candidate, or a denial of its introduction or of its COMM.
The predecessors of a row are the last earlier rows that touch the channels of its footprint.

### Native play

Native play runs a [deploy](#deploy) with the native phlo budget of protocol version 6.
It records one [journal row](#journal-row) for each operation, for host operations and space operations alike.
A space transaction of native play reads through [metered view reads](#metered-view-read) ([decision 47](decision-register.md#decision-47)).

### Ticket

A ticket is the reservation of one [journal row](#journal-row) while its operation runs again.
An operation gets the ticket of its row only when the predecessors of the row are complete.
Publishing the ticket completes the row, and dropping it returns the row to the available state.

### Native replay

Native replay runs a native deploy again against the journal of [native play](#native-play), as proposer self-certification and validator replay do.
Each operation takes the [ticket](#ticket) of its row and checks its footprint and its outcome against the row.
It then makes the observer calls of play.

### Native backend

The native backend replays the staged operations of a [space transaction](#space-transaction) in [native replay](#native-replay) (`NativeSpaceHost`, `rspace++/src/rspace/replay_rspace/native_session/space.rs`).
It never waits for a row: a row that is not ready fails the operation closed.
It fires a staged COMM only against the candidate that its row records ([decision 47](decision-register.md#decision-47)).

### Epoch closure

The epoch closure marks a native replay session as failed, so every operation that waits for a row wakes and fails.
The [native backend](#native-backend) closes the epoch when a staged operation fails, except after a published denial.
Without the closure, an operation could wait forever for the row of a failed operation ([verification §13.7](verification.md#137-the-forged-journal-probe-and-the-epoch-closure)).

### Exact value

The exact value of a number is the rational number that it denotes, with no rounding.
A float enters the numeric core of VectorDB spaces and of `rho:lang:vector` with its exact value.
Every consensus decision of the core compares exact values ([decision 54](decision-register.md#decision-54)).

### Dyadic number

A dyadic number is a rational number of the form $`m \cdot 2^{e}`$ with integers $`m`$ and $`e`$.
Every integer and every finite float is a dyadic number, so the numeric core sums such numbers as integers at one common exponent.

### Rounding to nearest

The rounding to nearest of an [exact value](#exact-value) $`x`$ is the `f64` value nearest to $`x`$, with ties to even, written $`\mathrm{RN}_{53}(x)`$.
A delivered score and a float reply of `rho:lang:vector` are one rounding of an exact value, and never a rounding of a partial sum.

### Widest kind

The widest kind of the inputs of a call of `rho:lang:vector` is the kind of its widest input: integer, then rational, then float.
The reply of `add`, `scale` and `dot` has that kind ([decision 66](decision-register.md#decision-66)).

### VectorDB collection

A VectorDB collection is a [collection](#collection) of the families `hashmap:vectordb` and `vector:vectordb`.
Its records can carry a vector, and a receive selects records by a similarity query.
It has no [collection counter](#collection-counter), so every record and every waiter has the order 0.

### Embedding

The embedding of a record of a [VectorDB collection](#vectordb-collection) is its vector.
It is the send param `embedding`, or else the payload when the payload is one non-empty list of numbers.
A record without an embedding is never selected by a query with a vector.

### Selection order

The selection order puts the better exact score first, and it keeps the [canonical order](#canonical-order) for equal scores.
A higher similarity and a lower distance are better.
A query without a vector keeps the canonical order.

### Eligible set

The eligible set of a query is the list of the records that the query can select, in the [selection order](#selection-order).
A record is eligible when its id is among the ids of the query and its score is defined.
Its score must also pass the threshold or the radius of the query.

### List mode

A receive is in list mode when its query has `k` or `results = "all"`.
Its one bind binds the list of the first eligible records, and the receive fires in one COMM over all of them.
A receive without these keys is in single mode, and it binds one record.

### LIST-CASCADE

LIST-CASCADE is the [cascade](#cascade) of a [VectorDB collection](#vectordb-collection).
After each removal, it checks the parked waiters in [list mode](#list-mode) again.
It fires the first one whose list is not empty, binds its pattern and passes its guard.
So no parked waiter without a guard can fire when a transaction ends ([decision 66](decision-register.md#decision-66), LM-b).

### Record id

The record id of a record is the 32-byte hash of its source produce.
It stays the same over a re-store of Rule E and over a re-issue, and it is the value that the query param `ids` takes.

### Mirror

The mirror of a [VectorDB collection](#vectordb-collection) is a copy of its records in the ChromaDB database of one node.
A space has a mirror when its factory call names `mirror = "chroma"`.
The mirror is local to the node and never enters a state root, so consensus never reads it ([decision 58](decision-register.md#decision-58)).

### Net write

The net write of a record in one transaction is the write that the [mirror](#mirror) needs: an upsert, a delete, or nothing.
The first and the last event of the [record id](#record-id) in the transaction decide it.
A peek that stores its record again writes nothing.

### Mirror journal

The mirror journal of an evaluation keeps the last [net write](#net-write) of each record of each collection.
A final result with no errors publishes it to the writer of the node, and any other result drops it (M1, [decision 66](decision-register.md#decision-66)).

### Dirty collection

A dirty collection is a mirror collection that misses writes, because the writer of the node did not apply one of its batches.
The next repair of the mirror on a finalized block includes it.

### Mirror writer

The mirror writer of a node applies the published batches of the [mirror journal](#mirror-journal) to the [mirror](#mirror), in publication order.
A publication never waits. A batch that finds the queue full, or that fails after three attempts, makes its collection a [dirty collection](#dirty-collection).

### Watermark

The watermark of a mirror collection is the last finalized block whose state a repair applied to it.
It never decreases, and `ann` reports it with its answer.

### Reconciler

The reconciler of a node repairs the [mirror](#mirror) to the finalized state.
On each finalized block, it repairs each stale or [dirty collection](#dirty-collection).
The repair sets each registered channel to the records of the finalized state, and then it sets the [watermark](#watermark).

### Recorded process

A recorded process is a system process whose reply the dispatcher records on the produce event of the reply.
Replay produces the recorded reply and asks no service, so nodes with different services compute the same state.
`rho:vectordb:embed` and `rho:vectordb:ann` are recorded processes ([decision 57](decision-register.md#decision-57)).

### Advisory query

An advisory query asks the [mirror](#mirror) for the ids of the records nearest to a query, through `rho:vectordb:ann`.
Its answer is a hint: only the query param `ids` of a receive turns the ids into a decision, and the exact engine decides.

## References

<a id="ref-anww13"></a>[ANWW13] J.-P. Aumasson, S. Neves, Z. Wilcox-O'Hearn, and C. Winnerlein, "BLAKE2: simpler, smaller, fast as MD5," in *Applied Cryptography and Network Security (ACNS 2013)*, Lecture Notes in Computer Science, Springer, 2013, pp. 119–135. [doi:10.1007/978-3-642-38980-1_8](https://doi.org/10.1007/978-3-642-38980-1_8)

<a id="ref-bbg95"></a>[BBG95] H. Berenson, P. Bernstein, J. Gray, J. Melton, E. O'Neil, and P. O'Neil, "A critique of ANSI SQL isolation levels," in *Proceedings of the 1995 ACM SIGMOD International Conference on Management of Data*, ACM, 1995, pp. 1–10. [doi:10.1145/223784.223785](https://doi.org/10.1145/223784.223785)

<a id="ref-ch00"></a>[CH00] K. Claessen and J. Hughes, "QuickCheck: a lightweight tool for random testing of Haskell programs," in *Proceedings of the Fifth ACM SIGPLAN International Conference on Functional Programming (ICFP 2000)*, ACM, 2000, pp. 268–279. [doi:10.1145/351240.351266](https://doi.org/10.1145/351240.351266)

<a id="ref-ch88"></a>[CH88] T. Coquand and G. Huet, "The calculus of constructions," *Information and Computation*, vol. 76, no. 2–3, pp. 95–120, 1988. [doi:10.1016/0890-5401(88)90005-3](https://doi.org/10.1016/0890-5401(88)90005-3)

<a id="ref-fips-agents"></a>[FIPS-AGENTS] M. Stay, "Agents," F1r3fly Improvement Proposal, 2025-08-20. [FIPS repository, approved/2025-08-20-Agents/Agents.md](https://github.com/F1R3FLY-io/FIPS/blob/main/approved/2025-08-20-Agents/Agents.md)

<a id="ref-fips-rrs"></a>[FIPS-RRS] M. Stay and L. G. Meredith, "Reifying RSpaces," F1r3fly Improvement Proposal, 2025-09-26. [FIPS repository, approved/2025-09-26-Reifying-RSpaces](https://github.com/F1R3FLY-io/FIPS/blob/main/approved/2025-09-26-Reifying-RSpaces/Reifying%20RSpaces.md)

<a id="ref-gel85"></a>[Gel85] D. Gelernter, "Generative communication in Linda," *ACM Transactions on Programming Languages and Systems*, vol. 7, no. 1, pp. 80–112, 1985. [doi:10.1145/2363.2433](https://doi.org/10.1145/2363.2433)

<a id="ref-kkt19"></a>[KKT19] I. Konnov, J. Kukovec, and T.-H. Tran, "TLA+ model checking made symbolic," *Proceedings of the ACM on Programming Languages*, vol. 3, no. OOPSLA, pp. 1–30, 2019. [doi:10.1145/3360549](https://doi.org/10.1145/3360549)

<a id="ref-lam94"></a>[Lam94] L. Lamport, "The temporal logic of actions," *ACM Transactions on Programming Languages and Systems*, vol. 16, no. 3, pp. 872–923, 1994. [doi:10.1145/177492.177726](https://doi.org/10.1145/177492.177726)

<a id="ref-mer88"></a>[Mer88] R. C. Merkle, "A digital signature based on a conventional encryption function," in *Advances in Cryptology — CRYPTO '87*, Lecture Notes in Computer Science, Springer, 1988, pp. 369–378. [doi:10.1007/3-540-48184-2_32](https://doi.org/10.1007/3-540-48184-2_32)

<a id="ref-mr05"></a>[MR05] L. G. Meredith and M. Radestock, "A reflective higher-order calculus," *Electronic Notes in Theoretical Computer Science*, vol. 141, no. 5, pp. 49–67, 2005. [doi:10.1016/j.entcs.2005.05.016](https://doi.org/10.1016/j.entcs.2005.05.016)

<a id="ref-pb"></a>[PB] Google, "Protocol Buffers documentation." [protobuf.dev](https://protobuf.dev/)

<a id="ref-roc25"></a>[Roc25] The Rocq Development Team, "The Rocq Prover," Zenodo, 2025. The DOI names every archived release, and the newest archived release is 9.1.0. [doi:10.5281/zenodo.15149628](https://doi.org/10.5281/zenodo.15149628)

<a id="ref-yml99"></a>[YML99] Y. Yu, P. Manolios, and L. Lamport, "Model checking TLA+ specifications," in *Correct Hardware Design and Verification Methods (CHARME 1999)*, Lecture Notes in Computer Science, vol. 1703, Springer, 1999, pp. 54–66. [doi:10.1007/3-540-48153-2_6](https://doi.org/10.1007/3-540-48153-2_6)
