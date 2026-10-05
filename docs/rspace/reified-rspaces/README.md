# Reified RSpaces

Reified RSpaces let a [Rholang](glossary.md#rholang) program create [tuple spaces](glossary.md#tuple-space) and use them as values [[FIPS-RRS](#ref-fips-rrs)].
Each [space](glossary.md#space) chooses an [outer store](glossary.md#outer-store) for its channels and an [inner collection](glossary.md#inner-collection) for the items at each channel.
A [factory](glossary.md#factory) creates a space and returns a [handle](glossary.md#handle).
A program then sends and receives on the names of the space, inside a `use` block or with a `new x : s` annotation.
Every space persists as ordinary channels in the one [RSpace](glossary.md#rspace) of the node.
So the existing [checkpoint](glossary.md#checkpoint), [replay](glossary.md#replay), and [merge](glossary.md#merge) machinery covers spaces, and the consensus code does not change ([decision 18](decision-register.md#decision-18), [M-1](decision-register.md#m-1)).

This directory documents the campaign that implements the [FIPS](glossary.md#fips) in this repository.
This README is the index and the reading order of the documents.

## Scope

- **In scope:** every outer store, inner collection, and [qualifier](glossary.md#qualifier) of the FIPS ([decision 1](decision-register.md#decision-1)).
  The scope also covers `use` blocks, factories, `getSpaceAgent`, theories, and [user agent-backed spaces](glossary.md#user-agent-backed-space) ([decision 8](decision-register.md#decision-8)).
- **Held:** the MeTTaTron space, which the MeTTa engine would back ([decision 2](decision-register.md#decision-2)).
- **Owned by other agents:** the Bag and PathMap work outside the FIPS ([decision 1](decision-register.md#decision-1)), and the [F1r3lang](glossary.md#f1r3lang) implementation ([decision 3](decision-register.md#decision-3)).
- **Gated:** the components that depend on F1r3lang wait for the readiness signal of the user (phase P12).
- **Out of scope:** activation and rollout ([decision 27](decision-register.md#decision-27)).

## Reading order

1. Read the [glossary](glossary.md) first. Each later document uses its terms and links its anchors.
2. Read the [decision register](decision-register.md) for the normative decisions and their reasons.
3. Read the specification, the syntax contract, and the agent API for the behavior of spaces.
4. Read the design, the PathMap delivery, and the theory seam for the structure of the implementation.
5. Read the security, performance, and verification documents and the FIPS matrix for the evidence.
6. Read the F1r3lang integration document for the merge path of phase P12.
7. Read the [design record](design-record/README.md) last, as history of the early design.

## Documents

The table lists every campaign document.
A document that a later phase writes shows the phase as "planned in Pn" and names its tracker leaves.

| Document | Content | Status |
|---|---|---|
| [README.md](README.md) | This index: scope, reading order, documents, diagrams, phases, and the checking procedure. | Written |
| [glossary.md](glossary.md) | Every campaign term with a stable anchor, in dependency order, with the background terms of Rholang, RSpace, and the verification tools. | Written |
| [decision-register.md](decision-register.md) | The normative register: decisions 1 to 35, M-1 to M-5, and I-1 to I-19, with dates, words of the user, reasons, and supersession links. | Written. `rrs-doc-design-record` (P10) keeps it current. |
| `specification.md` | The normative behavior of spaces: outer stores, disciplines, qualifiers, the sweep, the VectorDB bounds, and the vector operations. | Planned in P4 to P7, which add sections. P10 completes it (`rrs-perf-docs-final-spec-design-security`). |
| `fips-conformance-matrix.md` and `.json` | One row for each normative clause of the FIPS and of the agent section of the Agents FIPS, with code, proof, test, and document evidence. | Planned in P10 (`rrs-fips-clause-matrix`). P11 runs it on the frozen candidate. |
| [design.md](design.md) | The module structure: types, the agent traits, the generic space algorithm, the space transaction, and the metrics source. | The substrate section of P1 is written. The port leaves add sections from P4, and P10 completes the document. |
| `agent-api.md` | Factories, handles, methods, replies, errors, `getSpaceAgent`, space references, and user agent-backed spaces. | Planned in P4 and P8, which add sections. P10 completes it (`rrs-doc-agent-api`). |
| `theory-seam.md` | Admission verdicts, builtin theories, theory identity, charging, guard composition, and the path after the F1r3lang merge. | Planned in P7 (`rrs-theory-seam`, `rrs-reducer-theory-admission`). P10 completes it (`rrs-doc-theory-seam`). |
| `pathmap-prefix-delivery.md` | Paths, prefix keys, re-rooting, path markers, waiters, and the merge behavior of PathMap spaces. | Planned in P5 (`rrs-pathmap-prefix-delivery`). |
| [syntax-contract.md](syntax-contract.md) | The surface syntax, the marker-tuple encoding table, the Par builders, and the dual-frontend corpus. | The P3 sections are written: the grammar forms, the normalization mapping with the builders and the marker tuples, the errors, the keyword rule, and the corpus. P4 adds sections, and P12 adds the Rholang 1.4 productions. |
| [verification.md](verification.md) | The verification dossier: Rocq headlines, TLA invariants, negative controls, property tests, regression suites, the realignment closure, and later the example gate. | The P1 sections are written. Each later phase adds the sections of its models and suites. P10 completes it (`rrs-doc-verification-dossier`). |
| `security.md` | Capability checks, private channels, input validation, query limits, failure atomicity, and a threat model. | Planned in P10 (`rrs-perf-docs-final-spec-design-security`). |
| `performance.md` | Benchmarks of the space operations against frozen baselines, with any regressions. | Planned in P10 (`rrs-perf-docs-final-benchmarks`). |
| `f1r3lang-integration.md` | The F1r3lang merge: conflict resolutions, guard composition, and the Rholang 1.4 productions. | Planned in P10 (`rrs-doc-theory-seam`). P12 extends it after the readiness signal. |
| [design-record/](design-record/README.md) | The design ledger of 2026-08-15 with its provenance record. It is history and is exempt from the documentation lint. | Written |
| `../../rholang/21-reified-spaces.md` | The language chapter on Reified spaces, with the section "Writing your own space". | Planned in P10 (`rrs-doc-language-chapter`). |

## Diagrams

The diagram sources are PlantUML files in [diagrams/](diagrams/), and [render.sh](diagrams/render.sh) renders them.
Each source includes the shared house palette and the campaign aliases in [concept-colors.iuml](diagrams/concept-colors.iuml), so each concept has one color in every diagram.

Text metrics set the size and the layout of each SVG, so the renderer uses a fixed font set.
The file [fonts.lock](diagrams/fonts.lock) pins each font file by its SHA-256 digest.
The renderer gives Java only these fonts, a private Java home directory, and a fixed locale.
Thus the same source gives the same SVG bytes on a development host and in the pgmcp verifier sandbox.
If a font package upgrade changes a pinned digest, the renderer stops before it renders.
Update `fonts.lock`, render every diagram again, and review the images.

| Number | Diagram | Status |
|---|---|---|
| 01 | [Substrate components](diagrams/01-substrate-components.svg) | Drawn for the substrate in P1. P10 extends it to all components (`rrs-doc-diagrams`). |
| 02 | [Namespace bytes](diagrams/02-namespace-bytes.svg) | Drawn |
| 03 | Native handle lifecycle (sequence) | Planned in P4 (`rrs-agent-native-factory`) |
| 04 | User-agent translation (sequence) | Planned in P10 (`rrs-doc-diagrams`) |
| 05 | Head-only state machine | Planned in P10 (`rrs-doc-diagrams`) |
| 06 | [Commit of a space transaction](diagrams/06-space-txn-commit.svg) (activity) | Drawn in P1 |
| 06a | [Lifecycle of a space transaction](diagrams/06a-space-txn-lifecycle.svg) (state), a sub-figure of figure 06 | Drawn in P1 |
| 07 | [Checkpoint, replay, and sweep](diagrams/07-checkpoint-replay-sweep.svg) (sequence) | Drawn in P1. The restart suite adds the restart part (`rrs-tests-verification-restart-rehydrate`). |
| 08 | Merge flow of the counter conflict | Planned in P10 (`rrs-doc-diagrams`) |
| 09 | PathMap re-rooting and path markers | Planned in P5 (`rrs-pathmap-prefix-delivery`) |
| 10 | Theory admission | Planned in P7 (`rrs-theory-seam`) |
| 11 | Layout of the record envelope | Planned in P10 (`rrs-doc-diagrams`) |
| 12 | Routing decision | Planned in P10 (`rrs-doc-diagrams`) |
| 13 | [Verification pipeline](diagrams/13-verification-pipeline.svg) (component) | Drawn for the checks of P1. P10 extends it to all phases (`rrs-doc-diagrams`). |
| 14 | Phase Gantt chart | Planned in P10 (`rrs-doc-diagrams`) |
| 15 | [Normalization of the Reified syntax](diagrams/15-syntax-normalization.svg) (activity) | Drawn in P3 |

## Campaign phases

The realignment plan divides the campaign into thirteen phases.
The critical path is P0, P1, P2, P4, then P5 to P7 in parallel, then P9 and P11.
P3 runs in parallel in the parser worktree.

| Phase | Goal |
|---|---|
| P0 | Backup with a restore drill, the parser move, the restore of the node worktree, the ledger copy, and tracker hygiene |
| P1 | The space transaction substrate, with byte-identical ordinary execution |
| P2 | The marker-tuple encoding and the frontend-neutral layer |
| P3 | The v3 parser, the normalizers through the builders, and the dual corpus |
| P4 | The vertical slice of a HashMap queue space, from the factory to the cross-branch race |
| P5 | The bag, set, stack, cell, and priority queue disciplines, and the Array, Vector, HashSet, and PathMap stores |
| P6 | VectorDB, kernels, native vectors, and `rho:lang:vector` |
| P7 | Rehydration, temp and seq spaces, dangling handles, the theory seam, and charging |
| P8 | User agent-backed spaces |
| P9 | Consensus integration |
| P10 | Formal verification, the documents, the examples, the benchmarks, and the FIPS matrix |
| P11 | Full gates, the parser revision pin, and the unchanged-fixture gate |
| P12 | The F1r3lang merge, only after the readiness signal of the user |

## Normative status

The [decision register](decision-register.md) is normative.
It restates the decisions of the realignment plan, which the user approved on 2026-10-04.
The specification and the syntax contract state the required behavior in detail, and they follow the register.
The [design record](design-record/README.md) is history.
Where a document and the register differ, the register applies.

## Writing and checking the documents

Every campaign document follows the pgmcp documentation guidelines and ASD-STE100 Simplified Technical English [[STE](#ref-ste)].
The script [doc_lint.py](../../../scripts/verify-reified/doc_lint.py) checks the rules that a program can decide:

- Math uses the GitHub delimiters: a backtick span inside dollar signs inline, and a fence with the info string `math` for display.
- Links, anchors, glossary anchors, and citation keys resolve.
- Each diagram has a PlantUML source, uses the shared palette, and has a current SVG.
- Mermaid appears only for diagram types that PlantUML cannot draw.
- Prose keeps the STE limits on sentences, procedures, and paragraphs.
- Each Rholang snippet is a row of the syntax corpus or of the example manifest.
- Each DOI is well formed, and each Rocq citation names the cited declaration.

To check the documents, do these steps from the repository root:

1. Run `python3 scripts/verify-reified/doc_lint.py` to lint every campaign document.
2. Fix each reported line, which has the form `path:line: rule: detail`.
3. After a change to a diagram source, run `docs/rspace/reified-rspaces/diagrams/render.sh`.
4. Run `docs/rspace/reified-rspaces/diagrams/render.sh --check` to confirm that each SVG is current.
5. After a change to the linter, run `python3 -m unittest discover -s scripts/verify-reified/tests`.

The command `python3 scripts/verify-reified/doc_lint.py --list-rules` prints the rule catalog.
The renderer writes its scratch files under `target/verification/reified-rspaces/` and never under `/tmp`.

## References

<a id="ref-fips-rrs"></a>[FIPS-RRS] M. Stay and L. G. Meredith, "Reifying RSpaces," F1r3fly Improvement Proposal, 2025-09-26. [FIPS repository, approved/2025-09-26-Reifying-RSpaces](https://github.com/F1R3FLY-io/FIPS/blob/main/approved/2025-09-26-Reifying-RSpaces/Reifying%20RSpaces.md)

<a id="ref-ste"></a>[STE] ASD, "ASD-STE100 Simplified Technical English," Issue 9, January 2025. [asd-ste100.org](https://www.asd-ste100.org/)
