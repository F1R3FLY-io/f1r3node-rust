# Technical reports

Human-readable technical reports for scientists and engineers, distilled from the campaign
evidentiary ledgers under [`docs/design/`](../design/). The two genres divide cleanly:

- **A technical report** (this tree) is a *snapshot*: it presents the control and final treatment
  results of a campaign, ordered most-impressive first, with statistical plots and diagrams. It
  contains no intermediate results, no per-clause provenance tags, and no ledger glyph conventions.
- **An evidentiary ledger** (`docs/design/<topic>/<topic>-report-<date>.md`) is the *complete,
  living experiment log of record*: per-fix bodies, per-clause `DERIVED`/`MEASURED` provenance,
  bisection ladders, supersession chains, negative results, and discovery narratives. New evidence
  appends there first. Each ledger carries a reader-orientation banner pointing here, and each
  report here states the ledger as its source of record.

## Reports

| Report | Snapshot anchor | Evidentiary ledger |
|---|---|---|
| [Stack safety in the F1r3node Rholang interpreter](stack-safety/stack-safety-technical-report-2026-08-12.md) | `feature/mettail@9e44e570d` | [`stack-safety-report-2026-07-29.md`](../design/stack-safety/stack-safety-report-2026-07-29.md) |
| [PathMap-native EPathMap](pathmap/pathmap-technical-report-2026-08-12.md) | `feature/mettail@9e44e570d` | [`pathmap-report-2026-08-03.md`](../design/pathmap/pathmap-report-2026-08-03.md) |

**Anchor convention.** Every report names one *snapshot anchor* (the tree it describes) and the
*measurement anchors* its numbers were taken at (`repo@sha` pins). Results recorded after the
snapshot anchor live in the ledger until a new report revision is cut; the `SS-*` fix identifiers
are stable and never reused, so rows cited here resolve permanently.

## Data and reproducibility

- **Raw measurements** are the ledgers' durable TSVs (`docs/design/*/measurements/*.tsv`),
  referenced in place — never copied. Reproduction commands live in each ledger's Appendix A.
- **Curated figure/table data** lives in each report's `data/*.tsv`; every row carries a
  `ledger_section` and a verbatim `source_quote` so its provenance is machine-checkable.
- **pgmcp experiment ledger**: headline comparative claims are registered as pgmcp experiments
  (preregistered where the campaign preregistered them — e.g. E-8b, experiments 170/171 — and
  explicitly marked *retrospective* where this tree registered published ledger numbers after the
  fact: experiments 175, 176, 177). Each report's Data Availability appendix maps claim →
  experiment → TSV → ledger section.
- **`tools/mk_tables.py`** (Python 3, stdlib only) generates every register-derived table and the
  register-disposition figure from the curated data, and verifies them:
  `register` · `fixed-scale` · `emit` · `check` (byte-compares the tables embedded in the reports
  against fresh generations) · `provenance` (verifies every `source_quote` against its ledger
  section) · `coverage` (asserts the 96-identifier census) · `register-map` (emits the Figure S7
  PlantUML source). `tools/.fragments/` is transient build output and is never committed.

## Figures

Statistical plots are Wolfram Language sources (`figures/*.wl`) committed beside their rendered
SVGs; each script imports its data TSV and re-exports the SVG
(`wolframscript -file <name>.wl`; matplotlib may substitute, keeping filenames). Diagrams are
PlantUML (`figures/*.puml`, rendered with `plantuml -tsvg`). Ledger figures are embedded from
`../design/*/figures/` in place. All figures use the ledger Appendix E.1 concept palette
(one concept = one colour); before/after pairs are slate `#455A64` / green `#2E7D32`
(colour-vision-deficiency checked), and every mark carries a direct text label.

## Validation

Both reports (and the banner-edited ledgers) are held to the pgmcp documentation guidelines via
`mettail-rust/docs/languages/validate.sh <file>...` (document-level checks: balanced fences,
GitHub-renderable math delimiters, no inert math spans, citations with valid DOIs, terms defined
before use, literate pseudocode, valid snippets), plus this tree's own `mk_tables.py check`,
`provenance`, and `coverage` gates.
