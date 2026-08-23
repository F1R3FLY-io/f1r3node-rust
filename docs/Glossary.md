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

### Retry gate

This term moved to the [Casper glossary](casper/GLOSSARY.md#retry-gate).

### Main-parent base bias

This term moved to the [Casper glossary](casper/GLOSSARY.md#main-parent-base-bias).

### Remedy ladder

This term moved to the [Casper glossary](casper/GLOSSARY.md#remedy-ladder).

### Merged-frontier retry packaging

This term moved to the [Casper glossary](casper/GLOSSARY.md#merged-frontier-retry-packaging).

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
