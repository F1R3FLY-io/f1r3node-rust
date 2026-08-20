# Glossary

> This glossary is **load-bearing**: documentation, design decisions
> (`docs/theory/slashing/design/15-decision-records.md`), TDD plans
> (`docs/tdd-plans/`), and code review notes cite its anchors directly.
> Adding, renaming, or removing a term here is a documentation change,
> not a stylistic one.

This file keeps the repository's terminology consistent. It distinguishes the
canonical name for each concept from the near-synonyms that the codebase has
accumulated, and pins each name to a **Preferred usage** statement that
describes when the term applies and what to use instead in adjacent contexts.

The slashing subsystem's formal symbol table, acronym list, LTS labels, and
theorem-naming conventions live in
[docs/theory/slashing/design/02-glossary-and-notation.md](theory/slashing/design/02-glossary-and-notation.md),
which remains authoritative for mathematical notation. Unifying that document
into this one is planned once the current EPIC-011 tasks complete (see
`docs/Backlog.md`).

Run `/review-codebase --glossary-only` to audit anchor integrity and the
minimum-term floor.

## Domain

**protocol** — this repository implements a BFT consensus protocol node
(CBC Casper consensus, slashing enforcement, Rholang execution). Protocol
correctness — proved, model-checked, and tested — is the organizing concern.

## Canonical Terms

### Release candidate

A release candidate is one immutable source commit with its tested artifacts and [release evidence](#release-evidence). Standard release gates evaluate this identity.

**Preferred usage.** Use this term for the source, artifact, and evidence set. *Distinguish from* [Canary release](#canary-release): candidate identity versus candidate publication.

### Canary release

A canary release is the prerelease publication of a [release candidate](#release-candidate). Its Git tag, binaries, and versioned image tags are immutable.

**Preferred usage.** Use this term for the published prerelease. *Distinguish from* a mutable channel alias and a [stable release](#stable-release).

### Stable release

A stable release is a [release candidate](#release-candidate) that passed all mandatory gates. It uses a final Semantic Versioning tag.

**Preferred usage.** Use this term only after successful artifact promotion. *Distinguish from* [Canary release](#canary-release): final publication versus prerelease publication.

### Release evidence

Release evidence is the machine-readable record that binds gate results and artifact digests to one source SHA.

**Preferred usage.** Use this term for commit-specific release records. *Distinguish from* a dashboard latest verdict, which does not identify one candidate.

### Artifact promotion

Artifact promotion copies verified candidate bytes and image digests to stable release names. Artifact promotion does not rebuild the source.

**Preferred usage.** Use this term for canary-to-stable publication. *Avoid*: release build, when automation reuses candidate artifacts.

### Deployment Train

A Deployment Train is an independent release path that starts from a reviewed pull-request head SHA. A reviewed manifest controls each train.

**Preferred usage.** Use this term for the complete independent release path. *Distinguish from* a CI job, workflow run, or branch.

### 60h stability soak

The 60h stability soak is the fixed 60-hour pre-promotion soak of one release candidate on a multi-validator shard. A passing run is a mandatory gate for [stable release](#stable-release) promotion.

**Preferred usage.** Use this term for the pre-promotion release gate. *Avoid*: weekend soak. Machine identifiers keep the legacy values `weekend` and `weekend-60h` until a separate identifier migration. *Distinguish from* the [Dev integration soak](#dev-integration-soak): release gate versus integration monitoring.

### Dev integration soak

The dev integration soak is the scheduled variable-length soak of the `dev` integration branch. It publishes regression data and does not gate a release.

**Preferred usage.** Use this term for the scheduled integration-branch soak. *Avoid*: daily soak. The machine series key keeps the legacy value `daily` until a separate identifier migration. *Distinguish from* the [60h stability soak](#60h-stability-soak): integration monitoring versus a release gate.

### Test net

The test net is the continuously running network of shards that hosts [Shard soak-ins](#shard-soak-in) and serves select partners and customers. Its shards run stable releases; nodes that complete a soak-in period hold the [Anchor](#anchor) role. Unlike the per-iteration soak shards, the test net does not restart between runs.

**Preferred usage.** Use this term for the standing shard network. *Avoid*: long-running quorum of shards, standing quorum, and continuously running shard quorum. *Distinguish from* the casper test-network fixture, which is an in-process test helper, not infrastructure.

### Shard soak-in

A Shard soak-in is the post-promotion period in which a weekly [stable release](#stable-release) runs in the [test net](#test-net). The Shard soak-in measures node behavior with the current test net members, catches compatibility issues, and confirms that the new nodes stay up. Enrollment is scheduled for each stable release tag. The trigger is a stable release publication, which has passed the [60h stability soak](#60h-stability-soak) gate.

**Preferred usage.** Use this term for post-promotion test net trials. *Avoid*: Soak-in, without the Shard qualifier, in new prose. *Distinguish from* the [60h stability soak](#60h-stability-soak), which is a pre-promotion release gate on one candidate.

### Soak-in

Deprecated name for the [Shard soak-in](#shard-soak-in).

### Anchor

An Anchor is a node that completed its [Shard soak-in](#shard-soak-in) period and holds full membership in the [test net](#test-net).

**Preferred usage.** Use this term only after a completed Shard soak-in. *Distinguish from* a soaking node, which runs inside or adjacent to the test net without the Anchor role.

### Peak node RSS

Peak node resident set size (RSS) is the maximum combined memory use of all
shard nodes at one sampling time. The passive series reports the largest
iteration value in megabytes. The active series reports the median of
successful benchmark-segment peaks.

**Preferred usage.** Use this term for concurrent shard-node memory use.
*Distinguish from* [Peak node CPU](#peak-node-cpu): memory use versus processor
use. *Avoid*: host memory and separate per-node memory peaks.

### Peak node CPU

Peak node central processing unit (CPU) use is the maximum combined processor
use of all shard nodes at one sampling time. One hundred percent equals one
fully used processor core. Thus, multiple active cores can produce a value
above 100 percent. Each soak run reports the largest iteration value.

**Preferred usage.** Use this term for concurrent shard-node processor use.
*Distinguish from* [Peak node RSS](#peak-node-rss): processor use versus memory
use. *Avoid*: a single-node peak or a host-wide CPU peak.

### Finalization latency p95

Finalization latency p95 is the per-run median of 95th-percentile latency
values. The passive series uses `f1r3fly.propose.timing` `total_ms`
proposal-processing samples. The active series uses submit-to-finalize samples
from each controlled-load segment.

**Preferred usage.** Specify the passive series or the active series.
*Distinguish from* the other series because the two series use different
measurement boundaries. *Avoid*: using the two p95 metrics as synonyms.

### Too-far-ahead errors

Too-far-ahead errors are logged proposal rejections that occur when a proposal
is too far ahead of the last finalized block. Each soak run reports the sum of
these log events across its iterations.

**Preferred usage.** Use this term for the proposal-rejection count.
*Distinguish from* [Finalization latency p95](#finalization-latency-p95):
rejection count versus processing duration. *Avoid*: all proposal errors.

### LFB convergence spread

Last finalized block (LFB) convergence spread is the difference between the
largest and smallest LFB numbers across shard nodes in one sample. The
dashboard reports p95 and maximum run aggregates in blocks.

**Preferred usage.** Use this term to describe shard agreement on finalized
state. *Distinguish from* finalization distance from the block graph tip.
*Avoid*: block height and finalization latency.

### Verification tier

A verification tier is the CI budget class a formal check runs under: the
**PR-gate tier** (fast checks on every push and pull request), the **nightly
tier** (the fast TLC configurations that gate the scheduled
`slashing-tests` run), and the **exhaustive tier** (opt-in via
`RUN_EXHAUSTIVE_TLA=1`, dispatch-only, holding the configurations whose state
spaces exceed the per-config cap). Tier membership is defined in
`scripts/ci/check-tla-invariants.sh` and documented in
`docs/theory/slashing/design/14-test-plan.md` §14.6.

**Preferred usage.** Use when describing *where and when* a check runs and
what time budget it gets.
*Distinguish from* [Implementation tier](#implementation-tier): verification
tiers are CI budget classes; implementation tiers are code artifacts. A test
exercising all three implementation tiers can run in any verification tier.
*Avoid*: bare "tier" in new documents (ambiguous), "profile", "level".

### Implementation tier

An implementation tier is one of the three lock-step realizations of the
slashing pipeline defined in
`docs/theory/slashing/design/14a-tier-architecture.md`: **Tier 1 Production**
(the shipping Rust code), **Tier 2 Oracle** (the executable reference model),
and **Tier 3 Harness** (the synthetic test driver). The triple-bisimilarity
test pattern applies each event to all three and asserts agreement.

**Preferred usage.** Use when describing which realization of the pipeline a
piece of code or a test touches; qualify with the number and name ("Tier 2
Oracle") on first use in a document.
*Distinguish from* [Verification tier](#verification-tier): saying "the
exhaustive tier" never refers to Production/Oracle/Harness.
*Avoid*: "layer", "stage".

### Model

A model is a TLA+ specification of a slashing component: the base `.tla`
module together with its `MC_*.tla` instantiation module under
`formal/tlaplus/slashing/`. The detector model family is
`MC_EquivocationDetector*`; `MC_EquivocationDetectorEager_3v` is the only
three-validator detector model.

**Preferred usage.** Use for the mathematical object TLC explores. Say
"detector model" for members of the `MC_EquivocationDetector*` family.
*Distinguish from* [Configuration](#configuration): one model may be checked
under several configurations (safety, liveness, pre-fix regression).
*Avoid*: "spec"/"specification" for TLA+ artifacts — those words are reserved
for `docs/theory/slashing/slashing-specification.md`.

### Configuration

A configuration is an `MC_*.cfg` file instantiating a
[model](#model) for one TLC run: constant assignments plus the `INVARIANTS`
and/or `PROPERTIES` it checks. The configuration is the unit
`scripts/ci/check-tla-invariants.sh` iterates over, the unit
[verification-tier](#verification-tier) membership is assigned to, and the
unit the per-config cap applies to.

**Preferred usage.** Use when discussing what CI actually runs, times, or
moves between tiers.
*Distinguish from* [Model](#model): moving a configuration to the exhaustive
tier does not change the model it checks.
*Avoid*: "config file" for the pair of `.tla`+`.cfg` (that pair is the
model's instantiation; the configuration is specifically the `.cfg`).

### Cap timeout

A cap timeout is a [configuration](#configuration) exceeding
`TLC_PER_CONFIG_TIMEOUT` (default 45 minutes) of wall clock, killed by the
harness and reported distinctly from property failures. A cap timeout is a
**resource outcome, not a verdict**: it says nothing about whether the
checked property holds.

**Preferred usage.** Use for any red result caused by the wall-clock cap.
Load-bearing for EPIC-011: the exhaustive tier's red baseline is
timeout-red, and green comes from restructuring configurations to complete —
never from raising the cap without per-component attribution.
*Distinguish from* [Violation](#violation): only a violation indicts the
modeled algorithm; a cap timeout indicts the check's budget or structure.
*Avoid*: "failure" unqualified, "TLC error".

### Violation

A violation is a TLC counterexample: an invariant breach or temporal-property
failure accompanied by an error trace. A violation is the only red result
that indicts the modeled algorithm (or the model's faithfulness to it).

**Preferred usage.** Use only when TLC produced a trace. If the exhaustive
liveness passes ever complete and report one, EPIC-011's contingency applies:
stop and investigate the algorithm/model, do not tune the model until green.
*Distinguish from* [Cap timeout](#cap-timeout): both make CI red; they mean
different things and the CI script labels them differently.
*Avoid*: "bug" (a violation may instead reveal model infidelity).

### Safety configuration

A safety configuration is a [configuration](#configuration) checking
`INVARIANTS` only (e.g. `MC_EquivocationDetector_safety.cfg`). State-space
exploration without liveness-graph construction; completes in bounded time
proportional to distinct states.

**Preferred usage.** Use for the invariants-only half of a liveness/safety
split.
*Distinguish from* [Liveness configuration](#liveness-configuration): the
same model, the cheap half of the check.
*Avoid*: "invariant config", "fast config" (fast describes a tier, not a
property class).

### Liveness configuration

A liveness configuration is a [configuration](#configuration) checking
temporal `PROPERTIES`, which requires strongly-connected-component analysis
over the full behavior graph — the superlinear pass responsible for the
exhaustive tier's [cap timeouts](#cap-timeout). The existing
`MC_EquivocationDetector_liveness.cfg` demonstrates the pattern: split from
its combined configuration, it completes in seconds. The **liveness/safety
split** is the act of separating a combined configuration into a
[safety configuration](#safety-configuration) and a liveness configuration
with constants bounded to complete under the cap.

**Preferred usage.** Use for the temporal-properties half of a split, and
"liveness/safety split" for the restructuring pattern itself.
*Distinguish from* [Safety configuration](#safety-configuration): the
expensive half; bounding its constants must not reduce validator count below
the property's needs (three, for detector models).
*Avoid*: "temporal config".

### Equivocation

Equivocation is a validator signing two distinct blocks at the same sequence
number (Definition 4.1 of the slashing specification; taxonomy in
`02-glossary-and-notation.md` §2.6). The equivocation-class `InvalidBlock`
variants are `AdmissibleEquivocation`, `NeglectedEquivocation`, and
(post-fix) `IgnorableEquivocation`.

**Preferred usage.** Use for the offence itself.
*Distinguish from* [Equivocation detector](#equivocation-detector): the
offence versus the component that detects it.
*Avoid*: "double-signing" (Ethereum vocabulary; correct informally but not
canonical here).

### Equivocation detector

The equivocation detector is the detection-pipeline component that returns an
`InvalidBlock` verdict for a (validator, sequence) pair — the `detect(v, s)`
label in the slashing LTS, realized in Rust as `check_equivocations` and
modeled by the detector [model](#model) family.

**Preferred usage.** Use for the component; use "detector model" for its
TLA+ models. Detection of [equivocation](#equivocation) is inherently
multi-validator — which is why parking the only three-validator detector
model in the exhaustive tier left a coverage gap.
*Distinguish from* [Model](#model): the detector is code under verification;
a detector model is one artifact verifying it.
*Avoid*: "tracker" (the tracker is the storage-side record keeper, §05).

### Slash closure

The slash closure is the reverse-reachability fixed point over the
[neglect graph](#neglect-graph): `Closure₀ = DirectOffenders`;
`Closureᵢ₊₁ = Closureᵢ ∪ {v : NeglectEdges(v) ∩ Closureᵢ ≠ ∅}`. Its
properties are the T-11/T-12 theorem family
(`02-glossary-and-notation.md` §2.7.1).

**Preferred usage.** Use for the fixed-point operator and its result set.
*Distinguish from* [Neglect graph](#neglect-graph): the closure is computed
over the graph; they are not interchangeable.
*Avoid*: "slash set" (ambiguous with the LTS `Sl` state component).

### Neglect graph

The neglect graph is the directed evidence graph with validators as vertices
and an edge `neglecter → offender` wherever the neglecter cited an invalid
offender block without an accompanying slash
(`docs/theory/slashing/design/08-two-level-and-collusion.md` §08.2).

**Preferred usage.** Use for the evidence structure that
[slash closure](#slash-closure) traverses.
*Distinguish from* [Slash closure](#slash-closure): graph versus the
fixed point computed over it.
*Avoid*: "evidence graph" unqualified (the report action mutates which
edges are *active*; the neglect graph is the specific active-edge structure).

### Block proposal

Block proposal is the end-to-end process of selecting parents and deploys,
executing them, assembling and signing a candidate block, and self-validating
it before publication. The process consumes a [consensus snapshot](#consensus-snapshot),
applies [deploy admission](#deploy-admission), and finishes with
[block validation](#block-validation).

**Preferred usage.** Use for the end-to-end process of selecting parents and
deploys, executing them, assembling a block, and self-validating it; use
[Block creator](#block-creator) for the Rust module implementing that process.
*Distinguish from* [Block creator](#block-creator): the proposal is the process;
the creator is the Rust module implementing it.
*Avoid*: "proposer pipeline" and "create-block flow".

### Block creator

The block creator is the Rust module centered in
`casper/src/rust/blocks/proposer/block_creator.rs` that implements
[block proposal](#block-proposal), including [deploy admission](#deploy-admission),
state computation, assembly, and packaging.

**Preferred usage.** Use for the Rust module that implements
[Block proposal](#block-proposal); use "block proposal" for the process and
"validator" for the protocol participant.
*Distinguish from* [Block proposal](#block-proposal): the creator is a module;
the proposal is the process hidden behind its interface.
*Avoid*: "proposer" when referring specifically to the Rust module.

### Deploy admission

Deploy admission is the deterministic decision process that applies
eligibility, recovery, ordering, count limits, and byte limits before user
deploys enter a [block proposal](#block-proposal). It includes selection from
the [rejected deploy buffer](#rejected-deploy-buffer) but excludes Rholang
execution.

**Preferred usage.** Use for deterministic eligibility, recovery, ordering,
count limits, and byte limits applied before user deploys enter a proposed
block; use "execution" for running admitted deploys in Rholang.
*Distinguish from* [Block proposal](#block-proposal): admission decides which
user deploys may enter; proposal also selects parents, executes deploys,
assembles, signs, and self-validates.
*Distinguish from execution*: admission selects deploys; execution runs the
selected deploys and computes state effects.
*Avoid*: "deploy filtering" when recovery, ordering, or capacity policy is
also involved.

### Block validation

Block validation is the ordered classification of a received or self-created
block through structural, cryptographic, state-replay, equivocation, and
deploy checks. It consumes a [consensus snapshot](#consensus-snapshot) and
returns a valid, invalid, or exceptional outcome.

**Preferred usage.** Use for the ordered rules that classify a received or
self-created block; name the specific rule when discussing signature checks,
checkpoint replay, equivocation, or deploy constraints.
*Distinguish from* [Block proposal](#block-proposal): validation classifies a
block; proposal constructs one and invokes self-validation as its final step.
*Avoid*: "validation pipeline" when referring to one rule rather than the
whole ordered classification.

### Consensus snapshot

A consensus snapshot is the captured view of DAG metadata, selected parents,
justifications, deploy visibility, validator state, and shard configuration
used by [block proposal](#block-proposal) and [block validation](#block-validation).
It is treated as stable for the duration of either process.

**Preferred usage.** Use for the captured DAG and on-chain state consumed by
[Block proposal](#block-proposal) and [Block validation](#block-validation);
use "DAG" or "on-chain state" only for those constituent views.
*Distinguish from DAG*: the snapshot includes a DAG view plus parents,
justifications, deploy visibility, validator state, and configuration.
*Avoid*: "state" unqualified when the full captured view is intended.

### Test node

A test node is the in-process fixture that composes production-shaped Casper,
storage, runtime, and transport modules to drive integration scenarios. It
exercises [block proposal](#block-proposal) and [block validation](#block-validation)
without launching the production node runtime.

**Preferred usage.** Use for the in-process node fixture that drives Casper
integration scenarios; use "node" for the production runtime and "test
adapter" for a narrower dependency substitute.
*Distinguish from node/test adapter*: a test node composes production-shaped
modules into an in-process fixture; a test adapter substitutes one dependency
at a seam.
*Avoid*: "mock node" because the fixture contains substantial production
implementations.

### Rejected deploy buffer

The rejected deploy buffer is the persistent storage module holding
merge-rejected deploys that remain eligible for later
[deploy admission](#deploy-admission). Its contents survive beyond the block
whose [merge scope](#merge-scope) produced a rejection.

**Preferred usage.** Use for persistent storage of merge-rejected deploys that
may be admitted again; use "rejected deploys" for entries recorded in a block
body rather than the storage module.
*Distinguish from rejected deploys*: the buffer persists retryable work across
blocks; rejected deploys are block-body records of a particular merge result.
*Avoid*: "rejection cache" because persistence and retry eligibility are
load-bearing properties.

### Merge scope

Merge scope is the bounded ancestry whose state effects participate in
multi-parent merging for a [consensus snapshot](#consensus-snapshot). A merge
can place eligible work in the [rejected deploy buffer](#rejected-deploy-buffer)
when competing effects cannot all be retained.

**Preferred usage.** Use for the bounded ancestry whose state effects
participate in multi-parent merging; use "ancestor set" only for an
unconstrained graph traversal.
*Distinguish from ancestor set*: merge scope is bounded and semantically
selected for state merging; an ancestor set may be an unconstrained graph
traversal.
*Avoid*: "merge window" unless referring specifically to a numeric depth or
time parameter.

### Content ordering

Content ordering is the deterministic comparison of conflicting deploy chains
by content alone: total cost, then maximum single-deploy cost, then
lexicographic signature. The content of a deploy never changes, so content
ordering alone produces the same loser in every merge.

**Preferred usage.** Use for the content-deterministic comparison inside
conflict adjudication.
*Distinguish from* [Loss-aware adjudication](#loss-aware-adjudication):
content ordering is the tie-break that loss-aware adjudication subordinates.
*Avoid*: "cost ordering", because cost is only the first comparison key.

### Prior-rejection count

The prior-rejection count is the number of
[kept rejection records](#kept-rejection-record) for a deploy signature that a
merge can see in its view: the [merge scope](#merge-scope) plus the
base-lineage window. The count is on-chain data, so every validator derives
the same value for the same merge.

**Preferred usage.** Use for the consensus-visible priority input to
[loss-aware adjudication](#loss-aware-adjudication).
*Distinguish from* the lifecycle `rejection_count`, which is a node-local
observability value that includes duplicate records.
*Avoid*: "loss count" without qualification.

### Loss-aware adjudication

Loss-aware adjudication is the conflict-adjudication policy that ranks a
higher [prior-rejection count](#prior-rejection-count) above
[content ordering](#content-ordering). Every loss raises the priority of the
loser, so starvation stays bounded. The policy applies at all three
adjudication sites (issue #294, phase 1).

**Preferred usage.** Use for the phase-1 remediation policy of issue #294.
*Distinguish from* [Content ordering](#content-ordering): the fallback that
decides when prior-rejection counts are equal.
*Avoid*: "retry priority", which suggests a
[deploy admission](#deploy-admission) ordering change that did not occur.

### Kept rejection record

A kept rejection record is a rejection record without the duplicate flag. It
disputes a standing win of its deploy signature. It is the only record class
that counts toward the [prior-rejection count](#prior-rejection-count) and
that drives the retry disposition.

**Preferred usage.** Use when record provenance matters, such as priority
counting or [retry gate](#retry-gate) disposition.
*Distinguish from* a duplicate-flagged record, which testifies that the
effect of the signature is already present and disputes nothing.
*Avoid*: "valid record", because duplicate records are also valid consensus
content.

### Carrier

The carrier is the block that carried the rejected deploy copy that a merge
adjudicated. Each rejection record names its carrier. Recovery custody is
owner-scoped: only the sender of the carrier buffers the retry of that copy.

**Preferred usage.** Use for the block a rejection record names.
*Distinguish from* the recording block, which is the merge block whose body
holds the rejection record.
*Avoid*: "source block" without qualification.

### Retry gate

The retry gate is the rule that makes a retry legal only after the latest
[kept rejection record](#kept-rejection-record) of the signature settles
inside the frozen floor closure. The gate is a pure function of the block, so
every validator computes the same verdict (`PrematureDeployRetry`).

**Preferred usage.** Use for the floor-paced legality rule on re-proposal.
*Distinguish from* [Deploy admission](#deploy-admission) ordering: the gate is
a lower bound on when a retry may appear, not a selection policy.
*Avoid*: "retry timer" and "cooldown", because the gate keys on floor
settlement, not on wall-clock time.

### Main-parent base bias

Main-parent base bias is the starvation facet in which a merge bases on a
main parent that already commits the effect of a contender. The chain of the
retried deploy is then stale against the base, and the merge rejects it
correctly. A proposer that always bases on the contender side therefore
starves the retry structurally.

**Preferred usage.** Use for the phase-2 facet of issue #294
(`docs/casper/CONSENSUS_PHILOSOPHY.md` Section 2).
*Distinguish from* the content-ordering facet, which
[loss-aware adjudication](#loss-aware-adjudication) removed.
*Avoid*: "merge bias" without qualification.

### Remedy ladder

The remedy ladder is the ordered set of remedies for
[main-parent base bias](#main-parent-base-bias) in
`docs/casper/CONSENSUS_PHILOSOPHY.md` Section 5. The ladder orders options by
guarantee strength and risk, and escalation follows evidence (Principle P5).

**Preferred usage.** Use for the documented option set and its escalation
policy.
*Distinguish from* the decision record, which tracks what shipped and what
stays pending.
*Avoid*: "options list".

### Merged-frontier retry packaging

Merged-frontier retry packaging is [remedy ladder](#remedy-ladder) option B1:
the owner packages a gated retry only when its own tip already merges every
same-key contender the owner can see. The retry then executes fresh on top of
the settled contention instead of racing as a sibling. **Status: proposed** —
the phase-2 decision on issue #294 is pending, tracked as TDD plan behavior
B6.

**Preferred usage.** Use for ladder option B1, and state the proposal status
until the decision lands.
*Distinguish from* the [retry gate](#retry-gate): the gate is a consensus
legality rule; this packaging policy is node-local discretion on top of it.
*Avoid*: "retry deferral" without qualification.

## Architecture Stack Mapping

- **Formal-verification stack** = Rocq mechanization (`formal/rocq/`), TLA+
  [models](#model) (`formal/tlaplus/`), and Sage adversarial models
  (`formal/sage/`); artifacts that prove or model-check consensus properties.
- **Consensus** = the `casper` crate: detection pipeline
  ([equivocation detector](#equivocation-detector)), slashing, block
  validation, finalization.
- **Execution** = the `rholang` interpreter and `rspace++` tuple-space
  storage; deploys run here.
- **Transport** = the `comm` crate: P2P networking, Kademlia discovery,
  TLS 1.3.
- **Node surface** = the `node` crate: gRPC external/internal APIs, HTTP
  REST, CLI, REPL.

Two orthogonal tier taxonomies overlay this stack and must never be
conflated: [implementation tiers](#implementation-tier)
(Production/Oracle/Harness — code artifacts in the consensus stack entry) and
[verification tiers](#verification-tier) (PR-gate/nightly/exhaustive — CI
budget classes in the formal-verification stack entry).

## Usage Notes

- Canonical terms are case-sensitive; the **Preferred usage** statement is
  the source of truth.
- When a recommendation in a design document or discovery file uses a term
  not listed here, either add the term (looping `/review-codebase`
  Checkpoints G4–G7) or rephrase using an existing term.
- Generic engineering words (component, service, boundary, layer, API,
  module) are not banned in user prose but should not appear in
  architectural recommendations when a canonical term is available.
- Mathematical symbols, acronyms, LTS labels, and theorem names resolve in
  [02-glossary-and-notation.md](theory/slashing/design/02-glossary-and-notation.md)
  until the planned unification lands.

## Maintenance

- Update this file before merging code or documentation that introduces a
  new domain term.
- Removing a term requires removing or rewriting every anchor that links to
  it; the `/review-codebase --glossary-only` audit flags unresolved anchors.
- Renaming a term: add the new entry, mark the old entry deprecated with a
  pointer to the new anchor, and migrate references over time.
