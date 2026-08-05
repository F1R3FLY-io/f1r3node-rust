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

### Verification tier

A verification tier is the CI budget class a formal check runs under: the
**PR-gate tier** (fast checks on every push and pull request), the **nightly
tier** (the eight fast TLC configurations that gate the scheduled
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
