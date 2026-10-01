# Slashing Subsystem — Design Document

## Reading order

This is a **layered** document. Read straight through for a complete
mental model, or jump to the layer you care about.

| #  | File                                                                 | What you learn                                                                                                 |
|----|----------------------------------------------------------------------|----------------------------------------------------------------------------------------------------------------|
| 01 | [Introduction & motivation](01-introduction.md)                      | Why slashing exists, the threat model, design goals, related systems.                                          |
| 02 | [Glossary & notation](02-glossary-and-notation.md)                   | Every symbol, acronym, and term — defined before first use.                                                    |
| 03 | [Architecture](03-architecture.md)                                   | The five layers, thirteen sub-components, and how they interact (Diagram 01).                                  |
| 04 | [Detection & pipeline](04-detection-and-pipeline.md)                 | How an arriving block becomes a verdict, then a record, then a slash.                                          |
| 05 | [Storage & records](05-storage-and-records.md)                       | The DAG, the equivocation tracker, and the lock-free race that bug #2 closes.                                  |
| 06 | [Proposing & effect](06-proposing-and-effect.md)                     | How a `SlashDeploy` is assembled, signed, and executed in the PoS Rholang contract (Diagram 07).               |
| 07 | [Fork-choice & validator lifecycle](07-fork-choice-and-lifecycle.md) | How a slashed validator loses influence; the seven-state lifecycle (Diagram 06).                               |
| 08 | [Two-level slashing](08-two-level-and-collusion.md)                  | Why colluders are mutually destroyed; the BFT bound (Diagram 04).                                              |
| 09 | [Bug-fix manifest](09-bug-fixes-and-rationale.md)                    | The documented fixes and permitted Rust/Scala deltas, what each one was, why it was wrong, and how the post-fix code is correct. |
| 10 | [Bisimilarity (Rust ↔ Scala)](10-bisimilarity.md)                    | The headline observational-equivalence claim, and what "modulo" means (Diagram 10).                            |
| 11 | [Worked examples](11-worked-examples.md)                             | Ten end-to-end traces that exercise each component path (Diagrams 02, 03, 05, 09).                             |
| 12 | [Failure modes & recovery](12-failure-modes.md)                      | What goes wrong, why, and how the system recovers (transfer FIXME, lock race, stake-0, off-by-one density).    |
| 13 | [References](13-references.md)                                       | Citations with DOIs verified.                                                                                  |
| 14 | [Test plan](14-test-plan.md)                                         | Example-based, integration, and property-based test plan covering the documented use cases, theorem labels, and threat-model regressions. |
| 14a | [Tier architecture](14a-tier-architecture.md)                       | How the harness, Rocq oracle, and production adapter are kept observationally aligned.                          |
| 15 | [Decision records](15-decision-records.md)                           | Decisions and rejected alternatives for epoch-scoped authorization, slash candidate sources, duplicate justifications, and checked arithmetic. |

## How to use this document

1. **First read** — Start at §01 and walk through to §03. By the end
   you will know the threat model, every term, and the topology.
2. **Trace mode** — When debugging a particular slashing flow, jump
   to the relevant worked example in §11 and follow the diagram
   pointers backwards into §04 / §06 / §07.
3. **Audit mode** — When verifying that a particular component is
   correct, read §09 (bug-fix manifest) for the change rationale,
   then `../slashing-verification.md` for the proof, then the cited
   Rocq module.
4. **Cross-implementation mode** — When porting or comparing against
   the Scala or Ethereum reference, read §10 (bisimilarity) and
   §13 (references).

## Conventions

This document uses Unicode mathematical notation throughout. Symbol
definitions live in §02; if you see a symbol you don't recognize,
check §02 first.

- Rocq theorem names appear as `monospace`, with file:line citations
  like `Bisimulation.v:77`.
- TLA+ identifiers appear as `monospace`, e.g. `IsRealEquivocation`.
- Rust functions appear as `monospace::path`, e.g.
  `casper::handle_invalid_block`.
- Scala upstream paths use the Scala-style dotted form
  `coop.rchain.casper.MultiParentCasperImpl`.
- Sequence-number arithmetic uses Unicode minus (`s − 1`) in prose;
  ASCII (`s-1`) inside code-fenced excerpts to keep them
  mechanically transcribable.
- Boolean values are `⊤` / `⊥` in formal blocks, `true` / `false`
  in prose.
- Set equivalence (mutual containment) is written `≡`; strict
  function equality is `=`.

## Diagrams

All PlantUML source diagrams live at
[`../diagrams/`](../diagrams/). Click any rendered SVG in this
document to open the standalone image.

| #  | Diagram                                                                                                 | Used in  |
|----|---------------------------------------------------------------------------------------------------------|----------|
| 01 | [Component overview](../diagrams/01-component-overview.svg)                                             | §03      |
| 02 | [Admissible-equivocation slash flow](../diagrams/02-seq-admissible-equivocation.svg)                    | §04, §11 |
| 03 | [Ignorable-equivocation slash flow (post-fix #1)](../diagrams/03-seq-ignorable-equivocation-fixed.svg)  | §09, §11 |
| 04 | [Two-level slashing](../diagrams/04-seq-two-level-slashing.svg)                                         | §08, §11 |
| 05 | [Generic invalid-block dispatch (post-fix #3)](../diagrams/05-seq-invalid-block-dispatch-fixed.svg)     | §04, §09 |
| 06 | [Validator lifecycle](../diagrams/06-state-validator-lifecycle.svg)                                     | §07      |
| 07 | [PoS.slash() activity](../diagrams/07-activity-pos-slash-contract.svg)                                  | §06      |
| 08 | [Justifications → neglect data flow](../diagrams/08-dataflow-justifications-to-neglect.svg)             | §04, §08 |
| 09 | [Tracker race & locking fix](../diagrams/09-seq-tracker-race-and-fix.svg)                               | §05, §11 |
| 10 | [Specification ↔ Rocq ↔ TLA+ ↔ Rust correspondence](../diagrams/10-component-formal-correspondence.svg) | §10      |
| 11 | [Withdrawal transfer-failure fix](../diagrams/11-seq-withdrawal-flow-fix.svg)                           | §09, §11 |

## Status

The slashing subsystem is **specified, mechanized, and audited**. The
Scala-inherited bugs, Rust-introduced regressions, and deliberate
Rust-side widening are documented with proven-correct fixes or explicit
formal boundaries. Current-epoch slash authorization, received slash-deploy
authorization, checked sequence arithmetic, and duplicate-justification
validation are included in the formal and integration-test coverage.
