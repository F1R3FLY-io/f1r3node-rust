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
3. Read the [specification](specification.md), the [syntax contract](syntax-contract.md), and the [agent API](agent-api.md) for the behavior of spaces.
4. Read the [design](design.md), the [PathMap delivery](pathmap-prefix-delivery.md), and the [theory seam](theory-seam.md) for the structure of the implementation.
5. Read the [security](security.md), [performance](performance.md), and [verification](verification.md) documents and the [FIPS matrix](fips-conformance-matrix.md) for the evidence.
6. Read the [F1r3lang integration](f1r3lang-integration.md) document for the merge path of phase P12.
7. Read the [TensorLogic design](tensor-logic/README.md) for the tensor language that P12 installs.
8. Read the [design record](design-record/README.md) last, as history of the early design.

## Documents

The table lists every campaign document.
A document that a later step of P10 writes shows the step as "planned in P10" with the step, and it names its tracker leaf.

| Document | Content | Status |
|---|---|---|
| [README.md](README.md) | This index: scope, reading order, documents, diagrams, phases, and the checking procedure. | Written |
| [glossary.md](glossary.md) | Every campaign term with a stable anchor, in dependency order, with the background terms of Rholang, RSpace, and the verification tools. | Written |
| [decision-register.md](decision-register.md) | The normative register: decisions 1 to 88, M-1 to M-5, and I-1 to I-21, with dates, words of the user, reasons, and supersession links. | Written, with every decision up to decision 88 |
| [specification.md](specification.md) | The normative behavior of spaces: outer stores, disciplines, qualifiers, the sweep, the VectorDB bounds, and the vector operations. | Written. P4 to P8 wrote the sections of their phases (Table 1 of the document). P10 step S5 added the final scope and the determinism of section 11 (`rrs-perf-docs-final-spec-design-security`). |
| [fips-conformance-matrix.md](fips-conformance-matrix.md) and [its JSON source](fips-conformance-matrix.json) | One row for each normative clause of the FIPS and of the agent section of the Agents FIPS, with code, proof, test, and document evidence. | Written in P10 step S4 (`rrs-fips-clause-matrix`). `check-fips-matrix.py` checks it and renders the Markdown from the JSON. P11 runs it on the frozen candidate. |
| [design.md](design.md) | The module structure: types, the agent traits, the generic space algorithm, and the space transaction. | Written. P1 wrote the substrate, and P4 to P9 wrote the parts N1 to N19 of the native spaces. P10 step S5 added part N20 on the persistence and the recovery of space state. |
| [agent-api.md](agent-api.md) | Factories, handles, methods, replies, errors, `getSpaceAgent`, space references, and user agent-backed spaces. | Written. P4 wrote the native calls, and P8 wrote the user agent-backed spaces. P10 step S5 added every family, the nine gates, an example or a test of each abort, and the current limits (`rrs-doc-agent-api`). |
| [theory-seam.md](theory-seam.md) | Admission verdicts, builtin theories, theory identity and its bytes, resolution, the admission of a send, the check at a match, charging, leases, and the evidence. | Written in P7 step 4 (`rrs-theory-seam`, `rrs-reducer-theory-admission`), and checked in P10 step S5 (`rrs-doc-theory-seam`) |
| [pathmap-prefix-delivery.md](pathmap-prefix-delivery.md) | Paths, prefix keys, re-rooting, path markers, waiters, and the merge behavior of PathMap spaces. | Written in P5 wave W3. Phase 1 gives the store, the markers, the vehicle, the delivery plans, and the formal models. Phase 2 gives the prefix source of the join engine, the joins over prefixes, and the integration tests (`rrs-pathmap-prefix-delivery`). |
| [syntax-contract.md](syntax-contract.md) | The surface syntax, the protobuf field table, the Par builders, and the dual-frontend corpus. | Written. P3 wrote the surface sections, P4 the meaning at run time, and P6 the VectorDB params. P10 step S5 added an invalid row under each grammar form. P12 adds the Rholang 1.4 productions. |
| [verification.md](verification.md) | The verification dossier: Rocq headlines, TLA invariants, negative controls, property tests, regression suites, the realignment closure, and later the example gate. | Written for every phase: P1 in sections 1 to 10, P4 in 11, P5 in 12, decision 47 in 13, P6 in 14 to 26, P7 in 27 to 32, P8 in 33, P9 in 34 and P10 in 35 (`rrs-doc-verification-dossier`). |
| [security.md](security.md) | The threat model: assets, adversaries, trust boundaries, the controls SC-1 to SC-22 with their rejected inputs and evidence, the failure behavior, and the residual risks. | Written in P10 step S5 (`rrs-perf-docs-final-spec-design-security`) |
| [performance.md](performance.md) | The cost of the space operations in time and phlo: the method, the results of each benchmark, the comparison with the legacy branch and the host program, the examples from end to end, and the findings with the candidate optimizations. | Written in P10 step S8 (`rrs-perf-docs-final-benchmarks`, `rrs-bench-matching`, `rrs-bench-lmdb`) |
| [f1r3lang-integration.md](f1r3lang-integration.md) | The F1r3lang merge: the theory seam after the merge, installed theories, guard composition, and the Rholang 1.4 productions. | The theory part is written in P7 step 4, and P10 step S5 checked it. P12 extends it after the readiness signal (`rrs-doc-theory-seam`). |
| [tensor-logic/README.md](tensor-logic/README.md) | The TensorLogic design for P12: the design in five points, the reading order, the findings of the design pass and the binding decisions. | A proposal for P12, written in P9. The grammar waits for the approval of the user ([decision 64](decision-register.md#decision-64)), and P12 waits for the readiness signal SIG-F ([decision 65](decision-register.md#decision-65)). |
| [tensor-logic/concepts.md](tensor-logic/concepts.md) | The terms of TensorLogic: tensors, indices, joins, projections, rules, normalization, exact numbers, reflection and fragments. | A proposal for P12, written in P9. P12 waits for the readiness signal SIG-F ([decision 65](decision-register.md#decision-65)). |
| [tensor-logic/syntax.md](tensor-logic/syntax.md) | The lexical rules, the grammar, the precedence, the ambiguity rules, the embedding in Rholang 1.4, the value ABI and worked examples. | A proposal for P12, written in P9. P12 waits for the readiness signal SIG-F ([decision 65](decision-register.md#decision-65)). |
| [tensor-logic/static-semantics.md](tensor-logic/static-semantics.md) | Sorts, shape inference, typing rules, call signatures, the scope rule, broadcasting, safety, recursion classes, record-locality, the demand bound and the diagnostics. | A proposal for P12, written in P9. P12 waits for the readiness signal SIG-F ([decision 65](decision-register.md#decision-65)). |
| [tensor-logic/dynamic-semantics.md](tensor-logic/dynamic-semantics.md) | The denotation, the built-in functions, the correctly rounded functions, exact arithmetic, the evaluation algorithm, determinism and consensus state. | A proposal for P12, written in P9. P12 waits for the readiness signal SIG-F ([decision 65](decision-register.md#decision-65)). |
| [tensor-logic/gslt.md](tensor-logic/gslt.md) | The TensorLogic module in the definition language of F1r3lang, the sugar rewrites, the normal form, reflection, identity, the capability and admission theories. | A proposal for P12, written in P9. P12 waits for the readiness signal SIG-F ([decision 65](decision-register.md#decision-65)). |
| [tensor-logic/integration.md](tensor-logic/integration.md) | Score queries, programs through `rho:lang:vector`, rules, embedding reasoning, admission theories, `where` guards, replay, merges, costs and errors. | A proposal for P12, written in P9. P12 waits for the readiness signal SIG-F ([decision 65](decision-register.md#decision-65)). |
| [tensor-logic/verification-plan.md](tensor-logic/verification-plan.md) | The models, proofs, negative controls, properties and tests that P12 must deliver, in the six acceptance categories. | A proposal for P12, written in P9. P12 waits for the readiness signal SIG-F ([decision 65](decision-register.md#decision-65)). |
| [tensor-logic/design-rationale.md](tensor-logic/design-rationale.md) | The review of the legacy support, the rejected alternatives, the open questions K0 to K16, the missing facilities of F1r3lang, the work order and the changes to the P6 study. | A proposal for P12, written in P9. The open questions wait for the answers of the user. |
| [findings/space-replay-out-of-phlo.md](findings/space-replay-out-of-phlo.md) | A finding of P4: a deploy that ran out of phlo inside a space transaction replayed to a different error and cost. It holds the reproduction, the mechanism, the fix as built, and two related observations about the host code. | Written. Fixed by [decision 36](decision-register.md#decision-36) on 2026-10-05. Two questions stay open for their owners. |
| [findings/space-legacy-storage-charges.md](findings/space-legacy-storage-charges.md) | A finding of decision 45: in a legacy deploy, a space operation paid no storage charge, while its equivalent host program paid them. It holds the mechanism, a measurement, the reach in block processing, and the fix. | Written. Fixed by Option P of [decision 45](decision-register.md#decision-45) on 2026-10-06. |
| [findings/native-execution-cost-accounting.md](findings/native-execution-cost-accounting.md) | Five items of decision 47 for the cost-accounting owner: an untyped class of space errors, a session charge that depends on the thread count, two readers for host continuations, a lost wake-up of the host path under a forged journal, and the verification bytes of native replay. | Written. The space instance of the lost wake-up is fixed by commit `c6cb371ba`. The other items stay open for their owner. |
| [design-record/](design-record/README.md) | The design ledger of 2026-08-15 with its provenance record. It is history and is exempt from the documentation lint. | Written |
| [../../rholang/21-reified-spaces.md](../../rholang/21-reified-spaces.md) | The language chapter of Reified spaces: every family and qualifier, `use` blocks, annotations, factories, the handle methods, theories, queries, joins, VectorDB spaces and own spaces, each with an executed example. | Written in P10 step S5, after the first sections of P6 and P8 (`rrs-doc-language-chapter`) |
| [verification/test-port-ledger.json](verification/test-port-ledger.json) | The ledger of the legacy Reified tests: one row for each test of the legacy branch, with its disposition: port, retarget, or drop with a named successor. | Written in P9 step 7. `check-test-port-ledger.py` checks it. |

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
| 01 | [Components](diagrams/01-substrate-components.svg) (component) | Drawn for the substrate in P1. P10 step S6 extended it to the modules of every phase (`rrs-doc-diagrams`). |
| 02 | [Namespace bytes](diagrams/02-namespace-bytes.svg) | Drawn. P4 corrected the status of the op channels. |
| 03 | [Native handle lifecycle](diagrams/03-native-handle-lifecycle.svg) (sequence) | Drawn in P4 |
| 04 | [User-agent translation](diagrams/04-user-agent-translation.svg) (sequence) | Drawn in P8 step 6 |
| 05 | [Head-only collections](diagrams/05-head-only-queue.svg) (state) | Drawn in P4 for the queue. P5 wave W3 added the note on joins. P10 step S6 added the stack, the cell and the priority queue (`rrs-doc-diagrams`). |
| 06 | [Commit of a space transaction](diagrams/06-space-txn-commit.svg) (activity) | Drawn in P1. Decision 47 adds the branch of native replay. |
| 06a | [Lifecycle of a space transaction](diagrams/06a-space-txn-lifecycle.svg) (state), a sub-figure of figure 06 | Drawn in P1 |
| 07 | [Checkpoint, replay, and sweep](diagrams/07-checkpoint-replay-sweep.svg) (sequence) | Drawn in P1. Diagram 07a draws the restart. |
| 07a | [Restart and lazy attach](diagrams/07a-restart-lazy-attach.svg) (sequence), a sub-figure of figure 07 | Drawn in P7 step 5 (`rrs-tests-verification-restart-rehydrate`) |
| 07b | [LFS export and import](diagrams/07b-lfs-export-import.svg) (sequence), a sub-figure of figure 07 | Drawn in P10 step S6 for part N20 of the design |
| 08 | [Merge flow of the counter conflict](diagrams/08-counter-conflict-merge.svg) (activity) | Drawn in P4 |
| 09 | [PathMap delivery](diagrams/09-pathmap-delivery.svg) (activity) | Drawn in P5 wave W3 |
| 10 | [Theory admission](diagrams/10-theory-admission.svg) (activity) | Drawn in P7 step 4 |
| 11 | [Layout of the record envelope](diagrams/11-record-envelope.svg) (component) | Drawn in P5 wave W3, with the join continuation of version 2 |
| 12 | [Routing decision](diagrams/12-routing-decision.svg) (activity) | Drawn in P4 |
| 13 | [Verification pipeline](diagrams/13-verification-pipeline.svg) (component) | Drawn for the checks of P1. P10 step S6 extended it to every phase, with the aggregate gate (`rrs-doc-diagrams`). |
| 14 | [Phase Gantt chart](diagrams/14-phase-gantt.svg) (Gantt) | Drawn in P10 step S6. P10 step S11 added the closing commit of P10 and the bar of P11 (`rrs-doc-diagrams`). |
| 15 | [Normalization of the Reified syntax](diagrams/15-syntax-normalization.svg) (activity) | Drawn in P3 |
| 16 | [Space replay when a deploy runs out of phlo](diagrams/16-space-replay-out-of-phlo.svg) (sequence) | Drawn for the finding. Its third part shows the fix of decision 36. |
| 17 | [Legacy charges of a space transaction](diagrams/17-legacy-charge-chain.svg) (sequence) | Drawn for decision 45, Option P |
| 18 | [The scan driver](diagrams/18-scan-driver.svg) (activity) | Drawn in P5 wave W2 for the bag |
| 19 | [The keys of a PathMap path](diagrams/19-pathmap-keys.svg) (object) | Drawn in P5 wave W2 for the pure PathMap layer |
| 20 | [The re-rooting of one argument](diagrams/20-pathmap-reroot.svg) (activity) | Drawn in P5 wave W2 for the pure PathMap layer |
| 21 | [The fire count of a space transaction](diagrams/21-transaction-fire-count.svg) (activity) | Drawn in P5 wave W2 for decision 49 |
| 22 | [The allocation job](diagrams/22-allocation-job.svg) (sequence) | Drawn in P5 wave W2 for the Array and Vector stores |
| 23 | [The crossed stacks](diagrams/23-crossed-stacks-merge.svg) (activity) | Drawn in P5 wave W2 for plan revision M-1 |
| 24 | [The class fire of a set](diagrams/24-set-class-fire.svg) (activity) | Drawn in P5 wave W2 for the set |
| 25 | [The lanes of a priority queue](diagrams/25-priority-queue-lanes.svg) (activity) | Drawn in P5 wave W2 for the priority queue |
| 26 | [One transaction of the join engine](diagrams/26-join-engine.svg) (activity) | Drawn in P5 wave W3 for the joins of one space |
| 27 | [One PathMap operation on the join engine](diagrams/27-pathmap-prefix-source.svg) (activity) | Drawn in P5 wave W3, Phase 2, for the prefix source of the PathMap engine |
| 28 | [The selection of a VectorDB query](diagrams/28-vectordb-selection.svg) (activity) | Drawn in P6 step 8e |
| 29 | [List fires and LIST-CASCADE](diagrams/29-vectordb-list-cascade.svg) (activity) | Drawn in P6 step 8e |
| 30 | [The ChromaDB mirror of a VectorDB space](diagrams/30-mirror-journal-reconciler.svg) (activity) | Drawn in P6 step 8e. *Changed by [decision 98](decision-register.md#decision-98) and [decision 114](decision-register.md#decision-114): the mirror lost its journal and its reconciler, so the title names neither.* |
| 31 | [Recorded embeddings](diagrams/31-recorded-embeddings.svg) (sequence) | Drawn in P6 step 8e |
| 32 | [The advisory query](diagrams/32-advisory-query.svg) (sequence) | Drawn in P6 step 8e |
| 33 | [The number model](diagrams/33-number-model.svg) (activity) | Drawn in P6 step 8e |
| 34 | [The process harnesses of P9](diagrams/34-p9-process-harnesses.svg) (sequence) | Drawn in P9 step 8 |
| 34a | [The blocks of the N4 merge with an empty sibling](diagrams/34a-n4-merge-dag.svg) (component), a sub-figure of figure 34 | Drawn in P9 step 8 |
| 35 | [TensorLogic from end to end](diagrams/35-tl-components.svg) (component) | Drawn in P9 for the TensorLogic design of P12 |
| 36 | [One join and one projection](diagrams/36-tl-join-projection.svg) (component) | Drawn in P9 for the TensorLogic design of P12 |
| 37 | [The TensorLogic grammar, part 1: entry points, statements and let](diagrams/37-tl-grammar-statements.svg) (railroad) | Drawn in P9 for the TensorLogic design of P12 |
| 38 | [The TensorLogic grammar, part 2: expressions and index expressions](diagrams/38-tl-grammar-expressions.svg) (railroad) | Drawn in P9 for the TensorLogic design of P12 |
| 39 | [The tensor value ABI](diagrams/39-tl-value-abi.svg) (component) | Drawn in P9 for the TensorLogic design of P12 |
| 40 | [The static checks of TensorLogic](diagrams/40-tl-static-checks.svg) (activity) | Drawn in P9 for the TensorLogic design of P12 |
| 41 | [The scope rule on the Gaussian kernel](diagrams/41-tl-scope-rule.svg) (tree) | Drawn in P9 for the TensorLogic design of P12 |
| 42 | [The elaboration of the positional operators](diagrams/42-tl-broadcast-matmul.svg) (activity) | Drawn in P9 for the TensorLogic design of P12 |
| 43 | [The evaluation order of a TensorLogic program](diagrams/43-tl-evaluation-order.svg) (activity) | Drawn in P9 for the TensorLogic design of P12 |
| 44 | [The Ziv loop of a correctly rounded function](diagrams/44-tl-ziv-loop.svg) (activity) | Drawn in P9 for the TensorLogic design of P12 |
| 45 | [The semi-naive iteration](diagrams/45-tl-semi-naive.svg) (activity) | Drawn in P9 for the TensorLogic design of P12 |
| 46 | [The normalization pipeline of TensorLogic](diagrams/46-tl-normalization-pipeline.svg) (activity) | Drawn in P9 for the TensorLogic design of P12 |
| 47 | [The categories and constructors of the TensorLogic GSLT](diagrams/47-tl-gslt-categories.svg) (class) | Drawn in P9 for the TensorLogic design of P12 |
| 48 | [The installation and the content identity of TensorLogic](diagrams/48-tl-identity-install.svg) (component) | Drawn in P9 for the TensorLogic design of P12 |
| 49 | [A TL-0 score query](diagrams/49-tl-score-query.svg) (sequence) | Drawn in P9 for the TensorLogic design of P12 |
| 50 | [An eval call of rho:lang:vector](diagrams/50-tl-vector-eval.svg) (sequence) | Drawn in P9 for the TensorLogic design of P12 |
| 51 | [Rules set at a time](diagrams/51-tl-set-at-a-time.svg) (sequence) | Drawn in P9 for the TensorLogic design of P12 |
| 52 | [TL admission theories](diagrams/52-tl-admission.svg) (sequence) | Drawn in P9 for the TensorLogic design of P12 |
| 53 | [The diagnostic codes of TensorLogic](diagrams/53-tl-error-codes.svg) (mind map) | Drawn in P9 for the TensorLogic design of P12 |
| 54 | [The verification pipeline of TensorLogic](diagrams/54-tl-verification-pipeline.svg) (component) | Drawn in P9 for the TensorLogic design of P12 |
| 55 | [Trust boundaries and their controls](diagrams/55-security-trust-boundaries.svg) (component) | Drawn in P10 step S5 for `security.md` |
| 56 | [The path of one benchmark case](diagrams/56-benchmark-case-path.svg) (activity) | Drawn in P10 step S8 for `performance.md` |
| 57 | [The VerificationBytes of one receive against its collection](diagrams/57-receive-host-work.svg) (chart) | Drawn in P10 step S8 for `performance.md`, from the contract of the benchmarks |

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
| P12 | The F1r3lang merge, only after the readiness signal of the user, and the TensorLogic language of the [TensorLogic design](tensor-logic/README.md) |

### Status of the phases

Phases P0 to P11 are complete.
The next table gives the commit that closed each phase and the section of the dossier that records it.
Section 35 of the dossier records the steps of P10, and section 35.9 records the final gate set of P11.
Phase P11 ran the final gate set on a development host, and pgmcp ran no leaf ([decision 137](decision-register.md#decision-137)).
That gate set replaces the one pgmcp freeze of [decision 75](decision-register.md#decision-75).
Phase P12 waits for the readiness signal of F1r3lang.

| Phase | Closing commit | Record |
|---|---|---|
| P0 | `6f7a80da3`, after the restore of the realignment ([decision 5](decision-register.md#decision-5)) | [Section 9](verification.md#9-realignment-closure-checks) |
| P1 | `b783d6122` | [Sections 1 to 8](verification.md#1-purpose-and-scope) |
| P2 | `e9812d0e5` | [Section 4](verification.md#4-rocq-models), the module `Encoding.v` |
| P3 | `a0bcaf90e` in this tree, and the branch `feature/reified-rspaces-v3` of the parser | The [syntax contract](syntax-contract.md) |
| P4 | `294c95632` | [Section 11](verification.md#11-phase-p4-the-hashmapqueue-slice) |
| P5 | `418d81d7c` | [Section 12](verification.md#12-phase-p5-the-other-disciplines-and-outer-stores) |
| Decision 47 | `d559efa48` | [Section 13](verification.md#13-decision-47-spaces-in-native-execution) |
| P6 | `8ecc6b786` | Sections 14 to 26 |
| P7 | `be4e9def3` | Sections 27 to 32 |
| P8 | `d003151d3` | [Section 33](verification.md#33-phase-p8-the-spaces-of-user-agents) |
| P9 | `e8cd072ff` | [Section 34](verification.md#34-phase-p9-consensus-integration) |
| P10 | `c31318b45` | [Section 35](verification.md#35-phase-p10-the-formal-verification-closure-the-documents-the-examples-and-the-benchmarks) |
| P11 | `c31318b45`, with the pin of the parser revision in `cf4bcb410` | [Section 35.9](verification.md#359-step-s11-the-full-gate-set) |

Figure 1 shows the days of each phase, from the first commit of the phase to its closing commit.

![Gantt chart of the phases of the campaign on a daily scale from Sunday 2026-10-04 to Tuesday 2026-11-10. Realignment: P0, the backup, the restore and the ledger record, on October 4 (6f7a80da3). Foundations: P1, the storage substrate, on October 4 (b783d6122); P2, the marker encoding of the syntax, on October 4 (e9812d0e5); P3, the parser v3 and the normalizers, from October 4 to October 5 (a0bcaf90e). Space families: P4, the family hashmap:queue, on October 5 (294c95632); the milestone of decision 44, the merge of feature/cost-accounted-rho, on October 5 (881257d75); decisions 42, 43, 45 and 46 on October 6 (2c2b45bf5); P5, the other disciplines, the outer stores and the joins, from October 6 to October 7 (418d81d7c); decision 47, spaces in native execution, on October 7 (d559efa48); P6, the VectorDB family and the ChromaDB backend, from October 8 to October 9 (8ecc6b786); P7, the temp and seq qualifiers, HashSet, theories and rehydration, from October 9 to October 10 (be4e9def3). Agents and consensus: P8, the user agent-backed spaces, from October 10 to October 11 (d003151d3); P9, the consensus integration, from October 11 to October 12 (e8cd072ff). Verification and documents: P10, from October 12 to November 10 (c31318b45), and P11, the final gate set, from November 9 to November 10 (c31318b45). A note names the steps of P10: S1, S7, S2, S5, S6, S4, S8, S3, and S9 to S11. It says that P11 ran on a development host with no pgmcp run (decision 137). It also gives the phase with no date: P12, which waits for the readiness signal of F1r3lang. The colors mark the kind of work: realignment, storage substrate, syntax, space families and agents, merge and decisions, consensus integration, and verification, documents and the final gate set.](diagrams/14-phase-gantt.svg)

*Figure 1. Source: [diagrams/14-phase-gantt.puml](diagrams/14-phase-gantt.puml), rendered by [diagrams/render.sh](diagrams/render.sh).*

The pgmcp runs of the leaves take place in the freeze of [decision 75](decision-register.md#decision-75).
Changed by [decision 137](decision-register.md#decision-137): pgmcp runs no leaf, and the leaves close at claimed_done on the host verification.

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
