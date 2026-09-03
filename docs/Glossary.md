# Glossary

> This glossary is **load-bearing**: documentation, design decisions
> (`docs/casper/theory/slashing/design/15-decision-records.md`), TDD plans
> (`docs/tdd-plans/`), and code review notes cite its anchors directly.
> Adding, renaming, or removing a term here is a documentation change,
> not a stylistic one.

This file keeps the repository's terminology consistent. It distinguishes the
canonical name for each concept from the near-synonyms that the codebase has
accumulated, and pins each name to a **Preferred usage** statement that
describes when the term applies and what to use instead in adjacent contexts.

The slashing subsystem's formal symbol table, acronym list, LTS labels, and
theorem-naming conventions live in
[docs/casper/casper/theory/slashing/design/02-glossary-and-notation.md](casper/theory/slashing/design/02-glossary-and-notation.md),
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

### Failed-body settlement

See [Failed-body settlement](casper/GLOSSARY.md#failed-body-settlement) in the
Casper glossary.

### Adopted lifecycle state

See [Adopted lifecycle state](casper/GLOSSARY.md#adopted-lifecycle-state) in the
Casper glossary.

### State-effect identity

See [State-effect identity](casper/GLOSSARY.md#state-effect-identity) in the
Casper glossary.

### Exact state containment

See [Exact state containment](casper/GLOSSARY.md#exact-state-containment) in the
Casper glossary.

### State witness

See [State witness](casper/GLOSSARY.md#state-witness) in the Casper glossary.

### Settled floor set

See [Settled floor set](casper/GLOSSARY.md#settled-floor-set) in the Casper
glossary.


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

This term moved to the [Casper glossary](casper/GLOSSARY.md#verification-tier).

### Implementation tier

This term moved to the [Casper glossary](casper/GLOSSARY.md#implementation-tier).

### Model

This term moved to the [Casper glossary](casper/GLOSSARY.md#model).

### Configuration

This term moved to the [Casper glossary](casper/GLOSSARY.md#configuration).

### Cap timeout

This term moved to the [Casper glossary](casper/GLOSSARY.md#cap-timeout).

### Violation

This term moved to the [Casper glossary](casper/GLOSSARY.md#violation).

### Safety configuration

This term moved to the [Casper glossary](casper/GLOSSARY.md#safety-configuration).

### Liveness configuration

This term moved to the [Casper glossary](casper/GLOSSARY.md#liveness-configuration).

### Equivocation

This term moved to the [Casper glossary](casper/GLOSSARY.md#equivocation).

### Bond generation

This term moved to the [Casper glossary](casper/GLOSSARY.md#bond-generation).

### Validator lifetime

This term moved to the [Casper glossary](casper/GLOSSARY.md#validator-lifetime).

### Equivocation detector

This term moved to the [Casper glossary](casper/GLOSSARY.md#equivocation-detector).

### Slash closure

This term moved to the [Casper glossary](casper/GLOSSARY.md#slash-closure).

### Neglect graph

This term moved to the [Casper glossary](casper/GLOSSARY.md#neglect-graph).

### Block proposal

This term moved to the [Casper glossary](casper/GLOSSARY.md#block-proposal).

### Block creator

This term moved to the [Casper glossary](casper/GLOSSARY.md#block-creator).

### Deploy admission

This term moved to the [Casper glossary](casper/GLOSSARY.md#deploy-admission).

### Block validation

This term moved to the [Casper glossary](casper/GLOSSARY.md#block-validation).

### Consensus snapshot

This term moved to the [Casper glossary](casper/GLOSSARY.md#consensus-snapshot).

### Test node

This term moved to the [Casper glossary](casper/GLOSSARY.md#test-node).

### Rejected deploy buffer

This term moved to the [Casper glossary](casper/GLOSSARY.md#rejected-deploy-buffer).

### Merge scope

This term moved to the [Casper glossary](casper/GLOSSARY.md#merge-scope).

### Content ordering

This term moved to the [Casper glossary](casper/GLOSSARY.md#content-ordering).

### Prior-rejection count

This term moved to the [Casper glossary](casper/GLOSSARY.md#prior-rejection-count).

### Loss-aware adjudication

This term moved to the [Casper glossary](casper/GLOSSARY.md#loss-aware-adjudication).

### Kept rejection record

This term moved to the [Casper glossary](casper/GLOSSARY.md#kept-rejection-record).

### Carrier

This term moved to the [Casper glossary](casper/GLOSSARY.md#carrier).

### Occurrence carrier

This term moved to the [Casper glossary](casper/GLOSSARY.md#occurrence-carrier).

### Finalized state anchor

This term moved to the [Casper glossary](casper/GLOSSARY.md#finalized-state-anchor).

### Archive representative

This term moved to the [Casper glossary](casper/GLOSSARY.md#archive-representative).

### Retry gate

This term moved to the [Casper glossary](casper/GLOSSARY.md#retry-gate).

### Main-parent base bias

This term moved to the [Casper glossary](casper/GLOSSARY.md#main-parent-base-bias).

### Remedy ladder

This term moved to the [Casper glossary](casper/GLOSSARY.md#remedy-ladder).

### Merged-frontier retry packaging

This term moved to the [Casper glossary](casper/GLOSSARY.md#merged-frontier-retry-packaging).

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
  [02-glossary-and-notation.md](casper/theory/slashing/design/02-glossary-and-notation.md)
  until the planned unification lands.

## Maintenance

- Update this file before merging code or documentation that introduces a
  new domain term.
- Removing a term requires removing or rewriting every anchor that links to
  it; the `/review-codebase --glossary-only` audit flags unresolved anchors.
- Renaming a term: add the new entry, mark the old entry deprecated with a
  pointer to the new anchor, and migrate references over time.
