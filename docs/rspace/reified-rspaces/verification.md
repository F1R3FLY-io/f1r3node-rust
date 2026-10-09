# Reified RSpaces verification dossier

This dossier records the verification evidence of the Reified RSpaces campaign.
For each claim, it names the artifact that proves the claim, the command that runs the artifact, and the output of a failure.
Phase P1 writes this version, and each later phase adds the sections of its own models and suites.
Read the [glossary](glossary.md) first, because this dossier uses its terms and links its entries.
The [decision register](decision-register.md) is normative, and the [design](design.md) describes the code that this dossier verifies.

## Contents

1. [Purpose and scope](#1-purpose-and-scope)
2. [Acceptance categories](#2-acceptance-categories)
3. [Verification pipeline](#3-verification-pipeline)
4. [Rocq models](#4-rocq-models)
5. [TLA+ models](#5-tla-models)
6. [Negative controls](#6-negative-controls)
7. [Property-based tests and substrate suites](#7-property-based-tests-and-substrate-suites)
8. [Regression checks for ordinary execution](#8-regression-checks-for-ordinary-execution)
9. [Realignment closure checks](#9-realignment-closure-checks)
10. [Running the checks locally](#10-running-the-checks-locally)
11. [Phase P4: the hashmap:queue slice](#11-phase-p4-the-hashmapqueue-slice)
12. [Phase P5: the other disciplines and outer stores](#12-phase-p5-the-other-disciplines-and-outer-stores)
13. [Decision 47: spaces in native execution](#13-decision-47-spaces-in-native-execution)
14. [Phase P6, step 2: the exact numeric core](#14-phase-p6-step-2-the-exact-numeric-core)
15. [Phase P6, steps 3 and 4: the VectorDB driver and its join-engine paths](#15-phase-p6-steps-3-and-4-the-vectordb-driver-and-its-join-engine-paths)
16. [Phase P6, step 5: native certification of the VectorDB shapes](#16-phase-p6-step-5-native-certification-of-the-vectordb-shapes)
17. [Phase P6, step 6: `rho:lang:vector`](#17-phase-p6-step-6-rholangvector)
18. [Phase P6, step 7a: the recorded processes `embed` and `ann`](#18-phase-p6-step-7a-the-recorded-processes-embed-and-ann)
19. [Phase P6, step 7b: the mirror journal and its publication](#19-phase-p6-step-7b-the-mirror-journal-and-its-publication)
20. [Phase P6, step 7c: the mirror reconciler, the docker pin and the live leg](#20-phase-p6-step-7c-the-mirror-reconciler-the-docker-pin-and-the-live-leg)
21. [References](#references)

## 1. Purpose and scope

### 1.1 What P1 proves

Phase P1 delivers the [storage substrate](design.md#substrate) of spaces in `rspace++`: `SpaceTxnHost`, `SpaceStoreHandle`, and `SpaceTxn`.
The substrate makes each space operation act as the equivalent [host operation](glossary.md#host-operation) ([decision 19](decision-register.md#decision-19)).
So the existing checkpoint, replay, and merge code covers spaces, and the consensus code does not change.
P1 proves three groups of claims:

- **The substrate is equivalent to the host.** Twenty-three Rocq [headlines](glossary.md#headline) and six TLA+ invariants state the equivalence (sections [4](#4-rocq-models) and [5](#5-tla-models)). Property-based tests, witness tests, and deterministic suites check the same claims on the Rust code (section [7](#7-property-based-tests-and-substrate-suites)). [Decision 36](decision-register.md#decision-36) adds the headline H23 and the invariant T6: replay reproduces play, also when a deploy runs out of phlo in a space transaction.
- **Ordinary execution is unchanged.** The 22 items of the [consensus-preservation checklist](glossary.md#consensus-preservation-checklist) hold at the end of P1. Regression tests compare ordinary execution with corpora that capture runs recorded from the code of HEAD (section [8](#8-regression-checks-for-ordinary-execution)).
- **The realignment is closed.** The realignment leaves of the tracker have their evidence: a footprint over the full tree and the closure checks (section [9](#9-realignment-closure-checks)).

### 1.2 How to read a claim

Each claim has three parts: the artifact that proves it, the command that runs the artifact, and the failure signature.
Table 1 gives the failure signature of each kind of artifact.
A [gate](glossary.md#gate) prints one line for each leg and then a summary line with the counts `PASS`, `FAIL`, and `SKIP`.
A gate passes only with `FAIL=0` and `SKIP=0`, because a missing tool counts as a failure, never as a skip.

*Table 1. The failure signature of each kind of artifact.*

| Artifact | Command | Failure signature |
|---|---|---|
| Rocq headline | `scripts/check-reified-rspaces-proofs.sh` | `FAIL standalone <module>` (a coqc error), `FAIL headlines <module>` (an assumption or a wrong count), or `FAIL rocqchk <module>` |
| Rocq mutant | `scripts/verify-reified/rocq-negative-controls.py` | `FAIL <id>: the mutant compiles`, or `FAIL <id>: rejected in <name> (line <n>), expected one of <names>` |
| TLA+ invariant | `scripts/check-reified-rspaces-tla-invariants.sh` | `FAIL safe <cfg>`: TLC reports that an invariant is violated, with a counterexample trace |
| Unsafe control | `scripts/verify-reified/tlc-expect-violation.sh` | `FAIL unsafe <cfg>`: TLC finds no violation, or it reports another invariant |
| Complement of an Unsafe control | `tlc-expect-violation.sh --complement` | `FAIL unsafe <cfg> without <invariant>`: another invariant fails on the full state space |
| Apalache check | `scripts/verify-reified/apalache-expect.sh` | `FAIL apalache <name>`: another outcome, or a violation outside the expected conjunct |
| Rust test | `cargo test -p <crate> --test <target>` | `test <name> ... FAILED` and `test result: FAILED`. A property also prints `minimal failing input` |
| Python checker | `python3 scripts/verify-reified/<checker>` | a `FAIL:` line or a finding line `<path>: <detail>`, and exit status 1 |
| Document | `python3 scripts/verify-reified/doc_lint.py` | `<path>:<line>: <rule>: <detail>`, and exit status 1 |
| Diagram | `docs/rspace/reified-rspaces/diagrams/render.sh --check` | `FAIL <file>: <reason>`, and exit status 1 |

### 1.3 Terms and notation

*Table 2. Terms and symbols of this dossier. The [glossary](glossary.md) defines the other terms.*

| Term | Meaning |
|---|---|
| Base revision, HEAD | The commit `4d67b5ccc`, the tip of `feature/cost-accounted-rho` that the campaign merged on 2026-10-05 (section [8.8](#88-the-merge-of-featurecost-accounted-rho)). In this campaign, HEAD names the node code at this revision. Before the merge, the base revision was `6f7a80da3`, whose ordinary code equals `e748426`. |
| H1 to H23 | The 23 Rocq headlines of P1 (section [4](#4-rocq-models)). Decision 36 added H23 on 2026-10-05. |
| T1 to T6 | The six TLA+ invariants of P1 (section [5](#5-tla-models)). Decision 36 added T6 on 2026-10-05. |
| O1 | The entry of the invariant map for item 10 of the consensus-preservation checklist (section [7.5](#75-the-ord-phase)). |
| FR-1 to FR-10 | The fix rules of P1. Table S0 of the [design](design.md#substrate) lists them. |
| R1 to R9 | The nine regression tests of ordinary execution in `rholang/tests/reified_ordinary_regression.rs`. |
| TLC | The explicit-state model checker of TLA+. It visits every reachable state of a finite configuration [[YML99](#ref-yml99)]. |
| Apalache | The symbolic model checker of TLA+. It checks a formula over all states that a type allows, with a constraint solver [[KKT19](#ref-kkt19)]. |
| BASE, STEP | The two Apalache checks of an [inductive invariant](glossary.md#inductive-invariant). The display below defines them. |
| Unsafe configuration | A TLA+ configuration that turns on one wrong rule. It must refute exactly one invariant. |
| Complement | The same Unsafe configuration without the refuted invariant. TLC must then find no violation of the other invariants. |
| Mutant | A copy of a Rocq module or of a test in which one rule changes. A mutant must fail [[DLS78](#ref-dls78)]. |
| Recipe, step key | A [verification recipe](glossary.md#verification-recipe) of pgmcp, and the key of one of its commands. |
| $`\kappa(p)`$ | The [produce counter](glossary.md#produce-counter) of the produce identity $`p`$. |

The BASE check and the STEP check of an inductive invariant $`\mathit{IndInv}`$ are these two implications:

```math
\mathrm{BASE}:\ \mathit{Init} \Rightarrow \mathit{IndInv}
\qquad\qquad
\mathrm{STEP}:\ \mathit{IndInv} \land \mathit{Next} \Rightarrow \mathit{IndInv}'
```

Here $`\mathit{IndInv}'`$ is the invariant over the next state.
When both checks pass, induction on the length of a behavior shows that $`\mathit{IndInv}`$ holds in every reachable state.
This conclusion holds at every depth, for the constants of the configuration.

### 1.4 Later phases

Each later phase adds one section for each new model or suite, and it adds its rows to the tables of sections 2 and 8.
Phase P4 keeps its rows of those tables in its own section [11](#11-phase-p4-the-hashmapqueue-slice).
The Rocq gate also checks the module `Encoding.v` of phase P2 and the six modules of phase P4.
It also checks `AgentReadCoverage.v` of decision 42 and `LegacyCharges.v` of decision 45.
So its total count of closed headlines is 115 over 11 modules, not 23.
The section of phase P2 documents the 12 headlines of P2 in `Encoding.v`, and section 11 documents the 3 headlines that P4 adds to it.

## 2. Acceptance categories

The user binds each criterion of a tracker leaf to one of six [acceptance categories](glossary.md#acceptance-category).
Table 3 gives the evidence of each category in P1, the local gate that runs it, and the recipe steps that bind it.
The pgmcp runs of the P1 leaves take place in the same freeze as the runs of P4.
The user decided this on 2026-10-04, as [decision 35](decision-register.md#decision-35).
So this dossier names the planned recipe ids and step keys, without recipe hashes.

*Table 3. The six acceptance categories in P1.*

| Category | Evidence in P1 | Local gate | Recipe: step keys |
|---|---|---|---|
| Formally modeled | `check-model-map.py` checks the 70 items of the [model map](glossary.md#model-map), 32 of them required. Each item exists in the code and cites Rocq definitions and TLA+ operators that exist. Each of the 29 headlines (H1 to H23, T1 to T6) names an existing theorem or operator, and each fix rule names its items and headlines. The checker also checks each Rust line citation of the formal files. `check-closure.py modeled` checks the model items of the realignment leaves. | `python3 scripts/verify-reified/check-model-map.py` and `check-closure.py modeled` | `rrs-p1-checks`: `p1-model-map`, `p1-closure-modeled` |
| Formal verification passes | The 23 Rocq headlines close under the global context, and `rocqchk` accepts each module. The 27 Rocq mutants of P1 fail in their expected declarations. TLC explores 6 safe configurations completely. Each of 11 Unsafe configurations refutes exactly one invariant, and its complement violates no other invariant. Apalache proves BASE and STEP of `IndInv` (also with the legacy charges of decision 45) and `LockIndInv`, and 4 Error controls break them in the expected conjunct. | `scripts/check-reified-rspaces-proofs.sh`, `scripts/check-reified-rspaces-tla-invariants.sh` | `rrs-p1-rocq-ec`: `fresh-kernel`. `rrs-p1-rocq-vs`: `fresh-kernel`. `rrs-p1-rocq-negative`: `p1-rocq-negative`. `rrs-p1-tla-checkpoint`: its 29 steps (section [5.2](#52-checkpointreplaytla)). `rrs-p1-tla-lock`: its 9 steps (section [5.3](#53-txnlockcoveragetla)). |
| Every invariant extracted as a property-based test | The [invariant map](glossary.md#invariant-map) names at least one property test for each of the 29 P1 ids and for the checklist entry O1. The 25 P1 properties and `prop_ordinary_play_replay_equal` run 64 cases with a fixed seed. `check-closure.py invariants` checks the headlines of the realignment leaves. | `check-invariant-map.py --phase P1`, `check-invariant-map.py --phase ORD`, and the cargo targets of section [10.3](#103-rust-suites) | `rrs-p1-checks`: `p1-invariant-map`, `p1-invariant-map-ord`, `p1-closure-invariants`. `rrs-p1-rspace`: `p1-props`. `rrs-p1-regression`: `p1-regression-ordinary`. |
| Implementation complete | `check-complete.py --phase p1` finds the 11 entry points of P1 and no forbidden token in the 3 P1 sources. It also finds no ignored test and no `/tmp` path in the P1 tests, and each property file sets `failure_persistence: None`. `check-complete.py --production-scan` finds none of the 24 consensus tokens in production code. `check-footprint.py` passes the policies `p1` and `p1-realign`. `check-consensus-checklist.py --milestone p1` and `check-closure.py entry-points` pass. | `python3 scripts/verify-reified/check-complete.py` and the other checkers | `rrs-p1-checks`: `p1-complete`, `p1-production-scan`, `p1-footprint`, `p1-footprint-realign`, `p1-consensus-checklist`, `p1-closure-entry-points`. For the formal leaves: `rrs-p1-rocq-ec`: `fresh-kernel`, and `rrs-p1-tla-checkpoint`: `p1-tla-safe-mc-checkpointreplay-twoblock`. |
| Thoroughly tested | The substrate suites (18 library tests, 82 integration tests, 4 golden-corpus tests, 3 tests over `Par`, 1 host-bounds test), the whole `rspace++` suite, the regression suite of section [8](#8-regression-checks-for-ordinary-execution), the HEAD suites, the 273 checker self-tests, the audit harness, and the ledger-copy controls. | `cargo test` (section [10.3](#103-rust-suites)) and `python3 scripts/verify-reified/run-checker-tests.py` | `rrs-p1-rspace`: `p1-lib`, `p1-integration`, `p1-legacy-golden`. `rrs-p1-rspace-full`: `p1-full-rspace`. `rrs-p1-rholang`: `p1-rholang-par`, `p1-rholang-host-bounds`. `rrs-p1-regression`: its 10 steps (section [8.1](#81-consensus-preservation-checklist)). `rrs-p1-head-rspace`: `p1-head-storage`, `p1-head-replay`, `p1-head-observer`, `p1-head-history`, `p1-head-lib-history`. `rrs-p1-rholang-matcher`: `p1-head-rholang-matcher`. `rrs-p1-checks`: `p1-checker-tests`, `p1-audit-rocq`, `p1-audit-source`, `p1-ledger-copy`, `p1-ledger-copy-controls`. `rrs-p1-rocq-negative`: `p1-rocq-negative` (for `rrs-fv-event-correlation`). |
| Documented per the pgmcp guidelines | `doc_lint.py` passes every rule on every campaign document. `render.sh --check` finds each SVG byte-identical to a fresh render. The 83 unit tests of `doc_lint.py` pass. `check-closure.py docs` finds the headings, references, and anchors of the closure policy. | `python3 scripts/verify-reified/doc_lint.py`, `docs/rspace/reified-rspaces/diagrams/render.sh --check` | `rrs-p1-docs`: `p1-docs-lint`, `p1-docs-render`, `p1-docs-selftests`. `rrs-p1-checks`: `p1-doc-lint`, `p1-closure-docs`. |

Table 4 shows how the leaves that cite this dossier bind the six categories.
The realignment leaves `rrs-ra-03-04` and `rrs-ra-05-01` to `rrs-ra-05-06` also rely on the [design](design.md#substrate), whose headings the closure policy lists.

*Table 4. The recipe steps that bind each category of a leaf. A dash means that the category does not apply.*

| Leaf | Modeled | Formal verification | Invariant tests | Complete | Tested | Documented |
|---|---|---|---|---|---|---|
| `rrs-persistence-substrate-space-store-handle` | `p1-model-map` | `fresh-kernel` of both Rocq recipes, `p1-rocq-negative`, all steps of `rrs-p1-tla-checkpoint` and `rrs-p1-tla-lock` | `p1-invariant-map`, `p1-props` | `p1-complete`, `p1-production-scan`, `p1-footprint` | `p1-lib`, `p1-integration`, `p1-legacy-golden`, `p1-full-rspace`, `p1-rholang-par`, `p1-rholang-host-bounds` | `p1-docs-lint`, `p1-docs-render`, `p1-docs-selftests` |
| `rrs-fv-event-correlation` | `p1-model-map` | `fresh-kernel` of `rrs-p1-rocq-ec` | `p1-invariant-map`, `p1-props` | `fresh-kernel` of `rrs-p1-rocq-ec` | `p1-rocq-negative`, `p1-integration` | the three steps of `rrs-p1-docs`, `p1-closure-docs` |
| `rrs-fv-rocq-tla-two-block-replay-model` | `p1-model-map` | all steps of `rrs-p1-tla-checkpoint` | `p1-invariant-map`, `p1-props` | `p1-tla-safe-mc-checkpointreplay-twoblock` | `p1-integration` | the three steps of `rrs-p1-docs`, `p1-closure-docs` |
| `rrs-tests-verification-consensus-regression` | - | - | `p1-invariant-map-ord`, `p1-regression-ordinary` | `p1-consensus-checklist`, `p1-footprint`, `p1-complete`, `p1-production-scan` | the 10 steps of `rrs-p1-regression`, `p1-checker-tests` | the three steps of `rrs-p1-docs`, `p1-closure-docs` |
| `rrs-ra-03-04`, `rrs-ra-05-01` to `rrs-ra-05-06` | `p1-closure-modeled` | `fresh-kernel` of `rrs-p1-rocq-ec` (03-04 and 05-01 to 05-05) and of `rrs-p1-rocq-vs` (05-03 and 05-06), all steps of `rrs-p1-tla-lock-core` (05-04) and of `rrs-p1-tla-checkpoint` (05-06) | `p1-closure-invariants`, `p1-props` | `p1-closure-entry-points`, with `p1-complete` and `p1-footprint` (05-01 to 05-06) or `p1-footprint-realign` and `p1-production-scan` (03-04) | `p1-lib`, `p1-integration` | `p1-closure-docs`, `p1-doc-lint` |

## 3. Verification pipeline

Figure V1 shows how the sources of P1 reach the six acceptance categories.
Read it from the left to the right: sources, checks, local gates, pgmcp recipes, and categories.
The negative controls are checks that must fail, and their gates require the expected failure.
Each recipe runs the same command as its local gate, in the verifier sandbox.

![Verification pipeline of phase P1 as five columns from the left to the right. Sources: the P1 Rust code, the HEAD Rust code, the Rocq models EventCorrelation.v and VolatileSweep.v, the TLA+ models CheckpointReplay.tla and TxnLockCoverage.tla with 27 configurations, the maps and policies, the corpora, and the documents. Checks: the Rocq kernel checks, TLC on 6 safe configurations, the Apalache BASE check Init implies IndInv and STEP check IndInv and Next imply IndInv prime, the property-based tests with 64 cases, the deterministic suites, the Python checkers, the document checks, and the negative controls that must fail. Local gates: check-reified-rspaces-proofs.sh, check-reified-rspaces-tla-invariants.sh, cargo test, the Python checkers, and doc_lint.py with render.sh. pgmcp recipes: rrs-p1-rocq-ec, rrs-p1-rocq-vs and rrs-p1-rocq-negative, the TLA recipes, the Rust recipes, rrs-p1-checks with 18 checker steps, and rrs-p1-docs. Acceptance categories: formally modeled, formal verification passes, every invariant extracted as a property-based test, implementation complete, thoroughly tested, and documented. Green arrows show which recipe binds evidence of which category.](diagrams/13-verification-pipeline.svg)

*Figure V1. Source: [diagrams/13-verification-pipeline.puml](diagrams/13-verification-pipeline.puml), rendered by [diagrams/render.sh](diagrams/render.sh).*

## 4. Rocq models

### 4.1 Toolchain and kernel checks

The models use the Rocq Prover 9.1.1, compiled with OCaml 5.4.1, and its proof checker `rocqchk` 9.1.1 [[Roc25](#ref-roc25)].
Each module imports only the Rocq standard library, so a checker compiles it alone, with `coqc -q -noglob` and no `-Q` or `-R` option.
No module holds `Admitted`, `admit`, `Axiom`, `Parameter`, `Conjecture`, or `Abort`.
A section hypothesis is a parameter of each closed theorem, and a premise such as `key_injective` is a hypothesis of the statement, not an axiom.

The gate `scripts/check-reified-rspaces-proofs.sh` copies the tree into a scratch directory and runs seven legs there:

1. **Hygiene.** A scan of a copy without comments finds no axiom, admission, top-level hypothesis, checker bypass, or import outside `Stdlib`.
2. **Standalone.** `coqc -q -noglob` compiles each module in a fresh directory.
3. **Headlines.** Each `Print Assumptions` line prints `Closed under the global context`. The gate expects 17 for EventCorrelation.v and 6 for VolatileSweep.v.
4. **Kernel check.** `rocqchk -o -R . '' <module>` checks the compiled `.vo` file and each of its dependencies again. The context summary must report `Axioms: <none>`, no constant that relies on type-in-type, no unsafe fixpoint, and no assumed positivity.
5. **Project.** `make -j1` builds the library `ReifiedRSpaces` from `_CoqProject`, and `rocqchk -o -Q theories ReifiedRSpaces` checks it.
6. **Total.** The count of closed headlines is 115 over 11 modules. P1 has 23 and `Encoding.v` has 15. The six P4 modules have 67 (sections [11.3](#113-rocq-models-of-p4), [11.11](#1111-native-execution-of-decisions-46-and-47), [11.12](#1112-re-entry-of-decision-43) and [11.13](#1113-peek-re-store-of-decision-45)). `AgentReadCoverage.v` has 8 (section [11.10](#1110-metering-of-decision-42)), and `LegacyCharges.v` has 2 (section [11.14](#1114-legacy-charges-of-decision-45)). `Encoding.v` has 12 headlines of P2 and 3 of I-20.
7. **Negative controls.** The 75 mutants fail in their expected declarations. Sections [6.1](#61-rocq-mutants) and [11.5](#115-negative-controls-of-p4) hold 29 and 29 of them. Sections [11.10](#1110-metering-of-decision-42), [11.11](#1111-native-execution-of-decisions-46-and-47), [11.12](#1112-re-entry-of-decision-43), [11.13](#1113-peek-re-store-of-decision-45) and [11.14](#1114-legacy-charges-of-decision-45) hold 5, 1, 3, 3 and 5. Decision 47 later replaces the mutant of section [11.11](#1111-native-execution-of-decisions-46-and-47) with two (Table 47). The 11 pristine modules compile first.

The last host run printed `reified-rspaces proofs: PASS=53 FAIL=0 SKIP=0` (`target/verification/reified-rspaces/d45/option-p-gates/proofs.log`).
The `Print Assumptions` lines are [lines 7578 to 7594 of EventCorrelation.v](../../../formal/rocq/reified_rspaces/theories/EventCorrelation.v#L7578-L7594) and [lines 1744 to 1749 of VolatileSweep.v](../../../formal/rocq/reified_rspaces/theories/VolatileSweep.v#L1744-L1749).

In the pgmcp sandbox, the Rocq adapter compiles one standalone file with `coqc -q -noglob` in a fresh directory.
Then it runs `rocqchk -o` with an empty list of permitted assumptions.
The recipes `rrs-p1-rocq-ec` and `rrs-p1-rocq-vs` do this in their step `fresh-kernel`, for every theorem that a `Print Assumptions` line names.

### 4.2 Headlines of EventCorrelation.v

[EventCorrelation.v](../../../formal/rocq/reified_rspaces/theories/EventCorrelation.v) models a space transaction against the host [RSpace](glossary.md#rspace).
A space agent stages whole operations with an explicit outcome, and the commit runs the host call sequence of each operation.
Each headline in Table 5 has no assumption: `Print Assumptions` reports `Closed under the global context`.
The last column gives the property tests that the [invariant map](glossary.md#invariant-map) names for the headline.

*Table 5. The 17 headlines of EventCorrelation.v.*

| Id | Theorem | Statement | Assumptions | Property tests |
|---|---|---|---|---|
| H1 | `commit_trace_eq_host` (`EventCorrelation.v:5501`) | A commit of the staged operations makes exactly the observer calls, events, status, and replay bindings of the host run of the same operations. | None | `prop_space_txn_play_trace_equals_host`, `prop_space_txn_repeated_channel_groups_equal_host` |
| H2 | `commit_state_eq_host` (`EventCorrelation.v:5559`) | The committed store equals the host store: equal continuations, joins, and counters, and each data list a permutation of the host list. Since [decision 45](decision-register.md#decision-45), the statement has no premise on peeks. | None | `prop_space_txn_play_root_equals_host`, `prop_space_txn_repeated_channel_groups_equal_host` |
| H3 | `counter_timing_play_replay` (`EventCorrelation.v:6200`) | The play counter closure and the replay rule give the same COMM. A rejected replay fire rolls its increment back exactly, and the staged counter view advances as the host counter does. | None | `prop_counter_timing_play_equals_replay` |
| H4 | `verify_sound_complete` (`EventCorrelation.v:6290`) | `verify` pairs a COMM exactly when the log holds the trigger event and an unpaired recorded occurrence remains, and `recorded_comms` reports the same copies. A replay fire without its trigger event stages unpaired, with no pairing and no change of the bindings (conjunct 3, `stage_without_trigger_unpaired`). An unpaired fire never commits: it emits no event and no effect, and it keeps the bindings and the store (conjunct 6, `unpaired_fire_inert`). | None | `prop_space_txn_replay_divergence_typed_at_commit`, `prop_space_txn_replay_requires_trigger_event` |
| H5 | `replay_pairs_each_comm_once` (`EventCorrelation.v:6494`) | `rig` sets up the binding invariant from the play log. A replay transaction never pairs a COMM more often than the log records it, and its commit removes exactly the committed pairings. | None | `prop_space_txn_replay_pairs_play_log`, `prop_space_txn_repeated_channel_groups_equal_host`, `prop_space_txn_peek_restore` |
| H6 | `view_commit_coherent` (`EventCorrelation.v:5586`) | The staged view is the committed store. The commit stops at the first rejected operation, and the store then equals the view after the operations before it. | None | `prop_space_txn_view_equals_committed_store` |
| H7 | `joins_lockstep_preserved` (`EventCorrelation.v:5609`) | Staging and commit keep the join entries in lockstep with the waiting continuations. | None | `prop_space_txn_joins_lockstep` |
| H8 | `sub_order_between` (`EventCorrelation.v:8058`) | A sub-order sorts after its parent and before every later scheduler order. The sub-orders of one parent strictly increase, and no scheduler order equals a sub-order. | None | `prop_sub_orders_contiguous_and_disjoint` |
| H9 | `abort_inert` (`EventCorrelation.v:7159`) | A dropped transaction, also after access errors, and a commit after a store swap change no store, binding, observer call, event, or generation. The checkpoint then gives the same history map. | None | `prop_space_txn_abort_inert`, `prop_space_txn_non_canonical_inputs_fail_closed` |
| H10 | `coverage_total` (`EventCorrelation.v:6082`) | For every committed operation, an event of the same operation covers each store change, and every COMM follows its trigger event. | None | `prop_event_coverage_holds_for_generated_commits` |
| H11 | `peek_reproduces_trace_eq_host` (`EventCorrelation.v:5836`) | With the re-offer (FR-10) and the re-stores ([decision 45](decision-register.md#decision-45)) as the next operations, a commit makes the observer calls, events, status, and bindings of the host run, whose reducer sends the peeked data again, and the stores are equal. A fire never stores a datum, its receipt carries the stored persist flags, and the re-stores are exactly the sends of `produce_peeks`, each with the identity of its record. | None | `prop_space_txn_peek_restore` |
| H12 | `reporting_trace_is_replay_trace` (`EventCorrelation.v:7244`) | Reporting stages and commits exactly as replay does, and its host operations are those of replay. | None | `prop_space_txn_reporting_equals_replay`, `prop_space_txn_peek_restore` |
| H17 | `non_canonical_fails_closed` (`EventCorrelation.v:7101`) | A group that is not non-decreasing fails with `NonCanonicalChannelOrder` before any store or cache effect. A non-decreasing group passes, also when it repeats a channel, and the sorted list is the only canonical member of its permutation class. | None | `prop_space_txn_non_canonical_inputs_fail_closed`, `prop_space_txn_repeated_channel_groups_equal_host` |
| H19 | `directive_fails_closed` (`EventCorrelation.v:7284`) | In replay and reporting, a replay directive rejects the operation with no change of store, bindings, events, or effects, and the commit stops there. | None | `prop_space_txn_replay_directive_fails_closed` |
| H20 | `parent_order_single_txn` (`EventCorrelation.v:8331`) | `sub_order` is injective. A second commit under one parent in one evaluation scope fails before any effect, and a scope reset, a store swap, and a session change each reset the tracker. | None | `prop_space_txn_parent_order_single_commit` |
| H21 | `view_identity_is_store_identity` (`EventCorrelation.v:8452`) | View lookups and the installed guard respect the store identity, and the staged view agrees with the committed store. A view keyed by the Rust ordering disagrees for two channels with one store identity. | None | `prop_space_txn_store_identity_ignores_ord_only_fields`, `prop_space_txn_par_identity_ignores_locally_free` |
| H23 | `replay_reproduces_play_run` (`EventCorrelation.v:7834`) | Play and replay stage the same operations from the same view, and replay is rigged with the commit log of play. Then replay stages exactly the operations of play, and its commit makes the observer calls of play without the directive queries, the events of play, the status of play, and the store of play. So an operation that play rejects, for example for lack of phlo, fails replay at the same call, after the same prefix (decision 36). | None. The statement takes two premises: the observer ignores the directive queries, and it gives no directive. `replay_premises_satisfiable` shows the observer of a phlo limit that meets both. | `prop_space_txn_replay_reproduces_play_rejection` |

Since [decision 36](decision-register.md#decision-36), H1, H2, and H11 assume in replay that every staged fire paired (the premise `fire_paired`).
A successful staging implied this premise before decision 36, and H4 and H23 now cover the unpaired fire.

### 4.3 Headlines of VolatileSweep.v

[VolatileSweep.v](../../../formal/rocq/reified_rspaces/theories/VolatileSweep.v) models the [sweep](glossary.md#sweep), the checkpoint of the hot store, root neutrality, and the leaf record count.
Each headline in Table 6 has no assumption: `Print Assumptions` reports `Closed under the global context`.

*Table 6. The 6 headlines of VolatileSweep.v.*

| Id | Theorem | Statement | Assumptions | Property tests |
|---|---|---|---|---|
| H13 | `sweep_exact` (`VolatileSweep.v:879`) | The sweep removes exactly the data, continuations, and joins of the volatile channels, and it leaves every other entry. Through the handle, a sweep in a task with an open transaction fails closed with `NestedTransaction` (FR-9'). | None | `prop_sweep_volatile_exact_and_root_neutral` |
| H14 | `delete_absent_identity` (`VolatileSweep.v:956`) | A history delete of an absent key finds nothing and changes nothing, in `make_actions` and in the checkpoint. | None | `prop_sweep_volatile_exact_and_root_neutral` |
| H15 | `sweep_root_neutral` (`VolatileSweep.v:1251`) | With canonical cache keys and collision-free history keys, the checkpoint after the sweep succeeds and gives the trie of the durable changes alone. A deploy that touched only volatile keys leaves the root unchanged. | None | `prop_sweep_volatile_exact_and_root_neutral` |
| H16 | `leaf_count_is_length` (`VolatileSweep.v:1314`) | After a successful checkpoint, `root_leaf_record_count` of a channel equals the number of records that the hot store held at that channel. | None | `prop_root_leaf_record_count_matches_history` |
| H18 | `canonical_reads_root_neutral` (`VolatileSweep.v:1429`) | A read of a non-canonical key fails before any access. Reads of canonical keys change no stored list and leave the checkpoint root unchanged. | None | `prop_space_txn_reads_root_neutral` |
| H22 | `checkpoint_rejects_duplicate_key_actions` (`VolatileSweep.v:1613`) | The checkpoint refuses two actions on one history key, so two cached permutations of one group make it fail. Under FR-1', the canonical cache keys have distinct history keys. | None | `prop_space_txn_non_canonical_inputs_fail_closed` |

## 5. TLA+ models

### 5.1 Tools and gate legs

The models use TLC 2.19 (revision `5a47802` of 8 August 2024) and Apalache 0.58.3 [[Lam94](#ref-lam94), [YML99](#ref-yml99), [KKT19](#ref-kkt19)].
The gate `scripts/check-reified-rspaces-tla-invariants.sh` copies the directory `formal/tlaplus/reified_rspaces` into a scratch directory and runs four legs:

1. **Bounds.** Each configuration starts with a `\* BOUNDS:` line, and exactly one leg checks each configuration.
2. **Safe.** TLC explores the complete state space of each safe configuration, breadth-first. The gate never uses `-simulate`, `-dfid`, or `-seed`, and it runs TLC through `scripts/lib/tlc-run.sh` with a heap of at most 4 GiB.
3. **Unsafe.** `tlc-expect-violation.sh` requires that each Unsafe configuration refutes exactly its invariant. Then `tlc-expect-violation.sh --complement` requires that the configuration without that invariant violates no other invariant on the full state space.
4. **Apalache.** `apalache-expect.sh` runs BASE with `--length=0` and STEP with `--length=1`. An Error control passes only when the violation holds its expected marker, so a break in another conjunct fails the control.

The gate lists its legs in three arrays: `SAFE_CONFIGS`, `UNSAFE_CONFIGS`, and `APALACHE_CHECKS`.
The recipe generator reads the same arrays, so the gate and the recipes check the same configurations.
The state counts and depths below come from the last host run, the run of Option P of decision 45, which printed `reified-rspaces tla: PASS=162 FAIL=0 SKIP=0` (`target/verification/reified-rspaces/d45/tla-lc/gate.log`).
That run also checked the P4 modules of section [11.4](#114-tla-models-of-p4).

### 5.2 CheckpointReplay.tla

[CheckpointReplay.tla](../../../formal/tlaplus/reified_rspaces/CheckpointReplay.tla) composes a proposer that plays with a validator that replays, for one or two blocks.
The proposer stages and commits transactions, sweeps the volatile channels at the end of each deploy, and checkpoints.
`Rig` binds the play log, and the validator resets to the root of the previous block and replays the same transactions.
Every operation is non-persistent and uses one channel, and play and replay decide each outcome with the same rule.

Since [decision 36](decision-register.md#decision-36), the deploy of each block has the phlo limit $`\mathit{Budget}`$, counted in charges.
A committed operation charges its introduction, and a fire also charges its COMM.
A charge above the limit fails the deploy, and the commit keeps the operations before it.
A replay fire with no free recorded occurrence stages unpaired, and the replay commit stops there when its charges pass.
With the constant `Forge`, the proposer may also publish a log without the COMM of its last fire.

Since [decision 45](decision-register.md#decision-45), the constant `Peeks` adds peeks under Rule E.
The COMM of a peek fire removes the record, and a re-store with the same identity stores it again.
The constant `Legacy` runs a legacy deploy with the charges of Option P, which section [11.14](#1114-legacy-charges-of-decision-45) describes.

#### 5.2.1 Invariants of CheckpointReplay.tla

*Table 7. The invariants of CheckpointReplay.tla.*

| Id | Invariant | Meaning |
|---|---|---|
| T1 | `ReplayPrefixEqual` | The replay log is a prefix of the play log, and it equals the play log when the replay completes. |
| T2 | `SameRootIffSameState` | Two checkpoints have the same root exactly when they have the same hot-store state. |
| T3 | `PairedOnce` | Replay pairs each COMM at most as often as the play log records it. |
| T4 | `NoVolatileResidue` | No history trie, of play or of replay, holds data, continuations, or joins of a volatile channel. |
| T6 | `ReplayOutcomeEqual` | When the published log is the commit log of play, replay never diverges, and the replayed deploy ends with the failed flag and the spending of play. |
| U13 | `LegacyChargesFollowHost` | In a legacy deploy, whenever play has no open transaction, play has the legacy state and the stop point of the host program. U13 is an id of the P4 map (section [11.14](#1114-legacy-charges-of-decision-45)). |
| Auxiliary | `TypeOK` | The type invariant of the module. It is not a property of the substrate. |
| Auxiliary | `IndInv` | The inductive strengthening of T3 and T4 for the Apalache checks. TLC also checks it on every reachable state of both safe configurations. |

The five invariants have these definitions. Here $`\mathit{Vol}`$ is the set of volatile channels, and $`\mathit{EmptyPBag}`$ and $`\mathit{EmptyCBag}`$ map every identity to the count 0.
T6 uses these variables of the model:

- $`\mathit{forged}`$ holds when the proposer published a log without the COMM of its last fire.
- $`\mathit{rOk}`$ holds while the replay has not diverged. A diverged replay rejects the block.
- $`\mathit{pFailed}`$ and $`\mathit{rFailed}`$ are the failed flags of the deploy in play and in replay.
- $`\mathit{pSpent}`$ and $`\mathit{rSpent}`$ are the charges that play and replay spend. A rejected charge counts, so a failed deploy spends $`\mathit{Budget}`$.

```math
\begin{aligned}
\mathit{ReplayPrefixEqual} &\triangleq \mathit{rLen} \le \mathit{pLen} \land \forall i \in \{1, \dots, \mathit{rLen}\} : \mathit{rLog}[i] = \mathit{pLog}[i] \land (\mathit{phase} = \text{replaySwept} \Rightarrow \mathit{rLen} = \mathit{pLen}) \\
\mathit{SameRootIffSameState} &\triangleq \forall x, y \in \mathit{cks} : (x.\mathit{root} = y.\mathit{root}) \Leftrightarrow (x.\mathit{state} = y.\mathit{state}) \\
\mathit{PairedOnce} &\triangleq \forall m \in \mathit{Comms} : \mathit{paired}[m] \le \mathit{rigCount}[m] \\
\mathit{NoVolatileResidue} &\triangleq \mathit{NoResidue}(\mathit{trie}) \land \mathit{NoResidue}(\mathit{rTrie}) \\
\mathit{ReplayOutcomeEqual} &\triangleq \neg\, \mathit{forged} \Rightarrow \bigl(\mathit{rOk} \land (\mathit{phase} = \text{replaySwept} \Rightarrow \mathit{rFailed} = \mathit{pFailed} \land \mathit{rSpent} = \mathit{pSpent})\bigr) \\
\mathit{NoResidue}(\mathit{st}) &\triangleq \forall c \in \mathit{Vol} : \mathit{st}.\mathit{data}[c] = \mathit{EmptyPBag} \land \mathit{st}.\mathit{conts}[c] = \mathit{EmptyCBag} \land \neg\, \mathit{st}.\mathit{joins}[c]
\end{aligned}
```

The inductive invariant is a conjunction of ten parts:

```math
\mathit{IndInv} \triangleq \mathit{TypeOK} \land \mathit{TxnShapeInv} \land \mathit{ProgressInv} \land \mathit{DataInv} \land \mathit{PairedOnce} \land \mathit{NoVolatileResidue} \land \mathit{BindingInv} \land \mathit{StagedInv} \land \mathit{SweptClean} \land \mathit{LegacyInv}
```

`BindingInv` says that the bindings hold the recorded occurrences that replay has not paired yet.
`StagedInv` says that the pairings of an open replay transaction stay within them.
`SweptClean` says that a swept store has no volatile residue, and the other parts make the bounds of `TypeOK` inductive.
`LegacyInv` bounds the legacy logs, and it keeps each remaining budget non-negative until its run fails (section [11.14](#1114-legacy-charges-of-decision-45)).

#### 5.2.2 Configurations and results of CheckpointReplay.tla

Every configuration sets `SpacesInBlockHash = TRUE`.
The constant `Budget` is the phlo limit of a deploy, in charges, and `Forge` lets the proposer omit the COMM of its last fire.
The flags `UseUuid`, `RootIgnoresVolatile`, `DoublePair`, `JoinResidue`, `StagingDivergence`, and `AcceptUnpaired` turn on the wrong rules of the Unsafe configurations.
The flags `LegacyPreAfterObserve`, `LegacyTailDropped`, and `RetainPeek` of decision 45 do the same, and `Peeks` and `Legacy` select the mode.
The four older Unsafe configurations set `Budget=8`, which no run reaches, and `Forge=FALSE`.
The bounds column quotes the `\* BOUNDS:` line of each file exactly.

*Table 8. The configurations of CheckpointReplay.tla.*

| Configuration | Leg | Bounds | Distinct states | Depth | Result |
|---|---|---|---|---|---|
| `MC_CheckpointReplay.cfg` | safe | `Chans={"d1","d2","v1"}, Vol={"v1"}, Vals={"a","b"}, Konts={"k"}, MaxTxnOps=2, MaxTxns=2, Blocks=1, Budget=3, Forge=TRUE, Peeks=FALSE, Legacy=FALSE` | 145,255 | 22 | No invariant violated (T1 to T4, T6, `TypeOK`, `IndInv`) |
| `MC_CheckpointReplay_TwoBlock.cfg` | safe | `Chans={"d1","v1"}, Vol={"v1"}, Vals={"a"}, Konts={"k"}, MaxTxnOps=2, MaxTxns=2, Blocks=2, Budget=3, Forge=TRUE, Peeks=FALSE, Legacy=FALSE` | 94,049 | 41 | No invariant violated (T1 to T4, T6, `TypeOK`, `IndInv`) |
| `UuidUnsafe.cfg` | unsafe | `Chans={"d1","v1"}, Vol={"v1"}, Vals={"a"}, Konts={"k"}, MaxTxnOps=2, MaxTxns=2, Blocks=1, Budget=8, Forge=FALSE, Peeks=FALSE, Legacy=FALSE` | 1,904 | 10 | Refutes exactly `ReplayPrefixEqual` |
| `RootIgnoresVolatileUnsafe.cfg` | unsafe | `Chans={"d1","v1"}, Vol={"v1"}, Vals={"a"}, Konts={"k"}, MaxTxnOps=2, MaxTxns=2, Blocks=1, Budget=8, Forge=FALSE, Peeks=FALSE, Legacy=FALSE` | 329 | 6 | Refutes exactly `SameRootIffSameState` |
| `DoublePairUnsafe.cfg` | unsafe | `Chans={"d1","v1"}, Vol={"v1"}, Vals={"a"}, Konts={"k"}, MaxTxnOps=2, MaxTxns=2, Blocks=1, Budget=8, Forge=FALSE, Peeks=FALSE, Legacy=FALSE` | 2,765 | 12 | Refutes exactly `PairedOnce` |
| `JoinResidueUnsafe.cfg` | unsafe | `Chans={"d1","v1"}, Vol={"v1"}, Vals={"a"}, Konts={"k"}, MaxTxnOps=2, MaxTxns=2, Blocks=1, Budget=8, Forge=FALSE, Peeks=FALSE, Legacy=FALSE` | 329 | 6 | Refutes exactly `NoVolatileResidue` |
| `StagingDivergenceUnsafe.cfg` | unsafe | `Chans={"d1","v1"}, Vol={"v1"}, Vals={"a"}, Konts={"k"}, MaxTxnOps=2, MaxTxns=2, Blocks=1, Budget=3, Forge=FALSE, Peeks=FALSE, Legacy=FALSE` | 5,146 | 18 | Refutes exactly `ReplayOutcomeEqual` |
| `AcceptUnpairedUnsafe.cfg` | unsafe | `Chans={"d1","v1"}, Vol={"v1"}, Vals={"a"}, Konts={"k"}, MaxTxnOps=2, MaxTxns=2, Blocks=1, Budget=8, Forge=TRUE, Peeks=FALSE, Legacy=FALSE` | 2,988 | 12 | Refutes exactly `ReplayPrefixEqual` |
| `MC_CheckpointReplay_IndBase_Apalache.cfg` | Apalache BASE | `Chans={"d1","v1"}, Vol={"v1"}, Vals={"a"}, Konts={"k"}, MaxTxnOps=2, MaxTxns=2, Blocks=2, Budget=3, Forge=TRUE, Peeks=FALSE, Legacy=FALSE` | - | 0 | `NoError` |
| `MC_CheckpointReplay_IndStep_Apalache.cfg` | Apalache STEP | `Chans={"d1","v1"}, Vol={"v1"}, Vals={"a"}, Konts={"k"}, MaxTxnOps=2, MaxTxns=2, Blocks=2, Budget=3, Forge=TRUE, Peeks=FALSE, Legacy=FALSE` | - | 1 | `NoError` |
| `DoublePairUnsafe_IndStep_Apalache.cfg` | Apalache Error control | `Chans={"d1","v1"}, Vol={"v1"}, Vals={"a"}, Konts={"k"}, MaxTxnOps=2, MaxTxns=2, Blocks=2, Budget=3, Forge=TRUE, Peeks=FALSE, Legacy=FALSE` | - | 1 | `Error` in `StagedInv`, marker `vStaged[` |
| `MC_CheckpointReplay_Peek.cfg` | safe | `Chans={"d1","v1"}, Vol={"v1"}, Vals={"a"}, Konts={"k"}, MaxTxnOps=2, MaxTxns=2, Blocks=1, Budget=4, Forge=TRUE, Peeks=TRUE, Legacy=FALSE` | 34,991 | 22 | No invariant violated (T1 to T4, T6, `TypeOK`, `IndInv`) |
| `MC_CheckpointReplay_Legacy.cfg` | safe | `Chans={"d1","v1"}, Vol={"v1"}, Vals={"a"}, Konts={"k"}, MaxTxnOps=2, MaxTxns=2, Blocks=1, Budget=12, Forge=TRUE, Peeks=TRUE, Legacy=TRUE` | 34,351 | 22 | No invariant violated (T1 to T4, T6, U13, `TypeOK`, `IndInv`) |
| `MC_CheckpointReplay_Legacy_TwoBlock.cfg` | safe | `Chans={"d1"}, Vol={}, Vals={"a"}, Konts={"k"}, MaxTxnOps=2, MaxTxns=2, Blocks=2, Budget=12, Forge=TRUE, Peeks=TRUE, Legacy=TRUE` | 38,602 | 43 | No invariant violated (T1 to T4, T6, U13, `TypeOK`, `IndInv`) |
| `LegacyPreAfterObserveUnsafe.cfg` | unsafe | `Chans={"d1","v1"}, Vol={"v1"}, Vals={"a"}, Konts={"k"}, MaxTxnOps=2, MaxTxns=2, Blocks=1, Budget=12, Forge=TRUE, Peeks=TRUE, Legacy=TRUE` | 26,034 | 18 | Refutes exactly `ReplayOutcomeEqual` |
| `LegacyTailDroppedUnsafe.cfg` | unsafe | `Chans={"d1","v1"}, Vol={"v1"}, Vals={"a"}, Konts={"k"}, MaxTxnOps=2, MaxTxns=2, Blocks=1, Budget=12, Forge=TRUE, Peeks=TRUE, Legacy=TRUE` | 90 | 4 | Refutes exactly `LegacyChargesFollowHost` |
| `RetainPeekUnsafe.cfg` | unsafe | `Chans={"d1","v1"}, Vol={"v1"}, Vals={"a"}, Konts={"k"}, MaxTxnOps=2, MaxTxns=2, Blocks=1, Budget=12, Forge=TRUE, Peeks=TRUE, Legacy=TRUE` | 326 | 5 | Refutes exactly `LegacyChargesFollowHost` |
| `MC_CheckpointReplay_Legacy_IndBase_Apalache.cfg` | Apalache BASE | `Chans={"d1","v1"}, Vol={"v1"}, Vals={"a"}, Konts={"k"}, MaxTxnOps=1, MaxTxns=1, Blocks=2, Budget=12, Forge=TRUE, Peeks=TRUE, Legacy=TRUE` | - | 0 | `NoError` |
| `MC_CheckpointReplay_Legacy_IndStep_Apalache.cfg` | Apalache STEP | `Chans={"d1","v1"}, Vol={"v1"}, Vals={"a"}, Konts={"k"}, MaxTxnOps=1, MaxTxns=1, Blocks=2, Budget=12, Forge=TRUE, Peeks=TRUE, Legacy=TRUE` | - | 1 | `NoError` |
| `JoinResidueUnsafe_IndStep_Apalache.cfg` | Apalache Error control | `Chans={"d1","v1"}, Vol={"v1"}, Vals={"a"}, Konts={"k"}, MaxTxnOps=2, MaxTxns=2, Blocks=2, Budget=3, Forge=TRUE, Peeks=FALSE, Legacy=FALSE` | - | 1 | `Error` in `SweptClean`, marker `Swept"` |

#### 5.2.3 Unsafe controls and complements of CheckpointReplay.tla

*Table 9. What each Unsafe counterexample shows, and the complement run.*

| Control | Flag | Refuted invariant | What the counterexample shows | Complement |
|---|---|---|---|---|
| `UuidUnsafe` | `UseUuid = TRUE` | T1 `ReplayPrefixEqual` | Each identity carries a nonce of its run, so the validator logs other identities than the proposer, and the replay log stops being a prefix of the play log. | 6,233 states, depth 22, no other violation |
| `RootIgnoresVolatileUnsafe` | `RootIgnoresVolatile = TRUE` | T2 `SameRootIffSameState` | No sweep runs, and the root skips the volatile channels. So two checkpoints share a root but differ in volatile state. | 6,813 states, depth 22, no other violation |
| `DoublePairUnsafe` | `DoublePair = TRUE` | T3 `PairedOnce` | `verify` does not stop after the first bound key, so one COMM pairs under its consume key and again under its produce key. | 6,811 states, depth 22, no other violation |
| `JoinResidueUnsafe` | `JoinResidue = TRUE` | T4 `NoVolatileResidue` | The sweep removes data and continuations only, as `remove_all_continuations` does, so a volatile join entry reaches the history trie. | 6,813 states, depth 22, no other violation |
| `StagingDivergenceUnsafe` | `StagingDivergence = TRUE` | T6 `ReplayOutcomeEqual` | An unpaired replay fire fails at staging, which is the rule of decision 19. Play runs out of phlo at a produce fire of its second transaction, so the log lacks that COMM. Replay drops the transaction and rejects an honest block, as in the [replay finding](findings/space-replay-out-of-phlo.md). | 6,561 states, depth 22, no other violation |
| `AcceptUnpairedUnsafe` | `AcceptUnpaired = TRUE` | T1 `ReplayPrefixEqual` | An unpaired replay fire commits as if it paired. When the proposer omits the COMM of its last fire, the validator commits and logs that fire, so its log stops being a prefix of the published log. | 8,357 states, depth 22, no other violation |
| `LegacyPreAfterObserveUnsafe` | `LegacyPreAfterObserve = TRUE` | T6 `ReplayOutcomeEqual` | The pre-charge of a fire comes after its observer calls. Play runs out of phlo at that pre-charge, so the log lacks the fire. Replay then reaches the pairing check first and diverges on an honest block. | 34,227 states, depth 22, no other violation |
| `LegacyTailDroppedUnsafe` | `LegacyTailDropped = TRUE` | U13 `LegacyChargesFollowHost` | The job charges no tail after the commit, so the legacy log of play misses the post-charges of the last operation. | 33,887 states, depth 22, no other violation |
| `RetainPeekUnsafe` | `RetainPeek = TRUE` | U13 `LegacyChargesFollowHost` | A peek fire keeps its record, as the retention before Rule E did. The space then makes one host step fewer than the host program, which removes the record and sends it again. | 35,243 states, depth 22, no other violation |

#### 5.2.4 Apalache checks of CheckpointReplay.tla

The gate runs six Apalache checks on this module, with the names `base`, `step`, `step-doublepair-unsafe`, `step-joinresidue-unsafe`, `legacy-base`, and `legacy-step`.
BASE checks `Init` against `IndInv`, and STEP starts from `IndInit`, the set of every state that satisfies `IndInv`.
Both pass with `NoError`, so `PairedOnce` and `NoVolatileResidue` hold at every depth for the constants of the configuration.
With `DoublePair`, one replay stage step breaks `StagedInv`, and the violation holds the marker `vStaged[`.
With `JoinResidue`, one deploy-end step leaves a volatile join and breaks `SweptClean`, with the marker `Swept"`.

The budget, the forged logs, and the stops of decision 36 keep `IndInv` inductive, so both checks pass with `Budget=3` and `Forge=TRUE`.
The checks `legacy-base` and `legacy-step` prove `IndInv` with peeks and the legacy charges, with one operation in one transaction of each block.
The recipe steps are `p1-tla-apalache-base`, `p1-tla-apalache-step`, `p1-tla-apalache-step-doublepair-unsafe`, `p1-tla-apalache-step-joinresidue-unsafe`, `p1-tla-apalache-legacy-base`, and `p1-tla-apalache-legacy-step`.

#### 5.2.5 The two-block replay model

The configuration `MC_CheckpointReplay_TwoBlock.cfg` is the two-block replay model of the leaf `rrs-fv-rocq-tla-two-block-replay-model`.
The validator of block 2 resets to the root of block 1, and TLC explores 94,049 distinct states to depth 41 with no violation.
Its Unsafe controls are the nine configurations of Table 9 with their complements, which use `Blocks=1`.
Its Apalache checks use `Blocks=2`.
The invariant map names these property tests for T1 to T4 and T6:

- T1 `ReplayPrefixEqual`: `prop_space_txn_replay_pairs_play_log`, `prop_space_txn_cross_replay_with_host`, and `prop_space_txn_repeated_channel_groups_equal_host`.
- T2 `SameRootIffSameState`: `prop_space_txn_play_root_equals_host`.
- T3 `PairedOnce`: `prop_space_txn_replay_divergence_typed_at_commit`.
- T4 `NoVolatileResidue`: `prop_sweep_volatile_exact_and_root_neutral`.
- T6 `ReplayOutcomeEqual`: `prop_space_txn_replay_reproduces_play_rejection`.

The integration suite checks the same replay on the Rust code (section [7.4](#74-deterministic-substrate-suites)).
Its group B tests the checkpoints, and its group C tests the replay.

The recipe `rrs-p1-tla-checkpoint` has 29 steps.
Its TLC steps are `p1-tla-safe-mc-checkpointreplay`, `p1-tla-safe-mc-checkpointreplay-twoblock`, `p1-tla-uuidunsafe`, `p1-tla-uuidunsafe-complement`, `p1-tla-rootignoresvolatileunsafe`, `p1-tla-rootignoresvolatileunsafe-complement`, `p1-tla-doublepairunsafe`, and `p1-tla-doublepairunsafe-complement`.
They also include `p1-tla-joinresidueunsafe`, `p1-tla-joinresidueunsafe-complement`, `p1-tla-stagingdivergenceunsafe`, `p1-tla-stagingdivergenceunsafe-complement`, `p1-tla-acceptunpairedunsafe`, and `p1-tla-acceptunpairedunsafe-complement`.
Decision 45 adds `p1-tla-safe-mc-checkpointreplay-peek`, `p1-tla-safe-mc-checkpointreplay-legacy`, and `p1-tla-safe-mc-checkpointreplay-legacy-twoblock`.
It also adds `p1-tla-legacypreafterobserveunsafe`, `p1-tla-legacytaildroppedunsafe`, and `p1-tla-retainpeekunsafe`, each with its step `-complement`.
Its other steps are the six Apalache steps above.

### 5.3 TxnLockCoverage.tla

[TxnLockCoverage.tla](../../../formal/tlaplus/reified_rspaces/TxnLockCoverage.tla) models [lock coverage](glossary.md#lock-coverage) (FR-4).
Space transactions and host operations share one hot store.
A transaction takes the stripes of its lock keys at begin.
Each staging step checks coverage, also of the phase-B stripes of the join channels of a produce.
A host operation runs atomically under its own stripes, and TLC explores every interleaving of begins, stages, commits, and host operations.
The channels `a` and `b` have distinct stripes, and the groups include `<<"a","a">>` and `<<"b","b">>`, which repeat a channel (FR-1').

#### 5.3.1 Invariants of TxnLockCoverage.tla

*Table 10. The invariants of TxnLockCoverage.tla.*

| Id | Invariant | Meaning |
|---|---|---|
| T5 | `Serializable` | The committed history equals a serial order: some order of the committed units, one for each transaction and one for each host operation, gives every recorded outcome under the host rule and ends in the current store. |
| Auxiliary | `TypeOK` | The type invariant of the module. |
| Auxiliary | `LockIndInv` | The inductive strengthening of T5 for the Apalache checks. TLC checks it on every reachable state of `MC_TxnLockCoverage.cfg`. |
| Auxiliary | `CommitOrderWitness` | When the ghost variable `serialOk` holds, the commit order itself witnesses T5. TLC checks it on every reachable state of `MC_TxnLockCoverage.cfg`. |

```math
\begin{aligned}
\mathit{Serializable} &\triangleq \exists\, \pi \in \mathit{PermTable}[\mathrm{Len}(\mathit{hist})] : \mathit{SerialRunOK}(\mathit{Flatten}(\pi)) \\
\mathit{LockIndInv} &\triangleq \mathit{TypeOK} \land \mathit{serialOk} \land \mathit{ShapeInv} \land \mathit{LocksDisjoint} \land \mathit{BudgetInv} \land \forall t \in \mathit{Txns} : \mathit{tstate}[t] = \text{open} \Rightarrow \mathit{OpenInv}(t)
\end{aligned}
```

`OpenInv` says that the host rule allows each staged outcome of an open transaction, and that the transaction covers each one.
It also says that the staged view agrees with the store on every protected key.

#### 5.3.2 Configurations and results of TxnLockCoverage.tla

Every configuration of this module has the same bounds line: `Chans={"a","b"} (stripes 1, 2), Groups={<<"a">>,<<"b">>,<<"a","b">>,<<"a","a">>,<<"b","b">>} (FR-1'), Txns={"t1","t2"}, MaxTxnOps=2, MaxHostOps=2`.
The flags `CheckCoverage` and `CheckJoinCoverage` turn the two coverage checks on or off.

*Table 11. The configurations of TxnLockCoverage.tla.*

| Configuration | Leg | Distinct states | Depth | Result | Complement |
|---|---|---|---|---|---|
| `MC_TxnLockCoverage.cfg` | safe | 2,424,561 | 11 | No invariant violated (`Serializable`, `TypeOK`, `LockIndInv`, `CommitOrderWitness`) | - |
| `UncoveredChannelUnsafe.cfg` | unsafe | 1,428 | 5 | Refutes exactly `Serializable` | 7,423,409 states, depth 11, no other violation |
| `UnjoinedChannelUnsafe.cfg` | unsafe | 164,411 | 8 | Refutes exactly `Serializable` | 2,542,569 states, depth 11, no other violation |
| `MC_TxnLockCoverage_IndBase_Apalache.cfg` | Apalache BASE, `lock-base` | - | 0 | `NoError` | - |
| `MC_TxnLockCoverage_IndStep_Apalache.cfg` | Apalache STEP, `lock-step` | - | 1 | `NoError` | - |
| `UncoveredChannelUnsafe_IndStep_Apalache.cfg` | Apalache Error control, `lock-step-uncovered-unsafe` | - | 1 | `Error` in `OpenInv`, marker `stagedOne` | - |
| `UnjoinedChannelUnsafe_IndStep_Apalache.cfg` | Apalache Error control, `lock-step-unjoined-unsafe` | - | 1 | `Error` in `OpenInv`, marker `stagedOne` | - |

The two Unsafe controls show these counterexamples:

- `UncoveredChannelUnsafe` sets `CheckCoverage = FALSE`. A transaction stages an operation on a channel without its stripe (review finding F-LOCK-1). A host operation then changes that channel between the staging and the commit, and no serial order gives the recorded outcomes.
- `UnjoinedChannelUnsafe` sets `CheckJoinCoverage = FALSE`. A produce checks the stripe of its own channel but not the phase-B stripes of its join channels. Two transactions on disjoint stripes then each store one half of a waiting join, which no serial order allows.

The recipe `rrs-p1-tla-lock` has 9 steps: `p1-tla-safe-mc-txnlockcoverage`, `p1-tla-uncoveredchannelunsafe`, `p1-tla-uncoveredchannelunsafe-complement`, `p1-tla-unjoinedchannelunsafe`, `p1-tla-unjoinedchannelunsafe-complement`, `p1-tla-apalache-lock-base`, `p1-tla-apalache-lock-step`, `p1-tla-apalache-lock-step-uncovered-unsafe`, and `p1-tla-apalache-lock-step-unjoined-unsafe`.
The recipe `rrs-p1-tla-lock-core` has the same steps without the two complements, which saves about 18 minutes of a run.
The invariant map names `prop_space_txn_rejects_uncovered_channels` for T5.

## 6. Negative controls

A negative control is a check that must fail ([negative control](glossary.md#negative-control)).
It shows that a passing check depends on the rule that the control changes [[DLS78](#ref-dls78)].

### 6.1 Rocq mutants

The script `scripts/verify-reified/rocq-negative-controls.py` holds 75 mutants.
Table 12 holds 29 and Table 39 of section [11.5](#115-negative-controls-of-p4) holds 29.
Table 45 of section [11.10](#1110-metering-of-decision-42) holds 5, and Table 47 of section [11.11](#1111-native-execution-of-decisions-46-and-47) held 1, and holds 2 since decision 47.
Table 50 of section [11.12](#1112-re-entry-of-decision-43) and Table 55 of section [11.13](#1113-peek-re-store-of-decision-45) hold 3 each, and Table 59 of section [11.14](#1114-legacy-charges-of-decision-45) holds 5.
Each mutant replaces text that must occur exactly once in a scratch copy of its module, so a stale mutant fails.
The control passes only when the mutated copy fails to compile and the first error lies inside one of the expected declarations.

The pristine modules must compile first. The run after the merge of [decision 44](decision-register.md#decision-44) passed all 57 mutants of that time (`target/verification/reified-rspaces/merge-car/proofs-2.log`).
The run of [decision 42](decision-register.md#decision-42) passed all 62 mutants after the 10 pristine modules compiled (`target/verification/reified-rspaces/d42/proofs.log`).
The run of [decision 43](decision-register.md#decision-43) passed all 66 mutants after the 10 pristine modules compiled (`target/verification/reified-rspaces/d43/gates/proofs.log`).
The run of [decision 45](decision-register.md#decision-45) passed all 70 mutants after the 10 pristine modules compiled (`target/verification/reified-rspaces/d45/gates/proofs.log`).
The run of its Option P passed all 75 mutants after the 11 pristine modules compiled (`target/verification/reified-rspaces/d45/option-p-gates/proofs.log`).

*Table 12. The 29 Rocq mutants of the P1 modules. EC is EventCorrelation.v, and VS is VolatileSweep.v.*

| Id | Module | What the mutant changes | Must fail in | Rule it shows |
|---|---|---|---|---|
| `vs-strict-order` | VS | Adjacent channels of a group must differ (the FR-1 strictness). | `nondecreasing_sorted` | FR-1' |
| `vs-example-claims-fr1` | VS | The H22 example claims that `[x; x]` is not canonical. | `repeated_group_canonical_when_sorted` | FR-1' |
| `vs-unique-without-antisymmetry` | VS | The uniqueness lemma loses its antisymmetry premise. | `sorted_unique_canonical` | FR-1' |
| `vs-weak-antisymmetry` | VS | The antisymmetry premise concludes nothing. | `sorted_unique_canonical` | FR-1' |
| `vs-no-totality` | VS | The totality premise holds for every comparison. | `isort_canonical` | FR-1' |
| `vs-keys-ignore-order` | VS | Collision freedom says nothing about two continuation keys. | `sorted_keys_injective` | FR-1' |
| `vs-sweep-skips-canonical-check` | VS | The sweep does not validate the order of the stored join groups. | `validate_spec` | FR-1 |
| `vs-sweep-ignores-open-txn` | VS | The handle sweeps under an open transaction of the same task. | `sweep_exact` | FR-9' |
| `ec-check-rejects-repeats` | EC | The group check rejects a repeated channel again. | `canonical_nondecreasing` | FR-1' |
| `ec-h17-claims-fr1` | EC | H17 claims that a repeated channel fails. | `non_canonical_fails_closed` | FR-1' |
| `ec-unique-without-antisymmetry` | EC | The uniqueness lemma loses its antisymmetry premise. | `sorted_unique_canonical` | FR-1' |
| `ec-read-caches-rejected-key` | EC | A rejected continuations read caches its key. | `access_grows` | FR-1 |
| `ec-scope-does-not-reset` | EC | `begin_evaluation_scope` keeps the tracker. | `two_evaluations_commit` | FR-5' |
| `ec-store-swap-ignored` | EC | The tracker ignores the store identity. | `scope_parents_other` | FR-5' |
| `ec-session-change-ignored` | EC | The tracker ignores the session. | `scope_parents_other` | FR-5' |
| `ec-parent-reuse-admitted` | EC | A second commit under one parent in one scope is admitted. | `run_calls_scope` | FR-5' |
| `ec-two-evaluations-claim-fr5` | EC | The two-evaluation lemma claims that the second evaluation fails after a scope reset. | `two_evaluations_commit` | FR-5' |
| `ec-no-reset-claims-commit` | EC | The two-evaluation lemma claims that a commit without a reset reuses the parent. | `two_evaluations_commit` | FR-5' |
| `ec-fire-stores-incoming` | EC | A peek fire stores its non-persistent incoming datum again, in the view and in both commits. | `fire_stores_no_datum` | FR-10 |
| `ec-h11-claims-incoming-stored` | EC | H11 claims that a committed non-persistent produce-side peek fire stores its incoming datum. | `peek_reproduces_trace_eq_host` | FR-10 |
| `ec-peek-retains` | EC | A consume-side peek fire keeps its stored records, in the view and in both commits (the retention of FR-10). | `fire_view_removes` | Decision 45 |
| `ec-restore-skipped` | EC | The agent stages no re-store after a consume-side peek fire. | `restores_are_produce_peeks` | Decision 45 |
| `ec-restore-wrong-identity` | EC | The re-store sends the matched value instead of the stored record, so it takes another produce identity. | `restore_same_identity` | Decision 45 |
| `ec-plan-stores-incoming` | EC | The head-only plan stores the datum after a peek fire instead of the re-offer. | `offer_keeps_no_pending_match` | FR-10 |
| `ec-reoffer-due-without-a10` | EC | The re-offer is also due after a persistent waiter. | `offer_ops_reoffers` | FR-10 |
| `ec-replay-fire-fails-at-staging` | EC | An unpaired replay fire fails at staging, which is the rule of decision 19, so replay stages fewer operations than play. | `replay_stage_sim` | Decision 36 |
| `ec-unpaired-skips-observe-comm` | EC | An unpaired replay fire fails before its `observe_comm` call, so replay makes fewer calls than play. | `replay_produce_sim` | Decision 36 |
| `ec-unpaired-fire-commits` | EC | An unpaired replay fire commits: it logs, applies its effects, and unbinds a COMM that staging did not pair. | `unpaired_fire_inert` | Decision 36 |
| `ec-split-zero-child` | EC | A split appends the segment $`(s, i)`$ instead of the DR-84 segment $`(s, i + 1)`$, so a child segment can equal an operation segment. | `sched_part_nonzero` | Decision 44 |

Eleven mutants cover FR-1 and FR-1', six cover FR-5', and one covers FR-9'.
Four cover FR-10, three cover decision 36, one covers decision 44, and three cover decision 45.
[Decision 45](decision-register.md#decision-45) disabled the FR-10 controls `ec-kept-includes-incoming` and `ec-h11-claims-p1-deviation`, because Rule E removes the kept records that they mutate.
The script keeps both controls as comments with that reason.

### 6.2 TLA+ and Apalache controls

The TLA+ controls are the eight Unsafe configurations, with their complements, and the four Apalache Error controls.
Tables 8, 9, and 11 give each control, its refuted invariant, and what its counterexample shows.
The Error controls are `step-doublepair-unsafe` (marker `vStaged[`), `step-joinresidue-unsafe` (marker `Swept"`), `lock-step-uncovered-unsafe` (marker `stagedOne`), and `lock-step-unjoined-unsafe` (marker `stagedOne`).

### 6.3 Rust negative controls

The substrate suites hold five negative controls:

- `correlation_rejects_datum_without_produce_event` (`space_store_handle.rs:4064`) and `correlation_rejects_comm_before_trigger` (`space_store_handle.rs:4080`) show that the event correlation of the commit rejects a defect.
- `space_store_handle_remove_all_continuations_keeps_joins_control` (`reified_space_store_handle.rs:2199`) pins the HEAD behavior that `JoinResidueUnsafe` models.
- `space_store_handle_host_permuted_keys_break_checkpoint_control` (`reified_space_store_handle.rs:2510`) shows that the host refuses a checkpoint with one channel set under two orders.
- `space_store_handle_host_replay_repeated_channel_control` (`reified_space_store_handle.rs:3439`) shows that host replay of a repeated channel can compute another COMM than the play log.

The P4 suite `rholang/tests/reified_queue_replay.rs` holds the control of [decision 36](decision-register.md#decision-36), `space_queue_out_of_phlo_sweep_replays_equal` (`reified_queue_replay.rs:850`).
It plays a space deploy out of phlo at many limits, and it replays each play log as Casper does.
On the code before the fix, 143 of its 232 deploys replayed to another outcome than play (`target/verification/reified-rspaces/p4/fx-c/step1-sweep-before.log`).
After the fix, every deploy replays to the outcome of play (`step3-sweep-after.log` of the same directory).

The regression suite has its own controls (section [8.4](#84-negative-controls-of-the-regression-suite)).

### 6.4 Checker self-tests

The runner `scripts/verify-reified/run-checker-tests.py` runs the unit-test modules of the checkers in `scripts/verify-reified/tests/`.
A module passes only when it has at least one test and every test passes.
A skipped test, an expected failure, and an unexpected success make the module fail, because they give no evidence.
The runner sets `sys.dont_write_bytecode`, so a run writes no `__pycache__` directory into the snapshot.

*Table 13. The checker self-tests (last run: 278 tests, 0 failures).*

| Module | Tests | What the tests check |
|---|---|---|
| `test_checkers` | 108 | The consensus-checklist checker (21), the footprint checker (28), the completeness checker (11), the Rust citation rule (10) and module (4), the ORD phase of the invariant-map checker (8), the production scan (7), the model-map checker for a phase other than P1 (9), the P1 ids H23 and T6 of decision 36 in both map checkers (5), and the merge script of the queue parity golden (5). |
| `test_closure` | 87 | The policy loading (5), the modeled rules (11), the invariant rules (6), the entry-point rules (4), the document rules (10), the audit harness (22), the ledger-copy rules (11) and controls (7), and the runner itself (11). |
| `test_doc_lint` | 83 | Every rule of `doc_lint.py` on a passing fixture and a failing fixture, the command line, the render check, and a manifest row that names an example file. |

## 7. Property-based tests and substrate suites

### 7.1 Configuration

Each [property-based test](glossary.md#property-based-test) of P1 uses the library proptest 1.11.0 [[CH00](#ref-ch00), [PT](#ref-pt)].
Its configuration fixes `cases: 64`, `rng_seed: RngSeed::Fixed(SEED)` with `SEED = 0x0E0_5EED_2026_1004`, and `failure_persistence: None` (`reified_space_store_handle_props.rs:47-56`, `reified_space_store_handle_par.rs:347-353`).
The recipes also set `PROPTEST_CASES=64` and `PROPTEST_RNG_SEED=20261004` in their environment.
These variables feed only `ProptestConfig::default()`, and the explicit fields override them, so a run has the same cases with or without the variables.
Without failure persistence, a run writes nothing to the source tree.

The generators also make groups that are not canonical, with the probability 0.15, which the substrate must reject with no effect.
They make sorted groups that repeat a channel, with the probability 0.15, which the substrate must accept (FR-1').
Where a property allows peeks, a generated consume peeks with the probability 0.6.

### 7.2 The 25 P1 properties

*Table 14. The 25 properties of phase P1 in the invariant map.*

| # | Property | Location | Covers | What it checks |
|---|---|---|---|---|
| 1 | `prop_space_txn_play_trace_equals_host` | `reified_space_store_handle_props.rs:2389` | H1 | A commit of staged operations gives the observer calls and events of the host. |
| 2 | `prop_space_txn_play_root_equals_host` | `reified_space_store_handle_props.rs:2394` | H2, T2 | The committed state, and so the checkpoint root, equals the host state. |
| 3 | `prop_space_txn_replay_pairs_play_log` | `reified_space_store_handle_props.rs:2399` | H5, T1 | Replay pairs every COMM of the play log once and reaches the play root. |
| 4 | `prop_space_txn_cross_replay_with_host` | `reified_space_store_handle_props.rs:2404` | T1 | A host play log replays through transactions, a transaction play log replays through the host, and both logs are equal. |
| 5 | `prop_space_txn_replay_divergence_typed_at_commit` | `reified_space_store_handle_props.rs:2409` | H4, T3 | A replay fire whose COMM the log does not hold stages as in play, with no host call, and stays unpaired. The commit applies the operations before it, makes the calls of the fire through its COMM call, and fails with `ReplayDivergence`. Nothing of the fire or after it is logged, applied, or unbound. |
| 6 | `prop_space_txn_view_equals_committed_store` | `reified_space_store_handle_props.rs:2414` | H6 | The reads after staging equal the store after the commit, also with peek fires and their re-offers. |
| 7 | `prop_space_txn_joins_lockstep` | `reified_space_store_handle_props.rs:2419` | H7 | Join entries and continuations change in lockstep, in the view and in the committed store. |
| 8 | `prop_sweep_volatile_exact_and_root_neutral` | `reified_space_store_handle_props.rs:2424` | H13, H14, H15, T4 | The sweep removes exactly the volatile records, continuations, and joins, keeps the durable state, and does not move the root. |
| 9 | `prop_sub_orders_contiguous_and_disjoint` | `reified_space_store_handle_props.rs:2429` | H8 | Sub-orders sort between their parent and the next step, never equal a reduction order, and keep the events of a transaction together. |
| 10 | `prop_space_txn_abort_inert` | `reified_space_store_handle_props.rs:2445` | H9 | A transaction that drops without a commit leaves no event, observer call, write, or counter change, and it releases its locks. |
| 11 | `prop_event_coverage_holds_for_generated_commits` | `reified_space_store_handle_props.rs:2450` | H10 | Each committed store change has its event, and each COMM follows its trigger event. |
| 12 | `prop_space_txn_peek_restore` | `reified_space_store_handle_props.rs:2455` | H5, H11, H12 | A peek fire has the calls, events, and state of the HEAD peek path, with the re-offer and the re-stores as the next produces, and it replays and reports from the play log. |
| 13 | `prop_space_txn_reporting_equals_replay` | `reified_space_store_handle_props.rs:2460` | H12 | Reporting replays exactly as replay does, and it reports what the host reporting space reports. |
| 14 | `prop_root_leaf_record_count_matches_history` | `reified_space_store_handle_props.rs:2465` | H16 | The root leaf record count is the number of records in the leaf. |
| 15 | `prop_counter_timing_play_equals_replay` | `reified_space_store_handle_props.rs:2470` | H3 | Play counts the fired produce, replay increments before it matches, and both give the COMMs of the host. |
| 16 | `prop_space_txn_replay_requires_trigger_event` | `reified_space_store_handle_props.rs:2475` | H4 | Without the trigger event in the log, the fire stages unpaired, and the commit fails with `ReplayDivergence` after the calls of the fire, with the operations before it applied. With the complete log, the same fire pairs and commits. |
| 17 | `prop_space_txn_non_canonical_inputs_fail_closed` | `reified_space_store_handle_props.rs:2491` | H9, H17, H22 | A group that is not canonical fails before any store or cache effect, and the checkpoint root equals the reference. |
| 18 | `prop_space_txn_reads_root_neutral` | `reified_space_store_handle_props.rs:2501` | H18 | Reads of canonical keys never change the checkpoint root. |
| 19 | `prop_space_txn_replay_directive_fails_closed` | `reified_space_store_handle_props.rs:2511` | H19 | A native replay directive rejects its operation with no effect, and the operations before it stay applied. |
| 20 | `prop_space_txn_parent_order_single_commit` | `reified_space_store_handle_props.rs:2519` | H20 | At most one transaction commits under one parent order, and a second commit fails with `ParentOrderReused` and no effect. |
| 21 | `prop_space_txn_store_identity_ignores_ord_only_fields` | `reified_space_store_handle_props.rs:2524` | H21 | Channels that differ only in a field of the Rust ordering are one channel for every decision of the substrate. |
| 22 | `prop_space_txn_rejects_uncovered_channels` | `reified_space_store_handle_props.rs:2531` | T5 | An operation on a channel without its stripe fails with `UnlockedChannel` and no effect, and the final state equals a serial order. |
| 23 | `prop_space_txn_par_identity_ignores_locally_free` | `reified_space_store_handle_par.rs:356` | H21 | `Par` channels that differ only in `locally_free` stage, read, commit, and checkpoint as one channel. |
| 24 | `prop_space_txn_repeated_channel_groups_equal_host` | `reified_space_store_handle_props.rs:2536` | H1, H2, H5, H17, T1 | Sorted groups that repeat a channel give the calls, events, state, and root of the host, and they replay to the play root. |
| 25 | `prop_space_txn_replay_reproduces_play_rejection` | `reified_space_store_handle_props.rs:2483` | H23, T6 | Play rejects its n-th fallible observer call, as a deploy that runs out of phlo there, and keeps its log. Replay with that log fails at the same call, with the outcome, the observer calls without the directive queries, and the state of play, and it uses every recorded event. |

### 7.3 The 19 witness tests

A [witness test](glossary.md#witness-test) is a deterministic test of one case of a headline or a rule.
The invariant map lists 19 witness tests for P1.
The test `space_store_handle_commit_checks_cost_changed_entries` covers no headline, because the models do not include cost.

*Table 15. The 19 witness tests of phase P1.*

| Test | Location | Rule | Covers | What it shows |
|---|---|---|---|---|
| `space_store_handle_repeated_group_consume_fire_binds_distinct_records` | `reified_space_store_handle.rs:3208` | FR-1' | H1, H2, H17 | A consume on a group that repeats a channel binds one record for each occurrence. |
| `space_store_handle_repeated_group_produce_fire_binds_incoming_once` | `reified_space_store_handle.rs:3284` | FR-1' | H1, H2, H9, H17 | A produce-side fire binds the incoming datum exactly once, and a fire that binds it twice fails closed. |
| `space_store_handle_unsorted_repeat_fails_closed` | `reified_space_store_handle.rs:3371` | FR-1' | H9, H17, H18 | A repeat out of sorted order fails before any access, and the checkpoint keeps the reference root. |
| `space_store_handle_evaluation_scope_resets_parent_orders` | `reified_space_store_handle.rs:3497` | FR-5' | H20 | After `begin_evaluation_scope`, a second evaluation of one deploy commits under the same parent order. |
| `space_store_handle_store_reset_resets_parent_orders` | `reified_space_store_handle.rs:3532` | FR-5' | H20 | A reset or a revert starts a new tracking scope. |
| `space_store_handle_sweep_in_open_transaction_fails_closed` | `reified_space_store_handle.rs:3567` | FR-9' | H13 | A sweep in an open transaction of the same task fails with `NestedTransaction` before it takes a lock. |
| `space_store_handle_commit_checks_cost_changed_entries` | `reified_space_store_handle.rs:3751` | FR-7' | none | The always-on checks copy as many records for 1 stored record as for 32. |
| `space_store_handle_produce_peek_join_removes_stored_and_leaves_incoming_unstored` | `reified_space_store_handle.rs:3978` | FR-10 | H5, H6, H11 | A produce-side peek join removes its stored non-persistent record and never stores its incoming datum, in the view or in the store ([decision 45](decision-register.md#decision-45)). |
| `space_store_handle_peek_reoffer_fires_next_waiter` | `reified_space_store_handle.rs:4072` | FR-10 | H3, H6, H11 | The re-offer fires the next waiter, and the counter steps once for each produce. |
| `space_store_handle_peek_reoffer_stores_without_waiter` | `reified_space_store_handle.rs:4171` | FR-10 | H1, H3, H11 | With no second waiter, the re-offer stores the datum, as the HEAD re-produce does. |
| `space_store_handle_peek_reoffer_equals_head_reproduce` | `reified_space_store_handle.rs:4238` | FR-10 | H1, H11 | The re-offer has the calls, events, contents, and root of the HEAD re-produce. |
| `space_store_handle_peek_reoffer_authority_conflict_fails_at_reoffer` | `reified_space_store_handle.rs:4328` | FR-10 | H1, H6, H11 | A conflicting authority fails the commit at the re-offer, and the peek fire stays applied. |
| `space_store_handle_peek_reoffer_replay_and_report` | `reified_space_store_handle.rs:4369` | FR-10 | H1, H4, H5, H11, H12, T1 | A peek fire and its re-offer replay and report from their play log with the play root. |
| `space_store_handle_peek_fire_receipt_flags` | `reified_space_store_handle.rs:4492` | FR-10 | H1, H11 | Every result of a peek fire keeps the persist flag of the host, and the re-store and the re-offer report Stored with the identities of their records. |
| `space_store_handle_peek_restore_equals_head_reproduce` | `reified_space_store_handle.rs:4641` | FR-10 | H1, H2, H11, H12 | A peek and its re-stores have the events, observer calls, state, and root of the HEAD program, whose reducer sends the peeked data again, in play, replay, and reporting. |
| `space_store_handle_restore_without_removal_duplicates_record` | `reified_space_store_handle.rs:4792` | FR-10 | H2, H11 | Control: the removal and the re-store leave one copy of the record. A re-store without the removal leaves two copies, and a removal without the re-store loses the record. |
| `space_store_handle_receipt_keeps_the_host_flags` | `space_store_handle.rs:4435` | FR-10 | H11 | The receipt keeps the flag of the host for every result, and results that do not line up with the candidates fail closed. |
| `space_store_handle_replay_reproduces_play_rejection` | `reified_space_store_handle.rs:1997` | Decision 36 | H23, T6 | For each position of a fallible observer call, play fails there and keeps its log. Replay with that log fails at the same call, with the outcome, the calls, and the state of play. |
| `space_queue_out_of_phlo_sweep_replays_equal` | `reified_queue_replay.rs:850` | Decision 36 | H23, T6 | A queue deploy that runs out of phlo at each limit of a sweep replays to the error codes and the cost of play, which Casper compares. A divergent pair at the limit 100000 does too. |

### 7.4 Deterministic substrate suites

*Table 16. The deterministic suites of the substrate and their recipe steps.*

| Suite | Tests | Subject | Recipe: step |
|---|---|---|---|
| `rspace++/tests/reified_space_store_handle.rs`, group A | 22 | Host parity of every operation shape against a twin host. | `rrs-p1-rspace`: `p1-integration` |
| Group B | 5 | Checkpoints, history read-through, and the LMDB round trip `space_store_handle_lmdb_checkpoint_round_trip` (`reified_space_store_handle.rs:1324`). | The same step |
| Group C | 11 | Replay lookup parity with host replay, the typed divergence at commit, and the replay of a rejection of play ([decision 36](decision-register.md#decision-36)). | The same step |
| Group D | 7 | The volatile sweep. | The same step |
| Group E | 3 | Reporting delegation. | The same step |
| Group F | 15 | The review regressions: canonical order, store identity, trigger-key pairing, lock coverage, parent orders, nested begin, sweep bookkeeping, and peeks in replay. | The same step |
| Group G | 10 | The round-2 rules FR-1', FR-5', FR-7', and FR-9'. | The same step |
| Group H | 9 | The round-3 rule FR-10 and Rule E of [decision 45](decision-register.md#decision-45): the peek re-offer, the re-store, and the receipt flags. | The same step |
| Library tests of `space_store_handle.rs` (16), `space_play.rs` (1), and `space_replay.rs` (1) | 18 | Sub-orders, view rules, event correlation, store identity, canonical groups, lock stripes, the parent tracker, receipts, counter overflow, and the replay estimate of the sweep. | `rrs-p1-rspace`: `p1-lib` |
| `rspace++/tests/legacy_golden_corpus.rs` | 4 | Channel hashes, event identities, the empty radix root, and a non-empty root with its event. | `rrs-p1-rspace`: `p1-legacy-golden` |
| `rholang/tests/reified_space_store_handle_par.rs` | 3 | Property 23 and two tests of `Par` identity and continuation dedup. | `rrs-p1-rholang`: `p1-rholang-par` |
| `rholang/tests/reified_space_txn_host_bounds.rs` | 1 | `reified_space_txn_host_bounds_hold_for_par_hosts`: each host is a `SpaceTxnHost` over `Par`. | `rrs-p1-rholang`: `p1-rholang-host-bounds` |
| The whole `rspace++` suite, without the four timing tests that HEAD ignores | at least 795 | Every `rspace++` test. The count 795 comes from a run before the LMDB test existed. | `rrs-p1-rspace-full`: `p1-full-rspace` |

The test `space_store_handle_lmdb_checkpoint_round_trip` is the only substrate test that writes to a disk directory, which it makes under `TMPDIR`.
It commits a datum and a parked two-channel join, checkpoints to an LMDB directory, and drops every handle.
Then it reopens the directory, resets to the root, and compares data, continuations, joins, the root, and the leaf record counts.
Every other substrate test uses an in-memory store.

### 7.5 The ORD phase

The phase ORD of the invariant map has one entry, `O1`, of the kind `checklist`.
It binds item 10 of the consensus-preservation checklist (`host-replay-acceptance`) to the property `prop_ordinary_play_replay_equal`.
The witness tests of ORD are `reified_ordinary_regression_r9_play_replay_equality` (rule R9, covers O1) and `ordinary_play_replay_negative_control_detects_mutants` (a negative control, which covers nothing).
`check-invariant-map.py --phase ORD` checks that the checklist item names the property, and that both tests exist.
Section [8.3](#83-the-ordinary-playreplay-property) describes the property.

## 8. Regression checks for ordinary execution

### 8.1 Consensus-preservation checklist

The map `scripts/verify-reified/consensus-checklist.json` restates the 22 items of the checklist of the plan, with the checks that prove each item.
The checker `scripts/verify-reified/check-consensus-checklist.py` confirms each active check at one milestone (section [8.5](#85-milestone-rule-of-the-checklist-checker)).
At the milestone `p1`, it printed `ITEMS=22 CHECKS=116 PASS=109 FAIL=0 SCHEDULED=7 RETIRED=0`.

Table 17 gives each check with its recipe step and the category that it feeds.
Each step `p1-regression-*` belongs to the recipe `rrs-p1-regression`, and the steps `p1-footprint`, `p1-complete`, and `p1-production-scan` belong to `rrs-p1-checks`.
The letters give the category: (T) thoroughly tested, (P) every invariant extracted as a property-based test, and (C) implementation complete.
The step `p1-regression-ordinary` binds the category (P) through the property `prop_ordinary_play_replay_equal`.

A footprint check names its policy and mode.
Mode `byte_identical` needs a match in the key `unchanged`, and mode `approved_edits` also accepts an entry of the key `edits`.
At the milestone `p1`, the policy of every footprint check is `p1`.
The prefix `rs/` stands for `rspace++/src/rspace/`, and `ri/` stands for `rholang/src/rust/interpreter/`.

*Table 17. The 22 items of the consensus-preservation checklist and their checks at the milestone p1.*

| # | Item | Checks: test or checker, step, category | Assertion |
|---|---|---|---|
| 1 | COMM produce sort | `reified_ordinary_regression_r1_comm_produce_sort` (`reified_ordinary_regression.rs:1081`), `p1-regression-ordinary` (T)<br>`reified_ordinary_regression_r5_event_struct_and_proto_bytes` (`reified_ordinary_regression.rs:1561`), `p1-regression-ordinary` (T)<br>`prop_space_txn_play_trace_equals_host` (`reified_space_store_handle_props.rs:2389`), `p1-regression-substrate-props` (T)<br>Footprint `byte_identical`: `rs/trace/**`, `rs/rspace/ops_produce.rs`, `rs/rspace/ops_consume.rs`, `rs/replay_rspace/**`, `p1-footprint` (C)<br>Footprint `approved_edits`: `rs/replay_rspace.rs`, `p1-footprint` (C) | Join COMMs list their produces in the order of `COMM::new`, as at HEAD, and the substrate logs the same event bytes as the host. |
| 2 | No retention of peeked or matched-persistent data | `reified_ordinary_regression_r2_peek_removes_and_reproduces` (`reified_ordinary_regression.rs:1228`), `p1-regression-ordinary` (T)<br>`reified_ordinary_regression_r8_state_dump` (`reified_ordinary_regression.rs:1981`), `p1-regression-ordinary` (T)<br>`producing_then_consuming_on_same_channel_with_peek_should_return_continuation_and_data_and_remove_peeked_data` (`storage_actions_test.rs:323`), `p1-regression-head-storage` (T)<br>`consuming_then_producing_on_same_channel_with_peek_should_return_continuation_and_data_and_remove_peeked_data` (`storage_actions_test.rs:370`), `p1-regression-head-storage` (T)<br>`consuming_then_producing_on_same_channel_with_persistent_flag_should_return_continuation_and_data_and_not_insert_persistent_data` (`storage_actions_test.rs:414`), `p1-regression-head-storage` (T)<br>`peek_removal_does_not_refund_or_recharge_an_introduction` (`comm_observer_tests.rs:470`), `p1-regression-head-observer` (T)<br>`creating_a_comm_event_with_peek_consume_first_should_replay_correctly` (`replay_rspace_tests.rs:472`), `p1-regression-head-replay` (T)<br>`creating_a_comm_event_with_peek_produce_first_should_replay_correctly` (`replay_rspace_tests.rs:545`), `p1-regression-head-replay` (T)<br>`creating_comm_events_on_many_channels_with_peek_should_replay_correctly` (`replay_rspace_tests.rs:607`), `p1-regression-head-replay` (T)<br>Footprint `byte_identical`: `rspace++/tests/storage_actions_test.rs`, `rspace++/tests/replay_rspace_tests.rs`, `rspace++/tests/comm_observer_tests.rs`, `rspace++/tests/comm_observer_tests/**`, `rs/rspace/ops_consume.rs`, `rs/rspace/ops_produce.rs`, `p1-footprint` (C)<br>Footprint `approved_edits`: `ri/reduce.rs`, `p1-footprint` (C) | A peek removes the datum and the reducer produces it again with the same identity, and the six HEAD peek tests of decision 19 pass unchanged. |
| 3 | Greedy joins | `reified_ordinary_regression_r3_greedy_join_first_match` (`reified_ordinary_regression.rs:1424`), `p1-regression-ordinary` (T)<br>`joined_consume_with_same_channel_given_twice_followed_by_produce_should_not_error` (`storage_actions_test.rs:914`), `p1-regression-head-storage` (T)<br>Production scan, key `production_scan.patterns`: `datum_index_key`, `search_data_candidates`, `p1-production-scan` (C)<br>Phase scan of `p1`, key `forbidden_anywhere`: the same tokens, `p1-complete` (C)<br>Footprint `byte_identical`: `rs/match.rs`, `rs/space_matcher.rs`, `ri/matcher/**`, `p1-footprint` (C) | The greedy matcher leaves a join unmatched where a backtracking search would match it, and no arity index or backtracking search exists. |
| 4 | No production EventCoverage | Footprint `byte_identical`: `casper/src/rust/merging/block_index.rs`, `casper/src/rust/merging/mod.rs`, key `no_new_files`: `casper/src/**`, `p1-footprint` (C)<br>Production scan: `event_coverage`, `p1-production-scan` (C)<br>Phase scan of `p1`: `event_coverage`, `p1-complete` (C) | Both merging files equal HEAD, `casper/src` gains no file, and no source holds the token. |
| 5 | Leaf, serializer, and event-struct formats | `reified_ordinary_regression_r4_leaf_bytes` (`reified_ordinary_regression.rs:1477`), `p1-regression-ordinary` (T)<br>`reified_ordinary_regression_r5_event_struct_and_proto_bytes` (`reified_ordinary_regression.rs:1561`), `p1-regression-ordinary` (T)<br>`legacy_golden_corpus_nonempty_root_and_event` (`legacy_golden_corpus.rs:75`), `p1-regression-legacy-golden` (T)<br>Footprint `byte_identical`: `rs/internal.rs`, `rs/trace/**`, `rs/serializers/**`, `rs/history/**`, key `no_new_files`: `rs/trace/**`, `rs/serializers/**`, `rs/history/**`, `p1-footprint` (C) | Leaf bytes and event bytes equal the corpora, and a new field of a host struct fails to compile in R5. |
| 6 | Derived bincode and candidate ranking | `reified_ordinary_regression_r5_event_struct_and_proto_bytes` (`reified_ordinary_regression.rs:1561`), `p1-regression-ordinary` (T)<br>`deterministic_candidates_hash_unchanged` (`reified_ordinary_regression.rs:2269`), `p1-regression-ordinary` (T)<br>Footprint `byte_identical`: `rs/internal.rs`, `rs/trace/**`, `rs/space_matcher.rs`, `rs/rspace/ops_produce.rs`, `rs/rspace/ops_consume.rs`, `models/src/main/protobuf/**`, `models/build.rs`, `models/src/rust/serde_helpers.rs`, `p1-footprint` (C) | Consumes and produces take candidates in ascending order of the hash of their bincode, and the hashes equal the corpus. |
| 7 | Produce and Consume identities | `reified_ordinary_regression_r5_event_struct_and_proto_bytes` (`reified_ordinary_regression.rs:1561`), `p1-regression-ordinary` (T)<br>`legacy_golden_corpus_event_identities` (`legacy_golden_corpus.rs:42`), `p1-regression-legacy-golden` (T)<br>`legacy_golden_corpus_channel_hashes` (`legacy_golden_corpus.rs:28`), `p1-regression-legacy-golden` (T)<br>Footprint `byte_identical`: `rs/trace/**`, `rs/hashing/**`, `models/src/main/protobuf/**`, `models/build.rs`, `models/src/rust/serde_helpers.rs`, `p1-footprint` (C) | Event identities and channel hashes equal the corpora. |
| 8 | Event proto bytes | `reified_ordinary_regression_r5_event_struct_and_proto_bytes` (`reified_ordinary_regression.rs:1561`), `p1-regression-ordinary` (T)<br>Production scan: `write_exclusive`, `writeExclusive`, `p1-production-scan` (C)<br>Footprint `byte_identical`: `models/src/main/protobuf/**`, `models/build.rs`, `casper/src/**`, key `no_new_files`: `models/src/main/protobuf/**`, `p1-footprint` (C) | The prost bytes of the events equal the corpus, and no protobuf or Rust source holds the field of M-1. |
| 9 | Merger unchanged | Footprint `byte_identical`: `rs/merger/**`, `casper/src/rust/merging/**`, key `no_new_files`: `rs/merger/**`, `casper/src/**`, `p1-footprint` (C) | Both merger trees equal HEAD byte for byte and gain no file. |
| 10 | Host replay acceptance | `reified_ordinary_regression_r9_play_replay_equality` (`reified_ordinary_regression.rs:2070`), `p1-regression-ordinary` (T)<br>`prop_ordinary_play_replay_equal` (`reified_ordinary_regression.rs:3262`), `p1-regression-ordinary` (T, P)<br>`ordinary_play_replay_negative_control_detects_mutants` (`reified_ordinary_regression.rs:3273`), `p1-regression-ordinary` (T)<br>`ordinary_program_generator_covers_every_construct` (`reified_ordinary_regression.rs:3461`), `p1-regression-ordinary` (T)<br>Footprint `approved_edits`: `rs/replay_rspace.rs`, `p1-footprint` (C)<br>Footprint `byte_identical`: `rs/replay_rspace/**`, `casper/src/rust/rholang/replay_runtime.rs`, `p1-footprint` (C) | Replay of a fixed program and of 64 generated programs reaches the play roots with the play costs, and the property rejects both replay mutants. |
| 11 | Cost and host-work accounting | `reified_ordinary_regression_r6_cost_vectors` (`reified_ordinary_regression.rs:1890`), `p1-regression-ordinary` (T)<br>`reified_ordinary_regression_r9_play_replay_equality` (`reified_ordinary_regression.rs:2070`), `p1-regression-ordinary` (T)<br>`prop_ordinary_play_replay_equal` (`reified_ordinary_regression.rs:3262`), `p1-regression-ordinary` (T, P)<br>Footprint `byte_identical`: `ri/accounting/**`, `ri/host_work.rs`, `models/src/rust/host_work.rs`, `rs/rspace/accounting.rs`, key `no_new_files`: `ri/accounting/**`, `p1-footprint` (C) | The cost and host-work vectors equal the corpus, and play and replay charge the same cost. |
| 12 | Hot-store dedup identity | `space_store_handle_parity_continuation_dedup` (`reified_space_store_handle.rs:462`), `p1-regression-substrate-integration` (T)<br>Footprint `byte_identical`: `rs/hot_store.rs`, `rs/hot_store/**`, `rs/internal.rs`, `models/src/main/protobuf/**`, `models/build.rs`, key `no_new_files`: `rs/hot_store/**`, `p1-footprint` (C) | The host keeps one of two identical continuations, and the code of that identity equals HEAD. |
| 13 | Checkpoint root | `reified_ordinary_regression_r4_leaf_bytes` (`reified_ordinary_regression.rs:1477`), `p1-regression-ordinary` (T)<br>`reified_ordinary_regression_r9_play_replay_equality` (`reified_ordinary_regression.rs:2070`), `p1-regression-ordinary` (T)<br>`empty_state_hash_should_be_the_same_as_hard_coded_cached_value` (`casper/tests/util/rholang/runtime_spec.rs:18`), `p1-regression-casper-fixture` (T)<br>`state_hash_after_fixed_rholang_term_execution_should_be_hash_fixed_without_hard_fork` (`casper/tests/util/rholang/runtime_spec.rs:43`), `p1-regression-casper-fixture` (T)<br>`legacy_golden_corpus_nonempty_root_and_event` (`legacy_golden_corpus.rs:75`), `p1-regression-legacy-golden` (T)<br>`legacy_golden_corpus_empty_radix_root` (`legacy_golden_corpus.rs:59`), `p1-regression-legacy-golden` (T)<br>Footprint `byte_identical`: `rs/history/**`, `rs/hashing/**`, `rs/hot_store.rs`, `rs/hot_store_action.rs`, `rs/hot_store_trie_action.rs`, `rs/rspace/ispace_impl.rs`, `casper/tests/util/rholang/runtime_spec.rs`, `casper/src/rust/util/rholang/runtime_manager.rs`, `p1-footprint` (C) | The empty-state root, the fixed-term root, and the checkpoint roots equal their HEAD constants and corpora. |
| 14 | Scheduler footprints | Footprint `approved_edits`: `ri/deterministic_reduction.rs`, `p1-footprint` (C)<br>Footprint `byte_identical`: `models/src/main/protobuf/**`, `models/build.rs`, `p1-footprint` (C) | The scheduler code and the prost encoding of channel keys hold only approved edits, and P1 approves none. |
| 15 | System byte names | Footprint `byte_identical`: `ri/system_processes.rs`, `p1-footprint` (C) | The byte names and the fixed channels of the system processes equal HEAD. |
| 16 | No schema shift | `reified_ordinary_regression_r7_term_prost_and_bincode_bytes` (`reified_ordinary_regression.rs:1917`), `p1-regression-ordinary` (T)<br>`ordinary_source_normalization_unchanged` (`reified_source_normalization.rs:284`), `p1-regression-source` (T)<br>`empty_state_hash_should_be_the_same_as_hard_coded_cached_value` (`casper/tests/util/rholang/runtime_spec.rs:18`), `p1-regression-casper-fixture` (T)<br>`state_hash_after_fixed_rholang_term_execution_should_be_hash_fixed_without_hard_fork` (`casper/tests/util/rholang/runtime_spec.rs:43`), `p1-regression-casper-fixture` (T)<br>Footprint `byte_identical`: `*.proto`, `comm/src/main/protobuf/**`, `comm/src/scalapb/**`, `models/build.rs`, `models/src/rust/serde_helpers.rs`, `casper/tests/util/rholang/runtime_spec.rs`, `casper/src/rust/util/rholang/runtime_manager.rs`, key `no_new_files`: `models/src/main/protobuf/**`, `comm/src/main/protobuf/**`, `p1-footprint` (C)<br>Scheduled from p2 (owner P2): `protobuf_schema_unchanged_vs_head`, `ordinary_prost_unchanged`, and `ordinary_bincode_unchanged` in `models/tests/reified_encoding.rs`, step `encoding` | Twenty ordinary terms and every repository source keep their bytes, every protobuf file equals HEAD, and both HEAD constants hold. |
| 17 | `ISpace` implementors | `reified_space_txn_host_bounds_hold_for_par_hosts` (`reified_space_txn_host_bounds.rs:40`), `p1-regression-host-bounds` (T)<br>Production scan: `suffix_key`, `p1-production-scan` (C)<br>Footprint `byte_identical`: `rs/rspace_interface.rs`, `rs/replay_rspace_interface.rs`, `rs/tuplespace_interface.rs`, `rs/rspace/ispace_impl.rs`, `p1-footprint` (C)<br>Footprint `approved_edits`: `rs/replay_rspace.rs`, `rs/reporting_rspace.rs`, `ri/deterministic_reduction.rs`, `p1-footprint` (C) | The `rspace++` implementors compile and serve as `Par` hosts, and the traits equal HEAD. |
| 18 | No synthesized multi-channel join | Footprint `byte_identical` until p1: `ri/reduce.rs`, key `no_new_files`: `rholang/src/**`, `p1-footprint` (C)<br>Scheduled from p4 (owner P4): `reified_handle_cross_block_merge_no_conflict` and its control `reified_handle_cross_block_merge_join_handle_conflicts` in `rholang/tests/reified_merge.rs`, step `reified-merge` | P1 adds no file under `rholang/src` and leaves `reduce.rs` unchanged, so no synthesized machinery exists. |
| 19 | Namespaced channels never mergeable | Footprint `byte_identical` until p1: `ri/reduce.rs`, key `no_new_files`: `rholang/src/**`, `p1-footprint` (C)<br>Scheduled from p4 (owner P4): `reified_namespaced_channels_never_mergeable` in `rholang/tests/reified_queue.rs`, step `reified-queue` | P1 has no routing, and the detection of mergeable channels still matches only tagged tuples. |
| 20 | Volatile sweep scope | `prop_sweep_volatile_exact_and_root_neutral` (`reified_space_store_handle_props.rs:2424`), `p1-regression-substrate-props` (T)<br>`space_store_handle_sweep_in_open_transaction_fails_closed` (`reified_space_store_handle.rs:3567`), `p1-regression-substrate-integration` (T)<br>Footprint `byte_identical` until p1: `ri/reduce.rs`, `p1-footprint` (C)<br>Scheduled from p7 (owner P7): `reified_volatile_sweep_skips_ordinary_channels` in `rholang/tests/reified_volatile_sweep.rs`, step `reified-volatile-sweep` | The sweep removes exactly the listed volatile channels, and no production code calls it in P1. |
| 21 | Protected runtime files | Footprint `byte_identical`: `ri/contract_call.rs`, `ri/dispatch.rs`, `rs/rspace/ispace_impl.rs`, `ri/accounting/**`, key `no_new_files`: `ri/accounting/**`, `p1-footprint` (C) | The four protected paths equal HEAD byte for byte. |
| 22 | Parser conservative extension | `ordinary_source_normalization_unchanged` (`reified_source_normalization.rs:284`), `p1-regression-source` (T)<br>`ordinary_float_forms_unchanged` (`reified_source_normalization.rs:507`), `p1-regression-source` (T)<br>`reified_ordinary_regression_r7_term_prost_and_bincode_bytes` (`reified_ordinary_regression.rs:1917`), `p1-regression-ordinary` (T)<br>Footprint `approved_edits`: `ri/compiler/**`, `p1-footprint` (C) | Every source and every float form that HEAD accepts keeps its bytes. |

*Table 18. The 10 steps of the recipe `rrs-p1-regression` and their exact counts.*

| Step | Command arguments | Tests |
|---|---|---|
| `p1-regression-ordinary` | `-p rholang --test reified_ordinary_regression` | 13 |
| `p1-regression-source` | `-p rholang --test reified_source_normalization` | 2 |
| `p1-regression-casper-fixture` | `-p casper --test mod -- util::rholang::runtime_spec::` | 2 |
| `p1-regression-head-storage` | `-p rspace_plus_plus --test storage_actions_test` | 41 |
| `p1-regression-head-replay` | `-p rspace_plus_plus --test replay_rspace_tests` | 29 |
| `p1-regression-head-observer` | `-p rspace_plus_plus --test comm_observer_tests` | 27 |
| `p1-regression-legacy-golden` | `-p rspace_plus_plus --test legacy_golden_corpus` | 4 |
| `p1-regression-substrate-props` | `-p rspace_plus_plus --test reified_space_store_handle_props` | 24 |
| `p1-regression-substrate-integration` | `-p rspace_plus_plus --test reified_space_store_handle` | 82 |
| `p1-regression-host-bounds` | `-p rholang --test reified_space_txn_host_bounds` | 1 |

The HEAD suites `storage_actions_test`, `replay_rspace_tests`, and `comm_observer_tests` hold the six HEAD peek tests of [decision 19](decision-register.md#decision-19).
The casper module `runtime_spec` holds the two HEAD empty-state hash tests, whose constants must not change.
The recipes `rrs-p1-head-rspace` and `rrs-p1-rholang-matcher` run the other HEAD suites of the realignment leaves.
They are the HEAD history tests (`--test mod -- history::`, 24 tests, and `--lib -- rspace::history::`, 39 tests) and the 52 HEAD matcher tests of `rholang/tests/matcher/match_test.rs` (`--test mod -- matcher::match_test::`).

### 8.2 Corpora and provenance

A [regression corpus](glossary.md#regression-corpus) holds the expected outputs of a capture run.
A capture run writes into a capture directory, and the captures of ordinary execution and of float forms require that directory under `target/` (`reified_ordinary_regression.rs:318-340`, `reified_source_normalization.rs:375-429`).
A separate merge or copy then puts the result into `test-vectors/`, so a normal test run never writes to the source tree.

*Table 19. The corpora of P1, with their provenance and capture commands.*

| Corpus | Content | Provenance | Capture command |
|---|---|---|---|
| `test-vectors/ordinary-execution-v1.json` | The sections `r1_comm_produce_sort` to `r9_play_replay_equality` and `deterministic_candidates`. | Captured on 2026-10-05 in a clean export of `4d67b5ccc`, with the harness file copied in ([8.8](#88-the-merge-of-featurecost-accounted-rho)). The earlier captures were at `6f7a80da3` and at `11c39d77d`. Sections R1, R2, R4, R5, R7, R8 and R9 are byte-identical to them. Toolchain: `rustc 1.95.0-nightly (6efa357bf 2026-02-08)`. | The corpus records the command, which starts with `git archive 4d67b5ccc` and runs `cargo test --offline -p rholang --test reified_ordinary_regression` in the export with `RRS_CAPTURE_ORDINARY_OUT` under its `target`. Then `python3 scripts/verify-reified/merge-ordinary-capture.py --capture-dir <fragments> --output test-vectors/ordinary-execution-v1.json --capture-command '<the command>' --revision 4d67b5ccc42c4e510162d075f7ed423bf8175563 --code-identical-to '<the text of the corpus>'` joins the fragments `r1` to `r9` and `dc`. Its option `--check` compares the result with the corpus. |
| `test-vectors/ordinary-source-normalization-v1.json` | One entry for each of the 192 `.rho` and `.rhox` files of `4d67b5ccc`: 127 `ok`, 63 `error`, and 2 `panic`. | Captured on 2026-10-05 in a clean export of `4d67b5ccc`, with the parser of HEAD (`02cef80`). The placeholder `resourcePolicyGetter` takes the value of the default genesis, the empty text. The earlier capture was at `6f7a80da3`. Rule: each `ok` entry normalizes to identical prost and stable hashes. | In the export: `RRS_CAPTURE_SOURCES_OUT=<absolute directory under target> RRS_CAPTURE_REVISION=4d67b5ccc42c4e510162d075f7ed423bf8175563 cargo test --offline -p rholang --test reified_source_normalization -- --exact ordinary_source_normalization_unchanged`. The corpus records no command, and this one comes from the capture code (`reified_source_normalization.rs:176-223`, `reified_source_normalization.rs:284-290`). |
| `test-vectors/ordinary-float-forms-v1.json` | 15 float forms: 13 `ok`, and `sync_dot` and `sync_dot_new` rejected. | Captured on 2026-10-05 in a clean export of `4d67b5ccc`, with the parser revision `02cef80d32214ecdac3ee7d3b91c3786a34174d5` and no active parser patch. The earlier capture was at `11c39d77d`, and only the form `suffixed` changed. Rule: an accepted form keeps its hashes, and a rejected form can become accepted. | The corpus records the command: in the export, `RRS_CAPTURE_FLOATS_OUT` and `RRS_CAPTURE_REVISION=4d67b5ccc42c4e510162d075f7ed423bf8175563` with `cargo test --offline -p rholang --test reified_source_normalization -- --exact ordinary_float_forms_unchanged`. The capture also needs `RRS_CAPTURE_COMMAND`, which holds this command. |
| `test-vectors/legacy-golden-corpus-v1.json` | Channel hashes, event identities and bytes, the protobuf bytes of `Par`, and state roots. | Captured from three revisions: the legacy branch `rust/dev` at `95be4feb`, the legacy Reified branch at `ff479042`, and the current code at `e748426`. The file is byte-identical to its backup copy. | The corpus records seven commands of the earlier campaign, for example `python3 scripts/capture-legacy-golden-corpus.py`. Those capture scripts exist only in the backup, so the corpus is frozen evidence that this repository cannot capture again. |
| `test-vectors/head-file-digests-4d67b5ccc.json` | The SHA-256 digest of each of the 4,470 files that `4d67b5ccc` tracks (`roots: ["."]`). | Pinned on the host from the git objects of the base revision, on 2026-10-05. The file `test-vectors/head-file-digests-6f7a80da3.json` keeps the 4,300 digests of the base before the merge, which the milestone policies `p1`, `p1-realign` and `p2` used. | `python3 scripts/verify-reified/check-footprint.py --pin`. It needs git and the objects of the base revision, so it runs only on the host. |

### 8.3 The ordinary play/replay property

The property `prop_ordinary_play_replay_equal` (`reified_ordinary_regression.rs:3258-3270`) generates ordinary Rholang programs of one or two deploys.
It plays each program and replays it under the replay rules of Casper, and it requires that replay accepts what play did.
It is the property of the checklist entry O1, and its witness tests are R9 and the negative control of section [8.4](#84-negative-controls-of-the-regression-suite).

#### 8.3.1 Protocols D and B

The function `play_replay_equal` (`reified_ordinary_regression.rs:3248-3256`) runs two protocols on fresh runtimes, and both must pass:

- **Protocol D** (`reified_ordinary_regression.rs:3102-3167`). Play creates a checkpoint after each deploy. Replay resets to the start root once. For each deploy, replay rigs the log of the deploy, evaluates it, passes `check_replay_data`, and creates a checkpoint. Then it discards its event log, as Casper does after each deploy.
- **Protocol B** (`reified_ordinary_regression.rs:3173-3245`), the block protocol of Casper. Play takes the event log after each deploy (`casper/src/rust/rholang/runtime.rs:939`) and creates one checkpoint for the block. Replay resets once, rigs and evaluates each deploy, and discards its event log after each deploy (`casper/src/rust/rholang/replay_runtime.rs:192-193`, `casper/src/rust/rholang/replay_runtime.rs:599`). Then it creates one checkpoint for the block.

The pseudocode below gives both protocols in the order of the code.

```text
play_replay_equal(sources, label, mutation):
    protocol D:
        play, replay := fresh runtimes;  start := play.root
        for each deploy i:  play i with phlo 10,000,000;  require no error
                            (root_i, log_i) := play.create_checkpoint;  cost_i := play cost
        replay.reset(start)
        for each deploy i:  replay.rig(log_i);  replay i (the mutation changes the seed or the source)
                            require replay errors = play errors;  require check_replay_data passes
                            require replay.create_checkpoint.root = root_i;  replay.take_event_log
                            require replay cost = cost_i
    protocol B:
        play, replay := fresh runtimes;  start := play.root
        for each deploy i:  play i;  require no error;  log_i := play.take_event_log;  cost_i := play cost
        block := play.create_checkpoint;  require block.log is empty
        replay.reset(start)
        for each deploy i:  replay.rig(log_i);  replay i;  require replay errors = play errors
                            replay.take_event_log;  require check_replay_data passes;  require replay cost = cost_i
        require replay.create_checkpoint.root = block.root
```

#### 8.3.2 Finding: the replay checkpoint keeps the produce counter

The property found a latent asymmetry of HEAD that Casper never reaches.
The tracker records it as the finding `rrs-finding-head-replay-checkpoint-produce-counter`.
The campaign records it and does not change HEAD ([decision 12](decision-register.md#decision-12)).

- `RSpace::create_checkpoint` takes the event log and resets the [produce counter](glossary.md#produce-counter) (`rspace++/src/rspace/rspace/ispace_impl.rs:82-83`).
- `ReplayRSpace::create_checkpoint` checks the replay data, writes the history, and makes a new hot store, but it keeps the event log and the produce counter (`rspace++/src/rspace/replay_rspace.rs:166-181`).
- `ReplayRSpace::take_event_log` clears both (`rspace++/src/rspace/replay_rspace.rs:396-400`), and so does `ReplayRSpace::reset` (`rspace++/src/rspace/replay_rspace.rs:183-188`).

The first run of the property used protocol D without the discard of the replay event log.
In that run, deploy 0 stored the datum `@"rrs-prop-c01"!(0)`, and deploy 1 received it.
After its checkpoint, play counts the produce again from zero, but replay keeps the count of deploy 0.
So the [times-repeated](glossary.md#times-repeated) value of the COMM in replay differs from the recorded value, and replay does not pair the recorded COMM.
`check_replay_data` then failed with `UnusedCommEvent` in deploy 1 (`target/verification/reified-rspaces/p1-closeout/rt/ro-property-1.log`).

Casper never checkpoints a replay between two deploys without a discard of the event log.
So protocol D now discards it, as the comment at `reified_ordinary_regression.rs:3150` records.

#### 8.3.3 Generator bounds

*Table 20. The bounds of the generator of ordinary programs.*

| Bound | Value | Code |
|---|---|---|
| Deploys for each case | 1 or 2 | `ordinary_case` |
| Top-level terms for each deploy | 1 to 4, and the seed pair in deploy 0 | `ordinary_case`, `resolve_case` |
| Public channels | 3 levels (0, 1, 2), 2 channels on each level, `@"rrs-prop-c{l}{i}"` | `PUBLIC_LEVELS`, `PUBLIC_PER_LEVEL` |
| Sink channels | 2 channels on level 3, `@"rrs-prop-out{i}"`, never received | `SINK_COUNT`, `SINK_LEVEL` |
| Fresh channels | The name of the innermost `new`, on the level of its context | `gen_new`, `chan_level` |
| Join width | 1 to 3 distinct channels | `MAX_JOIN_WIDTH` |
| Aliased join | 2 binds on one public channel, at the top level only, weight 1 against 8 | `gen_aliased`, `gen_top_term` |
| Arity of sends and binds | 1 (weight 4) or 2 (weight 1) | `gen_arity` |
| Terms in a body, terms in a `new` | 1 or 2 | `gen_body`, `gen_new` |
| Nesting depth | At most 3, and a body on level 3 holds sink sends only | `MAX_DEPTH`, `gen_body` |
| Phlo for each deploy | 10,000,000, in play and in replay | `PROPERTY_PHLO` |

The generator excludes repeated channels in the source text, which HEAD rejects at normalization, and mixed arrows in one join.
It also excludes `contract`, `match`, `if`, guards, signed binds, URNs, and arithmetic on variables.
Each exclusion keeps every case free of errors and keeps the termination proof simple.

#### 8.3.4 Rules S1 to S3

The generator and the renderer enforce three rules by construction:

- **S1 (strata).** A receive body uses only channels whose level is above the highest level of the channels of the receive. The generator draws the body with `gen_body(top + 1, depth + 1)` (`reified_ordinary_regression.rs:2485-2506`).
- **S2 (no persistent pair).** No channel carries both `!!` and `<=` in one case, across deploys too. The renderer collects each channel that carries a persistent receive, and it prints `!!` only on other channels (`reified_ordinary_regression.rs:2843-2893`).
- **S3 (static channels).** A received variable never appears in channel position. The only exception is the aliased join, whose two variables carry one fixed public channel name. The resolver maps each channel to a static channel or to a `new` binder (`reified_ordinary_regression.rs:2807-2839`).

#### 8.3.5 Termination: Lemmas A and B

The proof below shows that every generated case terminates, so the property never depends on the phlo ceiling.
It uses these definitions:

- A **linear item** is a non-persistent datum or a non-persistent continuation, stored or in flight. A send with `!` makes a linear datum, and a receive with `<-` or `<<-` makes a linear continuation.
- The **level** of a public channel `@"rrs-prop-c{l}{i}"` is $`l`$. A sink has level 3, and a fresh name has the level of its context. The private channel `@"rrs-prop-alias{n}"` of an aliased join has level $`-1`$.
- The level of a continuation is the highest level of its channels, and the level of a COMM is the level of its continuation.

**Lemma B.** The body of a continuation of level $`m`$ issues operations only on channels of level greater than $`m`$.

*Proof.* Let a receive bind channels of the levels $`\ell_1, \dots, \ell_w`$, and let $`\mathit{top} = \max_j \ell_j`$.
The generator draws its body with `gen_body(top + 1, depth + 1)`.

`gen_body(lo, d)` draws only sink sends when $`\mathit{lo} \ge 3`$ or $`d \ge 3`$, and the sinks have level $`3 > \mathit{top}`$.
Otherwise it draws terms of `gen_term(lo, d, false)`.
These terms use public channels of the levels $`\mathit{lo}`$ to 2, the sinks, and the names of their own `new` terms.
Such a name has the level $`\mathit{lo}`$ of its context, and the parameter `false` keeps every enclosing fresh name out of the body.

By induction on the depth of the term, every channel of the body has a level of at least $`\mathit{lo} = \mathit{top} + 1`$.
By S3, no received variable of the body becomes a channel, so no other channel can occur.
The two special receives satisfy the lemma too.
The alias receive has level $`-1`$, and its body is the inner join on a public channel of level $`\ell \ge 0`$.
The inner join has the body `gen_body(level + 1, 1)`, with levels above $`\ell`$, and the seed receive on level 0 has a sink send as its body. ∎

**Lemma A.** Every COMM removes at least one linear item, and no re-produce or re-consume restores that item.

*Proof.* Let $`k`$ be the continuation of the COMM. There are two cases.

- *Case 1: $`k`$ is non-persistent* (`<-` or `<<-`). Then $`k`$ is a linear item. When $`k`$ is stored, the produce that fires it removes it, in play (`rspace++/src/rspace/rspace/ops_produce.rs:202-205`) and in replay (`rspace++/src/rspace/replay_rspace.rs:1414-1420`). When $`k`$ is the arriving consume, the host fires it and does not store it, in play (`rspace++/src/rspace/rspace/ops_consume.rs:124-143`) and in replay (`rspace++/src/rspace/replay_rspace.rs:1059-1062`).

  The reducer consumes again only for a persistent continuation (`rholang/src/rust/interpreter/reduce.rs:843-908`). Its re-produce paths issue only sends (`rholang/src/rust/interpreter/reduce.rs:717-783` and `rholang/src/rust/interpreter/reduce.rs:994-997`). So no step restores $`k`$.
- *Case 2: $`k`$ is persistent* (`<=`). By S2, no channel of $`k`$ carries `!!`, so each matched datum is non-persistent, which makes it a linear item. The host removes each non-persistent matched datum. On the produce side, `remove_matched_datum_and_join` removes the stored data (`rspace++/src/rspace/rspace/ops_produce.rs:228-248`, and `rspace++/src/rspace/replay_rspace.rs:1798-1818` in replay), and the arriving datum is never stored (`rspace++/src/rspace/rspace/ops_produce.rs:163-210`). On the consume side, `store_persistent_data` removes them (`rspace++/src/rspace/rspace/ops_consume.rs:192-202`, and `rspace++/src/rspace/replay_rspace.rs:1686-1696` in replay).

  The persistent re-produce (`rholang/src/rust/interpreter/reduce.rs:717-783`) needs a persistent arriving send, which S2 excludes. The peek re-produce (`rholang/src/rust/interpreter/reduce.rs:994-997`) needs a peek continuation, and $`k`$ has the single arrow `<=`. So nothing restores the matched data.

A join has at least one channel, so in both cases the COMM removes at least one linear item that nothing restores. ∎

**Theorem (termination).** Each deploy of a generated case performs finitely many COMMs, so its evaluation ends.

*Proof.* Let $`C_m`$ be the set of COMMs of level $`m`$, for $`m \in \{-1, 0, 1, 2\}`$.
No receive binds a sink, so no COMM has level 3.
We show by induction on $`m`$ that $`C_{-1}, \dots, C_m`$ are finite.
Assume that every $`C_{m'}`$ with $`m' < m`$ is finite, which holds trivially for $`m = -1`$.

1. $`K_m`$ is the set of continuations of level $`m`$ that are not re-consumes. By Lemma B, each of them comes from a static term or from the body of a COMM of a level below $`m`$. The static terms are finite, each body is finite, and the COMMs below $`m`$ are finite, so $`K_m`$ is finite.
2. The reducer consumes again only for a persistent continuation, so each non-persistent continuation of level $`m`$ is in $`K_m`$. By Case 1 of Lemma A, it fires at most once. So at most $`|K_m|`$ COMMs of $`C_m`$ fire a non-persistent continuation.
3. $`D_m`$ is the number of non-persistent data, stored or in flight, on the channels of the levels up to $`m`$. By Lemma B, only static terms and bodies of COMMs below $`m`$ increase $`D_m`$ with new sends, and their number is finite. A peek re-produce sends again exactly the non-persistent data that its own COMM removed, so a peek COMM never increases $`D_m`$. By Case 2 of Lemma A, each COMM of $`C_m`$ with a persistent continuation decreases $`D_m`$ by at least one, and $`D_m \ge 0`$. So these COMMs are at most the initial value of $`D_m`$ plus the finite number of new sends.

Steps 2 and 3 make $`C_m`$ finite, which completes the induction.
Each COMM runs one finite body and issues at most four re-produces or re-consumes, and an operation that does not fire stores one item.
So each deploy performs finitely many operations. ∎

The regression design estimates the extreme case of these bounds at about 800 COMMs and 2,000 terms, which costs about 2.6 million phlo.
The estimate uses the costs of R6: 697 phlo for a send and 4,421 phlo for a join program.
A typical case costs 5,000 to 50,000 phlo.

#### 8.3.6 Phlo ceiling and configuration

Each deploy gets `PROPERTY_PHLO` = 10,000,000 phlo, in play and in replay, which is about four times the extreme case.
The same limit applies to both, so it never changes the result of the property.
If a defect of the generator makes a program loop, the deploy ends after about 7,000 COMMs with `OutOfPhlogistonsError`.
The vector `out_of_phlo` of R6 shows that HEAD reports this error deterministically, and the conjunct on errors then fails.
R9 uses `EVALUATION_PHLO` = 100,000,000, so a loop under that limit would take minutes.

The configuration is `property_config` (`reified_ordinary_regression.rs:2304-2311`): 64 cases, the fixed seed `0x0E0_5EED_2026_1004`, and no failure persistence.
Each case builds its own multi-thread runtime with 2 workers, as R9 does.
The file `.cargo/config.toml` sets `RUST_MIN_STACK` to 8 MiB for the test threads.
In a debug build on the host, the property took 8.7 s, and the whole target of 13 tests took 10.6 s.

#### 8.3.7 The four conjuncts

For each deploy, in both protocols, the property requires four conjuncts:

1. Play reports no error, and replay reports the same errors as play.
2. `check_replay_data` passes, so replay used every recorded COMM.
3. The replay root equals the play root: after each deploy in protocol D, and for the block in protocol B.
4. The replay cost equals the play cost.

The property also requires that each case fires at least one COMM, so no case passes without content.
The seed pair of deploy 0 guarantees this COMM.

### 8.4 Negative controls of the regression suite

- **The named control.** `ordinary_play_replay_negative_control_detects_mutants` (`reified_ordinary_regression.rs:3273`) replays the programs of R9 and R8 as two deploys. The unmutated replay must pass, which shows that each failure below comes from its mutation alone. The mutants `ReplaySeed` (another seed in replay) and `ExtraSend` (one more send in replay) must fail. In the last run, each mutant failed `check_replay_data` in deploy 0 of protocol D with `UnusedCommEvent` and 16 leftover entries (`target/verification/reified-rspaces/p1-closeout/rt/negative-control-protocols-nocapture.log`).
- **The property mutant.** A one-off run set `ReplaySeed` in the property itself. The property failed on its first case and shrank it to the one-deploy program `@"rrs-prop-c00"!(0) | for (@x0 <- @"rrs-prop-c00") { @"rrs-prop-out0"!(x0) } | @"rrs-prop-c00"!(0)`. Its replay failed in protocol D with `UnusedCommEvent` (`target/verification/reified-rspaces/p1-closeout/regression-mutant-replay-seed.log`).
- **The cost mutant.** A one-off run changed a cost constant in a scratch copy of the worktree (`target/verification/reified-rspaces/p1-capture/mutant-65-r6.log`). R6 then failed on seven host-work vectors, for example `VerificationBytes` of the join program, 9,623,240 for the expected 9,623,236. The plan names this kind of drift, a constant changed from 65 to 66, as a reason for R6.
- **The checker unit tests.** The module `test_checkers` holds the negative controls of the regression checkers. They test the consensus-checklist checker (20 tests), the production scan (7), and the footprint checker (14). They also test the completeness checker (11) and the ORD phase of the invariant map (8).

### 8.5 Milestone rule of the checklist checker

The checker runs with `--milestone pN`, where the milestones are `p1` to `p12`, in this order.
A check is active from its milestone `from` to its milestone `until`.
A check without `from` is active from `p1`, and a check without `until` stays active to `p12`.
The value `@milestone` in a policy or a scope names the milestone of the run.
The checker prints `SCHEDULED` for a check before `from` and `RETIRED` for a check after `until`, and it does not look for their tests.
Each item needs at least one active check, or the checker prints `FAIL: item N has no active check at <milestone>`.

*Table 21. The scheduled checks and their owners.*

| Item | Check | Active from | Owner |
|---|---|---|---|
| 16 | `protobuf_schema_unchanged_vs_head`, `ordinary_prost_unchanged`, `ordinary_bincode_unchanged` | p2 | P2 |
| 18 | `reified_handle_cross_block_merge_no_conflict`, `reified_handle_cross_block_merge_join_handle_conflicts` | p4 | P4 |
| 19 | `reified_namespaced_channels_never_mergeable` | p4 | P4 |
| 20 | `reified_volatile_sweep_skips_ordinary_channels` | p7 | P7 |

The footprint checks of items 18, 19, and 20 are active until `p1` only.
At `p2`, items 18 and 19 therefore have no active check, and the run fails by design (`FAIL: item 18 has no active check at p2`).
**P2 must add active checks for items 18 and 19.**
The run at `p2` also fails until P2 adds its footprint policy `p2` and its completeness phase `p2`.

### 8.6 Grep gates

The production scan is the grep gate of the plan: `python3 scripts/verify-reified/check-complete.py --production-scan`.
It reads the roots `casper/src`, `comm/src`, `models/build.rs`, `models/src`, `node/src`, `rho-pure-eval/src`, `rholang/src`, and `rspace++/src`.
In a directory root, it reads each file with the suffix `.rs` or `.proto`, and it always reads a file root.
A line with a listed token, or a missing root, is a finding, and the last run printed `complete production scan: 783 files, 0 findings`.

*Table 22. The 24 tokens of the production scan and their reasons.*

| Token | Reason |
|---|---|
| `datum_index_key` | Item 3: no arity index. |
| `pattern_index_prefix` | Item 3: no arity index. |
| `select_indexed_candidates` | Item 3: no indexed selection of candidates. |
| `search_data_candidates` | Item 3: no backtracking search. |
| `changed_history_keys` | Item 4: no production trie diff for event coverage. |
| `changed_leaf_keys` | Item 4: no production trie diff for event coverage. |
| `checkpoint_unpublished` | Item 13: the checkpoint publication is unchanged. |
| `decode_checked` | Items 5 and 13: the decoding of the history trie is unchanged. |
| `UnsupportedPriorityMetadata` | Item 8 and I-5: no event message gets a priority field. |
| `write_exclusive` | Item 8 and M-1. |
| `writeExclusive` | Item 8 and M-1. |
| `query_specs` | Item 5 and M-2: host structs get no query field. |
| `querySpecs` | Item 8 and I-5: no event message gets a query field. |
| `TypedQuerySpec` | Item 17 and I-1: the host gets no typed query. |
| `consume_with_query_specs` | Item 17 and I-1: `ISpace` gets no query consume. |
| `suffix_key` | Item 17 and I-1. |
| `SpacePathKey` | Decision 7: Reified channels use the namespace tag `0xE0`, not a path key. |
| `space_channel_codec` | Decision 7: no path-key channel codec. |
| `par_wire_` | Decision 1: stack-safe `Par` codecs belong to the Bag campaign. |
| `ParBag` | Decision 1: `ParBag` belongs to the Bag campaign. |
| `BagReservation` | Decision 1: the Bag work belongs to the Bag campaign. |
| `use_blocks` | Item 16 and decision 18: no K15 protobuf field. |
| `e_free_body` | Item 16 and decision 18: no K15 protobuf field. |
| `event_coverage` | Item 4: no production EventCoverage. |

**The `0xF3` rule.** The key `forbidden_anywhere` of `complete-policies.json` lists `0xF3` and `0xf3`, because the Reified [namespace tag](glossary.md#namespace-tag) is `0xE0` ([decision 7](decision-register.md#decision-7)).
`check-complete.py --phase p1` applies these tokens to the three P1 sources, and it printed `complete phase p1: 91803 checks, 0 findings`.
The production scan does not list `0xF3`, because the MeTTaIL tag band `0xF0` to `0xF9` enters production code with the F1r3lang merge in P12.

### 8.7 Formal model and formal verification

The categories "formally modeled" and "formal verification passes" do not apply to the leaf `rrs-tests-verification-consensus-regression`.
The leaf adds tests, corpora, and checkers, and no behavior that a model could state.
Its claims are about HEAD code that P1 does not change.
The footprint pins that code, and the regression tests compare its outputs with the corpora.

### 8.8 The merge of feature/cost-accounted-rho

On 2026-10-05 the campaign merged `feature/cost-accounted-rho` at `4d67b5ccc` into `feature/reified-rspaces` (user: "yes, merge it carefully").
The merge base is `e748426`.
Since the merge, HEAD names the node code at `4d67b5ccc`, and every baseline of this section compares the campaign tree with that code.

*Table 19a. The baselines that moved to `4d67b5ccc`.*

| Baseline | Before the merge | After the merge | Evidence |
|---|---|---|---|
| Footprint digests | `test-vectors/head-file-digests-6f7a80da3.json` (4,300 files) | `test-vectors/head-file-digests-4d67b5ccc.json` (4,470 files) | `check-footprint.py --policy p4` and `--policy campaign` report 0 violations. So every file outside the approved Reified edits equals `4d67b5ccc`. |
| Protobuf schema | `test-vectors/protobuf-schema-head-6f7a80da3.json` | `test-vectors/protobuf-schema-head-4d67b5ccc.json` | The 18 pinned files equal `4d67b5ccc`. The branch changed `CasperMessage.proto` and `RhoTypes.proto` (`g_float32 = 39`), and the campaign changed none. |
| Ordinary execution | Captured in the campaign tree at `11c39d77d` | Captured in a clean export of `4d67b5ccc` | Sections R1, R2, R4, R5, R7, R8 and R9 are byte-identical to the earlier capture. Section DC, section R3 and section R6 changed, as Table 19b gives. |
| Source normalization | Captured at `6f7a80da3` | Captured in a clean export of `4d67b5ccc` | The same 192 files: 127 `ok`, 63 `error`, and 2 `panic`. Three contracts changed on the branch. |
| Float forms | Captured at `11c39d77d` | Captured in a clean export of `4d67b5ccc` | Only the form `suffixed` (`1.5f32`) changed its hashes, because the branch normalizes it to `g_float32`. |

**The capture method.** The command `git archive 4d67b5ccc` exports the tree into `target/scratch/base-4d67b5ccc`, which is not a worktree.
The two harness files `reified_ordinary_regression.rs` and `reified_source_normalization.rs` are copied into the export, and the capture runs there with `cargo test --offline`.
The export holds no Reified code and no parser `[patch]`, so each capture records the behavior of the branch alone.

The campaign tree then runs the same harness in verify mode, and it reproduces every corpus.
All 13 regression tests and both normalization tests pass.
A capture in the campaign tree itself would compare the hooks with their own output, so it would prove less.

*Table 19b. The changed sections of the ordinary-execution corpus, and their causes on the branch.*

| Section | Cause | Change of the harness |
|---|---|---|
| DC, deterministic candidates | DR-75: the host orders candidates by the hash of their source produce or consume, and it compares digests only between equal source hashes (`rspace++/src/rspace/candidate_order.rs`). | The oracle `canonical_rank` replaces `bincode_rank`. It sorts by (source hash, digest, index). |
| R3, greedy join | The same order. The data `[1, 5]` and `[1, 6]` no longer make the greedy matcher fail. | The witness data are now `[1, 1]` and `[1, 6]`. A search at `4d67b5ccc` gave `[1, 1]` and `[1, 2]` as witnesses, and seven other values as non-witnesses. |
| R6, cost vectors | DR-88 to DR-92 change the host-work reservations of native execution. Native funded execution now refuses the external call of the program `system_process`. | None. The legacy phlo of every program is unchanged. |

**The template value of the source corpus.** At `4d67b5ccc`, `TokenMetadata.rhox` has the placeholder `$$resourcePolicyGetter$$`.
The harness gave every unknown placeholder the value `0`, so the text `0rs!(` did not parse.
The harness now gives it the empty value, which is the value of the default genesis without a resource policy (`standard_deploys.rs`, `token_metadata_with_policy`).

**The citations.** A host file that the branch changed moves the line numbers of the citations into it.
A script maps each cited range from the pre-merge commit `294c95632` to the merged tree with the offsets of `git diff -U0`.
It moved 263 ranges and 54 continuation ranges of formal comments.
It reported 25 ranges whose cited lines changed, and each of those was checked by hand against the claim of its citation.
The script excludes the design ledger, whose bytes are pinned, and the closure policies, which pin the output of another audit.

**The changes of models and tests.** Two of the 25 checks found a changed claim, and one found a defect of the merge:

1. DR-84 changed the split of the scheduler: child $`i`$ of a split at step $`s`$ now appends the one segment $`(s, i + 1)`$. `EventCorrelation.v` models this split in `sched_part`. The lemma `sched_part_nonzero` replaces the parity lemma in the proof of `sched_op_no_extension`, and H8 `sub_order_between` keeps its statement. The property `prop_sub_orders_contiguous_and_disjoint` generates paths of the new shape.
2. The dispatcher now evaluates a fired body with `eval_continuation`, which detaches the evaluation in a reduction. A dispatch then ends the participant that makes it. The handle method `produce` sent its reply after its fires in that participant.
So a deploy that fired a waiter through a handle aborted the process. The reply is now the last branch of the outcome ([design N6](design.md#n6-scheduling)). The test `reducer_space_routing_then_runs_after_the_fires_in_its_own_branch` covers it, and `prop_use_wrapper_restores_ctx` found the defect.
3. DR-75 changed the waiter order of the host, so the test helper `peek_tried_first` of `reified_space_store_handle.rs` now orders waiters by the hash of their source consume.
4. The root entry `inj_with_host_work` of the reducer did not begin an evaluation scope. It now does, as `inj` and `inj_with_observation` do (FR-5').
   No production path reaches it, because the interpreter enters the reducer through `inj_with_observation`.
   The test `reducer_space_routing_inj_with_host_work_begins_an_evaluation_scope` creates a space and then sends to it twice through that entry.
   With the hook line commented out, the test fails in its first run with `ParentOrderReused` (`target/verification/reified-rspaces/merge-car/mutant-scope.log`).

**The results on the merged tree.** The gate run of 2026-10-05 passed all 46 steps (`target/verification/reified-rspaces/merge-car/gates/summary.txt`).
The library run of `rholang` printed `test result: ok. 2157 passed; 0 failed; 3 ignored`.
The Rocq gate printed `reified-rspaces proofs: PASS=45 FAIL=0 SKIP=0`, with 93 closed headlines and 66 negative checks (`target/verification/reified-rspaces/merge-car/proofs-2.log`).
The workspace build of all targets has no warnings.

## 9. Realignment closure checks

The realignment leaves get their evidence from two checks.

**The footprint policy `p1-realign`.** The digest file pins the full tree of the base revision: all 4,300 tracked files (`pinned_roots: ["."]`).
The policy extends `p1` and adds `unchanged: ["**"]`, so each pinned file must equal its digest, except the approved P1 edits.
Each file of the tree that the base revision does not pin must be in the exact list `allowed_new_files`, and each listed file must exist.
The file list comes from `git ls-files --cached --others --exclude-standard`, or from a walk of the tree when `.git` is absent, as in the pgmcp snapshot.
The list has 181 paths: the files that commit `11c39d77d` added and the files of the P1 close-out.

<details>
<summary>The 181 paths of <code>allowed_new_files</code></summary>

```text
docs/rspace/reified-rspaces/README.md
docs/rspace/reified-rspaces/decision-register.md
docs/rspace/reified-rspaces/design.md
docs/rspace/reified-rspaces/diagrams/01-substrate-components.puml
docs/rspace/reified-rspaces/diagrams/01-substrate-components.svg
docs/rspace/reified-rspaces/diagrams/02-namespace-bytes.puml
docs/rspace/reified-rspaces/diagrams/02-namespace-bytes.svg
docs/rspace/reified-rspaces/diagrams/06-space-txn-commit.puml
docs/rspace/reified-rspaces/diagrams/06-space-txn-commit.svg
docs/rspace/reified-rspaces/diagrams/06a-space-txn-lifecycle.puml
docs/rspace/reified-rspaces/diagrams/06a-space-txn-lifecycle.svg
docs/rspace/reified-rspaces/diagrams/07-checkpoint-replay-sweep.puml
docs/rspace/reified-rspaces/diagrams/07-checkpoint-replay-sweep.svg
docs/rspace/reified-rspaces/diagrams/13-verification-pipeline.puml
docs/rspace/reified-rspaces/diagrams/13-verification-pipeline.svg
docs/rspace/reified-rspaces/diagrams/concept-colors.iuml
docs/rspace/reified-rspaces/diagrams/fonts.lock
docs/rspace/reified-rspaces/diagrams/render.sh
docs/rspace/reified-rspaces/glossary.md
docs/rspace/reified-rspaces/verification.md
formal/reified-rspaces-invariant-map.json
formal/reified-rspaces-model-map.json
formal/rocq/reified_rspaces/Makefile
formal/rocq/reified_rspaces/_CoqProject
formal/rocq/reified_rspaces/theories/Encoding.v
formal/rocq/reified_rspaces/theories/EventCorrelation.v
formal/rocq/reified_rspaces/theories/VolatileSweep.v
formal/tlaplus/reified_rspaces/CheckpointReplay.tla
formal/tlaplus/reified_rspaces/DoublePairUnsafe.cfg
formal/tlaplus/reified_rspaces/DoublePairUnsafe_IndStep_Apalache.cfg
formal/tlaplus/reified_rspaces/JoinResidueUnsafe.cfg
formal/tlaplus/reified_rspaces/JoinResidueUnsafe_IndStep_Apalache.cfg
formal/tlaplus/reified_rspaces/MC_CheckpointReplay.cfg
formal/tlaplus/reified_rspaces/MC_CheckpointReplay_IndBase_Apalache.cfg
formal/tlaplus/reified_rspaces/MC_CheckpointReplay_IndStep_Apalache.cfg
formal/tlaplus/reified_rspaces/MC_CheckpointReplay_TwoBlock.cfg
formal/tlaplus/reified_rspaces/MC_TxnLockCoverage.cfg
formal/tlaplus/reified_rspaces/MC_TxnLockCoverage_IndBase_Apalache.cfg
formal/tlaplus/reified_rspaces/MC_TxnLockCoverage_IndStep_Apalache.cfg
formal/tlaplus/reified_rspaces/RootIgnoresVolatileUnsafe.cfg
formal/tlaplus/reified_rspaces/TxnLockCoverage.tla
formal/tlaplus/reified_rspaces/UncoveredChannelUnsafe.cfg
formal/tlaplus/reified_rspaces/UncoveredChannelUnsafe_IndStep_Apalache.cfg
formal/tlaplus/reified_rspaces/UnjoinedChannelUnsafe.cfg
formal/tlaplus/reified_rspaces/UnjoinedChannelUnsafe_IndStep_Apalache.cfg
formal/tlaplus/reified_rspaces/UuidUnsafe.cfg
rholang/tests/reified_ordinary_regression.rs
rholang/tests/reified_source_normalization.rs
rholang/tests/reified_space_store_handle_par.rs
rholang/tests/reified_space_txn_host_bounds.rs
rspace++/src/rspace/replay_rspace/space_replay.rs
rspace++/src/rspace/rspace/space_play.rs
rspace++/src/rspace/space_store_handle.rs
rspace++/tests/legacy_golden_corpus.rs
rspace++/tests/reified_space_store_handle.rs
rspace++/tests/reified_space_store_handle/support.rs
rspace++/tests/reified_space_store_handle_props.rs
scripts/check-reified-rspaces-proofs.sh
scripts/check-reified-rspaces-tla-invariants.sh
scripts/verify-reified/apalache-expect.sh
scripts/verify-reified/check-closure.py
scripts/verify-reified/check-complete.py
scripts/verify-reified/check-consensus-checklist.py
scripts/verify-reified/check-footprint.py
scripts/verify-reified/check-invariant-map.py
scripts/verify-reified/check-model-map.py
scripts/verify-reified/closure-policies.json
scripts/verify-reified/complete-policies.json
scripts/verify-reified/consensus-checklist.json
scripts/verify-reified/doc_lint.py
scripts/verify-reified/footprint-policies.json
scripts/verify-reified/merge-ordinary-capture.py
scripts/verify-reified/rocq-negative-controls.py
scripts/verify-reified/run-checker-tests.py
scripts/verify-reified/rust_citations.py
scripts/verify-reified/tests/fixtures/doc_lint/citation-doi/fail/doc.md.txt
scripts/verify-reified/tests/fixtures/doc_lint/citation-doi/pass/doc.md.txt
scripts/verify-reified/tests/fixtures/doc_lint/citation-key/fail/doc.md.txt
scripts/verify-reified/tests/fixtures/doc_lint/citation-key/pass/doc.md.txt
scripts/verify-reified/tests/fixtures/doc_lint/diagram-orphan/fail/diagrams/01-sample.puml
scripts/verify-reified/tests/fixtures/doc_lint/diagram-orphan/fail/diagrams/01-sample.svg
scripts/verify-reified/tests/fixtures/doc_lint/diagram-orphan/fail/diagrams/02-unused.puml
scripts/verify-reified/tests/fixtures/doc_lint/diagram-orphan/fail/doc.md.txt
scripts/verify-reified/tests/fixtures/doc_lint/diagram-orphan/fail/palette.iuml
scripts/verify-reified/tests/fixtures/doc_lint/diagram-orphan/pass/diagrams/01-sample.puml
scripts/verify-reified/tests/fixtures/doc_lint/diagram-orphan/pass/diagrams/01-sample.svg
scripts/verify-reified/tests/fixtures/doc_lint/diagram-orphan/pass/doc.md.txt
scripts/verify-reified/tests/fixtures/doc_lint/diagram-orphan/pass/palette.iuml
scripts/verify-reified/tests/fixtures/doc_lint/diagram-palette/fail/diagrams/01-sample.puml
scripts/verify-reified/tests/fixtures/doc_lint/diagram-palette/fail/diagrams/01-sample.svg
scripts/verify-reified/tests/fixtures/doc_lint/diagram-palette/fail/diagrams/02-uncolored.puml
scripts/verify-reified/tests/fixtures/doc_lint/diagram-palette/fail/diagrams/02-uncolored.svg
scripts/verify-reified/tests/fixtures/doc_lint/diagram-palette/fail/doc.md.txt
scripts/verify-reified/tests/fixtures/doc_lint/diagram-palette/fail/palette.iuml
scripts/verify-reified/tests/fixtures/doc_lint/diagram-palette/pass/diagrams/01-sample.puml
scripts/verify-reified/tests/fixtures/doc_lint/diagram-palette/pass/diagrams/01-sample.svg
scripts/verify-reified/tests/fixtures/doc_lint/diagram-palette/pass/doc.md.txt
scripts/verify-reified/tests/fixtures/doc_lint/diagram-palette/pass/palette.iuml
scripts/verify-reified/tests/fixtures/doc_lint/diagram-render/fail-latex/diagrams/01-sample.puml
scripts/verify-reified/tests/fixtures/doc_lint/diagram-render/fail-latex/doc.md.txt
scripts/verify-reified/tests/fixtures/doc_lint/diagram-render/fail-latex/palette.iuml
scripts/verify-reified/tests/fixtures/doc_lint/diagram-render/fail/diagrams/01-sample.puml
scripts/verify-reified/tests/fixtures/doc_lint/diagram-render/fail/doc.md.txt
scripts/verify-reified/tests/fixtures/doc_lint/diagram-render/fail/palette.iuml
scripts/verify-reified/tests/fixtures/doc_lint/diagram-render/pass/diagrams/01-sample.puml
scripts/verify-reified/tests/fixtures/doc_lint/diagram-render/pass/doc.md.txt
scripts/verify-reified/tests/fixtures/doc_lint/diagram-render/pass/palette.iuml
scripts/verify-reified/tests/fixtures/doc_lint/diagram-source/fail/diagrams/01-sample.svg
scripts/verify-reified/tests/fixtures/doc_lint/diagram-source/fail/doc.md.txt
scripts/verify-reified/tests/fixtures/doc_lint/diagram-source/fail/palette.iuml
scripts/verify-reified/tests/fixtures/doc_lint/diagram-source/pass/diagrams/01-sample.puml
scripts/verify-reified/tests/fixtures/doc_lint/diagram-source/pass/diagrams/01-sample.svg
scripts/verify-reified/tests/fixtures/doc_lint/diagram-source/pass/doc.md.txt
scripts/verify-reified/tests/fixtures/doc_lint/diagram-source/pass/palette.iuml
scripts/verify-reified/tests/fixtures/doc_lint/glossary-anchor/fail/doc.md.txt
scripts/verify-reified/tests/fixtures/doc_lint/glossary-anchor/fail/glossary.md
scripts/verify-reified/tests/fixtures/doc_lint/glossary-anchor/pass/doc.md.txt
scripts/verify-reified/tests/fixtures/doc_lint/glossary-anchor/pass/glossary.md
scripts/verify-reified/tests/fixtures/doc_lint/link/fail/doc.md.txt
scripts/verify-reified/tests/fixtures/doc_lint/link/fail/target.md
scripts/verify-reified/tests/fixtures/doc_lint/link/pass/doc.md.txt
scripts/verify-reified/tests/fixtures/doc_lint/link/pass/target.md
scripts/verify-reified/tests/fixtures/doc_lint/markdown-fence/fail/doc.md.txt
scripts/verify-reified/tests/fixtures/doc_lint/markdown-fence/pass/doc.md.txt
scripts/verify-reified/tests/fixtures/doc_lint/math-abut/fail/doc.md.txt
scripts/verify-reified/tests/fixtures/doc_lint/math-abut/pass/doc.md.txt
scripts/verify-reified/tests/fixtures/doc_lint/math-delimiter/fail/doc.md.txt
scripts/verify-reified/tests/fixtures/doc_lint/math-delimiter/pass/doc.md.txt
scripts/verify-reified/tests/fixtures/doc_lint/math-undelimited/fail/doc.md.txt
scripts/verify-reified/tests/fixtures/doc_lint/math-undelimited/pass/doc.md.txt
scripts/verify-reified/tests/fixtures/doc_lint/mermaid/fail/doc.md.txt
scripts/verify-reified/tests/fixtures/doc_lint/mermaid/pass/doc.md.txt
scripts/verify-reified/tests/fixtures/doc_lint/placement/fail/docroot/Bad_Name.md
scripts/verify-reified/tests/fixtures/doc_lint/placement/fail/docroot/README.md
scripts/verify-reified/tests/fixtures/doc_lint/placement/fail/docroot/diagrams/06A-upper-letter.puml
scripts/verify-reified/tests/fixtures/doc_lint/placement/fail/docroot/diagrams/06A-upper-letter.svg
scripts/verify-reified/tests/fixtures/doc_lint/placement/fail/docroot/diagrams/Sample.puml
scripts/verify-reified/tests/fixtures/doc_lint/placement/fail/docroot/diagrams/Sample.svg
scripts/verify-reified/tests/fixtures/doc_lint/placement/fail/docroot/diagrams/notes.txt
scripts/verify-reified/tests/fixtures/doc_lint/placement/fail/palette.iuml
scripts/verify-reified/tests/fixtures/doc_lint/placement/pass/docroot/README.md
scripts/verify-reified/tests/fixtures/doc_lint/placement/pass/docroot/diagrams/06a-sub-figure.puml
scripts/verify-reified/tests/fixtures/doc_lint/placement/pass/docroot/diagrams/06a-sub-figure.svg
scripts/verify-reified/tests/fixtures/doc_lint/placement/pass/docroot/diagrams/fonts.lock
scripts/verify-reified/tests/fixtures/doc_lint/placement/pass/docroot/guide.md
scripts/verify-reified/tests/fixtures/doc_lint/placement/pass/palette.iuml
scripts/verify-reified/tests/fixtures/doc_lint/rocq-cite/fail/doc.md.txt
scripts/verify-reified/tests/fixtures/doc_lint/rocq-cite/fail/theories/Sample.v
scripts/verify-reified/tests/fixtures/doc_lint/rocq-cite/pass/doc.md.txt
scripts/verify-reified/tests/fixtures/doc_lint/rocq-cite/pass/theories/Sample.v
scripts/verify-reified/tests/fixtures/doc_lint/rust-cite/fail/doc.md.txt
scripts/verify-reified/tests/fixtures/doc_lint/rust-cite/fail/src/rust_cite_fixture.rs
scripts/verify-reified/tests/fixtures/doc_lint/rust-cite/pass/doc.md.txt
scripts/verify-reified/tests/fixtures/doc_lint/rust-cite/pass/src/rust_cite_fixture.rs
scripts/verify-reified/tests/fixtures/doc_lint/snippet-corpus/fail-missing-corpus/doc.md.txt
scripts/verify-reified/tests/fixtures/doc_lint/snippet-corpus/fail-missing-corpus/test-vectors/reified-syntax-corpus-v1.json
scripts/verify-reified/tests/fixtures/doc_lint/snippet-corpus/fail/doc.md.txt
scripts/verify-reified/tests/fixtures/doc_lint/snippet-corpus/fail/examples/reified_rspaces/MANIFEST.toml
scripts/verify-reified/tests/fixtures/doc_lint/snippet-corpus/fail/test-vectors/reified-syntax-corpus-v1.json
scripts/verify-reified/tests/fixtures/doc_lint/snippet-corpus/pass/doc.md.txt
scripts/verify-reified/tests/fixtures/doc_lint/snippet-corpus/pass/examples/reified_rspaces/MANIFEST.toml
scripts/verify-reified/tests/fixtures/doc_lint/snippet-corpus/pass/test-vectors/reified-syntax-corpus-v1.json
scripts/verify-reified/tests/fixtures/doc_lint/ste-contraction/fail/doc.md.txt
scripts/verify-reified/tests/fixtures/doc_lint/ste-contraction/pass/doc.md.txt
scripts/verify-reified/tests/fixtures/doc_lint/ste-paragraph-length/fail/doc.md.txt
scripts/verify-reified/tests/fixtures/doc_lint/ste-paragraph-length/pass/doc.md.txt
scripts/verify-reified/tests/fixtures/doc_lint/ste-procedure-length/fail/doc.md.txt
scripts/verify-reified/tests/fixtures/doc_lint/ste-procedure-length/pass/doc.md.txt
scripts/verify-reified/tests/fixtures/doc_lint/ste-semicolon/fail/doc.md.txt
scripts/verify-reified/tests/fixtures/doc_lint/ste-semicolon/pass/doc.md.txt
scripts/verify-reified/tests/fixtures/doc_lint/ste-sentence-length/fail/doc.md.txt
scripts/verify-reified/tests/fixtures/doc_lint/ste-sentence-length/pass/doc.md.txt
scripts/verify-reified/tests/test_checkers.py
scripts/verify-reified/tests/test_closure.py
scripts/verify-reified/tests/test_doc_lint.py
scripts/verify-reified/tlc-expect-violation.sh
test-vectors/head-file-digests-6f7a80da3.json
test-vectors/legacy-golden-corpus-v1.json
test-vectors/ordinary-execution-v1.json
test-vectors/ordinary-float-forms-v1.json
test-vectors/ordinary-source-normalization-v1.json
```

</details>

**The closure checker.** `scripts/verify-reified/check-closure.py` reads `scripts/verify-reified/closure-policies.json`.
That policy binds 11 realignment leaves to their evidence: model items, optional model items, headlines, unmodeled items, entry points, document headings, document references, and anchors.
Each subcommand prints one `PASS` or `FAIL` line for each rule instance, and a subcommand that finds no rule instance fails.

- `modeled` checks that each model item of a leaf is in the model map, with an overlap of its covers and the leaf headlines.
- `invariants` checks that each leaf headline has a P1 property, and that the file of the property defines the test.
- `entry-points` checks that each entry-point pattern matches a line of its file.
- `docs` checks that each heading line occurs exactly once, that each reference text occurs, and that each anchor section holds its keywords.
- `audit --script PATH` runs an audit script in the harness below.
- `ledger-copy` and `ledger-copy-controls` check the copy of the design ledger.

**The audit harness.** The audit scripts find the repository with git, and the pgmcp snapshot has no `.git` directory.
So the harness builds a minimal repository in a fresh directory, copies `scripts/audit/`, and links the other roots.
It checks that the script names no path in the system temporary directory and uses `mktemp` only in its default directory.
Then it runs `bash -n` and the script, in a cleared environment with empty `HOME` and `TMPDIR` directories.

The harness compares the exit code, the citation total, and the set of mismatch lines with the policy.
It also requires that `HOME` and `TMPDIR` stay empty.
`check-rocq-line-refs.sh` exits with 1 after 125 citations and 7 mismatches (finding `rrs-finding-preexisting-rocq-citation-drift`), and `check-source-line-refs.sh` exits with 1 after 132 citations and 35 mismatches.
Both audits read only `docs/casper/theory`, so they do not check this dossier.

**The ledger copy by inverse redaction.** The repository holds a redacted copy of the design ledger in `design-record/`.
The copy has the SHA-256 digest `5f42c87067eddf859bd6aff837f2b67127061d621e5266330e96a979cae0e8ed`, 1,585 lines, one `[HOME]` token, and no home path.
The check replaces the token with the value of `RRS_REDACTION_HOME` and requires the digest of the original ledger, `ca3d290986ec599805fb8bb14fe51b89619fd126f431abb09d33be7744f6ba6d`.
The recipe sets that variable, so the user name never enters the repository.
`ledger-copy-controls` changes one byte of the copy and one digest in the README, and each change must fail the rules.

The last host runs passed every subcommand: `modeled` (164 rows), `invariants` (36), `entry-points` (62), `docs` (37), each audit (13), `ledger-copy` (11), and `ledger-copy-controls` (3).
The policy has no entry for the leaves that cite this dossier yet, so `docs` does not check this file in this version.

## 10. Running the checks locally

Table 23 lists the checkers of `scripts/verify-reified/` with the data that each one reads.
Each checker uses the Python 3 standard library only, and none of them needs git, except the pin of the footprint digests.

*Table 23. The checkers, their data, and their recipe steps.*

| Checker | Reads | Checks | Recipe steps |
|---|---|---|---|
| `check-model-map.py` | `formal/reified-rspaces-model-map.json`, the Rocq and TLA+ files, and the Rust sources | Each item exists in its Rust file, each item and fix rule names existing definitions and headlines, and each Rust line citation of a formal file resolves. | `p1-model-map` |
| `check-invariant-map.py` | `formal/reified-rspaces-invariant-map.json`, `consensus-checklist.json`, and the TLA+ configurations | Each id names tests that exist, each invariant of a configuration is mapped or auxiliary, and each `prop_` function of a test file is mapped. | `p1-invariant-map`, `p1-invariant-map-ord` |
| `check-footprint.py` | `footprint-policies.json` and `test-vectors/head-file-digests-4d67b5ccc.json` (the milestone policies `p1`, `p1-realign` and `p2` ran against `test-vectors/head-file-digests-6f7a80da3.json`) | The policy `campaign` freezes the files that no phase may change (items 2, 4, 5, 8, 9, 13, 16, 17, and 21, and decision 18). The policy `p1` extends it with the four one-line `rspace++` edits and the decision-14 audit edit. The policy `p1-realign` is section [9](#9-realignment-closure-checks). | `p1-footprint`, `p1-footprint-realign` |
| `check-complete.py` | `complete-policies.json`: the phase `p1` and the section `production_scan` | The entry points, the forbidden tokens, the hygiene of the tests, and the production scan of section [8.6](#86-grep-gates). | `p1-complete`, `p1-production-scan` |
| `check-consensus-checklist.py` | `consensus-checklist.json`, `footprint-policies.json`, and `complete-policies.json` | Each active check of each item at one milestone (sections [8.1](#81-consensus-preservation-checklist) and [8.5](#85-milestone-rule-of-the-checklist-checker)). | `p1-consensus-checklist` |
| `check-closure.py` | `closure-policies.json` and both maps | The seven subcommands of section [9](#9-realignment-closure-checks). | `p1-closure-modeled`, `p1-closure-invariants`, `p1-closure-entry-points`, `p1-closure-docs`, `p1-audit-rocq`, `p1-audit-source`, `p1-ledger-copy`, `p1-ledger-copy-controls` |
| `rust_citations.py` | The Rust sources | The rules of a Rust line citation: the path names one source, and each range lies in the file and starts and ends on code. `check-model-map.py` and `doc_lint.py` share it. | Part of the steps above |
| `doc_lint.py` | The campaign documents, the snippet corpora, the Rocq theories, the shared palette, and `render.sh` | Its 22 rules, which `--list-rules` prints. | `p1-docs-lint`, `p1-doc-lint` |
| `run-checker-tests.py` | The modules of `scripts/verify-reified/tests/` | The unit tests of the checkers (section [6.4](#64-checker-self-tests)). | `p1-checker-tests`, `p1-docs-selftests` |
| `docs/rspace/reified-rspaces/diagrams/render.sh` | The diagram sources and `fonts.lock` | Each SVG equals a fresh render by PlantUML 1.2026.8 with the seven pinned fonts, a private fontconfig, and a fixed locale. | `p1-docs-render` |

### 10.1 Preparation

1. Open a shell in the root of the repository.
2. Set `TMPDIR` to `$PWD/target/tmp`, because `/tmp` is RAM-backed on the development hosts.
3. Make sure that `systemd-run --user --scope` works, because the gates bound their heavy legs with it.
4. Inside a verifier scope, set `RRS_SANDBOX=1` instead, so the gates do not start a nested scope.

Each gate writes its logs under `target/verification/reified-rspaces/` and never under `/tmp`.

### 10.2 Formal gates

1. Run `scripts/check-reified-rspaces-proofs.sh` for the Rocq legs and the 75 mutants.
2. Confirm the last line `reified-rspaces proofs: PASS=53 FAIL=0 SKIP=0`.
3. Run `scripts/check-reified-rspaces-tla-invariants.sh` for the TLC, Unsafe, complement, and Apalache legs.
4. Confirm the last line `reified-rspaces tla: PASS=162 FAIL=0 SKIP=0`.
5. To run one mutant, run `python3 scripts/verify-reified/rocq-negative-controls.py --only <id>`.
6. To run one Unsafe control, run `scripts/verify-reified/tlc-expect-violation.sh <cfg> <invariant> <module>`.
7. To run its complement, add the option `--complement` before the configuration.

The TLA+ gate takes the longest time: `MC_TxnLockCoverage.cfg` takes about 6.5 minutes, and the complement of `UncoveredChannelUnsafe.cfg` about 10 minutes.

### 10.3 Rust suites

1. Run `cargo test -p rspace_plus_plus --lib space_store_handle` for the 18 library tests.
2. Run `cargo test -p rspace_plus_plus --test reified_space_store_handle` for the 82 integration tests.
3. Run `cargo test -p rspace_plus_plus --test reified_space_store_handle_props` for the 24 properties.
4. Run `cargo test -p rspace_plus_plus --test legacy_golden_corpus` for the 4 golden-corpus tests.
5. Run `cargo test -p rholang --test reified_space_store_handle_par --test reified_space_txn_host_bounds` for 3 and 1 tests.
6. Run `cargo test -p rholang --test reified_ordinary_regression` for R1 to R9 and the 4 tests of the close-out.
7. Run `cargo test -p rholang --test reified_source_normalization` for the 2 normalization tests.
8. Run `cargo test -p casper --test mod -- util::rholang::runtime_spec::` for the 2 empty-state hash tests.
9. Run `cargo test -p rspace_plus_plus --test storage_actions_test --test replay_rspace_tests --test comm_observer_tests` for the 41, 29, and 27 HEAD tests.
10. Run `cargo test -p rspace_plus_plus --test mod -- history::` and `cargo test -p rspace_plus_plus --lib -- rspace::history::`.
11. Run `cargo test -p rholang --test mod -- matcher::match_test::` for the 52 HEAD matcher tests.

Each run must report `0 failed` and `0 ignored`.
To repeat one property, run its target with `-- --exact <name>`, and the fixed seed gives the same cases.

### 10.4 Python checkers

1. Run `python3 scripts/verify-reified/check-model-map.py`.
2. Run `python3 scripts/verify-reified/check-invariant-map.py --phase P1` and then with `--phase ORD`.
3. Run `python3 scripts/verify-reified/check-footprint.py --policy p1` and then with `--policy p1-realign`.
4. Run `python3 scripts/verify-reified/check-complete.py --phase p1`.
5. Run `python3 scripts/verify-reified/check-complete.py --production-scan`.
6. Run `python3 scripts/verify-reified/check-consensus-checklist.py --milestone p1`.
7. Run `python3 scripts/verify-reified/check-closure.py <subcommand>` for each subcommand of section [9](#9-realignment-closure-checks).
8. Before the ledger-copy subcommands, set `RRS_REDACTION_HOME` to the home directory of the original ledger.
9. Run `python3 scripts/verify-reified/run-checker-tests.py test_checkers test_closure test_doc_lint`.

Each checker exits with 0 only when every rule holds.
At the P1 close-out, `check-invariant-map.py` without `--phase` exits with 3, because the planned P2 tests do not exist yet.

### 10.5 Documents

1. Run `python3 scripts/verify-reified/doc_lint.py` to lint every campaign document.
2. After a change to a diagram source, run `docs/rspace/reified-rspaces/diagrams/render.sh`.
3. Run `docs/rspace/reified-rspaces/diagrams/render.sh --check` to confirm that each SVG is current.

## 11. Phase P4: the hashmap:queue slice

Phase P4 delivers the first working Reified RSpace: the space family `hashmap:queue`, from the factory call to the cross-branch race.
This section records the evidence of P4 in the form of the P1 sections above.
It also holds the P4 rows of the tables of sections 2 and 8, so the P1 tables stay as P1 left them.
The [design](design.md#native-spaces) describes the code, the [specification](specification.md) states the rules, and the [agent API](agent-api.md) states the calls.

### 11.1 What P4 proves

P4 proves eleven groups of claims, with 80 Rocq headlines (Q1 to Q80) and 13 TLA+ invariants (U1 to U13):

- **Ids and records.** The Reified ids are injective, disjoint from every other name, and total to decode. The record envelopes round-trip, and their decoders accept only the image of their encoders (Q1 to Q19).
- **Counters and merges.** Two branches that use one queue race on its counter in merge check 1, and two deploys of one block form one chain. A merge without a race is a serial run (Q20 to Q28, U4).
- **The queue.** The queue is head-only on both sides, first in, first out, and bounded. It keeps the invariant INV after each operation (Q29 to Q37, U1 to U3).
- **Routing and references.** The classification of channels and of space references is total and deterministic. The use context acts as a lexical closure (Q38 to Q47).
- **Factories and handles.** The family indices, the constructors, the space ids, and the creation are deterministic. A persistent call aborts, and handle calls merge like contract calls (Q48 to Q55, U5 to U8).
- **The refresh of I-20.** The strict decode of a refreshed marker tuple is the decode of the model, and it commutes with substitution (Q56 to Q58).
- **Metering.** An operation reserves each read and copy before it makes it, and no reservation follows its commit. Its charges do not depend on the list order of the store or on unread payloads (Q59 to Q66, section [11.10](#1110-metering-of-decision-42)).
- **Native execution.** Native play and a bound native replay runtime run a space deploy. A native replay runtime that is not bound yet fails closed (Q67 as decision 47 amends it, section [11.11](#1111-native-execution-of-decisions-46-and-47)).
- **Re-entry.** A re-issued persistent operation keeps the order of its first round and re-enters at its place (Q68 to Q77, U9 to U12, section [11.12](#1112-re-entry-of-decision-43)).
- **Peek re-store.** A peek fire removes what the host removes, and the agent stores the record again (Q78, section [11.13](#1113-peek-re-store-of-decision-45)).
- **Legacy charges.** A space job makes the legacy charges and the stop of its host program (Q79, Q80, U13, section [11.14](#1114-legacy-charges-of-decision-45)).

The [invariant map](glossary.md#invariant-map) phase `P4` names at least one property test for each id.
The [model map](glossary.md#model-map) of P4 ties each id to its Rust items.

### 11.2 Acceptance categories in P4

Table 24 gives the evidence of each [acceptance category](glossary.md#acceptance-category) in P4 and its local gate.
The pgmcp runs of the P4 leaves take place in the freeze of [decision 35](decision-register.md#decision-35), so this table names only the local gates.

*Table 24. The six acceptance categories in P4.*

| Category | Evidence in P4 | Local gate |
|---|---|---|
| Formally modeled | `check-model-map.py --map formal/reified-rspaces-model-map-p4.json` checks the 200 items of the P4 model map, all of them required, and its 93 headlines. Each item exists in the code and cites Rocq definitions and TLA+ operators that exist. | `python3 scripts/verify-reified/check-model-map.py --map formal/reified-rspaces-model-map-p4.json` |
| Formal verification passes | The 115 Rocq headlines of eleven modules close under the global context, and `rocqchk` accepts each module. The 75 Rocq mutants fail in their expected declarations. TLC explores 4 P4 safe configurations completely, 7 P4 Unsafe configurations each refute exactly one invariant, and Apalache proves BASE and STEP of `HeadIndInv` (with and without re-issues) and of `HandleIndInv`. For U13, TLC explores the 2 legacy configurations of `CheckpointReplay.tla`, 2 Unsafe controls refute it, and Apalache proves BASE and STEP of `IndInv` (section [11.14](#1114-legacy-charges-of-decision-45)). | `scripts/check-reified-rspaces-proofs.sh`, `scripts/check-reified-rspaces-tla-invariants.sh` |
| Every invariant extracted as a property-based test | The invariant map names a property for each of Q1 to Q80 and U1 to U13: 78 properties in `reified_p4_props.rs`, 3 in `reified_legacy_charges.rs`, 3 properties of `Encoding.v`, and the 3 tests of `reified_native_fail_closed.rs`. It also names 27 witness tests. | `python3 scripts/verify-reified/check-invariant-map.py --phase P4`, and `cargo test -p rholang --test reified_p4_props` and `--test reified_legacy_charges` |
| Implementation complete | `check-complete.py --phase p4` finds the 76 entry points in the 35 P4 sources, and no forbidden token. `check-footprint.py --policy p4` checks the P4 edits of the base files. `check-consensus-checklist.py --milestone p4` confirms the active checks of the 22 items. | `python3 scripts/verify-reified/check-complete.py --phase p4` and the other checkers |
| Thoroughly tested | The 13 P4 suites of section [11.7](#117-integration-suites) with 175 tests, the 198 unit tests of the P4 modules, and the suites of ordinary execution. | `cargo test` (section [11.9](#119-running-the-p4-checks)) |
| Documented per the pgmcp guidelines | `doc_lint.py` passes every rule on every campaign document, and `render.sh --check` finds each SVG byte-identical to a fresh render. | `python3 scripts/verify-reified/doc_lint.py`, `docs/rspace/reified-rspaces/diagrams/render.sh --check` |

### 11.3 Rocq models of P4

#### 11.3.1 The gate in P4

The proofs gate of section [4.1](#41-toolchain-and-kernel-checks) now checks eleven modules, with the expected count of each module.
Table 25 gives the counts of all eleven modules, and the total is 115 closed headlines.
The run at the P4 milestone printed `reified-rspaces proofs: PASS=45 FAIL=0 SKIP=0` (`target/verification/reified-rspaces/p4/pc/gate-proofs.log`).
Its last leg ran 65 negative checks: the 9 pristine modules and the 56 mutants of that milestone.

The run after the merge of [decision 44](decision-register.md#decision-44) printed the same line (`target/verification/reified-rspaces/merge-car/proofs-2.log`).
Its last leg ran 66 negative checks with the 57 mutants of section [11.5](#115-negative-controls-of-p4).
The run of [decision 42](decision-register.md#decision-42) checked ten modules and printed `reified-rspaces proofs: PASS=49 FAIL=0 SKIP=0` (`target/verification/reified-rspaces/d42/proofs.log`).
Its last leg ran 72 negative checks: the 10 pristine modules and 62 mutants.
The run of Option P of decision 45 checked eleven modules and printed `reified-rspaces proofs: PASS=53 FAIL=0 SKIP=0` (`target/verification/reified-rspaces/d45/option-p-gates/proofs.log`).
Its last leg ran 86 negative checks: the 11 pristine modules and 75 mutants.

*Table 25. The modules of the proofs gate and their closed headlines.*

| Module | Headlines | Phase |
|---|---|---|
| [EventCorrelation.v](../../../formal/rocq/reified_rspaces/theories/EventCorrelation.v) | 17 | P1 (16) and decision 36 (1) |
| [VolatileSweep.v](../../../formal/rocq/reified_rspaces/theories/VolatileSweep.v) | 6 | P1 |
| [Encoding.v](../../../formal/rocq/reified_rspaces/theories/Encoding.v) | 15 | P2 (12) and I-20 (3) |
| [Namespace.v](../../../formal/rocq/reified_rspaces/theories/Namespace.v) | 11 | P4 |
| [RecordEnvelope.v](../../../formal/rocq/reified_rspaces/theories/RecordEnvelope.v) | 8 | P4 |
| [CounterConflict.v](../../../formal/rocq/reified_rspaces/theories/CounterConflict.v) | 9 | P4 |
| [HeadOnlyDisciplines.v](../../../formal/rocq/reified_rspaces/theories/HeadOnlyDisciplines.v) | 20 | P4 (9), decision 43 (10), and decision 45 (1) |
| [SpaceRefs.v](../../../formal/rocq/reified_rspaces/theories/SpaceRefs.v) | 11 | P4 (10) and decision 46 (1) |
| [SpaceFactory.v](../../../formal/rocq/reified_rspaces/theories/SpaceFactory.v) | 8 | P4 |
| [AgentReadCoverage.v](../../../formal/rocq/reified_rspaces/theories/AgentReadCoverage.v) | 8 | Decision 42 |
| [LegacyCharges.v](../../../formal/rocq/reified_rspaces/theories/LegacyCharges.v) | 2 | Decision 45, Option P |
| Total | 115 | |

Each headline of the tables below has no assumption: `Print Assumptions` reports `Closed under the global context`.
The last column gives the property tests that the invariant map names for the headline.

#### 11.3.2 Headlines of Namespace.v

Namespace.v models the ids under the namespace tag `0xE0` and the collection key of a channel.

*Table 26. The 11 headlines of Namespace.v.*

| Id | Theorem | Statement | Property tests |
|---|---|---|---|
| Q1 | `kind_byte_injective` (`Namespace.v:664`) | The KIND byte names its kind and its durability. | `prop_kind_byte_injective` |
| Q2 | `storage_id_injective` (`Namespace.v:727`) | The storage ids are injective in their space, kind, and key. | `prop_storage_id_injective` |
| Q3 | `handle_id_injective` (`Namespace.v:752`) | A handle id names exactly one native space. | `prop_handle_id_injective` |
| Q4 | `ephemeral_durable_disjoint` (`Namespace.v:862`) | No ephemeral id equals a durable id. | `prop_ephemeral_durable_disjoint` |
| Q5 | `reified_ids_disjoint_from_new_ids` (`Namespace.v:895`) | No Reified id equals the 32-byte id of a name that `new` makes. | `prop_reified_ids_disjoint_from_new_ids` |
| Q6 | `reified_ids_disjoint_from_byte_names_and_mettail` (`Namespace.v:910`) | No Reified id equals a 1-byte system name or a MeTTaTron id. | `prop_reified_ids_disjoint_from_byte_names_and_mettail` |
| Q7 | `op_ids_pairwise_distinct` (`Namespace.v:969`) | The op ids of the factories, the system processes, and the markers are pairwise distinct. | `prop_op_ids_pairwise_distinct` |
| Q8 | `decode_encode_reified_id` (`Namespace.v:633`) | The decoder inverts every constructor, and it accepts only their images. | `prop_decode_reified_id_roundtrip_and_sound` |
| Q9 | `space_and_collection_counters_distinct` (`Namespace.v:784`) | The space counter differs from every collection counter. | `prop_space_and_collection_counters_distinct` |
| Q10 | `name_id_routes_to_sid` (`Namespace.v:998`) | A name id decodes to its space. | `prop_name_id_routes_to_sid` |
| Q11 | `canon_respects_par_eq` (`Namespace.v:1038`) | Channels that are equal as `Par` values have one collection key. | `prop_canonical_inner_bytes_respects_par_eq` |

#### 11.3.3 Headlines of RecordEnvelope.v

RecordEnvelope.v models the record envelopes of [decision M-2](decision-register.md#m-2): the datum, the continuation, the counter, and the config record.

*Table 27. The 8 headlines of RecordEnvelope.v.*

| Id | Theorem | Statement | Property tests |
|---|---|---|---|
| Q12 | `datum_unwrap_wrap` (`RecordEnvelope.v:672`) | Unwrapping a wrapped datum gives the order and the payload back. | `prop_datum_envelope_roundtrip` |
| Q13 | `datum_wrap_injective` (`RecordEnvelope.v:680`) | The datum envelope is injective in the order and the payload. | `prop_datum_envelope_injective` |
| Q14 | `datum_unwrap_sound` (`RecordEnvelope.v:691`) | The datum decoder accepts only the image of the encoder. | `prop_datum_envelope_decoder_sound` |
| Q15 | `continuation_unwrap_wrap` (`RecordEnvelope.v:713`) | Unwrapping a wrapped continuation gives the order and the body back. | `prop_continuation_envelope_roundtrip` |
| Q16 | `counter_and_config_roundtrip` (`RecordEnvelope.v:1075`) | The counter and config records round-trip, the config as its canonical map. | `prop_counter_and_config_records_roundtrip` |
| Q17 | `envelope_kinds_disjoint` (`RecordEnvelope.v:1118`) | The tags of the four record kinds are pairwise distinct. | `prop_envelope_kinds_disjoint` |
| Q18 | `no_reified_datum_is_single_number` (`RecordEnvelope.v:1242`) | No Reified datum is a single number, so the merger never treats it as a number channel. | `prop_no_reified_datum_is_single_number` |
| Q19 | `unknown_version_rejected` (`RecordEnvelope.v:1184`) | A tag with another version or kind byte is rejected. | `prop_envelope_unknown_version_rejected` |

#### 11.3.4 Headlines of CounterConflict.v

CounterConflict.v restates merge check 1 of the baseline, and it proves the races and the serializability of the counter rule ([decision 31](decision-register.md#decision-31)).

*Table 28. The P4 headlines of CounterConflict.v.*

| Id | Theorem | Statement | Property tests |
|---|---|---|---|
| Q20 | `check1_race_iff` (`CounterConflict.v:495`) | Check 1 reports a race exactly for a shared non-persistent event that is not mergeable in both logs. | `prop_check1_restatement_matches_merger` |
| Q21 | `ordered_writes_race_on_counter` (`CounterConflict.v:910`) | Two branches that write one queue race on its counter. | `prop_queue_writes_race_on_counter` |
| Q22 | `ordered_reads_race_on_counter` (`CounterConflict.v:924`) | A peek bumps the counter too, so two branches that read one queue race. | `prop_queue_reads_race_on_counter` |
| Q23 | `first_ops_race_on_space_counter` (`CounterConflict.v:939`) | Two branches whose first operations open collections of one space race on the space counter. | `prop_first_ops_race_on_space_counter` |
| Q24 | `crossed_collections_race` (`CounterConflict.v:953`) | Branches that use two queues in crossed orders race on both counters. | `prop_crossed_collections_race` |
| Q25 | `same_block_ops_chain` (`CounterConflict.v:1113`) | Two deploys of one block that use one queue form one chain. | `prop_same_block_queue_ops_chain` |
| Q26 | `unordered_writes_commute` (`CounterConflict.v:1651`) | Writes to a collection without a counter never race, and the merge holds both serial orders. | `prop_counter_free_writes_commute` |
| Q27 | `merge_without_race_is_serializable` (`CounterConflict.v:1489`) | Without a race, the merged state and the observations are those of a serial run. | `prop_nonracing_queue_branches_serializable` |
| Q28 | `counter_free_peek_admits_write_skew` (`CounterConflict.v:2485`) | A peek without the counter admits the write skew of a peek and a dequeue, as a computed counterexample. | `prop_counter_free_peek_admits_write_skew` |

P5 adds two headlines to the module, and section [12.11](#1211-the-merge-of-the-p5-families) describes them.

#### 11.3.5 Headlines of HeadOnlyDisciplines.v

HeadOnlyDisciplines.v models the plans of the queue for every matcher, with persistent entries, peeks, and the fire rounds.

*Table 29. The 9 headlines of HeadOnlyDisciplines.v.*

| Id | Theorem | Statement | Property tests |
|---|---|---|---|
| Q29 | `queue_head_is_min_ord` (`HeadOnlyDisciplines.v:530`) | The head of each side has the least order and bytes. | `prop_queue_candidates_head_only` |
| Q30 | `queue_produce_preserves_inv` (`HeadOnlyDisciplines.v:928`) | A produce keeps INV: no pending match between the heads. | `prop_queue_produce_preserves_invariant` |
| Q31 | `queue_consume_preserves_inv` (`HeadOnlyDisciplines.v:948`) | A consume keeps INV. | `prop_queue_consume_preserves_invariant` |
| Q32 | `queue_fifo` (`HeadOnlyDisciplines.v:1135`) | Records leave in arrival order. | `prop_queue_fifo` |
| Q33 | `queue_head_blocks` (`HeadOnlyDisciplines.v:1218`) | A head that does not match blocks the entries behind it. | `prop_queue_head_blocks` |
| Q34 | `queue_loop_bounded_or_reissues` (`HeadOnlyDisciplines.v:1380`) | A plan stages at most `FIRE_ROUND_CAP` fires, and at most one more fire than the queue holds, or it re-issues. | `prop_queue_loop_bounded` |
| Q35 | `queue_ord_fresh` (`HeadOnlyDisciplines.v:1046`) | The order of an operation is new in its collection. | `prop_queue_ord_fresh` |
| Q36 | `queue_plan_permutation_invariant` (`HeadOnlyDisciplines.v:1552`) | The plan ignores the list order of the staged view. | `prop_queue_plan_permutation_invariant` |
| Q37 | `legacy_scan_violates_head_blocks` (`HeadOnlyDisciplines.v:3457`) | The legacy waiter scan breaks head-only matching, as a computed example. | `prop_legacy_scan_breaks_head_blocks` |

#### 11.3.6 Headlines of SpaceRefs.v

SpaceRefs.v models the classification of space references, the routing rules R1 to R7, the use context, and the dispatch of `HANDLE_OP`.

*Table 30. The 10 headlines of SpaceRefs.v.*

| Id | Theorem | Statement | Property tests |
|---|---|---|---|
| Q38 | `classify_space_ref_total_deterministic` (`SpaceRefs.v:485`) | Every space reference meets exactly one of the five rules of `classify_space_ref`. | `prop_classify_space_ref_total` |
| Q39 | `handle_requires_write_flag` (`SpaceRefs.v:516`) | A reference names a native space only as a bare handle or as a writable bundle of one. | `prop_handle_requires_write_flag` |
| Q40 | `reified_non_handle_not_a_space_ref` (`SpaceRefs.v:547`) | A Reified id that is not a handle is `NotASpaceRef`, never an agent. | `prop_reified_non_handle_not_a_space_ref` |
| Q41 | `route_total_deterministic` (`SpaceRefs.v:636`) | A route fails only on the key, and it depends on the context only through its native space. | `prop_route_total` |
| Q42 | `route_rules` (`SpaceRefs.v:660`) | The routing rules R1 to R5. | `prop_route_rules` |
| Q43 | `space_name_route_ignores_ctx` (`SpaceRefs.v:681`) | A space name routes to its own space under every context. | `prop_space_name_route_ignores_ctx` |
| Q44 | `r7_route_ignores_ctx` (`SpaceRefs.v:724`) | The R7 backend agrees with the reducer on every channel that is not forged. | `prop_r7_route_ignores_ctx` |
| Q45 | `ordinary_channels_route_default` (`SpaceRefs.v:746`) | The channels of an ordinary program route to the default space, and the fast path is exact. | `prop_ordinary_channels_route_default` |
| Q46 | `use_wrapper_restores_ctx` (`SpaceRefs.v:842`) | A body that the reducer stores under a context runs in that context when it fires. | `prop_use_wrapper_restores_ctx` |
| Q47 | `handle_dispatch_total` (`SpaceRefs.v:1008`) | The handle process drops a call on a non-handle, aborts on a dangling handle, and replies only on a reply name. | `prop_handle_dispatch_total` |

#### 11.3.7 Headlines of SpaceFactory.v

SpaceFactory.v models the families, the constructor, the derivation of the space id, the persistent sends, and the creation.

*Table 31. The 8 headlines of SpaceFactory.v.*

| Id | Theorem | Statement | Property tests |
|---|---|---|---|
| Q48 | `family_index_bijective` (`SpaceFactory.v:254`) | The 32 families have the indices 0 to 31, one each. | `prop_family_index_bijective`, `prop_get_space_agent_returns_family_capability` |
| Q49 | `aliases_map_to_family` (`SpaceFactory.v:336`) | The URNs and the FIPS aliases parse to their families, and a config names its family. | `prop_aliases_map_to_family`, `prop_get_space_agent_returns_family_capability` |
| Q50 | `qualifier_matrix` (`SpaceFactory.v:372`) | The HashSet store takes only `seq`, and P4 creates only default spaces. | `prop_qualifier_matrix`, `prop_qualifier_parse_roundtrip` |
| Q51 | `constructor_parse_canonical` (`SpaceFactory.v:496`) | Every canonical constructor parses back from its elements. | `prop_constructor_parse_canonical`, `prop_qualifier_parse_roundtrip` |
| Q52 | `constructor_parse_sound` (`SpaceFactory.v:520`) | The parser accepts only the canonical sequence of a qualifier, a theory, and params. The statement has no premise on the `locally_free` bits, because the parser rejects a qualifier element with bits as `NonCanonical`. | `prop_constructor_parse_sound` |
| Q53 | `sid_derivation_deterministic` (`SpaceFactory.v:650`) | The space id is the 32-byte draw of the random state of the call. | `prop_sid_derivation_deterministic` |
| Q54 | `persistent_op_call_aborts` (`SpaceFactory.v:712`) | A persistent send to an op that takes calls, or to a handle, aborts before the call. | `prop_persistent_op_call_aborts` |
| Q55 | `space_already_exists_is_deterministic_abort` (`SpaceFactory.v:797`) | A creation over an existing config record aborts in play and in replay alike, and a commit applies all three operations or none. | `prop_space_already_exists_is_deterministic_abort`, `prop_prepare_commit_atomic` |

#### 11.3.8 Encoding.v and the refresh of I-20

P4 refines [Encoding.v](../../../formal/rocq/reified_rspaces/theories/Encoding.v) of phase P2 in four ways for [decision I-20](decision-register.md#i-20).
The first three refinements are new headlines, which Table 32 lists.

*Table 32. The 3 headlines that I-20 adds to Encoding.v.*

| Id | Theorem | Statement | Property tests |
|---|---|---|---|
| Q56 | `refresh_strict_decode_erase` (`Encoding.v:1854`) | After the refresh, the strict decode is the decode of the model, and it accepts only the derived flags. | `marker_forging_grants_no_capability` |
| Q57 | `connective_marker_tuple_recognized` (`Encoding.v:1872`) | A canonical marker tuple whose flag `connective_used` is true is recognized. | `reified_encoding_roundtrip` |
| Q58 | `refreshed_decode_commutes_with_subst` (`Encoding.v:2105`) | After the refresh, the strict decode of a substituted tuple commutes with the substitution. | `reified_decode_commutes_with_substitution` |

The fourth refinement corrects two notes of the module:

- **The note on the abstraction α.** The function α maps a `GPrivate` with the flag `connective_used` false to the model term of a private name. A `GPrivate` with the flag true maps to the term of other values, because the marker decoder rejects such a head. The function α forgets the flags of every expression and of its `Par`.
- **The note on the substitution.** The premises of `decode_commutes_with_subst` hold up to the flags, because the baseline keeps the flags of a substituted tuple, list, or map. Q58 proves the statement with the flags.
- **The consistency of the premises.** The corollaries `decode_commutes_with_concrete_subst` (`Encoding.v:2151`) and `refreshed_decode_commutes_with_concrete_subst` (`Encoding.v:2162`) discharge every premise with a concrete substitution. So the premises are consistent.

### 11.4 TLA+ models of P4

The gate of section [5.1](#51-tools-and-gate-legs) checks three P4 modules with the legs Bounds, Safe, Unsafe, and Apalache.
The last host run, the run of Option P of decision 45, printed `reified-rspaces tla: PASS=162 FAIL=0 SKIP=0` (`target/verification/reified-rspaces/d45/tla-lc/gate.log`).
The counts of that run include the P1 legs of section 5.

#### 11.4.1 HeadOnlyDiscipline.tla

[HeadOnlyDiscipline.tla](../../../formal/tlaplus/reified_rspaces/HeadOnlyDiscipline.tla) explores every schedule of produces, consumes, and re-entries on one queue collection, with every value and every pattern.
With `Reissues = FALSE`, every entry is non-persistent and no waiter peeks, so a fire spends both partners.
With `Reissues = TRUE`, the model adds persistent sends, persistent receives, peek receives, and the elements in flight of [decision 43](decision-register.md#decision-43).
A round fires at most once (`Cap = 1`), because Apalache does not accept the recursive fire loop.
A round of the code with k fires is k rounds of the model with no operation between them.
HeadOnlyDisciplines.v covers every fire-round cap and every matcher.

*Table 33. The invariants of HeadOnlyDiscipline.tla.*

| Id | Invariant | Meaning | Property tests |
|---|---|---|---|
| U1 | `QuiescentNoPendingMatch` | With no element in flight, the head record and the head waiter never match (INV). | `prop_queue_no_pending_match_any_schedule`, `prop_queue_shadow_invariant_with_inflight` |
| U2 | `HeadBlocks` | Every fire involves the head of the other side, so a head that does not match blocks the entries behind it. | `prop_queue_head_blocks_any_schedule`, `prop_queue_reentry_head_blocks` |
| U3 | `FifoOrder` | An element fires only when no entry of its own side precedes it, also after a re-entry (QD-7, QD-9). | `prop_queue_head_blocks_any_schedule`, `prop_queue_reentry_head_blocks` |
| U9 | `ShadowedNoPendingMatch` | While an element is in flight, a matching pair of heads needs an element in flight that precedes the head of its side (SHADOW). | `prop_queue_shadow_invariant_with_inflight` |
| U10 | `ReissueKeepsOrd` | Every persistent element in flight carries the order of its first round. | `prop_queue_reissue_keeps_first_round_ord`, `prop_queue_loop_bounded` |
| U11 | `IntroducedOnce` | Each persistent element takes a fresh order exactly once. | `prop_queue_persistent_reissue_charged_once`, `prop_queue_reissue_keeps_first_round_ord` |
| U12 | `LeaveOrder` | The records that are not persistent leave in increasing order, each older than every such record that stays. | `prop_queue_fifo_any_schedule`, `prop_queue_fifo_with_reentries` |

U1 and U9 together state SHADOW in every state.
U1 replaces `HeadOnlyNoPendingMatch` of P4, which is U1 when nothing can be in flight.
U3 now states the fire order of each side, and U12 states the leave order that U3 stated before decision 43.
The inductive invariant `HeadIndInv` holds the seven invariants and the facts that make them inductive.
The orders and the carried orders are below the counter and unique, and the carried orders differ from the stored orders.
The elements in flight have their shapes, and history flags record that every fire so far kept U2, U3, and U12.

#### 11.4.2 CounterConflict.tla

[CounterConflict.tla](../../../formal/tlaplus/reified_rspaces/CounterConflict.tla) runs two branches of one block from one pre-state with the queues `q1` and `q2`.
Each branch runs one deploy of one or two operations: an enqueue, a dequeue, or a peek.
The merge then runs check 1 of the baseline, and without a race it applies the changes of both branches.

*Table 34. The invariants of CounterConflict.tla.*

| Id | Invariant | Meaning | Property tests |
|---|---|---|---|
| U4 | `NoMergedOrderFabrication` | When check 1 finds no race, the merged queues and the observations of both deploys are those of a serial run. | `prop_merged_queue_state_is_serial` |
| None | `SharedQueueRaces` | Two branches that use one queue race. This invariant is auxiliary, and Q21 and Q22 state it in Rocq. | None |
| None | `FirstOpsRace` | Two branches that open queues of one space race on the space counter. This invariant is auxiliary, and Q23 states it in Rocq. | None |

#### 11.4.3 NativeHandles.tla

[NativeHandles.tla](../../../formal/tlaplus/reified_rspaces/NativeHandles.tla) models factory calls, handle calls on live and fabricated spaces, restarts, and the merge of handle calls across blocks.

*Table 35. The invariants of NativeHandles.tla.*

| Id | Invariant | Meaning | Property tests |
|---|---|---|---|
| U5 | `HandleUnique` | One config record and one handle continuation for each created space id, and none for the others. | `prop_handle_unique` |
| U6 | `DanglingIsDeterministicError` | A call aborts with `DanglingHandle` exactly when its space has no config record. | `prop_dangling_deterministic_error` |
| U7 | `DurableSurvivesRestart` | Every space that existed at a restart still has its config record and its handle. | `prop_handle_survives_checkpoint_reset` |
| U8 | `HandleCallsMergeLikeContracts` | Two calls on disjoint collections of live spaces never conflict, and two calls on one opened collection race on its counter. | `prop_handle_calls_merge_like_contracts` |

The inductive invariant `HandleIndInv` holds the four invariants and the facts that make them inductive.
Live spaces are durable, and only live spaces have collection counters.
No counter or log grows faster than the steps.

#### 11.4.4 Configurations and results

*Table 36. The safe configurations of P4 and their results in the last host run.*

| Configuration | Bounds | Distinct states | Depth |
|---|---|---|---|
| `MC_HeadOnlyDiscipline.cfg` | `Vals={"a","b"}`, `Pats={"a","b","any"}`, `MaxOps=6`, `Reissues=FALSE` | 9059 | 7 |
| `MC_HeadOnlyDiscipline_Reissue.cfg` | `Vals={"a","b"}`, `Pats={"a","b","any"}`, `MaxOps=6`, `Reissues=TRUE` | 3473357 | 7 |
| `MC_CounterConflict.cfg` | `Qs={"q1","q2"}`, `Vals={"x","y"}`, pre-state queues unopened, `[x]`, or `[y]`, 1 or 2 operations in each branch, `WriteCounters=TRUE`, `ReadCounters=TRUE` | 46656 | 1 |
| `MC_NativeHandles.cfg` | `Rands={"r1","r2"}`, `Sids={"s1","s2","s3"}` with `s3` fabricated, `Colls={"k1","k2"}`, `MaxSteps=5`, `SingleChannelHandle=TRUE` | 2617 | 6 |

CounterConflict.tla checks the merge of each pair of branches once, so every state is an initial state, and TLC reports the depth 1.

*Table 37. The Unsafe controls of P4 and their complements.*

| Configuration | Wrong rule | Refutes exactly | Counterexample | Complement |
|---|---|---|---|---|
| `HeadOnlyScanUnsafe.cfg` | `ScanWaiters=TRUE`: a datum fires the first waiter that matches, as the legacy scan does | `HeadBlocks` | 71 states, depth 4 | 7899 states, depth 7, no other violation |
| `ReissueFreshOrdUnsafe.cfg` | `ReissueFreshOrd=TRUE`: each round of a re-issued element takes a fresh order (C0) | `IntroducedOnce` | 243 states, depth 4 | 3524808 states, depth 7, no other violation |
| `ReentryAtHeadUnsafe.cfg` | `ReentryAtHead=TRUE`, `MaxOps=7`: a re-entry is at the head whatever the order of the head, when the other side holds at most one entry | `FifoOrder` | 340728 states, depth 7 | 44207717 states, depth 8, no other violation |
| `NonPersistentFirstOrdUnsafe.cfg` | `NonPersistentFirstOrd=TRUE`: a re-issued element that is not persistent keeps its first order (C1); the configuration does not check `LeaveOrder` | `QuiescentNoPendingMatch` | 2857991 states, depth 7 | 3486737 states, depth 7, no other violation |
| `CounterDroppedUnsafe.cfg` | No operation uses a counter, and an enqueue takes the greatest order of the queue plus 1 | `NoMergedOrderFabrication` | An initial state: TLC prints `?` for the counts | 46656 states, depth 1, no other violation |
| `PeekCounterFreeUnsafe.cfg` | A peek does not bump the counter, which is the design before decision 31 | `NoMergedOrderFabrication` | An initial state: TLC prints `?` for the counts | 46656 states, depth 1, no other violation |
| `JoinDescriptorUnsafe.cfg` | The handle continuation joins $`H(\mathit{sid})`$ with a descriptor channel | `HandleCallsMergeLikeContracts` | 52 states, depth 4 | 2617 states, depth 6, no other violation |

The control `NonPersistentFirstOrdUnsafe.cfg` refutes exactly one invariant at its bound.
A scratch search with five operation kinds found a run of 10 rounds that also breaks `ShadowedNoPendingMatch`.
The complements at 6 and 7 rounds find no other violation (3,486,737 and 44,442,255 states).
A restriction that removed the longer run would have to stop other elements from entering flight, so the control stays unrestricted at its bound.

*Table 38. The Apalache checks of P4.*

| Check | Configuration | Length | Expected outcome |
|---|---|---|---|
| `head-base` | `MC_HeadOnlyDiscipline_IndBase_Apalache.cfg`, `MaxOps=3` | 0 | `NoError`: $`\mathit{Init} \Rightarrow \mathit{HeadIndInv}`$ |
| `head-step` | `MC_HeadOnlyDiscipline_IndStep_Apalache.cfg`, `MaxOps=3` | 1 | `NoError`: $`\mathit{HeadIndInv} \land \mathit{Next} \Rightarrow \mathit{HeadIndInv}'`$ |
| `head-step-scan-unsafe` | `HeadOnlyScanUnsafe_IndStep_Apalache.cfg` | 1 | An error in the conjunct `~partnersAtHead` |
| `head-reissue-base` | `MC_HeadOnlyDiscipline_Reissue_IndBase_Apalache.cfg`, `MaxOps=7` | 0 | `NoError`: $`\mathit{Init} \Rightarrow \mathit{HeadIndInv}`$ |
| `head-reissue-step` | `MC_HeadOnlyDiscipline_Reissue_IndStep_Apalache.cfg`, `MaxOps=7` | 1 | `NoError`: $`\mathit{HeadIndInv} \land \mathit{Next} \Rightarrow \mathit{HeadIndInv}'`$ |
| `handle-base` | `MC_NativeHandles_IndBase_Apalache.cfg`, `MaxSteps=4` | 0 | `NoError`: $`\mathit{Init} \Rightarrow \mathit{HandleIndInv}`$ |
| `handle-step` | `MC_NativeHandles_IndStep_Apalache.cfg`, `MaxSteps=4` | 1 | `NoError`: $`\mathit{HandleIndInv} \land \mathit{Next} \Rightarrow \mathit{HandleIndInv}'`$ |
| `handle-step-joindescriptor-unsafe` | `JoinDescriptorUnsafe_IndStep_Apalache.cfg` | 1 | An error in the conjunct with the marker `"user"` |

### 11.5 Negative controls of P4

The script `scripts/verify-reified/rocq-negative-controls.py` holds 75 mutants. Tables 12, 39, 45, 47, 50, 55 and 59 hold 29, 29, 5, 1, 3, 3 and 5 of them.

The run after the merge of [decision 44](decision-register.md#decision-44) passed all 57 mutants, after the 9 pristine modules compiled (`target/verification/reified-rspaces/merge-car/proofs-2.log`).
The run of [decision 42](decision-register.md#decision-42) passed all 62 mutants, after the 10 pristine modules compiled (`target/verification/reified-rspaces/d42/proofs.log`).
The run of [decision 43](decision-register.md#decision-43) passed all 66 mutants, after the 10 pristine modules compiled (`target/verification/reified-rspaces/d43/gates/proofs.log`).
The run of [decision 45](decision-register.md#decision-45) passed all 70 mutants, after the 10 pristine modules compiled (`target/verification/reified-rspaces/d45/gates/proofs.log`).
The run of its Option P passed all 75 mutants, after the 11 pristine modules compiled (`target/verification/reified-rspaces/d45/option-p-gates/proofs.log`).

*Table 39. The 29 Rocq mutants of P4.*

| Id | Module | What the mutant changes | Must fail in |
|---|---|---|---|
| `ns-ephemeral-bit-dropped` | Namespace.v | An ephemeral id has the durable KIND byte. | `scoped_kind_byte_spec` |
| `ns-op-ids-collide` | Namespace.v | `getSpaceAgent` takes the op of the engine factory. | `op_ids_pairwise_distinct` |
| `ns-short-sid-accepted` | Namespace.v | The decoder accepts a payload shorter than a space id. | `decode_tag_cons` |
| `re-tag-flag-ignored` | RecordEnvelope.v | The tag test accepts a tag with the flag true. | `is_tag_iff` |
| `re-list-flags-unchecked` | RecordEnvelope.v | The list decoder does not check the derived flag of the holder `Par`. | `list_elements_sound` |
| `re-datum-kind-unchecked` | RecordEnvelope.v | The order of a datum does not check the kind of the tag. | `datum_unwrap_sound` |
| `re-negative-order` | RecordEnvelope.v | The order decoder accepts a negative order. | `natural_of_sound` |
| `cc-counter-comm-peeks` | CounterConflict.v | The counter COMM of an operation has peeks, so check 1 does not see it. | `counter_consumed` |
| `cc-first-op-skips-space-counter` | CounterConflict.v | The first operation opens the collection counter without the COMM on the space counter. | `space_counter_consumed` |
| `cc-reified-mergeable` | CounterConflict.v | Reified channels are mergeable, so check 1 drops their races. | `no_mergeable_produces` |
| `cc-pre-state-any-creator` | CounterConflict.v | Every counter produce counts as a pre-state produce, so the chain disappears. | `same_block_ops_chain` |
| `ho-head-greatest-ord` | HeadOnlyDisciplines.v | The head is the entry with the greatest order. | `precedes_false_le` |
| `ho-datum-skips-tail` | HeadOnlyDisciplines.v | A datum skips the queued records and meets the head waiter. | `queue_produce_preserves_inv` |
| `ho-no-divergent-reissue` | HeadOnlyDisciplines.v | A fire whose partner stays continues instead of the re-issue. | `after_fire_continue` |
| `ho-counter-not-bumped` | HeadOnlyDisciplines.v | An operation does not bump the counter, so two operations share an order. | `bump_ord_next` |
| `sr-handle-ignores-write-flag` | SpaceRefs.v | A handle under a bundle without the write flag is a native space. | `classify_space_ref_total_deterministic` |
| `sr-forged-ignores-context` | SpaceRefs.v | A forged channel routes to the default space inside a `use` block. | `route_total_deterministic` |
| `sr-space-name-takes-context` | SpaceRefs.v | A space name routes to the space of the use context. | `route_total_deterministic` |
| `sr-wrapper-drops-context` | SpaceRefs.v | A stored body is not wrapped, so it runs without its context. | `use_wrapper_restores_ctx` |
| `sr-any-reply-name` | SpaceRefs.v | The handle replies on a value that is not a reply name. | `parse_message_ret` |
| `sf-hashset-takes-default` | SpaceFactory.v | The HashSet store takes every qualifier. | `qualifier_matrix` |
| `sf-family-index-collision` | SpaceFactory.v | The PathMap families start at the index 6, so two families share an index. | `from_family_index_of` |
| `sf-ann-not-a-call-op` | SpaceFactory.v | The op `ann` takes no calls, so a persistent send to it does not abort. P6 step 7 replaced `sf-vector-not-a-call-op`, because the call ops now end at `ann`. | `persistent_op_call_aborts` |
| `sf-recreate-allowed` | SpaceFactory.v | A creation over an existing config record stages a second space. | `space_already_exists_is_deterministic_abort` |
| `sf-theory-before-qualifier` | SpaceFactory.v | The encoder puts the theory before the qualifier. | `constructor_parse_canonical` |
| `sf-qualifier-bits-unchecked` | SpaceFactory.v | `qualifier_of` does not check the `locally_free` bits, so a qualifier element with bits parses. | `constructor_parse_sound` |
| `enc-refresh-keeps-stale-flags` | Encoding.v | The hooks decode without the refresh. | `refresh_strict_decode_erase` |
| `enc-use-flag-ignores-body` | Encoding.v | The derived flag of a USE tuple ignores the flag of the body. | `connective_marker_tuple_recognized` |
| `enc-strict-check-dropped` | Encoding.v | The strict decoder ignores the flags. | `refresh_strict_decode_erase` |

The TLA+ controls of P4 are the four Unsafe configurations of Table 37 and the two Apalache Error controls of Table 38.

### 11.6 Property-based tests

The 78 properties of `rholang/tests/reified_p4_props.rs` run the real code of P4.
A pure property and a property over the in-memory agent run 64 cases.
A property that evaluates deploys on a Rholang runtime runs 16 cases.
Every configuration fixes the seed `0x0E0D_2026_1005_4E71` and sets `failure_persistence: None`, and the runtime configuration limits the shrink to 64 iterations (`reified_p4_props.rs:151-176`).
So a run has the same cases on every host, and it writes nothing to the source tree.
The 3 properties of `rholang/tests/reified_legacy_charges.rs` (section [11.14](#1114-legacy-charges-of-decision-45)) run 64 cases each, with the seed `0x0E0D_2026_1006_0045`.

*Table 40. The 87 properties of phase P4 in the invariant map.*

| Group | Properties | Location | Covers |
|---|---|---|---|
| Namespace | `prop_kind_byte_injective` to `prop_canonical_inner_bytes_respects_par_eq` (11) | `reified_p4_props.rs:536-930` | Q1 to Q11 |
| Record envelopes | `prop_datum_envelope_roundtrip` to `prop_envelope_unknown_version_rejected` (8) | `reified_p4_props.rs:1404-1739` | Q12 to Q19 |
| Counters | `prop_check1_restatement_matches_merger` (`reified_p4_props.rs:2072`) and `prop_queue_writes_race_on_counter` to `prop_counter_free_peek_admits_write_skew` (8) | `reified_p4_props.rs:5456-5880` | Q20 to Q28 |
| Queue | `prop_queue_candidates_head_only` to `prop_legacy_scan_breaks_head_blocks` (9) | `reified_p4_props.rs:3019-3468` | Q29 to Q37, Q62 and Q63 for `prop_queue_plan_permutation_invariant`, and U10 for `prop_queue_loop_bounded` |
| Metering | `prop_agent_reservation_cut_rejects_without_effect` to `prop_agent_reserves_its_own_work_beyond_the_matcher` (3) | `reified_p4_props.rs:4528-4644` | Q59 to Q61, Q64 to Q66 |
| References and routing | `prop_classify_space_ref_total` to `prop_use_wrapper_restores_ctx` (9) | `reified_p4_props.rs:6235-6523` | Q38 to Q46 |
| Factories | `prop_family_index_bijective` to `prop_space_already_exists_is_deterministic_abort` (8) | `reified_p4_props.rs:6762-7131` | Q48 to Q55 |
| TLA+ invariants | `prop_queue_no_pending_match_any_schedule` to `prop_handle_calls_merge_like_contracts` (8) | `reified_p4_props.rs:7397-7973` | U1 to U8, and U12 |
| Leaf extras | `prop_handle_dispatch_total`, `prop_get_space_agent_returns_family_capability`, `prop_qualifier_parse_roundtrip`, `prop_prepare_commit_atomic` (4) | `reified_p4_props.rs:8382-8655` | Q47 to Q51, Q55 |
| Re-entry (decision 43) | `prop_queue_reissue_keeps_first_round_ord` to `prop_queue_persistent_reissue_charged_once` (8) | `reified_p4_props.rs:3782-4364` | Q68 to Q77, U1 to U3, and U9 to U12 |
| Peek re-store (decision 45) | `prop_queue_restore_realizes_plan` (1) | `reified_p4_props.rs:4716` | Q78 |
| Legacy charges (decision 45, Option P) | `prop_legacy_charges_equal_charging_rspace`, `prop_legacy_chain_log_is_host_order`, `prop_legacy_replay_stops_with_play` (3) | `reified_legacy_charges.rs:173-783` | Q79 and Q80, U13 for the last two |
| Native execution | `reified_native_play_runs_a_space_deploy`, `reified_native_replay_reproduces_a_space_deploy`, `reified_native_host_program_plays_and_replays_unchanged` (3) | `reified_native_fail_closed.rs:338-412` | Q67 |
| I-20 | `marker_forging_grants_no_capability` (`reified_marker_recognition.rs:660`), `reified_encoding_roundtrip` (`reified_encoding.rs:158`), `reified_decode_commutes_with_substitution` (`reified_marker_recognition.rs:1188`) (3) | Two test files of P2 | Q56 to Q58 |

*Table 41. The 27 witness tests of phase P4.*

| Test | Location | Covers |
|---|---|---|
| `reified_marker_flags_change_derived_flags_only` | `reified/reducer_hooks.rs:1961` | Q56 |
| `reified_marker_flags_use_body_with_connective_send_is_canonical` | `reified/reducer_hooks.rs:1726` | Q57 |
| `reified_marker_flags_variable_bound_to_connective_is_canonical` | `reified/reducer_hooks.rs:1776` | Q58 |
| `space_already_exists_aborts` | `reified/factory.rs:193` | Q55 |
| `persistent_sends_abort_on_every_call_op_and_every_handle` | `reified/system_channels.rs:667` | Q54 |
| `forged_channels_route_to_the_context_space` | `reified/route.rs:283` | Q42 |
| `agent_tagged_names_route_to_the_agent` | `reified/route.rs:312` | Q42 |
| `space_names_route_intrinsically` | `reified/route.rs:269` | Q43 |
| `ordinary_unforgeable_names_route_to_the_default_space` | `reified/route.rs:348` | Q45 |
| `handle_under_a_bundle_without_write_flag_is_not_writable` | `reified/scope.rs:198` | Q39 |
| `other_reified_ids_are_not_space_refs_and_other_values_are_agents` | `reified/scope.rs:212` | Q40 |
| `wrap_body_restores_the_context_as_a_use_tuple` | `reified/scope.rs:257` | Q46 |
| `scan_meter_copy_equals_clone_backing_copy_and_cleanup` | `reified/costs.rs:461` | Q59 |
| `space_matching_reserves_only_what_the_matcher_reads` | `spaces/matching.rs:249` | Q59 |
| `space_counter_bump_cut_budget_rejects_without_effect` | `spaces/counter.rs:743` | Q61 |
| `space_queue_peek_restores_the_head_record` | `spaces/generic.rs:3412` | Q78 |
| `space_queue_peek_of_a_persistent_record_restores_nothing` | `spaces/generic.rs:3485` | Q78 |
| `legacy_charges_of_a_store_and_a_park` | `reified/legacy_charges.rs:416` | Q79 |
| `legacy_charges_of_a_produce_fire_follow_handle_result` | `reified/legacy_charges.rs:442` | Q79 |
| `legacy_charges_of_a_consume_fire_refund_the_consume` | `reified/legacy_charges.rs:504` | Q79 |
| `legacy_charges_of_a_continuation_without_a_body_fail_closed` | `reified/legacy_charges.rs:546` | Q79 |
| `legacy_chain_is_active_only_in_a_metered_legacy_deploy` | `reified/legacy_charges.rs:567` | Q79, Q80 |
| `legacy_chain_charges_in_host_order` | `reified/legacy_charges.rs:632` | Q79 |
| `legacy_chain_pre_charge_stops_before_its_operation` | `reified/legacy_charges.rs:661` | Q79 |
| `legacy_chain_tail_fails_after_the_commit` | `reified/legacy_charges.rs:708` | Q79 |
| `space_queue_legacy_replay_equal` | `reified_queue_replay.rs:1001` | Q79, Q80 |
| `space_queue_legacy_out_of_phlo_sweep_replays_equal` | `reified_queue_replay.rs:862` | Q80 |

After [decision 46](decision-register.md#decision-46), the invariant-map checker printed `invariant-map: PASS=213 FAIL=0 MISSING=0 PENDING=0` for the phase `P4`, and `PASS=333` for all phases (`target/verification/reified-rspaces/d46/invariant-map-p4.log`, `invariant-map-all.log`).
The P4 entries of the map have the test status `required`, because every P4 test exists and passes.
The model-map checker printed `model-map: PASS=187 FAIL=0 PENDING=0` for the P1 map and `model-map: PASS=442 FAIL=0 PENDING=0` for the P4 map (`model-map-p1.log`, `model-map-p4.log` of the same directory).

### 11.7 Integration suites

Table 42 gives the P4 suites, with the exact counts of the last host run.
Each run printed `test result: ok` with 0 failures (`target/verification/reified-rspaces/d45/option-p-gates/`).

*Table 42. The integration suites of P4.*

| Suite | Tests | Subject |
|---|---|---|
| `models/tests/space_factory_urn.rs` | 6 | URN forms, aliases, the constructor, the qualifier matrix, and the family indices |
| `rholang/tests/reified_factory.rs` | 14 | Private channels, the creation records, the factory gates, `assertEqual`, `getSpaceAgent`, fixed arities, and persistent sends |
| `rholang/tests/reified_handle.rs` | 12 | The five methods, typed replies, the drop rule, the bare-handle rule, dangling handles, and persistent sends |
| `rholang/tests/reified_queue.rs` | 22 | Head-only delivery, the counters, the order envelope, re-issues and re-entries, divergent pairs, observer parity, the rejected joins, queries, and params, dangling names, the meta channels, and item 19 |
| `rholang/tests/reified_queue_replay.rs` | 7 of P4 | Play and replay equality with handle calls, peeks, and parked waiters, a typed divergence, reporting, and the two legacy tests of section [11.14](#1114-legacy-charges-of-decision-45) |
| `rholang/tests/reified_queue_restart.rs` | 2 | Orders and handles across a restart and a replay |
| `rholang/tests/reified_routing.rs` | 15 | The routing classes and rules, the use closure, R7, a reset, the agent gate, ordinary programs, and the unwrap of handles |
| `rholang/tests/reified_merge.rs` | 11 | Nine counter races and chains, and the two checks of item 18 |
| `rholang/tests/reified_queue_source.rs` | 1 | A source program end to end |
| `rholang/tests/reified_p4_props.rs` | 78 | The properties of section [11.6](#116-property-based-tests) and of decisions 43 and 45 |
| `rholang/tests/reified_p4_smoke.rs` | 1 | A smoke test of play and replay |
| `rholang/tests/reified_native_fail_closed.rs` | 3 | Native play and native replay of a space deploy and of a host program (section [11.11](#1111-native-execution-of-decisions-46-and-47)) |
| `rholang/tests/reified_legacy_charges.rs` | 3 | The legacy charges of Option P against `ChargingRSpace` and in replay (section [11.14](#1114-legacy-charges-of-decision-45)) |

The suite `reified_queue_replay.rs` also holds `space_queue_out_of_phlo_sweep_replays_equal`, the regression test of the [replay finding](findings/space-replay-out-of-phlo.md), so its last run printed 8 tests.

The P4 modules hold 198 unit tests: 191 in `rholang/src/rust/interpreter/reified/` and `spaces/`, and 7 in `models/src/rust/space_factory_urn.rs`.
Two of them came with the merge of [decision 44](decision-register.md#decision-44) (section [8.8](#88-the-merge-of-featurecost-accounted-rho)).
[Decision 42](decision-register.md#decision-42) replaced four unit tests that pinned the old meter and added eight (section [11.10](#1110-metering-of-decision-42)).
[Decision 43](decision-register.md#decision-43) added eight unit tests of the re-entry (section [11.12](#1112-re-entry-of-decision-43)), and decision 45 added ten.
At the P4 milestone, the library run of `rholang` printed `test result: ok. 1660 passed; 0 failed; 3 ignored`, with the 166 P4 tests of that time (`gate-rholang-lib.log` of the same directory).
The suites of ordinary execution of section 8 also passed in the same run.

### 11.8 Checklist items, footprint, and completeness

**Items 18 and 19.** At the milestone `p4`, the scheduled checks of Table 21 are active, and the footprint checks of P1 and the P2 recognition tests are retired.

*Table 43. The checks of items 18 and 19 at the milestone p4.*

| # | Item | Check | Assertion |
|---|---|---|---|
| 18 | No synthesized multi-channel join | `reified_handle_cross_block_merge_no_conflict` (`reified_merge.rs:515`) and its control `reified_handle_cross_block_merge_join_handle_conflicts` (`reified_merge.rs:571`), step `reified-merge` | Handle calls on disjoint collections merge like contract calls. A join of the handle with a descriptor channel makes every pair of calls conflict. |
| 19 | Namespaced channels never mergeable | `reified_namespaced_channels_never_mergeable` (`reified_queue.rs:1889`), step `reified-queue` | A tuple channel with a mergeable tag routes to the space inside a `use` block, and only the control channel outside is mergeable. No channel with the tag `0xE0` is mergeable. |

The checklist checker printed `consensus-checklist (milestone p4): ITEMS=22 CHECKS=118 PASS=112 FAIL=0 SCHEDULED=1 RETIRED=5` (`target/verification/reified-rspaces/p4/fx-c/gate-consensus-checklist-p4.log`).
The scheduled check is the P7 sweep test of item 20.

**The footprint policy `p4`.** The policy extends the policy `campaign`, so every other base file stays unchanged.
It holds one cumulative edit for each base file that P1, P2, and P4 change, such as the hook lines of `reduce.rs` and `rho_runtime.rs`.
Each new file must be an exact path of its list `allowed_new_files`.
The check passes with `footprint policy p4: checked <n> files, 0 violations`.

**The completeness phase `p4`.** The phase lists the 35 P4 sources, 76 entry points, 13 test files, and 2 property files.
`check-complete.py --phase p4` finds each entry point and no forbidden token, and it printed `complete phase p4: 242401 checks, 0 findings` (`target/verification/reified-rspaces/p4/pc/gate-complete-p4.log`).
The production scan printed `complete production scan: 826 files, 0 findings`.

### 11.9 Running the P4 checks

To run the P4 checks, do these steps from the repository root, after the preparation of section [10.1](#101-preparation):

1. Run `scripts/check-reified-rspaces-proofs.sh` and `scripts/check-reified-rspaces-tla-invariants.sh`.
2. Run `python3 scripts/verify-reified/check-invariant-map.py --phase P4`.
3. Run `python3 scripts/verify-reified/check-model-map.py --map formal/reified-rspaces-model-map-p4.json`.
4. Run `python3 scripts/verify-reified/check-complete.py --phase p4` and `check-complete.py --production-scan`.
5. Run `python3 scripts/verify-reified/check-footprint.py --policy p4`.
6. Run `python3 scripts/verify-reified/check-consensus-checklist.py --milestone p4`.
7. Run `cargo test -p rholang --test <suite>` for each `rholang` suite of Table 42.
8. Run `cargo test -p models --test space_factory_urn` and `cargo test -p rholang --lib`.
9. Run `python3 scripts/verify-reified/doc_lint.py` and `docs/rspace/reified-rspaces/diagrams/render.sh --check`.

Run each cargo command under the resource limits of section [10.3](#103-rust-suites), and write its log under `target/`.

### 11.10 Metering of decision 42

[Decision 42](decision-register.md#decision-42) aligns the host work of the queue agent with DR-88 of `feature/cost-accounted-rho`.
The agent reserves each of its own reads and copies on the meter of the job before it makes them.
It prepays the work of the outcome at staging.
The module `formal/rocq/reified_rspaces/theories/AgentReadCoverage.v` restates the metering rule of that branch.
The rule is prefix coverage: every prefix of a run reads at most the units that the same prefix has reserved.

```math
\forall\, p, s.\; t = p \mathbin{+\!\!+} s \;\Longrightarrow\; \sum_{e \in p} \mathrm{read}(e) \;\le\; \sum_{e \in p} \mathrm{reserved}(e)
```

Here $`t`$ is the event list of one operation, $`p`$ is a prefix of $`t`$, and $`\mathrm{read}`$ and $`\mathrm{reserved}`$ give the units of one event.
The module proves the 8 headlines of Table 44, with no axiom.
The gate run printed `PASS headlines AgentReadCoverage: 8 x 'Closed under the global context'`, and `rocqchk` reported `Axioms <none>` (`target/verification/reified-rspaces/d42/proofs.log`).

*Table 44. The headlines of AgentReadCoverage.v.*

| Id | Headline | Claim |
|---|---|---|
| Q59 | `operation_trace_covered` | Every collection operation satisfies prefix coverage, and the reads of the outcome after the commit are prepaid. |
| Q60 | `reservations_precede_commit` | The work after the commit reserves nothing. |
| Q61 | `cut_rejects_before_commit` | A staged reservation exceeds every budget below the reservations of the operation, so the operation fails before the commit. |
| Q62 | `scan_charge_permutation_invariant` | The charge of a head scan does not depend on the list order of its side. |
| Q63 | `operation_charge_permutation_invariant` | The charge of an operation does not depend on the list order of the staged view. |
| Q64 | `scan_charge_independent_of_unread_payloads` | The payloads that a scan does not copy do not change its charge. |
| Q65 | `p4_scan_covered_only_if_free` | Negative control: the P4 scan satisfies the rule only when its own work is zero. |
| Q66 | `unprepaid_trace_uncovered` | Negative control: without the prepaid reservations, the outcome after the commit breaks the rule. |

Table 45 gives the five mutants of the module.
Each mutant reverts one part of the rule, and the gate requires that it fails in the expected declaration.

*Table 45. The Rocq mutants of decision 42.*

| Id | What the mutant changes | Must fail in |
|---|---|---|
| `arc-read-before-reserve` | A step reads before it reserves, as the P4 code copied first. | `step_covered` |
| `arc-unwrap-not-prepaid` | A fired round reserves nothing for the decode of its continuation. | `round_staged_totals` |
| `arc-outcome-not-prepaid` | The operation does not prepay the buffer of the outcome. | `staged_totals` |
| `arc-outcome-reserves-after-commit` | The outcome reserves its buffer after the commit. | `operation_trace_covered` |
| `arc-scan-copies-every-entry` | A scan copies every entry instead of the head only. | `scan_reserved` |

Table 46 gives the tests of decision 42.
The properties run 64 cases each with the seed of section [11.6](#116-property-based-tests).

*Table 46. The tests of decision 42.*

| Test | Location | What it checks |
|---|---|---|
| `prop_agent_reservation_cut_rejects_without_effect` | `reified_p4_props.rs:4528` | The exact usage of an operation commits. One unit less in any charged dimension fails with `HostWorkRejected`, logs no event, and leaves the collection unchanged (Q59 to Q61, Q66). |
| `prop_agent_charge_independent_of_unread_payloads` | `reified_p4_props.rs:4579` | Padding of up to 4 KiB on the entries that an operation does not meet leaves its usage unchanged (Q64). |
| `prop_agent_reserves_its_own_work_beyond_the_matcher` | `reified_p4_props.rs:4644` | A forwarding meter records the reservations of the matcher, and the agent reserves more in each charged dimension (Q65). |
| `prop_queue_plan_permutation_invariant` | `reified_p4_props.rs:3417` | Two list orders of one collection give equal usage in every dimension (Q62, Q63). |
| `prop_queue_candidates_head_only` | `reified_p4_props.rs:3019` | A counting matcher bounds the matcher calls, because no charge counts them now, and the usage of `SearchCandidates` is zero. |
| `scan_meter_maps_amounts_to_the_branch_dimensions` | `reified/costs.rs:358` | A reservation charges VerificationOperations, VerificationBytes, and SearchStateBytes, and no other dimension. |
| `scan_meter_accepts_exact_credit_and_rejects_each_smaller_dimension` | `reified/costs.rs:432` | The exact credit suffices, and one unit less in any dimension rejects. |
| `scan_meter_copy_equals_clone_backing_copy_and_cleanup` | `reified/costs.rs:461` | A reserved copy charges the triple of the copy walk of the branch (Q59). |
| `scan_meter_skips_walks_without_a_budget` | `reified/costs.rs:495` | Without a budget, every reservation passes, and a metered encode gives the bytes of bincode. |
| `metered_matching_reserves_on_the_branch_dimensions` | `reified/costs.rs:517` | The metered matcher charges the three dimensions and no `SearchCandidates`. |
| `space_matching_reserves_only_what_the_matcher_reads` | `spaces/matching.rs:249` | `bind` adds nothing to the reservations of the matcher, and an unguarded `admits` charges one operation, whatever the size of the body (Q59). |
| `legacy_mapping_dropped_operations_and_backing` | `spaces/matching.rs:300` | Negative control: the old mapping of the meter kept only the scanned bytes of the matcher. |
| `space_counter_bump_reserves_its_work_on_the_branch_dimensions` | `spaces/counter.rs:719` | A bump charges the three dimensions and no `SearchCandidates`, and a zero budget rejects it. |
| `space_counter_bump_cut_budget_rejects_without_effect` | `spaces/counter.rs:743` | A cut budget rejects a bump before its commit, with no event and no change of the counters (Q61). |

Decision 42 changes no state, no event, and no outcome of an operation.
So the models of sections [11.3](#113-rocq-models-of-p4) and [11.4](#114-tla-models-of-p4) do not change.
The suites of Table 42 pass with their earlier assertions.

### 11.11 Native execution of decisions 46 and 47

[Decision 46](decision-register.md#decision-46) made spaces fail closed in native execution: native play and native replay rejected the first space operation of a deploy with `SpacesUnsupportedInNativeExecution`.
[Decision 47](decision-register.md#decision-47) replaces that rule, and native play and native replay now run spaces.
Native play stages and commits a space operation with the metered reads of the substrate.
Native replay runs each staged operation against its journal row through the native backend of the native replay session.
Section [13](#13-decision-47-spaces-in-native-execution) verifies the native backend, the view charges, and the scheduling.

The headline Q67, `native_execution_runs_spaces` of `SpaceRefs.v`, states the availability rule of decision 47.
Native play runs a space entry as legacy execution runs it, and a bound native replay runtime runs it as native play.
A native replay runtime that is not bound yet, and a detached runtime, stage nothing and fail with `RuntimeUnavailable`.
The headline `native_execution_fails_closed` of decision 46 stays in `SpaceRefs.v` as a comment.

*Table 47. The Rocq mutants of decision 47.*

| Id | What the mutant changes | Must fail in |
|---|---|---|
| `sr-native-play-rejects-spaces` | A host runtime rejects space jobs in native play, as decision 46 did. | `native_execution_runs_spaces` |
| `sr-unbound-native-replay-runs` | A native replay runtime runs space jobs before its environment binds it. | `native_execution_runs_spaces` |

The mutant of decision 46, `sr-native-play-runs-spaces`, is retired with its headline, because it states the rule of decision 47.

*Table 48. The tests of native execution.*

| Test | Location | What it checks |
|---|---|---|
| `reified_native_play_runs_a_space_deploy` | `reified_native_fail_closed.rs:338` | A native play of a factory call runs without an error and stores the records of the config and of the space counter (Q67). |
| `reified_native_replay_reproduces_a_space_deploy` | `reified_native_fail_closed.rs:384` | The native replay environment replays the play journal: no error, a complete journal, the phlo usage of play, equal authority events and byte observations, and the post-state root of play (Q67). |
| `reified_native_host_program_plays_and_replays_unchanged` | `reified_native_fail_closed.rs:412` | A program without spaces plays and replays natively without an error, with a complete journal, equal usage and the post-state root of play (Q67). |
| `native_replay_runtime_fails_closed` | `reified/runtime.rs:590` | A native replay runtime that is not bound yet fails every space entry with `RuntimeUnavailable`. |
| `native_replay_runtime_binds_once_and_resets_its_scope` | `reified/runtime.rs:608` | The environment binds a native replay runtime once, and its evaluation scope resets the committed parents of its store. |
| `host_runtime_is_available_outside_native_execution` | `reified/runtime.rs:647` | A host runtime runs space jobs when its budget runs no native deploy. |

**The hazard of decision 46.** A run of the suite with the guard of `ReifiedRuntime::host` removed and without a native backend (`target/verification/reified-rspaces/d46/mutant-no-native-guard.log`) shows why decision 46 failed closed.
Native play then ran the space operation without an error, and native replay failed with `native replay has incomplete operations`.
Decision 47 removes the guard and adds the native backend in one change, so native replay replays every journal row of native play.

### 11.12 Re-entry of decision 43

[Decision 43](decision-register.md#decision-43) makes a re-issued persistent operation keep the order of its first round.
The element re-enters at its place.
It is at the head of its side when that side is empty or its order precedes the head.
Otherwise it rests behind the head and fires nothing.
`operation_ord` (`spaces/generic.rs:1715-1743`) checks the kept order against the counter.
`Cx::at_head` (`spaces/generic.rs:918-939`) places the element, and `QueueDiscipline::precedes` (`spaces/disciplines/queue.rs:65-74`) compares two orders.

`HeadOnlyDisciplines.v` adds the 10 headlines of Table 49, with no axiom.
The module defines the elements in flight $`F`$ and the re-entry plans.
It also defines the well-formedness $`\mathrm{wf}'`$ of a collection with its elements in flight, and the shadow form of INV:

```math
\mathrm{SHADOW}(R, W, F) \iff \mathrm{INV}(R, W) \;\lor\; \exists f \in F.\ \mathit{ord}(f) < \mathit{ord}\bigl(\mathrm{head}(X_f)\bigr)
```

Here $`X_f`$ is the side of the element $`f`$.
The module compiles in 6 s, prints `Closed under the global context` for all 19 headlines, and `rocqchk` reports `Axioms <none>`.

*Table 49. The headlines of decision 43 in HeadOnlyDisciplines.v.*

| Id | Headline | Claim |
|---|---|---|
| Q68 | `persistent_reentry_never_spent` (`HeadOnlyDisciplines.v:2342`) | A re-entry never spends a persistent element. It re-issues again, or it ends stored or parked at its place. |
| Q69 | `queue_reissue_keeps_ord` (`HeadOnlyDisciplines.v:2373`) | A re-issued persistent element keeps the order of its first round, and its round bumps the counter by one and keeps $`\mathrm{wf}'`$. |
| Q70 | `queue_reentry_preserves_shadow_inv` (`HeadOnlyDisciplines.v:2712`) | Every fresh operation and every re-entry keeps $`\mathrm{wf}'`$ and SHADOW. |
| Q71 | `queue_quiescent_inv` (`HeadOnlyDisciplines.v:2781`) | SHADOW with no element in flight is INV, and every run of rounds keeps SHADOW and has INV whenever no element is in flight. |
| Q72 | `queue_fifo_with_reentries` (`HeadOnlyDisciplines.v:2946`) | Over runs with re-entries, the records that are not persistent leave in increasing order, each older than every such record that stays. |
| Q73 | `queue_head_blocks_reentry` (`HeadOnlyDisciplines.v:3021`) | A re-entry behind an older head rests at its place and fires nothing, and a fire meets the head of the other side. |
| Q74 | `queue_reentry_loop_bounded_or_reissues` (`HeadOnlyDisciplines.v:3089`) | A re-entry stages at most `FIRE_ROUND_CAP` fires, and a re-issue carries the first order exactly for a persistent element. |
| Q75 | `queue_reentry_permutation_invariant` (`HeadOnlyDisciplines.v:3149`) | A re-entry depends on the entries of the collection, not on their list order. |
| Q76 | `fresh_ord_reissue_loses_first_round_place` (`HeadOnlyDisciplines.v:3503`) | Negative control C0: with a fresh order, a re-issued persistent element rests behind a younger record. |
| Q77 | `nonpersistent_first_ord_reentry_breaks_quiescent_inv` (`HeadOnlyDisciplines.v:3556`) | Negative control C1: an element that is not persistent and keeps its order leaves a pending match with nothing in flight. |

Two proof strategies failed, and the module header records them.
An `injection` or `inversion` on an equation of `Some (offer_loop ...)` ran out of memory, so the proofs use the lemmas `some_injective` and `triple_injective` instead.
A lemma that unfolds SHADOW over the concrete loop never finished its `Qed`, so the lemmas `offer_end_shadow` and `receive_end_shadow` take the outcome as a variable.

*Table 50. The Rocq mutants of decision 43.*

| Id | What the mutant changes | Must fail in |
|---|---|---|
| `ho-reentry-fresh-ord` | A re-issued persistent operation takes the order of its bump. | `queue_reissue_keeps_ord` |
| `ho-reentry-ignores-head` | A re-entered element is at the head whatever the order of the head. | `reenter_offer_shadow`, `queue_reentry_preserves_shadow_inv` |
| `ho-nonpersistent-keeps-ord` | A re-issued element that is not persistent keeps its first order. | `queue_quiescent_inv` |

`HeadOnlyDiscipline.tla` models the re-entry with `Reissues = TRUE`, and Table 33 lists its invariants.
The gate run of decision 43 printed `reified-rspaces tla: PASS=135 FAIL=0 SKIP=0` (`target/verification/reified-rspaces/d43/tla-gate.log`).

*Table 51. The TLA+ checks of decision 43.*

| Check | Configuration | Result |
|---|---|---|
| Safe | `MC_HeadOnlyDiscipline_Reissue.cfg`, `MaxOps=6` | 3,473,357 states, depth 7, no violation of the eight invariants or of `HeadIndInv` |
| Unsafe C0 | `ReissueFreshOrdUnsafe.cfg` | Refutes exactly `IntroducedOnce`, and the complement explores 3,524,808 states |
| Unsafe re-entry | `ReentryAtHeadUnsafe.cfg`, `MaxOps=7` | Refutes exactly `FifoOrder`, and the complement explores 44,207,717 states |
| Unsafe C1 | `NonPersistentFirstOrdUnsafe.cfg` | Refutes exactly `QuiescentNoPendingMatch`, and the complement explores 3,486,737 states |
| Apalache BASE | `MC_HeadOnlyDiscipline_Reissue_IndBase_Apalache.cfg`, `MaxOps=7` | `NoError` |
| Apalache STEP | `MC_HeadOnlyDiscipline_Reissue_IndStep_Apalache.cfg`, `MaxOps=7` | `NoError` |

Four claims of the design changed in the model, and the header of the module records each reason.
`ShadowedNoPendingMatch` holds only while an element is in flight, so it does not repeat `QuiescentNoPendingMatch`.
`ReissueKeepsOrd` states the carried order and `IntroducedOnce` states the rounds, so one control cannot break both.
`FifoOrder` states the fire order of each side, and `LeaveOrder` keeps the leave order.
The control of the re-entry fires out of order only when the other side holds at most one entry, so it breaks no other invariant.

Table 52 gives the tests of decision 43.
The properties run 64 cases each with the seed of section [11.6](#116-property-based-tests), and `prop_queue_persistent_reissue_charged_once` runs 16 cases on a runtime.

*Table 52. The tests of decision 43.*

| Test | Location | What it checks |
|---|---|---|
| `space_queue_divergent_pairs_reissue_after_one_round` | `spaces/generic.rs:3571` | A divergent pair re-issues with the order of its first round in every round, on both sides, and each round bumps the counter. |
| `space_queue_round_cap_reissues_the_rest` | `spaces/generic.rs:3628` | A capped send and a capped receive re-issue with the order of their first round, and the rest rests at that order. |
| `space_queue_reissue_keeps_the_first_round_ord_and_identity` | `spaces/generic.rs:3743` | Every round of a capped persistent send stages the same record, so every produce event on the data channel is equal (Q69). |
| `space_queue_reissue_reenters_ahead_of_younger_records` | `spaces/generic.rs:3784` | A younger datum stored between two rounds stays behind the re-entry, which fires the head waiter. |
| `space_queue_reissue_stores_behind_an_older_reentry` | `spaces/generic.rs:3815` | A re-entry that finds an older entry at the head of its side rests at its place and fires nothing (Q73). |
| `space_queue_shadowed_pair_resolves_at_reentry` | `spaces/generic.rs:3855` | Between two rounds the heads match while the send in flight precedes the head record, and the next round resolves the pair (Q70). |
| `space_queue_nonpersistent_reissue_takes_a_fresh_ord` | `spaces/generic.rs:3900` | A datum that is not persistent re-issues with no order and joins the tail at its new order (Q77). |
| `space_queue_reissued_receive_keeps_its_first_round_ord` | `spaces/generic.rs:3930` | A capped persistent receive re-enters ahead of a younger waiter and parks at its first order. |
| `space_queue_reentry_rejects_an_inconsistent_order` | `spaces/generic.rs:3964` | A kept order outside the orders of the counter, or equal to the order of a stored entry, fails with `Inconsistent` and changes nothing. |
| `space_queue_replay_reproduces_the_reentries` | `spaces/generic.rs:4053` | Replay stages the transactions of a divergent pair and of a capped re-entry, pairs the three equal COMMs of the pair, and reaches the root of play. |
| `space_queue_reissue_persistent_send` | `reified_queue.rs:1043` | In a deploy, a capped persistent send rests at its first order, and its produce events on the data channel are equal. |
| `space_queue_reissue_reenters_ahead_of_younger_records` | `reified_queue.rs:1114` | In a deploy, a body of the first round sends a younger datum, and the second round re-enters ahead of it and fires the head waiter. |
| `space_queue_reissue_keeps_one_produce_identity` | `reified_queue.rs:1163` | A capped persistent send has one introduction row for its one identity, and every observed event has its row. |
| `space_queue_divergent_pair_exhausts_phlo` | `reified_queue.rs:1270` | A divergent pair repeats one produce identity and one COMM, and the counter bumps of its rounds spend the phlo. |
| `prop_queue_loop_bounded` | `reified_p4_props.rs:3308` | A re-issue carries the first order exactly when the operation is persistent (Q74). |
| `prop_queue_reissue_keeps_first_round_ord` | `reified_p4_props.rs:3782` | In runs with re-entries between other operations, every re-issue keeps the first order of a persistent element, every step bumps the counter once, and $`\mathrm{wf}'`$ holds (Q68, Q69). |
| `prop_queue_shadow_invariant_with_inflight` | `reified_p4_props.rs:3831` | After every step of a run with re-entries SHADOW holds, and a step with no persistent element in flight leaves no pending match (Q70, Q71). |
| `prop_queue_fifo_with_reentries` | `reified_p4_props.rs:3873` | In runs with re-entries, the records that are not persistent leave in increasing order, each older than every such record that stays (Q72). |
| `prop_queue_nonpersistent_reissue_fresh_ord` | `reified_p4_props.rs:3933` | A send that fires 64 or more peek waiters re-issues unchanged, and its re-entry stores at most its record at the order of its new bump (Q77). |
| `prop_queue_reentry_head_blocks` | `reified_p4_props.rs:4096` | A re-entry behind an older head of its side fires nothing and rests at its kept order, and a fire meets the head of the other side (Q73, Q74). |
| `prop_queue_reentry_permutation_invariant` | `reified_p4_props.rs:4179` | Two list orders of one collection give the same re-entry: outcomes, events, entries, and host work (Q75). |
| `prop_fresh_ord_reissue_loses_first_round_place` | `reified_p4_props.rs:4278` | The kept order re-enters ahead of a younger datum and fires the persistent waiter again, and a fresh order rests behind it and fires nothing (Q76). |
| `prop_queue_persistent_reissue_charged_once` | `reified_p4_props.rs:4364` | On a runtime, a persistent send or receive that re-issues has one data-channel identity and one introduction row, and it rests at its first order (Q69). |

Each property must fail when the rule that it checks is broken.
The script `.reified-rspaces-work/d43/run_mutants.py` applies five mutants to `spaces/generic.rs`, runs the unit tests of the agent and the P4 properties, and restores the file.
Each mutant made at least one of its expected tests fail (`target/verification/reified-rspaces/d43/mutants-2.log` and `mutants-3.log`).

*Table 53. The Rust mutants of decision 43.*

| Mutant | What it breaks | Tests that fail |
|---|---|---|
| `fresh-ord-reissue` | A persistent re-issue carries no order (C0). | `prop_queue_reissue_keeps_first_round_ord`, `prop_queue_loop_bounded`, `prop_queue_persistent_reissue_charged_once`, `prop_fresh_ord_reissue_loses_first_round_place`, and 8 unit tests |
| `reentry-joins-the-tail` | A re-entry ignores its kept order and joins the tail. | `prop_queue_shadow_invariant_with_inflight`, `prop_fresh_ord_reissue_loses_first_round_place`, and 5 unit tests |
| `reentry-ignores-head` | A re-entry fires from the head even behind an older head. | `prop_queue_reentry_head_blocks`, `space_queue_reissue_stores_behind_an_older_reentry` |
| `reentry-head-by-list-position` | A re-entry compares its order with the first entry of the list. | `prop_queue_reentry_permutation_invariant` |
| `nonpersistent-keeps-ord` | A re-issue that is not persistent keeps its order (C1). | `prop_queue_nonpersistent_reissue_fresh_ord`, `space_queue_nonpersistent_reissue_takes_a_fresh_ord` |

**Charges.** A re-issued persistent element stages one record in every round.
So the accounting registers its introduction once in each deploy, as the host does for a persistent produce that it produces again.
Each round pays its counter bump.
A divergent pair also repeats one COMM identity, so each later round pays only its bump.
The pair still ends when the phlo runs out.

### 11.13 Peek re-store of decision 45

[Decision 45](decision-register.md#decision-45) (Rule E) makes a peek fire on a space remove what the host removes.
The substrate keeps no record for a peek, also not the stored candidates of a produce-side peek join.
`check_peeks` (`rspace++/src/rspace/space_store_handle.rs:1519-1534`) only admits the peek set, and `removed_records` (`rspace++/src/rspace/space_store_handle.rs:1592-1625`) returns every stored non-persistent candidate of a fire.
`fired_receipt` (`rspace++/src/rspace/space_store_handle.rs:3202-3238`) keeps the persist flags of the host's `wrap_result`.
The play and replay backends always call `remove_matched_datum_and_join` or `store_persistent_data`.

The agent then makes a [re-store](glossary.md#re-store).
`receive_head` copies a peeked head record that is not persistent (`rholang/src/rust/interpreter/spaces/generic.rs:1642-1647`).
Right after the fire, `Discipline::restore` and its default `Cx::restore` (`rholang/src/rust/interpreter/spaces/generic.rs:419-426`, `1066-1081`) stage a produce of the same record bytes with the outcome Store and `persist = false`.
`Introduction::for_record` (`rholang/src/rust/interpreter/spaces/record.rs:2145-2158`) gives the re-store the cost authority of the record, as `produce_peeks` does.
The re-store bumps no counter, and the record keeps its envelope, its order, and its place at the head.

The substrate change and the re-store land in one commit.
The hot store does not deduplicate data, so a re-store over the old substrate would store a second copy.
A removal without the re-store would lose the record.

The P4 queue has no joins.
A peek receive fires only when no waiter is queued, so the plain Store of the re-store is exact.
The substrate tests found that a re-store in a produce-side peek join can fire another waiter, in 2 of 24 generated runs.
So P5 must offer such a re-store through the produce plan.

*Table 54. The Rocq results of decision 45.*

| Id | Headline | Claim |
|---|---|---|
| H2 | `commit_state_eq_host` (`EventCorrelation.v:5559`) | The committed store equals the host store, with no premise on peeks. |
| H11 | `peek_reproduces_trace_eq_host` (`EventCorrelation.v:5836`) | A transaction with the re-stores right after a peek fire commits with the calls, events, status, bindings, and store of the host program, whose reducer sends the peeked data again. A fire never stores a datum, and the re-stores are exactly the sends of `produce_peeks`. |
| Q78 | `queue_restore_realizes_plan` (`HeadOnlyDisciplines.v:3402`) | For a consume, a produce, and a re-entry, the staged operations of the code, with the re-store, give the records of the logical plan under the substrate effects of Rule E, up to list order. |

`HeadOnlyDisciplines.v` also proves two computed counterexamples with no assumption.
In `restore_over_retention_duplicates` (`HeadOnlyDisciplines.v:3594`), the re-store over the old retention duplicates the peeked record.
In `removal_without_restore_loses_record` (`HeadOnlyDisciplines.v:3624`), the removal without the re-store loses it.
`EventCorrelation.v` keeps its 17 headlines, and `HeadOnlyDisciplines.v` now has 20.

*Table 55. The Rocq mutants of decision 45 in HeadOnlyDisciplines.v.*

| Id | What the mutant changes | Must fail in |
|---|---|---|
| `ho-restore-skipped` | The code stages no re-store after a peek fire of a record that is not persistent. | `restore_fire_step`, `queue_restore_realizes_plan` |
| `ho-peek-retains` | A peek fire keeps the records that it binds, as the retention before decision 45 did. | `restore_fire_step`, `queue_restore_realizes_plan` |
| `ho-restore-persistent` | The code also re-stores a peeked persistent record, which the fire keeps. | `restore_fire_step`, `queue_restore_realizes_plan` |

The three mutants of decision 45 in `EventCorrelation.v` are in Table 12: `ec-peek-retains`, `ec-restore-skipped`, and `ec-restore-wrong-identity`.

*Table 56. The tests of decision 45.*

| Test | Location | What it checks |
|---|---|---|
| `space_store_handle_peek_events_equal_host_and_restores` | `reified_space_store_handle.rs:661` | A peek and its re-store have the events, calls, contents, and root of the host, also with a produce-side peek and its re-offer. |
| `space_store_handle_peek_fires_replay_and_report` | `reified_space_store_handle.rs:3006` | A peek fire and its re-store replay and report from their play log, and the play log and root equal the HEAD program. |
| `space_store_handle_produce_peek_join_removes_stored_and_leaves_incoming_unstored` | `reified_space_store_handle.rs:3978` | A produce-side peek join removes its stored record and never stores its incoming datum. |
| `space_store_handle_peek_fire_receipt_flags` | `reified_space_store_handle.rs:4492` | Every result of a peek fire keeps the persist flag of the host. |
| `space_store_handle_peek_restore_equals_head_reproduce` | `reified_space_store_handle.rs:4641` | A peek and its re-stores equal the HEAD program in play, replay, and reporting: events, calls, counters, state, and root. |
| `space_store_handle_restore_without_removal_duplicates_record` | `reified_space_store_handle.rs:4792` | Control: Rule E leaves one copy, a re-store without the removal leaves two, and a removal without the re-store leaves none. |
| `prop_space_txn_peek_restore` | `reified_space_store_handle_props.rs:2455` | Generated peek runs with re-offers and re-stores equal the HEAD peek path exactly, and they replay and report (H5, H11, H12). |
| `space_store_handle_receipt_keeps_the_host_flags` | `space_store_handle.rs:4435` | The receipt keeps the flag of the host, and results that do not line up fail closed. |
| `space_queue_peek_restores_the_head_record` | `spaces/generic.rs:3412` | A queue peek logs the consume, the COMM, and the re-store, and the re-store has the identity of the first produce. |
| `space_queue_peek_restore_registers_the_record_authority` | `spaces/generic.rs:3445` | The re-store registers the authority of the record, and the peek registers its own. |
| `space_queue_peek_of_a_persistent_record_restores_nothing` | `spaces/generic.rs:3485` | A peek of a persistent record removes nothing and stores nothing again. |
| `space_queue_observer_parity` | `reified_queue.rs:1336` | In deploys, the space events of each queue step equal the host events of the same program, the re-store after a peek included. |
| `prop_queue_restore_realizes_plan` | `reified_p4_props.rs:4716` | Over runs that peek often, a peek keeps the records, its receipt keeps the persist flag, and the re-store follows the COMM with the identity of the record (Q78). |

The script `.reified-rspaces-work/d45/run_mutants.py` applies three mutants to `spaces/generic.rs`, runs the unit tests of the agent and the property `prop_queue_restore_realizes_plan`, and restores the file.
Each mutant made at least one of its expected tests fail (`target/verification/reified-rspaces/d45/mutants.log`).

*Table 57. The Rust mutants of decision 45.*

| Mutant | What it breaks | Tests that fail |
|---|---|---|
| `restore-skipped` | A peek fire of a record that is not persistent stages no re-store. | `prop_queue_restore_realizes_plan`, `space_queue_peek_restores_the_head_record`, `space_queue_peek_restore_registers_the_record_authority` |
| `restore-persistent-too` | A peek of a persistent record also stores the record again. | `prop_queue_restore_realizes_plan`, `space_queue_peek_of_a_persistent_record_restores_nothing` |
| `restore-with-peek-authority` | The re-store registers the authority of the peek, not of the record. | `space_queue_peek_restore_registers_the_record_authority` |

**Gate.** The gate of decision 45 built the workspace with no warning, and `cargo clippy -p rholang -p rspace_plus_plus --all-targets -- -D warnings` passes.
`cargo fmt --all -- --check` passes after three earlier test files of the campaign were formatted and 29 clippy findings in campaign tests were fixed.
The substrate suites, the agent and queue suites, the P4 properties, the ordinary regression, and both empty-state-hash tests of Casper pass.
The Rocq gate printed `reified-rspaces proofs: PASS=49 FAIL=0 SKIP=0`, with 113 closed headlines and 80 negative checks (`target/verification/reified-rspaces/d45/gates/proofs.log`).
The map checkers, the closure checks, the completeness phases, the document lint, and the render check pass (`target/verification/reified-rspaces/d45/gates/summary.txt`).

The audit `scripts/audit/check-rocq-line-refs.sh` reports 7 mismatches, all in Casper documents outside the campaign, and it reported the same 7 at the commit of decision 43.

**Charges.** A consume-side peek of a record that is not persistent now makes one more produce, as the host program does.
The accounting observer charges it as a new introduction of the record, with the record's own authority.
The legacy storage charges of `ChargingRSpace` follow in Option P of decision 45 (section [11.14](#1114-legacy-charges-of-decision-45)).

### 11.14 Legacy charges of decision 45

[Decision 45](decision-register.md#decision-45) (Option P) makes each staged space operation of a legacy deploy pay the [legacy charges](glossary.md#legacy-charge) of its host call.
The [finding](findings/space-legacy-storage-charges.md) records the gap that Option P closes.
Section [N10 of the design](design.md#n10-legacy-charges) gives the rule, and [Rule SM-8a](specification.md#13-entries-and-operations) states it.
This section gives the evidence: the Rocq module `LegacyCharges.v`, the TLA+ invariant `LegacyChargesFollowHost`, the tests, and the negative controls.

**The code.** `produce_charges` (`reified/legacy_charges.rs:59-83`) and `consume_charges` (`reified/legacy_charges.rs:88-112`) restate the pre-charge of `ChargingRSpace`.
They also restate the post-charges of its `handle_result`, and `fire_post` (`reified/legacy_charges.rs:142-174`) gives the post-charges of a fire in that order.

**The chain.** `LegacyChain::attach` (`reified/legacy_charges.rs:262-279`) puts three steps into the callback `before_observe` of operation $`i`$.
The steps are the carried post-charges of operation $`i-1`$, the registration of operation $`i`$, and the pre-charge of operation $`i`$.
`LegacyChain::finish` (`reified/legacy_charges.rs:283-292`) charges the post-charges of the last operation after the commit.
Every staging site of `spaces/generic.rs` and `spaces/counter.rs` stages through the chain, and each prepared job calls `finish` before its outcome.
Outside test code, only `LegacyChain` calls `txn.produce` and `txn.consume` in the P4 sources.

**The Rocq model.** `LegacyCharges.v` models one committed transaction of a legacy deploy with a metered budget.
The charges `pre` and `post` of an operation are section variables, so the theorems hold for all charges of all operations.
The model also leaves the cost values, the debit, and the acceptance of each registration and observer call abstract.
The status of a stop names its point: a registration, a pre-charge, an observer call, or a post-charge.
The property `prop_legacy_charges_equal_charging_rspace` ties the abstract charges to the code, because it compares them with the real `ChargingRSpace`.

*Table 58. The Rocq results of decision 45, Option P.*

| Id | Headline | Claim |
|---|---|---|
| Q79 | `legacy_rebracket_eq_host` (`LegacyCharges.v:713`) | For every list of operations, budget, initial log, and call history, the space run makes the legacy log, remaining budget, status, calls, and applied operations of the host program. A stop at the pre-charge of operation $`i`$ applies the first $`i`$ operations, and a stop at a post-charge of operation $`i`$ applies the first $`i+1`$ operations. |
| Q80 | `legacy_replay_stops_with_play` (`LegacyCharges.v:747`) | Replay stages the operations of play and pairs each operation whose body play applied. Then replay makes the legacy log, remaining budget, status, calls, and applied operations of play, and it never diverges. |

The lemma `commit_finish_rebracket` (`LegacyCharges.v:449`) is the core of both proofs.
By induction on the operations, the commit loop and the tail equal the carry and then the flat host run.
The proof only re-associates binds, so it holds for every charge rule, call acceptance, and replay pairing.
Q79 applies the lemma to the empty chain, and `host_ops_stops` gives the applied operations at each stop point.
Q80 rewrites both runs to the flat form, and `host_op_pairing` shows that a pairing check matters only after play applied the body.
The two premises of Q80 restate H23 of `EventCorrelation.v`, because the modules do not import each other.

The module also proves two counterexamples with no assumption.
In `legacy_tail_dropped_breaks_eq` (`LegacyCharges.v:828`), a run without the tail misses the post-charges of its last operation.
In `legacy_pre_after_observe_breaks_prefix` (`LegacyCharges.v:879`), a late pre-charge stops at the point of the host, but with one more operation applied.
`legacy_negative_controls_computed` (`LegacyCharges.v:923`) computes both counterexamples on a concrete instance.
The module compiles alone, prints `Closed under the global context` for both headlines, and `rocqchk` reports `Axioms <none>`.

*Table 59. The Rocq mutants of decision 45, Option P, in LegacyCharges.v.*

| Id | What the mutant changes | Must fail in |
|---|---|---|
| `lc-tail-dropped` | The job never runs the tail, so the post-charges of the last operation are lost. | `commit_finish_rebracket` |
| `lc-carry-dropped` | A callback does not charge the carry, so the post-charges of each operation before the last are lost. | `commit_finish_rebracket` |
| `lc-pre-after-body` | The pre-charge comes after the body of its operation, at two sites of the model. | `commit_finish_rebracket` |
| `lc-replay-unpaired` | Q80 loses the premise that replay pairs each operation whose body play applied. | `legacy_replay_stops_with_play` |
| `lc-unpaired-skips-observe` | An unpaired replay body diverges before its observer calls, as before decision 36. | `host_op_pairing` |

**The TLA+ model.** `CheckpointReplay.tla` gains the mode constants `Peeks` and `Legacy` and three Unsafe flags.
`Legacy` runs a legacy deploy, and a ghost host program `HostOp` makes the same host steps with the charges of `ChargingRSpace`.
The agent stages each step through `PlayApply` or `ReplayApply`, whose callbacks charge the carry and the pre-charge, and `ChainTail` charges the tail after the commit.
The charge values are abstract, one for each label of `ChargingRSpace`.
The invariant U13 compares play and replay with the ghost program:

```math
\begin{aligned}
\mathit{LegacyChargesFollowHost} \triangleq\; \mathit{Legacy} \Rightarrow\; &\bigl(\neg(\mathit{phase} = \text{play} \land \mathit{pos} \ge 1) \Rightarrow \mathit{pLeg} = \mathit{hLeg} \land \mathit{pOps} = \mathit{hOps} \land \mathit{pFailed} = \mathit{hFailed}\bigr) \\
&\land\; (\mathit{phase} = \text{replaySwept} \Rightarrow \mathit{rLeg} = \mathit{hLeg})
\end{aligned}
```

The symbols have these meanings:

- $`\mathit{pLeg}`$, $`\mathit{rLeg}`$, and $`\mathit{hLeg}`$ are the legacy states of play, of replay, and of the host program. Each holds the log, its length, and the remaining budget.
- $`\mathit{pOps}`$ and $`\mathit{hOps}`$ count the host steps that play and the host program applied, so they mark the stop point.
- $`\mathit{pFailed}`$ and $`\mathit{hFailed}`$ hold when the deploy and the program failed.
- $`\mathit{phase} = \text{play} \land \mathit{pos} \ge 1`$ holds while play has an open transaction.

*Table 60. The TLA+ checks of decision 45, Option P.*

| Check | Configuration | Result |
|---|---|---|
| Safe, legacy | `MC_CheckpointReplay_Legacy.cfg`, `Budget=12`, `Peeks=TRUE`, `Legacy=TRUE` | 34,351 states, depth 22, no violation of T1 to T4, T6, U13, `TypeOK`, or `IndInv` |
| Safe, legacy, two blocks | `MC_CheckpointReplay_Legacy_TwoBlock.cfg` | 38,602 states, depth 43, no violation |
| Safe, peeks in token mode | `MC_CheckpointReplay_Peek.cfg`, `Budget=4`, `Peeks=TRUE`, `Legacy=FALSE` | 34,991 states, depth 22, no violation |
| Unsafe, late pre-charge | `LegacyPreAfterObserveUnsafe.cfg` | Refutes exactly T6 `ReplayOutcomeEqual`, and the complement explores 34,227 states |
| Unsafe, no tail | `LegacyTailDroppedUnsafe.cfg` | Refutes exactly U13, and the complement explores 33,887 states |
| Unsafe, retained peek | `RetainPeekUnsafe.cfg` | Refutes exactly U13, and the complement explores 35,243 states |
| Apalache BASE | `MC_CheckpointReplay_Legacy_IndBase_Apalache.cfg`, `MaxTxnOps=1`, `MaxTxns=1`, `Blocks=2` | `NoError` |
| Apalache STEP | `MC_CheckpointReplay_Legacy_IndStep_Apalache.cfg`, the same bounds | `NoError` |

The two configurations of section [5.2](#52-checkpointreplaytla) keep their counts of 145,255 and 94,049 states.
`LegacyPreAfterObserve` refutes T6 and not U13, because observers charge no phlo in a legacy deploy and the charges of play keep their order.
The divergence shows in replay, where the late pre-charge lets an unpaired fire reach the pairing check first.
A STEP with `MaxTxnOps=2` also returned `NoError`, but it took 20 minutes, so the gate keeps the bound 1.

**Limits of the model.** The model covers single-channel operations that are not persistent.
The registration of an authority costs no phlo, so the model omits it.
The abstract charges keep the net charge of each host step positive, so a negative deploy cost is out of its scope.
The deviations D.7 of the design report and the order of the evaluation charges of the reducer are not modeled.
The host program uses the staging order of the space.

*Table 61. The tests of decision 45, Option P.*

| Test | Location | What it checks |
|---|---|---|
| `prop_legacy_charges_equal_charging_rspace` | `reified_legacy_charges.rs:173` | Generated host programs run on the real `ChargingRSpace`. For each call, the charges of the equivalent staged operation equal the new log entries, value and label. All 8 shapes of call occur (Q79). |
| `prop_legacy_chain_log_is_host_order` | `reified_legacy_charges.rs:667` | Generated transactions with generated budgets make the legacy log and the stop of the host program. All 6 kinds of transaction occur, the stops in the commit and in the tail included (Q79). |
| `prop_legacy_replay_stops_with_play` | `reified_legacy_charges.rs:775` | Replay with the log of play makes the legacy log, the stop, and the store of play (Q80). |
| `legacy_charges_of_a_store_and_a_park` | `reified/legacy_charges.rs:416` | A store pays the storage of its record and the event fee of one channel. A park pays the storage of its continuation and the event fee of its channels. |
| `legacy_charges_of_a_produce_fire_follow_handle_result` | `reified/legacy_charges.rs:442` | A produce-side fire pays the refunds, the event fee, and the COMM fee in the order of `handle_result`. |
| `legacy_charges_of_a_consume_fire_refund_the_consume` | `reified/legacy_charges.rs:504` | A consume-side fire always refunds the storage of its consume. |
| `legacy_charges_of_a_continuation_without_a_body_fail_closed` | `reified/legacy_charges.rs:546` | A continuation without a body fails closed, where `ChargingRSpace` panics. |
| `legacy_chain_is_active_only_in_a_metered_legacy_deploy` | `reified/legacy_charges.rs:567` | The chain charges only a legacy deploy with a metered budget. |
| `legacy_chain_charges_in_host_order` | `reified/legacy_charges.rs:632` | The callbacks and the tail make the log of the host program. |
| `legacy_chain_pre_charge_stops_before_its_operation` | `reified/legacy_charges.rs:661` | A pre-charge that runs out of phlo stops the commit before its operation, and the operations before it stay applied. |
| `legacy_chain_tail_fails_after_the_commit` | `reified/legacy_charges.rs:708` | A tail that runs out of phlo fails after the full commit. |
| `space_queue_legacy_replay_equal` | `reified_queue_replay.rs:1001` | Legacy deploys on queues replay with the cost, the storage charges in order, every charge as a multiset, and the root of play. |
| `space_queue_legacy_out_of_phlo_sweep_replays_equal` | `reified_queue_replay.rs:862` | The out-of-phlo sweep of decision 36 in a legacy deploy: every limit replays to the outcome of play. |

The order test compares the storage charges in order and every charge as a multiset.
In a legacy deploy the reducer charges its own evaluation steps as its parallel branches run, so their order depends on the schedule.
This is also true for host programs, and section 6 of the [finding](findings/space-legacy-storage-charges.md#6-limits-that-stay) records it.

The script `.reified-rspaces-work/d45/run_mutants_option_p.py` applies six mutants to `reified/legacy_charges.rs` and runs the expected tests.
Each mutant made at least one of its expected tests fail (`target/verification/reified-rspaces/d45/option-p-mutants.log`), and the script restored the file.

*Table 62. The Rust mutants of decision 45, Option P.*

| Mutant | What it breaks | Tests that fail |
|---|---|---|
| `tail-dropped` | `finish` charges nothing, so the post-charges of the last operation are lost. | `legacy_chain_charges_in_host_order`, `legacy_chain_tail_fails_after_the_commit`, `space_queue_legacy_replay_equal` |
| `pre-before-carry` | A callback charges its pre-charge before the carried post-charges. | `legacy_chain_charges_in_host_order` |
| `store-without-event-fee` | A store pays no event fee. | `prop_legacy_charges_equal_charging_rspace`, `legacy_charges_of_a_store_and_a_park` |
| `refund-ignores-persist` | A fire refunds the storage of every bound record, also of a persistent one. | `prop_legacy_charges_equal_charging_rspace`, `legacy_charges_of_a_produce_fire_follow_handle_result`, `legacy_charges_of_a_consume_fire_refund_the_consume` |
| `consume-refund-ignores-id` | A persistent continuation is never refunded, also when the trigger has its id. | `prop_legacy_charges_equal_charging_rspace`, `legacy_charges_of_a_produce_fire_follow_handle_result`, `legacy_charges_of_a_consume_fire_refund_the_consume` |
| `active-outside-legacy` | The chain is active in every deploy. | `legacy_chain_is_active_only_in_a_metered_legacy_deploy` |

**Gate.** The gate of Option P checked the workspace with no warning, and `cargo clippy -p rholang -p rspace_plus_plus --all-targets -- -D warnings` passes.
Its 35 Rust legs passed (`target/verification/reified-rspaces/d45/option-p-gates/summary-rust.txt`).
They include the substrate suites, the 3 properties of Option P, the P4 suites, the ordinary regression, and both empty-state-hash tests of Casper.
The 26 unit tests of `reduce::reified_hooks` and `deterministic_reduction::reified_schedule`, which the module filters of the gate miss, also passed.

The Rocq gate printed `reified-rspaces proofs: PASS=53 FAIL=0 SKIP=0`, with 115 closed headlines and 86 negative checks (`target/verification/reified-rspaces/d45/option-p-gates/proofs.log`).
The TLA+ gate printed `reified-rspaces tla: PASS=162 FAIL=0 SKIP=0` (`target/verification/reified-rspaces/d45/tla-lc/gate.log`).
The map checkers, the closure checks, the completeness phases, the footprint check, and the 273 checker self-tests pass (`target/verification/reified-rspaces/d45/option-p-gates/summary-checkers.txt`).
The audit `scripts/audit/check-rocq-line-refs.sh` reports the same 7 mismatches as at the commit of Rule E, all in Casper documents outside the campaign.
The Option P edits of `spaces/generic.rs` and `spaces/counter.rs` moved lines, so 93 older citations of the documents and the models moved with them.

## 12. Phase P5: the other disciplines and outer stores

Phase P5 adds the disciplines bag, set, cell, stack, and priority queue, the outer stores Array, Vector, and PathMap, and same-space joins.
[Decision 48](decision-register.md#decision-48) sets the order of the work.
Its first step, W0, freezes a baseline of the queue before the engine changes.

### 12.1 The queue parity golden

The suite `rholang/tests/reified_queue_parity.rs` pins what the queue agent does on a fixed set of deploys.
The golden file `test-vectors/reified-queue-parity-v1.json` was captured at the commit of Option P of decision 45 (`2c2b45bf5`).
Every later step of P5 must leave the golden unchanged.

*Table 63. The scenarios of the queue parity golden.*

| Scenario | Deploys | What it pins |
|---|---|---|
| `fifo_and_peek_restore` | 5 | FIFO enqueue and dequeue, and two consume-side peeks with their re-stores (Rule E of decision 45) |
| `produce_side_peek_reoffer` | 5 | A peek waiter with the re-offer to the next waiter (decision 30), and the store after a peek with no waiter behind it |
| `capped_persistent_send` | 2 | A persistent send to 66 waiters, past `FIRE_ROUND_CAP`, with the re-issue that keeps the order of its first round (I-21, decision 43) |
| `reentry` | 3 | The re-entry of decision 43 ahead of a younger datum |
| `divergent_pair` | 1 | A persistent receive and a persistent send that re-issue until the phlo runs out (A10) |
| `persistent_operations` | 3 | A persistent send with two waiters, and a persistent receive that drains a queue |
| `legacy_budget` | 5 | The deploys of a legacy budget, with the legacy charges of Option P |

Each record of a deploy holds these values:

- The events of the deploy, in order. Each event has every field of its identity: the hashes, the channels, the peeks, and `times_repeated`.
- The phlo cost, the error codes, and the root after the deploy.
- For a token budget, the host-work usage of each dimension, under a metered budget with no binding limit.
- For a legacy budget, the storage charges in order and all legacy charges as a multiset.

The deploys name the space only through injected names, so no host operation interleaves with the space jobs.
A token scenario runs twice on fresh stores: with the plain evaluation of a user deploy, and under a metered host-work budget.
The suite requires the same events, costs, errors, and roots from both runs.
The counts of waiters are literals, so the deploys of the golden do not change with `FIRE_ROUND_CAP`.

**Determinism.** Two captures on the same host gave byte-identical fragments.
The second capture assembles to the same golden file (`merge-queue-parity-capture.py --check`).

**Negative controls.** The script `.reified-rspaces-work/p5/run_parity_mutants.py` applies four mutants and restores each file (`target/verification/reified-rspaces/p5/parity-mutants.log`).
Each mutant makes its expected scenarios fail, in the expected fields of their records.

*Table 64. The mutants of the queue parity golden.*

| Mutant | What it changes | What fails |
|---|---|---|
| `fire-round-cap-63` | `FIRE_ROUND_CAP` is 63. | All six token scenarios, in their events, roots, and host work |
| `store-without-event-fee` | A store pays no event fee in a legacy deploy. | `legacy_budget`, in its charges and its cost |
| `compare-charges-two-operations` | Each metered comparison reserves two operations. | All six token scenarios, in their host work only |
| `dispatch-extra-comparison` | The dispatch of the family agent reserves one more comparison (S1). | All six token scenarios, in their host work only |

**Capture.** A new capture replaces the baseline, so only a recorded decision that changes the queue can justify it.
To capture, do these steps from the repository root:

1. Set `RRS_CAPTURE_QUEUE_PARITY_OUT` to an absolute directory under `target/`.
2. Run `cargo test -p rholang --test reified_queue_parity`. Each scenario writes one fragment.
3. Run `python3 scripts/verify-reified/merge-queue-parity-capture.py --capture-dir <directory> --output test-vectors/reified-queue-parity-v1.json --capture-command <command>`.

The merge script fails on a missing fragment, on a fragment of another scenario, and on an unknown fragment.
Five checker self-tests cover these rules and the check mode (Table 13).

### 12.2 The engine spine (S1)

Step S1 of P5 turns the agent of P4 into an engine that every family reuses ([design N4](design.md#n4-genericspace-and-the-queue-discipline)).
It wires no new family. The family agent has one arm, `hashmap:queue`, and the batches of the next wave add the other families.

S1 makes these changes:

- `FamilyAgent` (`spaces/families.rs:447-538`) reads the config once and runs the agent of the family that the config names. The runtime installs it.
- `Discipline` holds the plans of the operations. The default plans are the head driver `offer_head` and `receive_head`, the P4 plans as free functions over the discipline.
- `Cx` (`spaces/generic.rs:717-720`) is the only path from a plan to the transaction. It adds group fires over `[D; n]`, retirements and lifts.
  `Cx` and `AllocCx` expose no raw transaction, and the legacy chain of a plan is private to `generic.rs`. So the types keep the design rule that only `generic.rs` and `counter.rs` stage operations.
- `run_with` (`spaces/generic.rs:2288-2424`) checks the key, the params and the query before the bump, and bumps the counter only for a discipline that uses one.
- The outer-store hooks check a routed key, stage the records of a creation, and allocate index names. A store without an allocator answers with `Baseline` and no work.
- The substrate gives the canonical order of DR-75 over borrowed views (`space_store_handle.rs:342-350`, `382-394`).

**Acceptance.** The queue parity golden of 12.1 is unchanged, with its host-work usage.
Every P4 suite passes unchanged, apart from the helper literals that the new fields need.
A mutant that adds one comparison to the family dispatch makes all six token scenarios fail, in their host work only (Table 64).
So the golden pins the one comparison of the family check.

*Table 65. The tests of the engine spine.*

| Test | Location | What it checks |
|---|---|---|
| `space_engine_staged_kinds_stay_one_byte` | `generic.rs:5042` | A staged kind is one byte, so the reserved staged list of a head-only plan keeps its size. |
| `space_engine_checks_params_and_queries_before_the_bump` | `generic.rs:5049` | An unknown key, a query that the discipline does not take, and a rejected value fail before the bump. The counter and the log stay unchanged. |
| `space_engine_plans_see_the_round_counter_and_the_operation` | `generic.rs:5135` | A plan sees the counter value of its round (`Cx::bumped`), and each settle hook sees its operation (`OpView`). |
| `space_engine_capacity_abort_stages_nothing` | `generic.rs:5185` | A store or a park on a full side fails with `CellOccupied`, and the transaction applies nothing. |
| `space_engine_group_fire_lines_up_and_dispatches_its_prefix` | `generic.rs:5220` | A fire over `[D; 3]` that binds one result removes three records and dispatches one result. A group that does not line up stages nothing. |
| `space_engine_retire_and_lift_fire_without_dispatch` | `generic.rs:5284` | A retirement and a lift remove their records and dispatch nothing. |
| `space_engine_outcome_dispatches_the_prefix_of_a_group_fire` | `generic.rs:5336` | The outcome keeps the prefix of a prefix fire and rejects a missing or extra count. |
| `space_engine_staged_list_growth_is_reserved_before_it_grows` | `generic.rs:5806` | A list at its capacity reserves the doubled buffer before it grows. A budget that is one operation short rejects the growth and leaves the list unchanged. |
| `space_engine_operation_ord_without_a_counter_is_zero` | `generic.rs:5854` | Without a counter every order is 0, and a re-issued order other than 0 is `Inconsistent`. |
| `space_engine_allocation_without_an_allocator_is_baseline` | `generic.rs:5875` | An allocation in a HashMap space is `Baseline` and logs nothing, and a missing config is `DanglingHandle`. |
| `space_engine_allocator_store_creates_and_allocates_its_names` | `generic.rs:5901` | A store with an allocator stages its creation record and allocates names through the take and the put of the allocator. |
| `space_engine_family_agent_runs_a_queue_as_its_own_agent_does` | `generic.rs:5945` | The family agent gives the fires, events, records, waiters and host work of the queue agent. |
| `space_engine_family_agent_rejects_a_family_that_is_not_wired` | `generic.rs:5997` | An operation, an allocation and a creation of a family that is not wired fail with `FamilyUnavailable` and log nothing. |
| `space_families_wire_exactly_the_listed_families` | `families.rs:593` | Only the wired families have an agent and param schemas, and each family takes the schemas of its discipline. |
| `space_families_dispatch_by_config_reserves_one_comparison_per_arm` | `families.rs:748` | Each arm that the dispatch tries reserves one comparison of the names of the config and of the arm. The arm of `hashmap:queue` comes first, so it reserves what the P4 agent reserves. |
| `space_order_primitives_reserve_the_restated_formula` | `space_store_handle.rs:3910` | Each primitive of the space order reserves what `MeteredOrder` of the branch reserves. Both orders run the one `canonical_order`. |
| `harness_session_runs_a_queue_space_under_the_family_agent` | `reified_p5_harness.rs:28` | The shared session creates a space and runs deploys under the family agent. |
| `harness_agent_env_runs_the_family_agent_in_fifo_order` | `reified_p5_harness.rs:65` | Over the shared agent environment, the family agent delivers in send order and bumps once for each operation (64 cases). |

**The shared harness.** The P5 suites include `rholang/tests/support/` with `#[path]`.
It holds the property runner, the agent environment and the runtime session of the P4 queue suite for any family.
It also holds the host side of the legacy-charges differential.
The differential moved there from `reified_legacy_charges.rs`, whose three properties pass unchanged.

**Refinements of the design sketch.** S1 implements the sketch of the P5 design (A.3.3) with these refinements, so that each change keeps the parity of the queue:

- A staged kind stays one byte. A group fire that dispatches a prefix keeps its count in a separate list. A count in the kind would grow the reserved staged list of every queue operation.
- `Discipline::restore` takes the record, not the stored datum. The record, its channel and `persist = false` fix the identity of the re-store, and the copy of the source would add host work.
- The key check is the hook `OuterStore::check_key(config, channel, key)`. `OuterStore::key(channel)` stays the routing rule, because routing computes the key before the config is read.
- `SpaceCreation` carries the family of its factory. The family agent sends a creation to that family, whose agent checks that the config names it. Thus the lock keys of the outer store are known at the start of the creation.
- An allocation locks the config channel and `A(sid)` from the start, because the family is known only after the config read.
- A settle bumps the counter, as every round of an operation does ([decision 31](decision-register.md#decision-31), [decision 43](decision-register.md#decision-43)).
- The settle hooks receive a view of their operation (`OpView`), and `Cx::bumped` gives the counter value of the round.
  A settle builds the `SettleOp` of a re-issue from its operation, and the body of a `K_lift` continuation holds the counter value of the round.

### 12.3 The engine models

Two Rocq modules and one TLA+ module model the two drivers of the engine: the head driver of S1 and the scan driver ([design N11.2](design.md#n112-the-scan-driver)).
An appendix of `AgentReadCoverage.v` models the metering of the S1 primitives.
The models count the fire of a receive before its peek restore, as the code does.
A second send that meets a full transaction re-issues before it fires ([decision 49](decision-register.md#decision-49)).

*Table 66. The headlines of the engine models.*

| Headline | Location | Property test |
|---|---|---|
| `space_match_is_injected_match` | `Match.v:615` | `prop_engine_match_is_the_injected_matcher` |
| `guard_once_greedy_like_head` | `Match.v:658` | `prop_engine_guard_checked_once_as_the_host_checks_it` |
| `wildcard_matcher_degenerate` | `Match.v:718` | `prop_engine_wildcard_matcher_breaks_pattern_enforcement` |
| `consume_fires_only_matching` | `GenericRSpace.v:1096` | `prop_engine_consume_fires_only_matching` |
| `produce_fires_only_matching` | `GenericRSpace.v:1109` | `prop_engine_produce_fires_only_matching` |
| `unordered_no_pending_match` | `GenericRSpace.v:1124` | `prop_engine_no_guard_free_pending_match` |
| `peek_restore_equals_host_reproduce` | `GenericRSpace.v:1156` | `prop_engine_peek_restore_equals_host_reproduce` |
| `scan_reissue_identity_stable` | `GenericRSpace.v:1300` | `prop_engine_scan_reissue_keeps_identity` |
| `engine_primitives_trace_covered` | `AgentReadCoverage.v:998` | `prop_engine_primitives_reserve_before_they_read`, `prop_cell_reservation_cut_rejects_without_effect` |

`GenericRSpace.tla` checks the bag, the set and the cell with the invariants V1 to V8.
It covers every schedule of five operations and every canonical order.
The safe runs find 5,750,278 states (bag), 5,721,998 states (set) and 127,901 states (cell), with no violation.
Table 67 lists the Unsafe controls. Each one refutes exactly its invariant, and its complement run, without that invariant, finds no violation.

*Table 67. The Unsafe controls of the engine model.*

| Control | What it changes | Refuted invariant |
|---|---|---|
| `WildcardUnsafe.cfg` | the legacy wildcard matcher fires every pattern | `PatternEnforcement` |
| `SetRetireOneUnsafe.cfg` | a set consume retires one member of the class | `SetClassRetired` |
| `SetNoProduceRetireUnsafe.cfg` | a set send that fires a waiter retires nothing | `SetClassRetired` |
| `CellNoCapacityUnsafe.cfg` | the cell has no capacity check | `CellCapacity` |
| `PeekNoRestoreUnsafe.cfg` | a peek fire removes the record and stores nothing | `Conservation` |
| `RestoreOpAuthorityUnsafe.cfg` | the re-store takes the authority of the operation | `PeekRestoresRecord` |
| `RestoreNoReofferUnsafe.cfg` | the re-store is a plain store with no re-offer | `PeekRestoresRecord` |

`rholang/tests/reified_engine_props.rs` holds one property for each headline and invariant: 16 properties, 2 witness tests and 2 oracle controls.
The properties run the real bag, queue and cell disciplines and a probe of the set.
Each property asserts a minimum count of the cases that reach its key behavior.
The 24 Rocq negative controls fail inside the lemma whose statement they make false (Table 68).

*Table 68. The negative controls of the engine models, by group.*

| Group | Controls | What they revert |
|---|---|---|
| `mt-*` | 7 | the injected matcher, the guard rule of the host, and the wildcard instance (F17 to F19) |
| `gr-*` | 13 | the fire rules, the restore of Rule E, and the re-issue identity (F20 to F22, decision 43) |
| `arc-*` | 4 | the reservations of the staged list, of group fires, of the capacity check and of prefix outcomes (decision 42) |

### 12.4 The bag and the scan driver

The bag runs on the scan driver ([design N11.2](design.md#n112-the-scan-driver)). Table 69 lists its headlines.

*Table 69. The headlines of the bag and of the waiter side.*

| Headline | Location | Property test |
|---|---|---|
| `bag_visit_order_canonical` | `DataCollection.v:243` | `prop_bag_visit_order_canonical` |
| `bag_consume_first_spatial_match` | `DataCollection.v:445` | `prop_bag_consume_first_spatial_match` |
| `bag_persistent_match_not_removed` | `DataCollection.v:536` | `prop_bag_persistent_match_not_removed` |
| `bag_multiplicity_preserved` | `DataCollection.v:559` | `prop_bag_multiplicity_preserved` |
| `bag_removal_keeps_equal_duplicate` | `DataCollection.v:578` | `prop_bag_removal_keeps_equal_duplicate` |
| `bag_peek_restore_is_host_second_send` | `DataCollection.v:725` | `prop_bag_legacy_charges_equal_host`, `prop_bag_restore_registers_the_record_authority` |
| `scan_trace_covered` | `DataCollection.v:856` | `prop_bag_reservation_cut_rejects_without_effect` |
| `waiter_removal_by_record_identity` | `ContinuationCollection.v:273` | `prop_bag_waiter_removal_by_record_identity` |
| `pattern_equality_removal_can_remove_another_waiter` | `ContinuationCollection.v:289` | `prop_bag_equal_pattern_waiters_stay_distinct` |
| `park_byte_identity_idempotent` | `ContinuationCollection.v:308` | `prop_bag_park_byte_identity_idempotent` |
| `waiter_visit_order_canonical` | `ContinuationCollection.v:337` | `prop_bag_waiter_visit_order_canonical` |
| `head_waiter_unique` | `ContinuationCollection.v:367` | `prop_head_waiter_unique` |

`rholang/tests/reified_bag.rs` holds 18 tests and 13 properties.
Its host differential runs each program on a real `ChargingRSpace` and on the bag agent, with the follow-up calls of the reducer.
The differential compares the events and the charges.
The 16 Rocq negative controls fail inside their expected headlines.
Five single changes of `scan.rs` each make tests fail (Table 70), and an MD5 check shows that each run restored the file.

*Table 70. The Rust mutation checks of the scan driver.*

| Mutation | Failing tests |
|---|---|
| a peek re-store without the re-offer to the waiters | 7 |
| a re-store with the authority of the operation | 2 |
| the guard of a receive on every candidate | 9 |
| an offer that stops at the first guard refusal | 6 |
| the records in list order, not in canonical order | 11 |

### 12.5 The cell

*Table 71. The headlines of the cell.*

| Headline | Location | Property test |
|---|---|---|
| `cell_capacity_one` | `CellDiscipline.v:672` | `prop_cell_capacity_one` |
| `cell_restore_never_occupied` | `CellDiscipline.v:858` | `prop_cell_restore_never_occupied` |
| `cell_reentry_as_fresh` | `CellDiscipline.v:889` | `prop_cell_reentry_as_fresh` |

`rholang/tests/reified_cell.rs` holds 17 tests and 4 properties, and `cell.rs` holds 4 unit tests.
Seven Rocq negative controls fail inside their expected declarations: three on the capacity check, two on the re-store, and two on the re-entry rule.
Three Rust mutants of `cell.rs` make the expected tests fail, and each twin property has a control of its own in the run.

### 12.6 The Array and Vector stores and the allocator

*Table 72. The headlines of the Array and Vector stores and of the allocator.*

| Headline | Location | Property test |
|---|---|---|
| `fixed_array_allocates_at_most_size` | `OuterStorage.v:313` | `prop_fixed_array_allocates_at_most_size` |
| `fixed_array_out_of_names_exact` | `OuterStorage.v:330` | `prop_fixed_array_out_of_names_exact` |
| `cyclic_array_index_mod_size` | `OuterStorage.v:376` | `prop_cyclic_array_index_mod_size` |
| `allocation_batch_atomic` | `OuterStorage.v:402` | `prop_allocation_batch_atomic` |
| `fixed_array_indices_sequential` | `OuterStorage.v:345` | `prop_fixed_array_indices_sequential` |
| `allocator_indices_distinct` | `OuterStorage.v:450` | `prop_allocator_indices_distinct` |
| `allocator_identities_unique` | `OuterStorage.v:555` | `prop_allocator_identities_unique` |
| `alloc_trace_covered` | `OuterStorage.v:695` | `prop_alloc_trace_covered` |
| `vector_index_is_counter` | `VectorStore.v:184` | `prop_vector_index_is_counter` |
| `vector_gensym_distinct` | `VectorStore.v:204` | `prop_vector_gensym_distinct` |
| `vector_gensym_monotonic` | `VectorStore.v:219` | `prop_vector_gensym_monotonic` |
| `hashmap_iteration_permutation_invariant` | `HashMapStore.v:168` | `prop_hashmap_iteration_permutation_invariant` |
| `hashmap_get_put_other` | `HashMapStore.v:213` | `prop_hashmap_get_put_other` |

`rholang/tests/reified_array_vector.rs` holds 25 tests and 15 properties, and the allocator and the stores hold 17 unit tests.
The 21 Rocq negative controls fail inside their expected headlines.
`CounterConflict.tla` models the allocator: `MC_CounterConflict_Allocator.cfg` finds 326,700 states with no violation.
The Unsafe control `LazyAllocatorUnsafe.cfg` creates the allocator at the first allocation, and it refutes exactly `NoDuplicateIndex`.

Three tests run the Array and Vector families end to end.
`space_array_all_disciplines_supported` runs every wired discipline through the family agent on a fixed Array, a cyclic Array and a Vector space.
`space_array_new_in_use_allocates_indices` and `space_array_newspace_annotation_allocates` run Rholang deploys.
In them, `gensym`, a `new` inside a `use` block and an annotated `new` take index names in allocation order.
A forged channel inside the `use` block of an Array space fails with `NotAnIndexName`, and a `new` beyond the size fails with `OutOfNames`.

### 12.7 The PathMap layer

*Table 73. The headlines of the PathMap layer.*

| Headline | Location | Property test |
|---|---|---|
| `prefix_key_iff_prefix` | `PathMapPrefix.v:584` | `prop_prefix_key_iff_prefix`, `prop_segments_are_a_prefix_code`, `prop_path_keys_are_injective`, `prop_metered_keys_equal_unmetered_keys` |
| `reroot_ident` | `PathMapPrefix.v:843` | `prop_reroot_ident` |
| `reroot_compose` | `PathMapPrefix.v:850` | `prop_reroot_compose`, `prop_head_adapter_roundtrip`, `prop_prefixed_maps_keep_their_order` |
| `reroot_injective` | `PathMapPrefix.v:922` | `prop_reroot_injective` |
| `fips_example` | `PathMapPrefix.v:959` | `prop_unit_shift` |
| `candidate_order_total` | `PathMapPrefix.v:1014` | `prop_candidate_order_total` |

The layer holds 33 unit tests, 11 of them properties, which run on the shared runner of `rholang/tests/support/props.rs`.
The 17 Rocq negative controls fail inside their expected lemmas. One control changes two places, the serializer of a tag and its parser.

### 12.8 The families

*Table 74. The P5 headlines of the factories.*

| Headline | Location | Property test |
|---|---|---|
| `registered_families_complete` | `SpaceFactory.v:1354` | `prop_registered_families_complete`, `prop_factory_config_names_its_family` |
| `aliases_bind_family_capability` | `SpaceFactory.v:1431` | `prop_aliases_bind_family_capability`, `prop_factory_config_names_its_family` |
| `constructor_params_validated` | `SpaceFactory.v:1834` | `prop_constructor_params_validated` |
| `priorities_validated` | `SpaceFactory.v:1971` | `prop_priorities_validated` |
| `array_size_validated` | `SpaceFactory.v:2084` | `prop_array_size_validated` |

`rholang/tests/reified_families.rs` holds 10 tests and 6 properties, and the factory modules hold 27 unit tests.
Every check of the suite runs for each registered family, so a new arm extends the suite with no edit.
The 20 Rocq negative controls fail inside their expected lemmas, and the 6 controls of P4 still pass on the extended module.

### 12.9 The receive queries and the allocation jobs

`query_source_routes_inner_channel` (`SpaceRefs.v:1344`) states the route of a receive query.
`prop_query_bind_routes_inner_channel` (the hooks) and `prop_query_source_routes_inner_channel` (`rholang/tests/reified_hooks_p5.rs`) test it.
`rholang/tests/reified_hooks_p5.rs` holds 9 tests and 1 property, and the five Rocq negative controls fail inside the headline.
The test `reducer_space_routing_allocation_jobs_give_index_names` checks that each allocation job stamps the authority of the process that evaluates the `new` on the allocator record.

### 12.10 The stack and the fire count of a transaction

`StackDiscipline.v` models the head driver of the stack, the settle loop, and one fire count for each transaction ([decision 49](decision-register.md#decision-49)).
The model threads one count through the settle before the plan, the plan and the settle after it.
A plan opens only below the cap.

*Table 75. The headlines of the stack.*

| Headline | Location |
|---|---|
| `stack_head_is_max_ord` | `StackDiscipline.v:556` |
| `stack_produce_preserves_inv` | `StackDiscipline.v:1043` |
| `stack_consume_preserves_inv` | `StackDiscipline.v:1049` |
| `stack_lifo` | `StackDiscipline.v:1255` |
| `stack_head_blocks` | `StackDiscipline.v:1348` |
| `stack_peek_restore_needs_no_settle` | `StackDiscipline.v:1405` |
| `stack_reentry_at_lifo_place` | `StackDiscipline.v:1444` |
| `stack_reissue_keeps_ord` | `StackDiscipline.v:1576` |
| `stack_settle_terminates` | `StackDiscipline.v:1959` |
| `stack_settle_keeps_unspent_records` | `StackDiscipline.v:2151` |
| `stack_plan_permutation_invariant` | `StackDiscipline.v:2493` |
| `stack_without_settle_breaks_inv` | `StackDiscipline.v:2571` |
| `settle_trace_covered` | `StackDiscipline.v:2902` |

Four headlines take the premise $`0 < \mathit{cap}`$, because `open_plan` opens no plan at a cap of 0, and `FIRE_ROUND_CAP` is 64.
`stack_settle_terminates` bounds the whole transaction: a produce, a consume, a settle job and both re-entries each make at most `FIRE_ROUND_CAP` fires.

`StackDiscipline.tla` checks `LifoOrder` (V11), `StackNoPendingMatch` (V12), `NoRecordLost` (V13), `ReissueKeepsOrd` (V14) and `TxnFiresBounded` (V15).
The safe run finds 12,002,269 states at depth 34 with no violation.
It reaches 41 plan-open re-issues and 2,058 cap stops of a settle.

*Table 76. The Unsafe controls of the stack model.*

| Control | What it changes | Refuted invariant |
|---|---|---|
| `StackNoSettleUnsafe.cfg` | the stack never settles | `StackNoPendingMatch` |
| `StackReentryAtTopUnsafe.cfg` | a re-entry always reaches the top | `LifoOrder` |
| `StackPerPhaseCountUnsafe.cfg` | each phase of a transaction starts a count of its own | `TxnFiresBounded` |
| `StackPlanOpensAtCapUnsafe.cfg` | a plan opens at the cap with no check | `TxnFiresBounded` |

`rholang/tests/reified_stack.rs` holds 26 tests and 8 properties. The property `prop_stack_txn_fires_bounded_at_the_cap` reaches each cap stop.
It covers the settle job at the cap, the settle before a plan at the cap, and the plan that opens at the cap.
It checks every transaction against the bound.
The 20 Rocq negative controls fail inside their expected lemmas. Three of them change the count of a transaction, and each fails in `stack_settle_terminates`.

### 12.11 The merge of the P5 families

M-1 makes merges serializable for each collection, because every operation on a collection with a counter bumps the counter.
The second wave of W2 checks this rule for the bag, the cell, the stack, and the queues of Array and Vector spaces.

*Table 77. The merge headlines of the P5 families.*

| Headline | Location | Design name | Property test |
|---|---|---|---|
| `shared_collection_race` | `CounterConflict.v:889` | F23 `cell_ops_race_on_counter` | `prop_cell_ops_race_on_counter` |
| `crossed_stacks_without_removal_counters_fabricate` | `CounterConflict.v:2578` | the same name | `prop_crossed_stacks_without_removal_counters_fabricate` |
| `crossed_collections_race` (Q24 of P4) | `CounterConflict.v:953` | `crossed_stacks_race` | `prop_crossed_stacks_race` |
| `unordered_writes_commute` (Q26 of P4) | `CounterConflict.v:1651` | the bag row | `prop_bag_writes_commute` |
| `set_retirements_race` | `CounterConflict.v:1992` | F24, the same name | `prop_set_retirements_race` |
| `unordered_set_writes_commute` | `CounterConflict.v:2024` | F25, the same name | `prop_unordered_set_writes_commute` |
| `pathmap_token_race` | `CounterConflict.v:2205` | the same name ([decision 51](decision-register.md#decision-51)) | `prop_pathmap_token_race` |

The plan of the model is a section variable, so the race theorems of P4 also hold for the stack and the cell.
A cell operation that fails with `CellOccupied` fails its deploy before it stages anything.
The block index leaves a failed deploy out of the merge (`casper/src/rust/merging/block_index.rs:127-130`).
So `shared_collection_race` covers every pair of cell operations that a merge can see.

`CounterConflict.tla` models stacks when a configuration replaces `Lifo` with `LifoOn`.
`MC_CounterConflict_Stacks.cfg` checks `NoMergedOrderFabrication`, `SharedQueueRaces` and `FirstOpsRace` for stacks, and TLC finds 46,656 states with no violation.
`RemovalCounterFreeUnsafe.cfg` refutes exactly `NoMergedOrderFabrication`: a pop that does not bump the counter admits a merge that no serial order gives.
Its first counterexample needs only one stack: a pop against a peek and a push.
The four new Rocq negative controls fail inside their expected declarations.

At the end of wave W2, `rholang/tests/reified_merge_p5.rs` held 8 tests and 5 properties.
The tests compute the merge as `dag_merger.rs` does, and each race test also strips the counter events, after which the branches merge.
Two `new` blocks inside `use` of one Array or Vector space race on the allocator `A(sid)` (`rholang/tests/reified_merge_p5.rs:930-991`).
The tests of items 18 and 19 of the consensus checklist are at `rholang/tests/reified_merge_p5.rs:1161-1300`.
The test of item 18 for the joins of P5c is at `rholang/tests/reified_merge_p5.rs:2152-2235`.

Wave W3 extends these checks to the set, the priority queue and the joins, and it adds 8 tests and 2 properties.
So `reified_merge_p5.rs` now holds 16 tests and 7 properties.
A set has no counter, so its merges follow the HEAD checks of its records.
Writes to one set merge, and the merged view is the view of each serial order (F25, `rholang/tests/reified_merge_p5.rs:1857-1896`).
Two branches that each retire one stored member of a set race in check #1 on the produce of that member (F24, `rholang/tests/reified_merge_p5.rs:1898-1936`).

Two dequeues of one priority queue race on its counter and on the head record that both take (`rholang/tests/reified_merge_p5.rs:1940-1981`).
A peek of a priority queue races with a send and with a dequeue on the counter alone (`rholang/tests/reified_merge_p5.rs:1983-2012`).
The merge index counts no consume of a peek COMM, so without the counter events each of these pairs merges.
A join races on the counter of each of its collections, and it merges with operations on other collections (`rholang/tests/reified_merge_p5.rs:2024-2074`).
A bag join has no counter, and it meets a send on one of its collections as a potential COMM in check #2 (`rholang/tests/reified_merge_p5.rs:2076-2104`).
The properties `prop_set_retirements_race` and `prop_unordered_set_writes_commute` run 16 cases each (`rholang/tests/reified_merge_p5.rs:2239-2311`).

Phase 2 of the PathMap engine adds headline 15, `pathmap_token_race`, to `CounterConflict.v` (R129).
Two deploys from one pre-state that each run a PathMap step that bumps the token race in check #1 on $`S(\mathit{sid})`$.
A step bumps the token when its discipline is ordered, or when its transaction changed the state ([decision 51](decision-register.md#decision-51)).
The predicate `pstep_changes` of the model is the `changed` rule of the engine, with the class retire of a set added.
Its two new negative controls fail inside `exec_pmap_consumes_token` and `store_changes`.

The test `space_collection_conflicts_pathmap_token_race` checks that two writes to disjoint paths of one bag space race on the token (`rholang/tests/reified_merge_p5.rs:2375-2416`).
A bag branch that only peeks bumps no token, so it merges with a write, and a peek of a queue races with a write.
The property `prop_pathmap_token_race` generates pairs of sends and peeks on the bag and the queue (`rholang/tests/reified_merge_p5.rs:2457-2517`).
Two branches race exactly when both bump the token, and the same logs without the token events merge.
So `reified_merge_p5.rs` now holds 17 tests and 8 properties.

### 12.12 Determinism and the pattern-enforcement matrix

These suites cover the six families that were wired when the second wave started.
They are the bag, queue, stack and cell of `hashmap`, and the queues of `array` and `vector`.
At the end of wave W2, `rholang/tests/reified_determinism.rs` held 6 tests and 3 properties, and `rholang/tests/reified_pattern_enforcement.rs` held 8 tests and 1 property.

**Canonical iteration.**
The hot store puts each new entry at the front of its list.
A checkpoint sorts each list by its encoded bytes (`rspace++/src/rspace/serializers/serializers.rs:18-26`), and the next reads load it in that order (`rspace++/src/rspace/rspace/ispace_impl.rs:60-95`).
The tests build one entry set in three list orders (`rholang/tests/reified_determinism.rs:528-559`, `671-730`).
The orders are the order of the agent operations, a reverse order through raw host operations, and the order of the history.

One probe script gives the same results, events, legacy charges and root in the three orders.
Each test also runs one Rholang program in two fresh runtimes and compares the root, the event log and the cost (`rholang/tests/reified_determinism.rs:1053-1067`).

**Host work and the list order.**
A head scan of a queue, a stack or a cell does the same work in every list order (`rholang/src/rust/interpreter/spaces/generic.rs:829-845`).
So the host work of these disciplines does not depend on the list order.

The DR-75 order of a bag charges each comparison of its heapsort (`rspace++/src/rspace/space_store_handle.rs:432-460`, `shared/src/rust/fallible_sort.rs:3-33`).
The number of comparisons depends on the list order, so the host work of a bag selection depends on it too.
The `MeteredOrder` of the cost-accounted-rho branch charges in the same way, so this follows [decision 40](decision-register.md#decision-40).
The list order is a function of the pre-state root and the event sequence, so play and replay charge the same.
The headlines Q62 and Q63 of section [11.10](#1110-metering-of-decision-42) state the permutation invariance of the head scans, not of the DR-75 order.

**Serialization failure.**
An agent serializes candidates only to order a tie.
A bag uses the DR-75 tie digest (`rspace++/src/rspace/candidate_order.rs:122-135`, `rspace++/src/rspace/space_store_handle.rs:473-478`).
A queue, a stack or a cell uses the tie-break encode of two entries with one order (`rholang/src/rust/interpreter/spaces/disciplines/queue.rs:88-132`, `rholang/src/rust/interpreter/spaces/disciplines/stack.rs:227-271`).
The agent never stores two entries with one order, so the ordered tests store such entries with raw host operations.

The injection is a host-work cut ([decision 42](decision-register.md#decision-42)).
A budget that ends exactly at the first read of the step makes the selection fail with `HostWorkRejected` before it stages anything.
The rows, the legacy charges, the events and the root stay unchanged (`rholang/tests/reified_determinism.rs:1784-1974`).
The tests check this on a play space, on a rigged replay space and after a checkpoint reset.

The search for the exact cut compares two runs that differ only in the payload length of the tie.
For a limit $`L`$, $`r_1(L)`$ and $`r_2(L)`$ are the rejections of the two runs.
$`u_1(L)`$ and $`u_2(L)`$ are their usages of the three agent dimensions.
The first limit at which the two runs differ is

```math
U^{*} = \min \{\, L \mid (r_1(L), u_1(L)) \neq (r_2(L), u_2(L)) \,\}
```

The cut is $`U^{*}`$ less the scanned bytes of the reservations from the first read of the step to the payload read (`rholang/tests/reified_determinism.rs:1668-1729`).
The rejection alone is not monotone in $`L`$, because a byte vector is encoded one byte at a time.
So the search compares the usages too.

**The pattern-enforcement matrix (N3).**
For each wired row, a pattern for 42 never receives 43 in either order (`rholang/tests/reified_pattern_enforcement.rs:317-365`).
A pattern for 42 receives 42 on a fresh name.
The unsupported pairs fail at every layer: the URN, the registration, the constructor, the agent and a Rholang deploy (`rholang/tests/reified_pattern_enforcement.rs:648-769`).
The qualifiers `temp` and `seq` give `QualifierUnavailable` until P7 (F17), and for an Array family this error comes before `MissingParam` (`rholang/tests/reified_pattern_enforcement.rs:779-824`).

*Table 78. The pattern-enforcement matrix after the joins of wave W3.*

| Row | `default` | `temp` | `seq` |
|---|---|---|---|
| the 18 wired rows: the bag, set, cell, queue, stack and priority queue of `hashmap`, `array` and `vector` | runs: a pattern for 42 never receives 43 | `QualifierUnavailable` | `QualifierUnavailable` |
| the join rows: the queue, the bag and the set of `hashmap` | runs: a join for 42 never receives 43 | `QualifierUnavailable` | `QualifierUnavailable` |
| the 6 PathMap rows: the bag, queue, stack, set, cell and priority queue of `pathmap` | runs: a pattern for 42 never receives 43 | `QualifierUnavailable` | `QualifierUnavailable` |
| the HashSet families, `hashmap:vectordb`, `vector:vectordb` | no factory | no factory | no factory |
| `pathmap:vectordb`, `array:vectordb`, `hashset:vectordb` | `UnsupportedPair` | `UnsupportedPair` | `UnsupportedPair` |

Phase 2 of the PathMap engine (section [12.16](#1216-the-pathmap-engine)) wires the PathMap families and adds their six rows.
P6 wires `hashmap:vectordb` and `vector:vectordb`, and section [15.5](#155-tests) adds their rows.
Only the HashSet families then have no factory.

**Wave W3.**
The joins batch extends both suites to the 18 wired families and to the joins.
`reified_determinism.rs` adds 7 tests (`rholang/tests/reified_determinism.rs:1127-1183`, `1213-1298`, `2041-2100`).
They check the canonical iteration of set views, of the priority-queue order, of the Array and Vector rows and of join groups.
They also inject the serialization failure into the set and the priority queue, and `prop_candidate_serialize_failure_cut_is_exact` now covers both.

`reified_pattern_enforcement.rs` adds 12 rows (`rholang/tests/reified_pattern_enforcement.rs:395-467`).
They are the set and the priority queue of `hashmap`, and the bag, stack, set, cell and priority queue of `array` and `vector`.
Three join rows check that `for (@42 <- a & @y <- b)` never receives the record 43 of `a`, in both orders (`rholang/tests/reified_pattern_enforcement.rs:500-586`).
The rows use the queue, the bag and the set of `hashmap`, and the records and the join waiter stay.
A join for 42 on two fresh names receives 42.
Now `reified_determinism.rs` holds 13 tests and 3 properties, and `reified_pattern_enforcement.rs` holds 23 tests and 1 property.

**Phase 2 of the PathMap engine.**
`reified_pattern_enforcement.rs` adds the six rows `pattern_enforcement_pathmap_*` (`rholang/tests/reified_pattern_enforcement.rs:1021-1101`).
A name of a PathMap space is the path of one element, so its record stays on the data channel of that path.
The waiter for 42 stays on the index channel of the name, and no COMM touches these channels or the root index channel $`I(\varepsilon)`$.
The token orders every operation of the space, so the orders of a counted discipline run across the two names.
Now `reified_pattern_enforcement.rs` holds 29 tests and 1 property.

### 12.13 The set

`SetDiscipline.v` proves the 8 headlines of Table 79 with no axiom, and its 16 negative controls fail inside their expected lemmas.

*Table 79. The headlines of the set.*

| Headline | Location | Property test |
|---|---|---|
| `set_key_injective` | `SetDiscipline.v:297` | `prop_set_key_injective` |
| `set_view_dedups_equal_pars` | `SetDiscipline.v:704` | `prop_set_view_dedups_equal_pars` |
| `set_consume_retires_class` | `SetDiscipline.v:792` | `prop_set_consume_retires_class` |
| `set_retire_comm_lines_up` | `SetDiscipline.v:872` | `prop_set_retire_comm_lines_up` |
| `set_produce_fire_retires_stored_class` | `SetDiscipline.v:1151` | `prop_set_produce_fire_retires_stored_class` |
| `set_peek_restores_representative_only` | `SetDiscipline.v:1205` | `prop_set_peek_restores_representative_only` |
| `set_membership_full_compare_on_equal_hash` | `SetDiscipline.v:1274` | `prop_set_membership_full_compare_on_equal_hash` |
| `set_key_trace_covered` | `SetDiscipline.v:1389` | `prop_set_key_trace_covered` |

`rholang/tests/reified_set.rs` holds 21 tests and 10 properties, and `set.rs` holds 6 unit tests.
The property `prop_set_class_retired_any_schedule` checks the TLA+ invariant `SetClassRetired` (V6), and `prop_set_legacy_charges_equal_host` compares the legacy charges with `ChargingRSpace`.
A host differential runs each generated program on the host where the host can express it, and 62 of 64 programs compare fully.

The membership test of the driver compares each candidate key with the class key once, so the driver keeps no ordered map of classes.
A metered job stores only a record whose key exists under its metering.
The test `space_set_discipline_failed_canonicalization_rejects_before_staging` shows a metered send of a payload without a key that fails before it stages anything.

### 12.14 The priority queue

`PriorityQueue.v` proves 20 headlines, and `RecordEnvelope.v` adds the 2 slot headlines of the priority queue (Table 80).
The 35 negative controls of `PriorityQueue.v` and the 5 new controls of `RecordEnvelope.v` fail inside their expected lemmas.

*Table 80. The headlines of the priority queue.*

| Headline | Location | Property test |
|---|---|---|
| `pq_level_order` | `PriorityQueue.v:863` | `prop_pq_level_order` |
| `pq_unprioritized_lowest` | `PriorityQueue.v:887` | `prop_pq_level_order` |
| `pq_produce_preserves_inv` | `PriorityQueue.v:1390` | `prop_pq_no_pending_match_any_schedule` |
| `pq_consume_preserves_inv` | `PriorityQueue.v:1404` | `prop_pq_no_pending_match_any_schedule` |
| `pq_settle_oldest_first` | `PriorityQueue.v:1899` | `prop_pq_oldest_first_any_schedule` |
| `pq_restricted_waiter_in_level_queue` | `PriorityQueue.v:1921` | `prop_pq_fifo_order_any_schedule` |
| `pq_unrestricted_first_matching_level` | `PriorityQueue.v:1943` | `prop_pq_fifo_order_any_schedule` |
| `pq_fifo_within_level` | `PriorityQueue.v:2151` | `prop_pq_fifo_order_any_schedule` |
| `pq_head_blocks_within_level` | `PriorityQueue.v:2324` | `prop_pq_fifo_order_any_schedule` |
| `pq_peek_restores_head` | `PriorityQueue.v:2486` | `prop_pq_no_pending_match_any_schedule` |
| `pq_reissue_takes_level_fifo_place` | `PriorityQueue.v:2673` | `prop_pq_reissue_keeps_ord_any_schedule` |
| `pq_reentered_waiter_keeps_seniority` | `PriorityQueue.v:2716` | `prop_pq_reissue_keeps_ord_any_schedule` |
| `pq_settle_keeps_unspent_records` | `PriorityQueue.v:2864` | `prop_pq_fifo_order_any_schedule` |
| `pq_settle_terminates` | `PriorityQueue.v:3181` | `prop_pq_settle_bounded_any_schedule` |
| `pq_plan_permutation_invariant` | `PriorityQueue.v:3655` | `prop_pq_plan_permutation_invariant` |
| `pq_priority_domain` | `PriorityQueue.v:3730` | `prop_pq_priority_domain` |
| `pq_no_level_storage` | `PriorityQueue.v:3903` | `prop_pq_no_level_storage` |
| `pq_without_settle_breaks_inv` | `PriorityQueue.v:4002` | `prop_pq_no_pending_match_any_schedule` |
| `pq_settle_order_matters` | `PriorityQueue.v:4081` | `prop_pq_oldest_first_any_schedule` |
| `level_scan_trace_covered` | `PriorityQueue.v:4550` | `prop_pq_reservation_cut_rejects_without_effect` |
| `datum_priority_slot_roundtrip` | `RecordEnvelope.v:1506` | `prop_pq_record_slots_roundtrip` |
| `continuation_policy_slot_roundtrip` | `RecordEnvelope.v:1542` | `prop_pq_record_slots_roundtrip` |

`PriorityQueue.tla` checks `PqLevelOrder` (V17), `PqOldestFirst` (V18), `PqNoPendingMatch` (V19), `NoRecordLost` (V20), `ReissueKeepsOrd` (V21) and `TxnFiresBounded` (V22).
`MC_PriorityQueue.cfg` finds 3,534,991 states at depth 23, and `MC_PriorityQueue_Deep.cfg` finds 16,580,900 states at depth 33, with no violation.

*Table 81. The Unsafe controls of the priority-queue model.*

| Control | What it changes | Refuted invariant |
|---|---|---|
| `PqNoSettleUnsafe.cfg` | the queue never settles | `PqNoPendingMatch` |
| `PqAnyOrderUnsafe.cfg` | the settle fires any fireable lane head | `PqOldestFirst` |
| `PqReentryAtHeadUnsafe.cfg` | a re-entry always reaches the head of its level | `PqLevelOrder` |
| `PqPerPhaseCountUnsafe.cfg` | each phase of a transaction starts a count of its own | `TxnFiresBounded` |
| `PqPlanOpensAtCapUnsafe.cfg` | a plan opens at the cap with no check | `TxnFiresBounded` |

`PqReentryAtHeadUnsafe.cfg` uses a narrow universe with 7 operations, because the shortest violating trace has 7 rounds.
Each complement run holds the other invariants.

`rholang/tests/reified_priority_queue.rs` holds 28 tests and 12 properties, and `priority_queue.rs` and `record.rs` hold 15 unit tests of the priority queue.
The property `prop_pq_txn_fires_bounded_at_the_cap` reaches 54 settle jobs at the cap and 96 transactions with `FIRE_ROUND_CAP` fires.

### 12.15 The joins of one space

`JoinCascade.v` proves 17 of the 19 headlines of Table 82 with no axiom, and its 23 negative controls fail inside their expected lemmas.
`RecordEnvelope.v` proves `continuation_v2_roundtrip`, and `CounterConflict.v` proves `join_ops_race_on_every_counter`.

*Table 82. The headlines of the joins.*

| Headline | Location | Property test |
|---|---|---|
| `join_search_is_head_greedy` | `JoinCascade.v:1468` | `prop_join_search_is_head_greedy` |
| `join_produce_search_agrees_with_head` | `JoinCascade.v:1871` | `prop_join_produce_search_agrees_with_head` |
| `join_incoming_binds_once` | `JoinCascade.v:1146` | `prop_join_incoming_binds_once` |
| `join_fires_heads_only` | `JoinCascade.v:3210` | `prop_join_fires_heads_only` |
| `join_repeated_channel_distinct_records` | `JoinCascade.v:1244` | `prop_join_repeated_channel_distinct_records` |
| `join_set_bind_retires_class` | `JoinCascade.v:2973` | `prop_join_set_bind_retires_class` |
| `cascade_terminates` | `JoinCascade.v:4497` | `prop_cascade_terminates_any_schedule` |
| `join_txn_fires_bounded` | `JoinCascade.v:4642` | `prop_join_txn_fires_bounded_at_the_cap` |
| `cascade_restores_inv` | `JoinCascade.v:7230` | `prop_cascade_restores_inv_any_schedule` |
| `cascade_order_independent` | `JoinCascade.v:3144` | `prop_cascade_order_independent` |
| `lift_reoffer_equals_direct_comm` | `JoinCascade.v:3576` | `prop_lift_reoffer_equals_direct_comm` |
| `cascade_bumps_touched_counters` | `JoinCascade.v:4056` | `prop_cascade_bumps_touched_counters` |
| `bind_permutation_restores_order` | `JoinCascade.v:2148` | `prop_bind_permutation_restores_order` |
| `plan_then_lock_stable` | `JoinCascade.v:5723` | `prop_plan_then_lock_covers_every_staged_channel` |
| `join_reissue_keeps_bind_ords` | `JoinCascade.v:4130` | `prop_join_reissue_keeps_bind_ords` |
| `peek_join_restores_stored_candidates` | `JoinCascade.v:5156` | `prop_peek_join_restores_stored_candidates` |
| `greedy_join_can_miss_assignment` | `JoinCascade.v:1345` | `prop_greedy_join_limit` |
| `continuation_v2_roundtrip` | `RecordEnvelope.v:1734` | `prop_continuation_v2_roundtrip` |
| `join_ops_race_on_every_counter` | `CounterConflict.v:1947` | `prop_join_ops_race_on_every_counter` |

The cascade invariant (CI) has one argument for every discipline that cascades: the head-only disciplines (decision 11) and the set (decision 50).
CI says that every fireable waiter has a dirty collection.
At the start, the settle before the plan makes the scope dirty.
Each waiter that is fireable at the start binds a collection of the scope (a premise of the headline).
When the settle ends without a stop, NEXT-FIRE finds nothing, so INV_head holds, and CI holds with no dirty collection when the plan opens.

Each step keeps CI.
A fire or a store makes dirty the collections that it changes, and a waiter away from those collections keeps its fireability.
A park adds a waiter that is not fireable, and TOUCH changes no record and no waiter.
A retirement of a set removes records of the bound collections only, and these collections are dirty.
When NEXT-FIRE finds nothing, CI gives INV_head: no waiter is fireable, guarded waiters included.

A bag needs no cascade.
A removal from a bag never makes greedy succeed (`ref_fill_mono`).
OFFER stores a record only when no waiter of its collection fires, and RECEIVE parks a waiter only when greedy finds no match for it.
So INV_scan holds at every end (the bag clause of `cascade_restores_inv`).

The monotonicity of plan 3.6 fails for a set (finding F-JF-1).
A class sits at the place of its first member, and the class moves when that member leaves while a persistent member stays.
`set_join_class_shift_breaks_inv_scan` computes a transaction of the set without a cascade: it starts with INV_scan and ends without it (`~ INV_scan`).
Decision 50 makes joined set collections cascade, and the same lemma computes that RUN-JOIN then ends with INV_head.

The engine follows the alignment points of 2026-10-07.
A settle job has no plan, and it loads its pending second sends before any fire (DV-1).
The plan opens only below the fire cap (DV-2).
A peek fire sends the element in flight again, also when it is persistent (DV-3, decision 30).
A stop that owes nothing re-issues its resume only (DV-5).
The element of the operation never stays in the pending second sends of a settle job (A1).

`JoinCascade.tla` checks `QuiescentNoPendingMatch`, `JoinFiresHeadsOnly`, `DistinctRecordsPerBind`, `IncomingBoundOnce`, `CascadeMeasureDecreases`, `TxnFiresBounded`, `NoRecordLost`, `ReissueKeepsBindOrds` and `TouchedCountersBumped` on the queue, the stack, the bag and the set (Table 83).

*Table 83. The safe configurations of the join model.*

| Configuration | Discipline and bounds | States | Depth |
|---|---|---|---|
| `MC_JoinCascade_Queue.cfg` | queue, c1 and c2, all four groups, patterns a and any, guard none, 4 rounds | 3,554,626 | 5 |
| `MC_JoinCascade_Stack.cfg` | stack, c1 and c2, all four groups, pattern any, guards none and onlyB, 4 rounds | 901,390 | 5 |
| `MC_JoinCascade_Bag.cfg` | bag, c1 and c2, groups <c2>, <c1,c2> and <c1,c1>, patterns a and any, guards none and onlyB, 4 rounds | 20,853,878 | 5 |
| `MC_JoinCascade_Set.cfg` | set, c1, groups <c1> and <c1,c1>, patterns a and any, guard none, 5 rounds (the class move) | 3,919,424 | 6 |

Every configuration uses the payloads a and b, a cap of 2 fires, and persistent and peek operations, and no configuration finds a violation.

*Table 84. The Unsafe controls of the join model.*

| Control | What it changes | Refuted invariant |
|---|---|---|
| `NoLiftUnsafe.cfg` | the drain has no cascade | `QuiescentNoPendingMatch` |
| `NoSetCascadeUnsafe.cfg` | the set has no cascade (decision 50 reverted) | `QuiescentNoPendingMatch` |
| `PlainRestoreUnsafe.cfg` | a second send is stored plainly | `QuiescentNoPendingMatch` |
| `StoredRecordTwiceUnsafe.cfg` | the search ignores its claims | `DistinctRecordsPerBind` |
| `IncomingTwiceUnsafe.cfg` | the incoming record binds at every matching position | `IncomingBoundOnce` |
| `HeadOnIncomingOnlyUnsafe.cfg` | the produce side checks one head only | `JoinFiresHeadsOnly` |
| `JoinPerPhaseCountUnsafe.cfg` | each phase starts a fire count of its own | `TxnFiresBounded` |
| `RestoreDroppedUnsafe.cfg` | a peek fire sends nothing again | `NoRecordLost` |
| `JoinReissueFreshOrdsUnsafe.cfg` | a re-issued persistent receive carries no ords | `ReissueKeepsBindOrds` |
| `LazyTouchUnsafe.cfg` | TOUCH bumps the own collections only | `TouchedCountersBumped` |
| `PeekPersistUnsafe.cfg` | a receive peeks and persists | `CascadeMeasureDecreases` |

Each complement run holds the other invariants.
`NoSetCascadeUnsafe.cfg` uses 5 operations, because the class move needs three records, a parked join and a receive.

**The suites.**
`rholang/tests/reified_joins.rs` holds 34 tests of the join engine.
They cover the search of each discipline, the classes and the retirement of the set, the heads, the cascade and its round cap.
They also cover the aborts of cross-space joins, merge check 3 for a parked join, replay, restart, the legacy charges and the reservation cut.
`space_joins_queue_programs_run_as_joins` runs as joins the five programs of the P4 test `space_queue_join_rejected`, which decision 29 disabled (`rholang/tests/reified_joins.rs:2452-2577`).

`rholang/tests/reified_join_props.rs` holds 23 properties, the negative control `reference_model_reports_owed_pairs`, and the keyed differential of the store seams.
The properties are the 19 of Table 82, one each for `NoRecordLost`, `QuiescentNoPendingMatch` and `CascadeMeasureDecreases`, and `prop_join_engine_agrees_with_drivers_without_joins`.
Each property runs 64 cases with the fixed P5 seed.

**The store seams.**
The record methods of `JoinSource` ([design N12.11](design.md#n1211-the-seam-of-a-store)) must leave the keyed engine unchanged, and two tests check this.
`space_join_engine_reserved_types_keep_their_sizes` pins the sizes of the ten types that the engine reserves by size (`rholang/src/rust/interpreter/spaces/join.rs:5607-5629`).
`join_seams_keyed_differential` runs 80 programs with the fixed P5 seed over a metered recording host (`rholang/tests/reified_join_props.rs:4622-4687`).
For each program, it hashes the results, the legacy charges, the phlo, the events, the lock keys, the staged operations and the root.
It also hashes the host-work usage at every begin, staged operation and commit.

The golden `test-vectors/reified-join-seams-v1.json` holds these digests at the commit `930b1d64c`, before the seams.
The corpus covers the six disciplines with 2,341 staged operations.
Joins occur in 60 programs, peeks in 51 and persistent operations in 42.
Four programs reach the fire cap, and eight refuse a second cell record.

### 12.16 The PathMap engine

Phase 1 of wave W3 adds the PathMap store, its markers and its delivery plans ([design N13](design.md#n13-wave-w3-of-p5-the-pathmap-engine), [PathMap delivery](pathmap-prefix-delivery.md)).
`PathMapPrefix.v` proves the 11 headlines of Table 85 in addition to the 6 headlines of section [12.7](#127-the-pathmap-layer), with no axiom.
Its 37 new negative controls fail inside their expected lemmas, and the 17 controls of wave W2 still fail inside theirs.

*Table 85. The headlines of the PathMap engine (Phase 1).*

| Id | Headline | Location | Property test |
|---|---|---|---|
| R97 | `marker_record_roundtrip` | `PathMapPrefix.v:1350` | `prop_marker_record_roundtrip` |
| R98 | `vehicle_carries_payload` | `PathMapPrefix.v:1382` | `prop_vehicle_carries_payload` |
| R99 | `path_element_rules` | `PathMapPrefix.v:1455` | `prop_path_element_rules` |
| R100 | `delivery_vehicle_single_channel` | `PathMapPrefix.v:1911` | `prop_pathmap_delivery_is_single_channel` |
| R101 | `prefix_contact_anchored` | `PathMapPrefix.v:1948` | `prop_candidates_cover_the_prefix_in_key_order` |
| R102 | `peek_restores_delivered_marker` | `PathMapPrefix.v:2010` | `prop_marker_invariant_preserved`, `prop_waiter_prefixes_are_most_specific_first` |
| R103 | `no_match_remains` | `PathMapPrefix.v:2380` | `prop_candidates_cover_the_prefix_in_key_order`, `prop_waiter_prefixes_are_most_specific_first`, `prop_pathmap_no_match_remains_any_schedule` |
| R104 | `marker_invariant_preserved` | `PathMapPrefix.v:2560` | `prop_marker_invariant_preserved` |
| R105 | `heads_no_match` | `PathMapPrefix.v:3038` | `prop_pathmap_head_cascade_leaves_no_fireable_head` |
| R106 | `token_ord_reentry` | `PathMapPrefix.v:3126` | `prop_pathmap_token_orders_reenter_at_their_place` |
| R107 | `prefix_scan_trace_covered` | `PathMapPrefix.v:3268` | `prop_pathmap_reservation_cut_rejects_without_effect` |

The integration suite `rholang/tests/reified_pathmap.rs` of Phase 2 holds the five properties of Table 85 that Phase 1 left pending, and `prop_pathmap_merge_detects_prefix_contact` of V25.
Deterministic witness tests cover these headlines too, for example `a_removal_retires_the_record_and_every_other_marker` and `head_orders_take_the_heads_of_a_path`.

`PathMapPrefix.tla` runs the bag discipline on prefixes, with the second send of Rule E and a fork into two branches that merge under check #2.
It checks `NoMatchRemains` (V23), `MarkerCount` (V24) and `MergeDetectsPrefixContact` (V25), and Table 86 lists its configurations.

*Table 86. The configurations of the PathMap engine model.*

| Configuration | Bounds | Distinct states | Result |
|---|---|---|---|
| `MC_PathMapPrefix.cfg` | four paths, all patterns, three operations | 341,931 at depth 4 | no violation |
| `MC_PathMapPrefix_Peek.cfg` | a chain of three paths, four operations | 51,074 at depth 5 | no violation |
| `MC_PathMapPrefix_Merge.cfg` | four paths, two operations, then a fork of two branches | 320,034 at depth 4 | no violation |

*Table 87. The Unsafe controls of the PathMap engine model.*

| Control | What it changes | Refuted invariant |
|---|---|---|
| `SpaceWideIndexUnsafe.cfg` | every marker on one space-wide index channel | `MergeDetectsPrefixContact` |
| `PathMapExactPathOnlyUnsafe.cfg` | a send meets the waiters of its exact path only | `NoMatchRemains` |
| `PathMapNoRetireUnsafe.cfg` | a delivery removes only the marker of its COMM | `MarkerCount` |
| `PathMapMarkerNotRestoredUnsafe.cfg` | the second send does not restore the marker of its COMM | `MarkerCount` |

Each complement run holds the other invariants.
A run with three operations and a fork after every receive passed 63 million states before it was stopped.
So the merge configuration forks after two operations.

The modules `markers.rs`, `delivery.rs` and `outer/pathmap.rs` hold 27 unit tests, 7 of them properties.
The test `pathmap_segment_codec_matches_epm1_vectors` checks the channel ids of the store against the EPM1 transcription vectors.

**Phase 2.** The prefix source `spaces/pathmap/join.rs` runs every PathMap operation on the join engine ([design N13](design.md#n13-wave-w3-of-p5-the-pathmap-engine)).
It also runs the joins over prefixes of wave W4, and five unit tests in the module check its keys and depths.

Phase 2 adds no headline to `PathMapPrefix.v`, whose 17 headlines keep their lines, and its 54 negative controls still fail inside their lemmas.
An addendum of the module traces each method of the prefix source to its definitions.
It also records the marker authority and the restore of FINISH.
`PathMapPrefix.tla` does not change, because it models no fire cap and the engine adds no modeled step.

`rholang/tests/reified_pathmap.rs` holds 33 tests and 6 properties.
Section [11.4](pathmap-prefix-delivery.md#114-integration-tests-and-properties) of the PathMap delivery describes the tests, and its Table 6a lists the properties.
The tests drive the six PathMap families through the agent and through Rholang programs.
They cover the FIPS example, the unit rule, the most specific waiter, the markers, the vehicle, the merge contact, replay, restart and the legacy charges.
Three cap tests check the restore of FINISH: the owed markers return before the commit, and a stale carried offer stores nothing.
The invariant map now has no pending property.

### 12.17 Running the P5 checks

To run the P5 checks, do these steps from the repository root, after the preparation of section [10.1](#101-preparation):

1. Run `cargo fmt --all -- --check`, `cargo clippy -p rholang --lib --tests -- -D warnings` and `cargo check --workspace --all-targets`.
2. Run `scripts/check-reified-rspaces-proofs.sh` and `scripts/check-reified-rspaces-tla-invariants.sh`.
3. Run `python3 scripts/verify-reified/check-invariant-map.py --phase P5`.
4. Run `python3 scripts/verify-reified/check-model-map.py --map formal/reified-rspaces-model-map-p5.json`.
5. Run `python3 scripts/verify-reified/check-complete.py --phase p5` and `check-complete.py --production-scan`.
6. Run `python3 scripts/verify-reified/check-footprint.py --policy p5`.
7. Run `python3 scripts/verify-reified/check-consensus-checklist.py --milestone p5`.
8. Run `cargo test -p rholang --test <suite>` for each suite of Table 88, and `cargo test -p rholang --lib`.
9. Run `python3 scripts/verify-reified/doc_lint.py` and `docs/rspace/reified-rspaces/diagrams/render.sh --check`.
10. Run `cargo build --release -p node`.

Run each cargo command under the resource limits of section [10.3](#103-rust-suites), and write its log under `target/`.
Step 10 is necessary because `cargo check` computes no layout of an async state machine ([design N1](design.md#n1-module-map-and-hook-lines)).

*Table 88. The P5 suites.*

| Suite | Tests | Section |
|---|---|---|
| `reified_queue_parity` | 7 | [12.1](#121-the-queue-parity-golden) |
| `reified_p5_harness` | 2 | [12.2](#122-the-engine-spine-s1) |
| `reified_engine_props` | 20 | [12.3](#123-the-engine-models) |
| `reified_bag` | 31 | [12.4](#124-the-bag-and-the-scan-driver) |
| `reified_cell` | 21 | [12.5](#125-the-cell) |
| `reified_array_vector` | 40 | [12.6](#126-the-array-and-vector-stores-and-the-allocator) |
| `reified_families` | 16 | [12.8](#128-the-families) |
| `reified_hooks_p5` | 10 | [12.9](#129-the-receive-queries-and-the-allocation-jobs) |
| `reified_stack` | 34 | [12.10](#1210-the-stack-and-the-fire-count-of-a-transaction) |
| `reified_merge_p5` | 25 | [12.11](#1211-the-merge-of-the-p5-families) |
| `reified_determinism` | 16 | [12.12](#1212-determinism-and-the-pattern-enforcement-matrix) |
| `reified_pattern_enforcement` | 30 | [12.12](#1212-determinism-and-the-pattern-enforcement-matrix) |
| `reified_set` | 31 | [12.13](#1213-the-set) |
| `reified_priority_queue` | 40 | [12.14](#1214-the-priority-queue) |
| `reified_joins` | 34 | [12.15](#1215-the-joins-of-one-space) |
| `reified_join_props` | 25 | [12.15](#1215-the-joins-of-one-space) |
| `reified_pathmap` | 39 | [12.16](#1216-the-pathmap-engine) |

The unit tests of the PathMap layer and of the PathMap engine (sections [12.7](#127-the-pathmap-layer) and [12.16](#1216-the-pathmap-engine)) run in `cargo test -p rholang --lib`.

## 13. Decision 47: spaces in native execution

[Decision 47](decision-register.md#decision-47) runs spaces in native execution, the default path of protocol version 6.
This section verifies the native replay backend of spaces, the metered view reads of the substrate, and the scheduling of space intents.
Section [11.11](#1111-native-execution-of-decisions-46-and-47) verifies the availability rule of the runtime, and the [findings](findings/native-execution-cost-accounting.md) record five items for the cost-accounting owner.

### 13.1 What decision 47 proves

[Native play](glossary.md#native-play) stages each space operation with [metered view reads](glossary.md#metered-view-read) and records a [journal row](glossary.md#journal-row) for it.
[Native replay](glossary.md#native-replay) replays each staged operation against its row through the [native backend](glossary.md#native-backend) of the session.
The checks of this section establish five properties:

1. Native replay over the journal of native play reproduces play: the observer calls, the effects, the status, and one row for each operation (H24, T8).
2. A recorded denial gives the failure of play, with the operations before it applied (H25).
3. A tampered or forged row fails closed with no effect, and no operation waits forever for it (H26, T8, `NoDeadlock`).
4. A view load charges the same on a cache hit and on a miss (H27).
5. The transactions of one space run in one conflict group in operation order (`NoBlockedReadiness`).

### 13.2 Rocq models

`EventCorrelation.v` models the protocol of `NativeBackend::apply_op` over abstract operations, a store, COMMs, footprints, and a budget decision function of the call history.
Native play and native replay share that function, because native replay runs the contract of play.
The section `NativeReplay` defines native play, the outcome fit, and native replay.
Native replay has six steps: the row, the footprint, the fit, the introduction, the candidate with the COMM, and then the effects of play.
The section `ViewCharges` defines a view load over a cache and a history, with the meter `accept_all` of the native history reader.

*Table 89. The Rocq headlines of decision 47.*

| Id | Headline | Statement |
|---|---|---|
| H24 | `native_replay_reproduces_play` (`EventCorrelation.v:8617`) | Over the rows of native play, native replay makes the calls, the effects and the status of play. Play records one row for each operation that it runs. |
| H25 | `native_replay_denial_keeps_prefix` (`EventCorrelation.v:8663`) | When play runs out of phlo at operation k, native replay ends with OutOfPhlogistons, the calls of play, and the store of the first k operations. |
| H26 | `native_replay_fails_closed` (`EventCorrelation.v:8708`) | No ready row, a forged footprint, an outcome that does not fit, and another candidate each end before any effect. Another candidate also leaves its COMM unobserved. |
| H27 | `view_charge_cache_independent` (`EventCorrelation.v:8773`) | With a coherent cache, a view load returns the history value and charges its key and its value only. |

The lemma `native_denial_premise_satisfiable` (`EventCorrelation.v:8850`) shows a run that satisfies the premise of H25, so H25 is not vacuous.
`EventCorrelation.v` keeps 21 closed headlines, and the gate now expects 246 closed headlines in total.

### 13.3 TLA+ model NativeReplay.tla

The user placed the TLA+ models of decision 47 in a new module, `NativeReplay.tla`, so `CheckpointReplay.tla` and its 20 configurations stay unchanged.
The module holds two specifications, and each configuration picks one.
`OutcomeSpec` replays every transaction of up to three stores and fires at every phlo limit up to 6.
Its journal is honest, or it has one tampered well-formed row.
`ScheduleSpec` runs five intents of two spaces and two host operations, with stripes that collide and every footprint-consistent forgery of a space row.

*Table 90. The configurations of NativeReplay.tla.*

| Configuration | Specification | Invariants | Result |
|---|---|---|---|
| `MC_NativeReplay_Outcome.cfg` | `OutcomeSpec` | `NativeReplayOutcomeEqual` (T8) | Holds, 12,630 distinct states |
| `MC_NativeReplay_Schedule.cfg` | `ScheduleSpec` without the native key | `NoBlockedReadiness`, `NoDeadlock` | Hold, 1,741 distinct states |
| `MC_NativeReplay_Schedule_NativeKey.cfg` | `ScheduleSpec` with the native key | `NoBlockedReadiness`, `NoDeadlock` | Hold, 283 distinct states |
| `TamperedOutcomeUnsafe.cfg` | `OutcomeSpec` | `NativeReplayOutcomeEqual` | Refuted |
| `NoSpaceKeyUnsafe.cfg` | `ScheduleSpec` | `NoBlockedReadiness` | Refuted |
| `WaitUnderLocksUnsafe.cfg` | `ScheduleSpec` | `NoDeadlock` | Refuted |
| `NoInvalidateUnsafe.cfg` | `ScheduleSpec` | `NoDeadlock` | Refuted |

The safe scheduling configuration has no native key, so `NoBlockedReadiness` rests on the key [2]||sid of [I-11](decision-register.md#i-11) alone.
The key [2, 0] of native execution is a second guarantee and not a premise, as design v2 states.

### 13.4 Negative controls

*Table 91. The negative controls of decision 47.*

| Control | Kind | What it changes | Must fail in |
|---|---|---|---|
| `ec-native-searches-again` | Rocq mutant | The backend fires against any recorded candidate, as a search of the candidates again would. | `native_replay_fails_closed` |
| `ec-native-ignores-denial` | Rocq mutant | The backend applies an operation whose row records a denial. | `native_replay_reproduces_play` |
| `ec-native-effect-before-prepare` | Rocq mutant | The backend changes the store before it checks the candidate. | `native_replay_fails_closed` |
| `ec-view-charges-on-miss-only` | Rocq mutant | A view load charges its value on a miss only. | `view_charge_cache_independent` |
| `TamperedOutcomeUnsafe` | TLC | Replay skips the outcome and candidate checks. | `NativeReplayOutcomeEqual` |
| `NoSpaceKeyUnsafe` | TLC | Space intents lose [2]\|\|sid and [2, 0]. | `NoBlockedReadiness` |
| `WaitUnderLocksUnsafe` | TLC | A space transaction waits for a row while it holds its stripes. | `NoDeadlock` |
| `NoInvalidateUnsafe` | TLC | A failed space row does not close the epoch. | `NoDeadlock` |
| The epoch-closure control | Rust | The closure of `apply_op` is disabled. | `space_native_forged_footprints_never_hang` |

The four Rocq mutants pass the gate, which now runs 361 negative checks.
With the closure disabled, the Rust control fails with "native replay hangs: space row 29, host row 54" (`target/verification/reified-rspaces/d47/step4/negative-control-no-invalidate.log`).

### 13.5 Property-based tests

The phase D47 of the [invariant map](glossary.md#invariant-map) maps each headline and invariant of decision 47 to a property.
The model map `formal/reified-rspaces-model-map-d47.json` maps the Rust items to them.
Each property runs 64 cases with the fixed seed `0x0D47_5EED_2026_1007` and keeps no failure file.
A plan is a list of transactions of one to three sends and receives over three channels, and the agent's rule decides each staged operation.

*Table 92. The properties of decision 47.*

| Property | Covers | What it checks |
|---|---|---|
| `prop_native_space_replay_reproduces_play` | H24, T8 | Native replay commits the receipts, the store and the produce counters of play, uses one row for each operation, and keeps the epoch open. |
| `prop_native_space_replay_denial_keeps_prefix` | H25, `NoDeadlock` | A recorded denial at one operation fails the commit with OutOfPhlogistons, applies the operations before it, and keeps the epoch open. |
| `prop_native_space_replay_fails_closed` | H26, T8, `NoDeadlock` | A misfit outcome, another candidate, or a row that is not ready fails the commit closed with the prefix applied, and closes the epoch. |
| `prop_view_charge_cache_independent` | H27 | Cold play, cold native replay and warm play reserve the same view charges in the same order. |
| `prop_space_key_keeps_one_space_in_one_group` | `NoBlockedReadiness` | The key [2]\|\|sid puts the intents of each space in one group in operation order, and the native key makes one group. |

The first four properties are in `native_session/space.rs`, and the last is in `reified/schedule.rs`.
The witness tests of the map are the deterministic tests of sections 13.6 and 13.7.

### 13.6 The native certification suite

The suite `reified_native_spaces` certifies space deploys as proposer self-certification and validator replay do, at the v6 limits of the offered funded protocol.
Its harness, `rholang/tests/support/native.rs`, restates the derivations of `production_limits.rs`, because a rholang test cannot depend on casper.
It encodes the budget recording and the journal into the evidence wire and decodes them.
It then runs the journal check, the trace binding, the native replay environment, its evaluation, the completeness check, the usage, and the exported root.

*Table 93. The tests of the native certification suite.*

| Test | Location | What it checks |
|---|---|---|
| `space_native_replay_equal_{hashmap,pathmap,array,vector}` | `reified_native_spaces.rs:280-289` | Each of the 24 families plays natively without error and delivers its results. Native replay reproduces the errors, a complete journal, the usage, the evidence and the root. |
| `space_native_journal_rows_per_operation` | `reified_native_spaces.rs:400` | One row for each produce and consume of the log, one COMM record for each COMM, and sub-order paths numbered from 0 in each transaction. |
| `space_native_handle_calls_after_factory` | `reified_native_spaces.rs:409` | The factory parks the handle first, and each handle call follows the previous operation on the handle. |
| `space_native_persistent_retry_rows` | `reified_native_spaces.rs:478` | A persistent element offered again in flight is a retry of a space row, and native replay consumes the retry. |
| `space_native_denial_replay` | `reified_native_spaces.rs:550-622` | Out of phlo at each charged attempt of the largest space transaction, found by bisection on the limit, play and native replay agree. |
| `space_native_scope_reset_after_restore` | `reified_native_spaces.rs:637` | A second evaluation after a restore reproduces play. |
| `space_native_forged_predecessor_fails_closed` | `reified_native_spaces.rs:680` | The journal check rejects a forged predecessor, and a matching forged footprint fails closed at the footprint check. |
| `space_native_forged_footprints_never_hang` | `reified_native_spaces.rs:776` | Every forgery of a space row on a later host row ends within 60 s and is rejected. |

The unit tests of the backend add `native_space_replay_view_charges_equal_play` (`native_session/space.rs:1750`) and `native_space_replay_rejects_another_valid_candidate` (`native_session/space.rs:1793`).
The scheduler adds `space_native_single_conflict_group` (`reified/schedule.rs:724`) and `space_native_space_key_groups` (`reified/schedule.rs:743`).

### 13.7 The forged-journal probe and the epoch closure

The journal check derives the predecessors of each row from the footprints, so a footprint-consistent forgery passes it.
A probe of 2026-10-07 forged the journal of a queue deploy of 37 rows.
Each forgery added a channel of a later host row to a space row.
Of 109 forgeries, 45 replays ended and were rejected, and 64 replays hung.
The forged space row failed closed, but its ticket only returned the row to the available state, so the later host operation waited forever.

Commit `c6cb371ba` adds the [epoch closure](glossary.md#epoch-closure): `NativeBackend::apply_op` closes the epoch when a staged operation fails, except after a published denial.
Every waiting operation then wakes and fails, and the certification fails closed.
The test `space_native_forged_footprints_never_hang` now ends every forgery, and the control `NoInvalidateUnsafe` of `NativeReplay.tla` shows the hang of the code before the fix.

The probe also forged host rows on later host rows: 66 of 111 replays ended and 45 hung, with no space row involved.
That lost wake-up is in the host path of native replay, outside this campaign, and the [findings](findings/native-execution-cost-accounting.md#6-f-47-4-a-failed-host-operation-leaves-its-waiters-waiting) record it as F-47-4.

### 13.8 Measured facts

- Native execution stamps the cost authority of the deploy into the data of each produce. So operation identities, the order of [decision 41](decision-register.md#decision-41) for bag and set candidates, and the root differ between native and legacy play. The suite therefore compares native play with native replay only.
- Native replay reserves about 1.2 MB of `VerificationBytes` for each budget attempt, after about 36 MB for the environment. The v6 limit bounds a certified deploy near 190 attempts ([M-47-1](findings/native-execution-cost-accounting.md#7-m-47-1-verification-bytes-of-native-replay)).
- A certification has one host-work budget, and a restore does not refund it. So `space_native_scope_reset_after_restore` uses a smaller program, because two evaluations of a family program exceed the limit.

### 13.9 Running the decision-47 checks

To run the decision-47 checks, do these steps from the repository root, after the preparation of section [10.1](#101-preparation):

1. Run `scripts/check-reified-rspaces-proofs.sh` and `scripts/check-reified-rspaces-tla-invariants.sh`.
2. Run `cargo test -p rspace_plus_plus --lib -- native_session::space` and `cargo test -p rholang --lib -- reified_schedule`.
3. Run `cargo test -p rholang --test reified_native_spaces` and `cargo test -p rholang --test reified_native_fail_closed`.
4. Run `python3 scripts/verify-reified/check-invariant-map.py --phase D47`.
5. Run `python3 scripts/verify-reified/check-model-map.py --map formal/reified-rspaces-model-map-d47.json`.
6. Run `python3 scripts/verify-reified/check-footprint.py --policy d47` and `check-complete.py --phase d47`.
7. Run `python3 scripts/verify-reified/check-consensus-checklist.py --milestone d47`.

## 14. Phase P6, step 2: the exact numeric core

[Decision 54](decision-register.md#decision-54) makes every consensus decision of a VectorDB space exact.
Step 2 of the P6 design adds the exact numeric core, `rholang/src/rust/interpreter/tensor/`, and the VectorDB driver of step 3 uses it.
This section records the formal model of the core, its cross-witnesses, its reference vectors and its tests.

### 14.1 What step 2 proves

The core decides each bound and each order of the six metrics with exact rationals, and it never takes a root.
For a query vector $`q`$ and a record vector $`e`$, let $`u = q \cdot e`$ and $`N = \lVert q \rVert^2 \lVert e \rVert^2`$.
The value $`\mathrm{RN}_{53}(x)`$ is the binary64 value nearest to $`x`$, with ties to even.
The checks of this section establish six properties:

1. The squared forms of cosine and euclidean decide the real comparisons (S1 to S3).
2. The bounds are monotone, and the kernels satisfy their metric laws (S4 to S8, S12).
3. A cosine lies in $`[-1, 1]`$ (S9).
4. The order of each metric is a total preorder, and the selection order is a strict total order (S10, S13).
5. Top-k and list mode are deterministic, and they need the tie-break by rank (S14 to S16).
6. Every delivered value is $`\mathrm{RN}_{53}`$ of the exact value (sections 14.4 and 14.6).

### 14.2 The Rocq model Similarity.v

The cosine of two rational vectors is usually irrational, because $`\lVert q \rVert \, \lVert e \rVert`$ is a square root.
So `Similarity.v` proves its decisions in every ordered commutative ring with a decidable order, and the real numbers are such a ring.
With $`s = \lVert q \rVert \, \lVert e \rVert > 0`$, the real comparison and the decision of `within` are:

```math
\cos(q, e) \ge t \iff t\,s \le u,
\qquad
\mathit{cosine\_within}(u, t, N) \iff
(u \ge 0 \land t < 0) \lor
(u \ge 0 \land t \ge 0 \land t^2 N \le u^2) \lor
(u < 0 \land t < 0 \land u^2 \le t^2 N).
```

Headline S1 states that the two sides agree when $`s^2 = N`$, so the decision needs no root.
Headline S11 shows that an embedding of ordered rings keeps the decisions, so the rational decision of the Rust code is the real decision.
Part 5 of the module discharges every hypothesis with the canonical rationals `Qc`, which model `num_rational::BigRational`, and with the embedding of `Z` into `Qc`.

*Table 94. The Rocq headlines of P6 step 2 (formal/rocq/reified_rspaces/theories/Similarity.v).*

| Id | Headline | Statement |
|---|---|---|
| S1 | `cosine_squared_form_iff` (`Similarity.v:412`) | For $`s > 0`$, $`t\,s \le u`$ holds exactly when $`\mathit{cosine\_within}(u, t, s^2)`$. |
| S2 | `cosine_order_squared_form` (`Similarity.v:471`) | With $`s_i^2 = \lVert q \rVert^2 n_i`$, the real order $`u_1 s_2 \le u_2 s_1`$ holds exactly when the order of `better` holds. |
| S3 | `euclidean_radius_squared_form` (`Similarity.v:545`) | For a distance $`d \ge 0`$, $`d \le r`$ holds exactly when $`r \ge 0`$ and $`d^2 \le r^2`$. |
| S4 | `threshold_monotone` (`Similarity.v:565`) | A lower threshold admits every record that a higher one admits (cosine, dot, jaccard). |
| S5 | `radius_monotone` (`Similarity.v:600`) | A larger radius admits every record that a smaller one admits (euclidean, manhattan, hamming). |
| S6 | `dot_laws` (`Similarity.v:658`) | The dot product is symmetric, and its exact sum does not depend on the order of the coordinates. |
| S7 | `distance_laws` (`Similarity.v:745`) | The squared euclidean distance is a squared metric, and manhattan is a metric. |
| S8 | `cauchy_schwarz` (`Similarity.v:812`) | $`(a \cdot b)^2 \le \lVert a \rVert^2 \lVert b \rVert^2`$ for vectors of one length. |
| S9 | `cosine_bounds` (`Similarity.v:830`) | Every record passes the threshold $`-1`$, and no record passes a threshold above 1. |
| S10 | `score_orders_total` (`Similarity.v:931`) | The order of each of the six metrics is a total preorder on its defined scores. |
| S11 | `squared_forms_embed` (`Similarity.v:1000`) | An embedding of ordered rings keeps the decisions of S1 to S3. |
| S12 | `binary_laws` (`Similarity.v:1042`) | Hamming is a metric, and the jaccard counts satisfy $`\lvert q \land e \rvert \le \lvert q \lor e \rvert`$. |
| S13 | `selection_total_order` (`Similarity.v:1169`) | The selection order is irreflexive, transitive, and total on distinct tie keys. |
| S14 | `topk_deterministic` (`Similarity.v:1199`) | Two sorted arrangements of one multiset of candidates are equal. |
| S15 | `list_selection_deterministic` (`Similarity.v:1278`) | The first k eligible candidates depend only on the multiset of records. |
| S16 | `kappa_dropped_not_unique` (`Similarity.v:1311`) | Negative result: by score alone, two equal-score records have two sorted arrangements. |
| S17 | `Qc_ordered_ring` (`Similarity.v:1384`) | `Qc` satisfies every hypothesis of the ordered ring. |
| S18 | `similarity_headlines_Qc` (`Similarity.v:1399`) | S1, S2, S3, S8 and S9 hold in `Qc`. |
| S19 | `squared_forms_embed_Z_Qc` (`Similarity.v:1454`) | S11 holds for the embedding of the integers into the rationals. |
| S20 | `selection_Qc` (`Similarity.v:1480`) | S13 and S14 hold for rational scores. |

The module has 20 closed headlines, and the gate now expects 266 closed headlines in total.
Rocq has no model of IEEE-754 binary64, so the rounding to $`\mathrm{RN}_{53}`$ is verified by the tests and the legs of sections 14.4 and 14.6.

### 14.3 The Z3 cross-witnesses

Z3 decides nonlinear real arithmetic completely, so its checks hold in the real numbers themselves.
Each script refutes the negation of each claim, and each negative control must have a counterexample.

*Table 95. The Z3 scripts of P6 step 2 (formal/z3/reified_rspaces).*

| Script | Claims (`unsat`) | Controls (`sat`) | Result |
|---|---|---|---|
| `squared_form_cosine.py` | S1, S2, S3, S4 for cosine, S5 for euclidean, Cauchy-Schwarz and the bounds of S9 in three dimensions, and a witness that the threshold 1 admits parallel vectors | squares only, swapped norms, an unsquared radius | 12 of 12 pass in 14 s |
| `topk_tiebreak_total_order.py` | the selection order is irreflexive, asymmetric, transitive and total on distinct keys, and the cross-multiplied order is transitive, total and equal to the order of the quotients | the order by score alone, and the cross-multiplied order without a positive union | 10 of 10 pass |
| `metric_algebra.py` | the laws of dot, dist2, manhattan, hamming and jaccard, and the polarization identity $`\lVert a - b \rVert^2 = \lVert a \rVert^2 - 2\,a \cdot b + \lVert b \rVert^2`$ | the triangle inequality of dist2, and the zero law of dot | 17 of 17 pass |

### 14.4 The reference vectors

The Sage leg `formal/sage/reified_rspaces/similarity_metrics.sage` generates `test-vectors/numeric-reference-v1.json` with a fixed seed.
The file holds 24 rounding cases, 15 square-root cases and 143 metric cases.
Sage decides each cosine and euclidean bound with real algebraic numbers, so it does not use the squared forms.
MPFR rounds the normal range, and the subnormal range rounds to multiples of $`2^{-1074}`$.
The leg also checks the metric laws on 200 random rational cases, and it fails when the regenerated file differs from the committed file.

The receipt `scripts/verify-reified/numeric_reference_receipt.py` recomputes every case with the Python standard library only.
It rounds with the correctly rounded integer division of CPython, and it takes square roots with 600 decimal digits.
The Rust test `tensor_numeric_reference_vectors_agree` reads the same file.
So three implementations agree on all 182 cases: the Rust core, the Sage leg and the receipt.

### 14.5 Negative controls

*Table 96. The negative controls of P6 step 2.*

| Control | Kind | What it changes | Must fail in |
|---|---|---|---|
| `sim-squares-without-signs` | Rocq mutant | Cosine compares the squares only. | `cosine_squared_form_iff` |
| `sim-order-swapped-norms` | Rocq mutant | The cosine order multiplies each square by its own norm. | `cosine_order_squared_form` |
| `sim-radius-unsquared` | Rocq mutant | Euclidean compares $`d^2`$ with $`r`$. | `euclidean_radius_squared_form` |
| `sim-total-without-distinct-keys` | Rocq mutant | The selection order claims totality without distinct tie keys. | `precedes_total` |
| `sim-unit-threshold-claimed` | Rocq mutant | The bound claims that no record passes the threshold 1. | `cosine_bounds` |
| `cosine-negative-case-reversed` | Rust mutant | `within` reverses the case of a negative cosine and a negative threshold. | 3 tests |
| `cosine-order-swapped-norms` | Rust mutant | `better` multiplies each square by its own norm. | 2 tests |
| `ties-round-up` | Rust mutant | The rounding breaks a tie upward. | 2 tests |
| `dyadic-sum-keeps-unit` | Rust mutant | DYADIC-SUM shifts its sum but keeps its old unit. | 6 tests (5 at step 2, and the step-6 test `vector_ops_are_exact_before_the_rounding`) |
| `euclidean-radius-unsquared` | Rust mutant | `within` compares $`d^2`$ with $`r`$. | 3 tests |

The five Rocq mutants pass the gate (`rocq-negative-controls.py`), and the Z3 controls are in Table 95.
Step 8a adds the Rust mutant `dot-f64-accumulation` (Table 124).
The harness `scripts/verify-reified/tensor-mutants.py` applies each Rust mutant in turn, runs the tensor tests, and restores the source files.
Each mutant fails the tests that the table counts, and the harness confirms that the restored files equal the originals.

### 14.6 Property-based tests

The phase P6 of the [invariant map](glossary.md#invariant-map) maps each headline to its properties, and the model map `formal/reified-rspaces-model-map-p6.json` maps the Rust items to the headlines.
Each property runs 64 cases with the fixed seed `0x0F6E_5EED_2026_1008` and keeps no failure file.

*Table 97. The properties of P6 step 2 (tensor/exact.rs).*

| Property | Covers | What it checks |
|---|---|---|
| `prop_cosine_threshold_matches_the_squared_form` | S1, S18 | `within` decides $`\cos \ge t`$ as the order of the signed squares. |
| `prop_cosine_order_matches_the_squared_form` | S2, S18 | `better` orders two cosine scores as their exact cosines. |
| `prop_euclidean_radius_matches_the_squared_form` | S3, S18 | A distance is within a radius exactly when $`d \le r`$. |
| `prop_thresholds_are_monotone` | S4 | Lower thresholds admit more records under cosine, dot and jaccard. |
| `prop_radii_are_monotone` | S5 | Larger radii admit more records under euclidean, manhattan and hamming. |
| `prop_exact_kernels_are_permutation_invariant` | S6 | The exact kernels do not depend on the order of the coordinates. |
| `prop_distance_laws_hold` | S7 | dist2 and manhattan satisfy their metric laws. |
| `prop_cauchy_schwarz_holds` | S8, S18 | Cauchy-Schwarz holds exactly. |
| `prop_cosine_bounds_hold` | S9, S18 | Every cosine passes $`-1`$, none passes a threshold above 1, and a delivered cosine lies in $`[-1, 1]`$. |
| `prop_score_orders_are_total_preorders` | S10 | `better` is reflexive, antisymmetric in its result and transitive for each metric. |
| `prop_dyadic_and_rational_kernels_agree` | S11, S19 | The dyadic path and the rational path give equal exact results. |
| `prop_binary_metric_laws_hold` | S12 | Hamming and the jaccard counts satisfy their laws. |
| `prop_selection_order_is_a_strict_total_order` | S13, S20 | The selection order is a strict total order on distinct ranks. |
| `prop_topk_is_deterministic` | S14, S16, S20 | Every input order ranks to one sorted arrangement. |
| `prop_list_selection_is_deterministic` | S15 | The first k eligible candidates do not depend on the input order. |
| `prop_rationals_form_an_ordered_ring` | S17 | `BigRational` satisfies the hypotheses that Part 5 discharges for `Qc`. |

The witness tests of the map are the deterministic tests of the module, the reference vectors and the kappa control `tensor_selection_without_ranks_is_not_unique` included.
Four oracle properties of `tensor/num.rs` check $`\mathrm{RN}_{53}`$ against IEEE operations, which round correctly.
They check the identity on binary64 values, the quotient of two integers up to $`2^{53}`$, the square root, and the widening of a binary32 value.

### 14.7 Running the step-2 checks

To run the step-2 checks, do these steps from the repository root, after the preparation of section [10.1](#101-preparation):

1. Run `scripts/check-reified-rspaces-proofs.sh`.
2. Run `cargo test -p rholang --lib -- tensor::`.
3. Run the three scripts of `formal/z3/reified_rspaces` with `python3`.
4. Copy `similarity_metrics.sage` to a directory under `target/`, and run it there with `RRS_REPO_ROOT` set to the repository root.
5. Run `python3 scripts/verify-reified/numeric_reference_receipt.py`.
6. Run `python3 scripts/verify-reified/check-invariant-map.py --phase P6`.
7. Run `python3 scripts/verify-reified/check-model-map.py --map formal/reified-rspaces-model-map-p6.json`.
8. Run `python3 scripts/verify-reified/check-footprint.py --policy p6` and `check-complete.py --phase p6`.
9. Run `python3 scripts/verify-reified/tensor-mutants.py`, which must report `FAIL=0`.

## 15. Phase P6, steps 3 and 4: the VectorDB driver and its join-engine paths

Steps 3 and 4 of the P6 design add the VectorDB discipline.
It has a driver for a collection without a join group, and paths in the join engine for the other collections.
They land in one commit, because a settle job that carries owed second sends always runs in the join engine.
This section records the rules of the code, the tests that check them and the mutation controls of the tests.
The decisions of plan decision 66 apply: LM-b, the payload as the default vector, `ListModeInJoin`, and the binary metrics on binary vectors only.

### 15.1 Selection

Let $`Q`$ be a query and $`R`$ the records of a collection.
The eligible set $`E(Q, R)`$ holds each record that has a score, passes the bound of $`Q`$, and has its id in `ids`.
The selection order $`\prec`$ sorts $`E(Q, R)`$ by the better score, then by the rank $`\kappa`$ in the canonical order of DR-75 ([decision 60](decision-register.md#decision-60)).
Single mode binds the first record of $`E(Q, R)`$ whose value the pattern binds.
List mode binds one value: the list of the payload values of the first $`m`$ records.

```math
L(Q, R) = (e_1, \dots, e_m), \qquad m = \min\bigl(k, \lvert E(Q, R) \rvert\bigr), \qquad e_1 \prec e_2 \prec \dots \prec e_m
```

`eligible` (`vectordb.rs:881-952`) computes $`E(Q, R)`$ with the exact scores of section 14.
`ranked` (`vectordb.rs:1003-1029`) sorts it with `tensor::exact::rank`, and `list_selection` (`vectordb.rs:1068-1117`) builds $`L(Q, R)`$.

### 15.2 LM-b: the offer to a list waiter and the list cascade

A list waiter $`W`$ parks with its query when its list is empty, or when its pattern or its guard refuses the list.
OFFER-VDB fires $`W`$ with an incoming record $`x`$ when $`x \in L(W, R \cup \{x\})`$, and the pattern and the guard admit that list.
The fire is one COMM over `[D]` with $`x`$ (`list_offer`, `vectordb.rs:1123-1142`).
Each other member of the list that is not persistent leaves through a `K_retire` COMM (`retire_members`, `vectordb.rs:1151-1194`).
A removal can also make a parked list waiter fireable (CE-2 of the arbitration).
So after each removal, LIST-CASCADE lifts the first member of the list of the first such waiter and offers it again (`run_tail`, `vectordb.rs:1773-1808`).

A single-mode waiter needs no cascade.
The eligibility of a record does not depend on the other records, so a removal never exposes a single-mode waiter without a guard.

### 15.3 Rule E and the carries at the round cap

A peek fire removes each member that is not persistent, and the same transaction sends each one again in delivery order ([decision 45](decision-register.md#decision-45)).
The driver runs the owed second sends first and the cascade second, as the DRAIN-AND-CASCADE of the join engine does.
At the round cap, the transaction stops ([decision 49](decision-register.md#decision-49)).
It re-issues one `SettleOp` whose cascade state holds the owed second sends and the re-issue of the operation (`conclude`, `vectordb.rs:1814-1840`).
A settle with a cascade state always runs in the join engine, which applies the same rules.
A second send keeps its record bytes, so it keeps its embedding and its id.

### 15.4 The join engine

`JoinWindow::Similarity` is the window shape of the VectorDB discipline.
Each position on a VectorDB collection sees its own eligible records in its own selection order (`similarity_bind`, `join.rs:3277-3345`).
The engine has its own list paths: OFFER-LIST (`join.rs:3756-3808`), RECEIVE-LIST (`join.rs:4083-4186`) and the list cascade (`join.rs:4286-4315`, `4357-4447`).
A join with a list-mode bind fails with `ListModeInJoin` before any staging (`join.rs:458`).
A PathMap space has no VectorDB family, so the PathMap source fails closed on the similarity window.

### 15.5 Tests

*Table 98. The example tests of P6 steps 3 and 4 (`rholang/tests/reified_vectordb.rs`).*

| Test | What it checks |
|---|---|
| `space_vectordb_single_mode_takes_the_best_record` | Single mode binds the record with the best score. |
| `space_vectordb_scores_are_the_correctly_rounded_exact_scores` | `scores` delivers $`\mathrm{RN}_{53}`$ of the exact score of cosine, dot, euclidean and manhattan. |
| `space_vectordb_bounds_select_the_eligible_records` | A threshold and a radius bound $`E`$, and a parked waiter decodes after its commit. |
| `space_vectordb_fixed_point_and_rational_thresholds` | A fixed-point threshold and a rational threshold are exact. |
| `space_vectordb_list_mode_delivers_one_list_in_selection_order` | A list is one value in one COMM over `[D; m]`. |
| `space_vectordb_reads_without_a_vector_follow_the_canonical_order` | A query without a vector reads the records in the canonical order. |
| `space_vectordb_payload_is_the_vector_by_default` | A send of one numeric list uses it as its embedding, and `embedding` overrides it. |
| `space_vectordb_ids_keep_only_the_listed_records` | `ids` keeps only the records with the listed source hashes. |
| `space_vectordb_binary_metrics_read_binary_vectors_only` | Hamming and jaccard read binary vectors only, and jaccard of two empty supports has no score. |
| `space_vectordb_errors_have_their_codes` | Each malformed send and query fails with its code, and nothing commits. |
| `space_vectordb_capacity_bounds_records_not_waiters` | `capacity` bounds the records and not the waiters. |
| `space_vectordb_reservation_cut_rejects_without_effect` | One unit less of host work in any charged dimension rejects the operation with no effect ([decision 42](decision-register.md#decision-42)). |
| `space_vectordb_peek_restores_records_with_their_embedding` | Rule E sends each peeked record again with its embedding. |
| `space_vectordb_list_peek_restores_carry_at_the_cap` | The second sends that the round cap leaves are carried by one settle job. |
| `space_vectordb_ce1_list_waiter_fires_with_the_incoming_datum` | CE-1, in the driver and in the engine. |
| `space_vectordb_ce2_removal_cascade_fires_the_list_waiter` | CE-2, in the driver and in the engine. |
| `space_vectordb_list_cascade_carries_at_the_cap` | LIST-CASCADE stops at the round cap, and its settle job finishes it. |
| `space_vectordb_guard_refusal_parks_until_a_worse_record_passes` | The asymmetry of [decision 37](decision-register.md#decision-37). |
| `space_vectordb_join_binds_each_position_by_its_query` | Each bind of a join sees the records of its own query. |
| `space_vectordb_join_waiter_fires_on_an_eligible_send` | A join waiter fires only on a send that its query makes eligible. |
| `space_vectordb_list_mode_in_a_join_fails` | A join with a list-mode bind fails with `ListModeInJoin`. |
| `space_vectordb_replay_equal` | Replay stages the transactions of play, and the roots are equal. |
| `space_vectordb_source_programs_select_by_similarity` | Rholang programs pass the whole stack: the factory params, `embedding`, a list with scores, a parked waiter, and the `config` reply. |
| `space_vectordb_source_errors_have_their_codes` | A program fails with the code of each send, query, join and factory error, and leaves no space data. |

Two P5 suites also cover the two new families.
The pattern-enforcement matrix N3 (`rholang/tests/reified_pattern_enforcement.rs`) has the rows `hashmap:vectordb` and `vector:vectordb`, and the join row `hashmap:vectordb`.
A receive without a query reads a VectorDB collection in the canonical order, so a literal pattern selects as on a bag.
The property of the matrix also draws these rows.
The family suite (`rholang/tests/reified_families.rs`) checks that the P6 rows come after the P5 rows and aliases.
It also checks the required params `capacity` and `dimensions`, the value rules of the four params, and gate 9.

*Table 99. The properties of P6 steps 3 and 4.*

| Property | Statement |
|---|---|
| `prop_vectordb_driver_and_engine_agree` | The driver and the engine fire the same continuations with the same values, and they leave the same records and waiters. |
| `prop_vectordb_no_pending_match` | After each operation of a program without guards, a fresh receive with the pattern and the query of each parked waiter does not fire. |
| `prop_vectordb_list_is_ordered_and_complete` | A list holds $`\min(k, \lvert E \rvert)`$ values, and its delivered scores follow the selection order. |
| `prop_vectordb_replay_equal` | Replay of each program gives the outcomes of play, pairs every COMM and reaches the root of play. |

Each property runs 64 cases with the fixed seed of the P5 runner and keeps no failure file.
Step 8b maps these properties to the headlines of `VectorDbDiscipline.v`, the Rocq model of the discipline (section [22](#22-phase-p6-step-8b-the-model-of-the-vectordb-discipline-and-its-properties)).

### 15.6 Negative controls

The harness `scripts/verify-reified/vectordb-mutants.py` applies each mutant in turn, runs the suite, and restores the source files.

*Table 100. The mutation controls of P6 steps 3 and 4.*

| Mutant | What it changes | Tests that fail |
|---|---|---|
| `driver-no-list-cascade` | The driver never searches the list cascade. | 6, both properties included |
| `engine-no-list-cascade` | The engine never finds a list waiter for its cascade. | 5, both properties included |
| `no-member-retire` | A list fire retires no other member. | 3, `prop_vectordb_list_fires_are_atomic` included |
| `selection-reversed` | The selection order is reversed. | 16 |
| `driver-rule-e-dropped` | A peek of the driver sends no record again. | 8, `prop_vectordb_records_are_conserved` included |
| `engine-bind-ignores-query` | A similarity bind of the engine reads the canonical order. | 3 |

The counts are those of the run of step 8c, with the properties and the witness test of steps 8b and 8c.
Sections [22.3](#223-negative-controls) and [23.4](#234-the-property-of-atomic-list-fires) describe them.

The first run of the harness refuted one hypothesis: the random programs did not reach the list cascade.
Only the example tests failed under the two cascade mutants.
So each program now starts with sends and list waiters whose pattern refuses a list of one or of three.
Both properties then fail under both mutants.

### 15.7 Running the step-3 checks

To run the checks of steps 3 and 4, do these steps from the repository root:

1. Run `cargo test -p rholang --test reified_vectordb --test reified_families --test reified_pattern_enforcement`.
2. Run `cargo test -p rholang --lib -- spaces:: reified::`.
3. Run `python3 scripts/verify-reified/vectordb-mutants.py`, which must report `FAIL=0`.
4. Run `python3 scripts/verify-reified/check-footprint.py --policy p6` and `check-complete.py --phase p6`.

## 16. Phase P6, step 5: native certification of the VectorDB shapes

Step 5 of the P6 design certifies the VectorDB shapes in native execution, as proposer self-certification and validator replay do ([decision 47](decision-register.md#decision-47)).
The tests extend the native certification suite of section [13.6](#136-the-native-certification-suite) and use its harness at the v6 limits of the offered funded protocol.
Each VectorDB plan stages only through `Cx` and reserves each step on the meter of its job.
So native play and native replay make the same reservations and the same rows.
This section records the tests, the host work that they measured, and what the measurements mean for the family.

### 16.1 Tests

*Table 101. The native certification tests of P6 step 5 (`rholang/tests/reified_native_spaces.rs`).*

| Test | What it checks |
|---|---|
| `space_native_vectordb_replay_equal` | Both VectorDB families play three programs natively at the v6 limits, and native replay reproduces each play. The programs cover single mode, a peek, a threshold, a radius, the list modes, a join with queries, a persistent receive and the two paths of LM-b. |
| `space_native_vectordb_cascade_carries_at_the_cap` | At the v6 limits, play rejects the round-cap program for host work and records no evidence. With 32 times the v6 limits, the receive stops at the round cap, its settle job carries the other fires, and native replay reproduces the play. |
| `space_native_vectordb_errors_reproduce` | `CapacityExceeded` and `DimensionMismatch` abort the deploy in native play, and native replay reproduces the failure. |
| `space_native_vectordb_denial_replay` | Out of phlo at each charged budget attempt of the largest transaction of LM-b, play and native replay agree. |

A source program reaches a shape only within the syntax contract.
Only a linear bind takes a query (`syntax-contract.md`), so a peek and a persistent receive read the canonical order of their collection.
The list receive of two runs after the sends of its collection, so it fires in one COMM over `[D; 2]`.
The two lists of LM-b fire through an offer, so no COMM of that program consumes one channel twice.
The harness gains `native_play_with` and `certify_with`, which take the host-work budget of play and of replay.

### 16.2 Host work of the VectorDB shapes

The first run of the round-cap program failed: native play rejected it with `HostWorkRejected`.
A probe then played the program at several sizes with a host-work budget that has no limit, and it read the usage of each dimension.
The probe programs draw only the name `a` from the space.
`VerificationBytes` dominates. `SearchStateBytes` reaches 0.49 of its limit at 32 waiters and 1.85 at 70, and every other dimension stays below 0.10.

*Table 102. The `VerificationBytes` of native play of the round-cap program by its size, against the v6 limit of 268,435,456 bytes.*

| List waiters and documents | `VerificationBytes` | Fraction of the v6 limit |
|---|---|---|
| 4 | 67,613,061 | 0.25 |
| 8 | 144,768,768 | 0.54 |
| 16 | 370,762,628 | 1.38 |
| 32 | 1,111,771,123 | 4.14 |
| 64 | 3,757,681,570 | 14.00 |
| 70 | 4,431,927,764 | 16.51 |

The growth is quadratic in the size of the collection.
Each operation loads all records or all waiters of its collection, and the number of operations grows with the collection.

A second probe sampled the stack of every 16th reservation of `VerificationBytes` for 16 parked list waiters over 17 records (Table 103).

*Table 103. The sources of the `VerificationBytes` that the space meter reserves for 16 parked list waiters over 17 records.*

| Source | Share |
|---|---|
| The decode of the space config in `begin_collection`, once for each operation | 26.2% |
| The load of the records of the collection in `eligible` and in the offer of a send | 30.4% |
| The load of the waiters of the collection when a receive parks | 20.1% |
| The decode of the vector of each record in `eligible` | 7.7% |
| The other reservations | 15.6% |

Each reservation follows the walk model of `shared/src/rust/clone_backing.rs`, which the campaign adopts from the cost-accounting branch ([decision 40](decision-register.md#decision-40)).
A walk charges every node of a value and every entry of its work list, so it charges much more than the encoded bytes.
One walk of the config map charges 175,565 bytes, and the bincode of the map has 2,003 bytes.
The decode of the config makes five walks, because `SpaceConfig::from_par` reads the map, copies its fields, encodes the config again and compares the two maps.
So one decode charges 1,228,955 bytes, once for each operation of a collection.

*Table 104. Native play and native replay of the step-5 programs, as fractions of the v6 limit of `VerificationBytes`.*

| Program | Budget attempts | Play | Replay |
|---|---|---|---|
| The `hashmap:queue` program of section 13.6 | 121 | 0.30 | 0.68 |
| All VectorDB shapes in one deploy | 110 | 0.44 | 1.07 |
| The two paths of LM-b | 74 | 0.27 | 0.68 |
| The round-cap program of the probe | 655 | 16.51 | 18.66 |

Native replay charges more than native play: 2.2 to 2.6 times for the three small programs, and 1.13 times for the round-cap program.
The replay environment reserves about 36 MB before its evaluation and about 1.2 MB for each budget attempt ([M-47-1](findings/native-execution-cost-accounting.md#7-m-47-1-verification-bytes-of-native-replay)).
So the suite runs the VectorDB shapes as two deploys, and each one fits the v6 limits in play and in replay.

### 16.3 Consequences

- At the v6 limits, a natively certified deploy holds only a small VectorDB collection. In play, 8 records and 8 list waiters use 0.54 of the limit, and 16 of each exceed it.
- The round-cap shape needs about 19 times the v6 limit of `VerificationBytes` in replay. The suite certifies its carry with 32 times the v6 limits, and it certifies the rejection at the v6 limits.
- A host-work rejection is a platform failure. Native play records no evidence, so the proposer refuses the offer and no block includes it.
- The walk model and the v6 limits belong to the owner of cost accounting, and this campaign changes neither (M-3). The tracker adds these measurements to M-47-1 for that owner.

### 16.4 Running the step-5 checks

To run the checks of step 5, do these steps from the repository root:

1. Run `cargo test -p rholang --test reified_native_spaces`.
2. Run `python3 scripts/verify-reified/check-footprint.py --policy p6` and `check-complete.py --phase p6`.

## 17. Phase P6, step 6: `rho:lang:vector`

Step 6 of the P6 design adds the system process `rho:lang:vector` on the channel `[0xE0, 0x23]`, with the body reference 235 (design v3 4.7).
It has five operations: `add`, `scale`, `dot`, `norm` and `distance`.
There is no `eval` ([decision 62](decision-register.md#decision-62)).
A program calls it as `vec!?(op, x…)`, and the process replies on the reply channel ([decision 57](decision-register.md#decision-57)).

### 17.1 Calls and replies

A call has one or two operands.
So the reducer packs the arguments after the reply channel into one list, as for a factory call (I-18).
The process therefore receives `[ret, [op, x…]]` and has the fixed arity 2.
A call with params aborts with `ParamsUnsupported`, and a persistent call aborts with `PersistentOpCall` (A21).

The reply is one datum: `[true, value]`, or `[false, code, detail]`, so `!?` and `try … catch` apply.
The detail is a fixed text and never repeats an operand.
A host-work rejection is not a reply: the deploy fails with `HostWorkRejected` ([decision 40](decision-register.md#decision-40)).

*Table 105. The operations of `rho:lang:vector` (`rholang/src/rust/interpreter/tensor/ops.rs`).*

| Operation | Operands | Value |
|---|---|---|
| `add` | two vectors of one length | the vector $`a + b`$, in the widest kind |
| `scale` | a vector $`v`$, then a number $`s`$ | the vector $`s \cdot v`$, in the widest kind |
| `dot` | two vectors of one length | the number $`a \cdot b`$, in the widest kind |
| `norm` | one vector $`v`$ | the `f64` $`\mathrm{RN}_{53}(\sqrt{v \cdot v})`$ |
| `distance` | two vectors of one length | the `f64` $`\mathrm{RN}_{53}(\sqrt{\lVert a - b \rVert^2})`$ |

The kinds are ordered: an integer (`GInt`, `GBigInt`, `GBool`), then a rational (`GBigRat`, `GFixedPoint`), then a float (`GDouble`, `GFloat32`).
The value has the widest kind of the inputs (plan decision 66).
An integer leaves as `GInt`, or as `GBigInt` beyond the range of `i64`.
A rational leaves as a reduced `GBigRat`, also when its value is an integer.
A float leaves as the `GDouble` of $`\mathrm{RN}_{53}`$ of the exact value, and a value beyond the finite range of `f64` replies `ValueOutOfRange`.

*Table 106. The reply codes of `rho:lang:vector`.*

| Code | When |
|---|---|
| `UnknownMethod` | The first element of the call is not the name of an operation. |
| `BadArgument` | The operation has another number of operands. |
| `NotAVector` | A vector operand is not one list of numbers. |
| `NotANumber` | An element or the factor of `scale` is not a number. |
| `NonFiniteValue` | A number is NaN or an infinity. |
| `MalformedNumber` | A big rational has a denominator that is not positive. |
| `DimensionMismatch` | The two vectors of a pair have different lengths. |
| `ValueOutOfRange` | A float value is beyond the range of `f64`, or a fixed-point scale is above 4096. |

### 17.2 Host work

The process builds its meter from the host-work budget of the deploy, which the reduction context holds, as `submit_space` reads it.
Each step reserves its work before it runs:

- the comparison of the method name and the kind test of each element
- the decodes and each exact operation
- each rounding, and the encode of each number and of each list.

The encode of an integer reserves $`\ell(x) + 1`$ operations, $`8\ell(x)`$ scanned bytes and $`8\ell(x) + 1`$ backing bytes (`reserve_integer_encode`).
The process touches no space, so it runs without an `Intent::Space`.
Its work is a pure function of its arguments, so native play and native replay reserve the same amounts.

### 17.3 Tests

*Table 107. The tests of step 6.*

| Test | Location | What it checks |
|---|---|---|
| `vector_ops_keep_the_widest_kind` and six more unit tests | `tensor/ops.rs` | The kinds, the widening to `GBigInt`, the exact sums, the correct roots, every reply code, the reply shape, and the reservations with a rejection one unit below. |
| `space_system_ops_vector_ops_reserve_on_the_deploy_budget` | `reified/system_processes.rs` | The process reserves on the budget of the reduction context, and one unit less rejects the call with no reply. |
| `p6_processes_have_the_table_values` | `reified/system_channels.rs` | The channel, the arity and the body reference, and the ops that pack their arguments: `0x00` to `0x20` and `0x23`. |
| `vector_ops_source_programs_compute_each_operation` | `rholang/tests/reified_vector_ops.rs` | Each operation replies its value to a Rholang program. |
| `vector_ops_keep_the_widest_kind_in_source` | the same | Big integers, big rationals, fixed points, booleans, floats and `f32` values give their widest kind. |
| `vector_ops_are_exact_before_the_rounding` | the same | $`[10^{16}, 1, -10^{16}] \cdot [1, 1, 1] = 1`$, where `f64` accumulation gives 0. |
| `vector_ops_failures_reply_their_codes` | the same | Each failure that a program can make replies its code, and the deploy goes on. |
| `vector_ops_try_catch_reads_the_reply` | the same | `try … catch` reads the value and the code of a reply. |
| `vector_ops_params_and_persistent_sends_abort` | the same | Params abort with `ParamsUnsupported`, and a persistent call with `PersistentOpCall`. |
| `vector_ops_native_replay_equal` | the same | Six calls play natively at the v6 limits, and native replay reproduces the play. |

*Table 108. The properties of step 6 (`rholang/tests/reified_vector_ops.rs`).*

| Property | Statement | Oracle |
|---|---|---|
| `prop_vector_ops_agree_with_an_exact_oracle` | `add`, `scale` and `dot` on integers, rationals and fixed points equal the exact values, in the widest kind. | `BigRational` arithmetic |
| `prop_vector_ops_float_inputs_give_floats` | With a float among the inputs, the values are floats, and they equal the exact values. | `f64` quotients of values with fewer than 34 significant bits |
| `prop_vector_ops_dot_laws` | $`a \cdot b = b \cdot a`$, and one permutation of both vectors keeps the value (S6 of `Similarity.v`). | the operation itself |
| `prop_vector_ops_distance_laws` | The distance is not negative and symmetric, it is 0 from a vector to itself, and the norm of $`v`$ is its distance to 0 (S7). | the operation itself |
| `prop_vector_ops_roots_are_correctly_rounded` | No `f64` is nearer to $`\sqrt{v \cdot v}`$ than the reply of `norm`. | the squares of the midpoints next to the reply, compared exactly |

The float oracle stays independent of the conversions of `num-rational`.
Its generator keeps every value of `add`, `scale` and `dot` exact in `f64`, so an IEEE quotient of two exact `f64` values is exact.
Design step 8 adds the properties to the invariant map with those of section 15.

### 17.4 Negative controls

The harness `scripts/verify-reified/vector-ops-mutants.py` applies each mutant, runs every test whose name holds `vector_ops`, and restores the source files.
It runs the library tests and the suite with `--no-fail-fast`.
The first run had no such flag.
A failure of the library tests then stopped cargo before the suite, so the counts hid the strength of the suite.

*Table 109. The mutation controls of step 6.*

| Mutant | What it changes | Tests that fail |
|---|---|---|
| `kind-narrowest` | The value of `add` has the narrowest kind. | 4, both oracle properties included |
| `rational-by-value` | An integral rational leaves as an integer. | 6 |
| `norm-without-root` | `norm` replies the square of the norm. | 4, the rounding property included |
| `distance-is-dot` | `distance` takes the root of the dot product. | 3, the distance laws included |
| `no-dimension-check` | Two vectors of different lengths pass. | 3 |
| `vector-not-packed` | The reducer does not pack a call, so it never meets the process. | 6 |
| `reply-without-ok` | The reply is the bare value. | 6 |
| `process-unmetered` | The process ignores the host-work budget of the deploy. | 1 (the process test) |

Native replay cannot see `process-unmetered`, because play and replay then both reserve nothing and still agree.
Only the process test, which reads the budget of the reduction context, kills it.

### 17.5 Running the step-6 checks

To run the checks of step 6, do these steps from the repository root:

1. Run `cargo test -p rholang --lib --test reified_vector_ops -- vector_ops`.
2. Run `python3 scripts/verify-reified/vector-ops-mutants.py`, which must report `FAIL=0`.
3. Run `python3 scripts/verify-reified/check-footprint.py --policy p6` and `check-complete.py --phase p6`.

## 18. Phase P6, step 7a: the recorded processes `embed` and `ann`

Step 7a of the P6 design adds two system processes of the VectorDB family (design v3 4.9, [decision 57](decision-register.md#decision-57)).
`rho:vectordb:embed` is on the channel `[0xE0, 0x24]`, with the body reference 236 and the arity 2.
`rho:vectordb:ann` is on the channel `[0xE0, 0x25]`, with the body reference 237 and the arity 5.
Both are recorded processes: the dispatcher records the reply of a call on its produce event (`non_deterministic_ops()`).
Replay produces the recorded reply.

Both run in native execution, and native replay produces the recorded replies (plan decision 68).
The campaign targets only the latest execution model, so every test of this step runs native play and native replay certification (plan decision 69).
The `config` reply of a mirrored space also names its mirror.

### 18.1 Calls and replies

A program calls `embed!?(text)` or `embed!?([text…])`.
The reply is `[true, vector, model]` for one text, or `[true, [vector…], model]` for a list.
Each vector is a list of `GFloat32` values, the exact output of the model, and `model` names the model.

A program calls `ann!?(space, channel, query, options)`.
The mirror of the VectorDB space `space` answers the `k` records of the collection of `channel` that are nearest to `query`.
The reply is `[true, ids, distances, watermark]`.
`ids` is a list of 32-byte `GByteArray` values, the record ids that the exact query param `ids=` takes.
`distances` is a list of `GFloat32` values, nearest first, and `watermark` is an `Int`.
The ids are hints: only `ids=` turns them into a decision, and the order of the exact engine decides ([decision 58](decision-register.md#decision-58)).

A failure replies `[false, code, detail]`, with a fixed detail that never repeats an argument.
A call with params aborts with `ParamsUnsupported` (I-8), and a persistent call aborts with `PersistentOpCall` (A21), because `is_call_op` now covers `0x00` to `0x25`.

*Table 110. The reply codes of `embed` and `ann` (`reified/vectordb_services.rs`).*

| Code | Process | When |
|---|---|---|
| `BadArgument` | both | The argument of `embed` is not a text or a non-empty list of texts. The query of `ann` is not a text or a numeric vector whose elements are finite as `f32` values. |
| `EmbeddingUnavailable` | both | The node has no model, the model fails, or the model gives another number of vectors than texts. |
| `EmbeddingRateLimited` | `embed` | The call holds more than 64 texts or more than 64 KiB of text. |
| `NonFiniteEmbedding` | both | The model gives a NaN or an infinity. |
| `NotAVectorDbSpace` | `ann` | The space is not a native VectorDB space: a user agent, a dangling handle or another family. |
| `MirrorDisabled` | `ann` | The VectorDB space has no `mirror`. |
| `MirrorUnavailable` | `ann` | The node has no mirror, or the mirror does not answer. |
| `DimensionMismatch` | `ann` | The query vector, or the embedding of a text query, has other dimensions than the space. |
| `InvalidOption` | `ann` | The options are not a map with the one key `k`, an `Int` in $`[1, \mathit{capacity}]`$. |

The quota of `embed` is local to the node, because the CPU time of the model has no price.
It refuses a larger call before the model runs, and the refusal is recorded like any other reply.

### 18.2 The services of a node

The processes ask two services of the node (`reified/vectordb_services.rs`).
The embedder turns texts into vectors, and the mirror answers advisory nearest-neighbour queries.
The services live in one registry of the node process: `install` sets them, and `current` reads them when a process runs.
They are not consensus state: a play output is recorded, and replay never asks a service.

*Table 111. The services of a node.*

| Build | Embedder | Mirror |
|---|---|---|
| without the `chromadb` feature | unavailable: `EmbeddingUnavailable` | unavailable: `MirrorUnavailable` |
| with the `chromadb` feature (`node_services`) | `SbertEmbedder`: the SBERT model `all-MiniLM-L6-v2` of 384 dimensions, loaded on its first call and run on a blocking thread | `ChromaMirror`: the ChromaDB server of `CHROMA_ENDPOINT`, `CHROMA_TENANT` and `CHROMA_DATABASE`, one database for each node |

The names of the mirror are pure functions of consensus values:

- The collection of a space is `rrs-` and the lowercase hex of the first 16 bytes of its space id (`collection_name`).
- The channel digest of a collection key is the lowercase hex of $`\mathrm{Blake2b256}`$ of its canonical inner channel bytes (`channel_digest`).
  These bytes also name the data channel $`D(\mathit{sid}, \mathit{key})`$.
- A mirror record carries its channel digest in the metadata key `channel`, and its id is the lowercase hex of the 32-byte record id.
- A collection carries its watermark in the metadata key `watermark`.

`ann` asks the mirror only for the records of its channel.
A collection that the server does not hold yet answers no neighbours and the watermark 0.
A neighbour whose id is not 32 bytes long cannot name a record, so the reply drops it.

The `config` reply of a space with a mirror holds one more entry, `"mirror"`.
Its value is the map `{"backend": b, "collection": "rrs-…"}`, where `b` is the value of the factory param `mirror`.
The stored config record does not change.

### 18.3 Recording and replay

The dispatcher records the whole reply of a call on its produce event, and replay hands the process the recorded output (`interpreter/dispatch.rs:75-91`).
Every failure is a reply, so it is recorded too.
An error of a recorded process would mark its produce failed, and replay could not reproduce it (`interpreter/reduce.rs:704-706`).

`embed` reads no consensus state.
In play it checks the shape of its argument and the quota, and then asks the embedder.
In replay it produces the recorded reply at once.

`ann` reads consensus state before it asks a service (plan decision 67).
Play and replay both make its checks, in this order:

1. The space is a native VectorDB space, else `NotAVectorDbSpace`.
   A read-only space job reads its config record, as `getSpaceAgent` does.
2. The config has the param `mirror`, else `MirrorDisabled`.
3. The query is a text, or a numeric vector of the dimensions of the space, else `DimensionMismatch` or `BadArgument`.
4. The options are a map of `k`, an `Int` in $`[1, \mathit{capacity}]`$, else `InvalidOption`.

The config read is a journaled operation of native execution, and the reads reserve host work on the budget of the deploy.
So play and replay must both make the checks.
Replay then produces the recorded reply and never asks a service.
Only play asks the services: it embeds a text query, and it asks the mirror for the neighbours and the watermark.

Design v3 had replay return the recorded output first.
With that rule, native replay of an `ann` call fails with "native replay has incomplete operations", because the journal of play holds the config read.
A budgeted evaluation outside native execution showed the host-work side: play reserved 1,815,346 `VerificationBytes` more than replay for one call.

In native execution at the v6 limits, one `ann` call reserves about 3.6 M `VerificationBytes` in play and about 16.6 M in native replay.
The factory call, the `gensym` and the evidence of a deploy add about 7.9 M in play and about 68 M in native replay.
So one deploy holds about a dozen `ann` calls before native replay exceeds the limit of 268,435,456, while play stays far below it.
This play and replay asymmetry belongs to the cost-accounting owner (measurement M-47-1).

A failure of the config job is not a reply: it fails the deploy, as for `getSpaceAgent`, and so does a host-work rejection.
When a deploy runs out of phlo, the produce of the reply fails.
The deploy then fails with `ProduceFailureWithOutput`, and the event log marks the produce of the call failed, as for the other recorded processes.

Native replay cannot yet certify such a play.
It refuses to dispatch a call whose recorded produce is marked failed (`interpreter/reduce.rs:704-706`), so the journaled reply attempt of play stays unconsumed.
The record-and-replay mechanism of external service calls, which the cost-accounting owner implements, covers this case (plan decision 68, task 10904).
When it merges, `embed` and `ann` move to it, and the out-of-phlo test certifies the rejection by native replay.

### 18.4 The Rocq models

The two ops extend the op table and the range of the ops that take calls, so three models follow the code:

- `SpaceFactory.v` defines `embed_op = 36` and `ann_op = 37`, and `is_call_op` ends at `ann_op`.
  Headline 7, `persistent_op_call_aborts`, now states `PersistentOpCall` for the ops 33 to 37 and `SendOk` for the ops 38 to 47.
- `Encoding.v` lists the indices `0x00` to `0x25` in `reserved_system_indices`, so `markers_disjoint_reserved` covers the two new ids.
- `Namespace.v` adds the two ops to `named_ops`, so headline 7 of that module, `op_ids_pairwise_distinct`, states 47 distinct op indices.
  The property `prop_op_ids_pairwise_distinct` of `rholang/tests/reified_p4_props.rs` checks the same 47 ops of the code.
- The Rocq mutant `sf-ann-not-a-call-op` replaces `sf-vector-not-a-call-op` in Table 39.
  A change of `vector_op` no longer changes the range of the call ops, so the old mutant would survive.

The Rocq gate passes with `PASS=109 FAIL=0 SKIP=0`, 266 closed headlines and 367 negative checks.
The new mutant fails inside `persistent_op_call_aborts`, as expected.

### 18.5 Tests

*Table 112. The tests of step 7a.*

| Test | Location | What it checks |
|---|---|---|
| `vectordb_services_names_are_pure_functions_of_consensus_values` | `reified/vectordb_services.rs` | The collection name reads the first 16 bytes of the space id, and the channel digest is the hex of $`\mathrm{Blake2b256}`$ of the key. |
| `vectordb_services_default_to_unavailable` | the same | The unavailable services refuse every call. |
| `p6_processes_have_the_table_values` | `reified/system_channels.rs` | The three processes of P6: channel, arity and body reference, and only `vector` packs its arguments. |
| `persistent_sends_abort_on_every_call_op_and_every_handle` | the same | `!!` aborts on `0x24` and `0x25` with `PersistentOpCall`. |
| `space_handle_methods_config_reports_the_mirror` | `reified/handle.rs` | The `config` reply of a mirrored space keeps the stored entries and adds the mirror, and the record does not change. |
| `vectordb_embed_replies_and_codes` | `rholang/tests/reified_vectordb_chroma.rs` | One text, a list, the boundaries of the quota (64 texts and 64 KiB pass), each reply code, that only checked calls reach the model, and native replay of each reply. |
| `vectordb_embed_replay_equal` | the same | Native replay with another embedder produces the replies of play and calls no embedder. |
| `vectordb_embed_out_of_phlo_rejects_output` | the same | One phlo below the usage of a call, the reply does not appear, the call is marked failed, and two plays agree. |
| `vectordb_ann_replies_and_codes` | the same | A numeric query, a text query, every reply code, the collection and the channel digest that the mirror receives, the drop of a malformed id, and native replay of each reply, in three deploys. |
| `vectordb_ann_replay_equal` | the same | Native replay with another mirror reproduces the advisory answers and a recorded outage, and asks no mirror and no embedder. |
| `vectordb_ann_ids_feed_the_exact_query` | the same | Across two deploys, the advisory ids, with a stale id among them, restrict an exact list query, and the exact order decides, not the order of the mirror. |
| `vectordb_embed_and_ann_reject_params_and_persistent_sends` | the same | Params abort with `ParamsUnsupported`, and `!!` with `PersistentOpCall`, before any service call. |
| `vectordb_config_reports_the_mirror` | the same | A program reads the mirror entry of a mirrored space, and an unmirrored space has none. Native replay certifies the deploy. |

Each certification has a time limit of 120 seconds.
A native replay that hangs then fails its test instead of blocking the suite.
Native replay hangs when a recorded process replies differently than in play, a liveness defect that belongs to the cost-accounting owner (F-47-4).
The test support plays a chain of native deploys on one runtime (`NativeChain`).
Each deploy certifies from the checkpoint root of the deploy before it.

The registration suites learn the two processes: `P6_PROCESS_PINS` of `rholang/tests/reified_factory.rs` pins their rows, and `rholang/tests/reified_families.rs` reads `P6_PROCESSES`.
The tests install a mock embedder and a fake mirror (`rholang/tests/support/backends.rs`).
A test holds the lock of the services from its start to its end.
So the tests of one binary never share a backend.
The backends of the `chromadb` feature compile with `cargo check -p rholang --features chromadb`.
The sandbox runs only the fake backends.

### 18.6 Negative controls

The harness `scripts/verify-reified/vectordb-services-mutants.py` applies each mutant and restores the source files after it.
It runs the suite and the unit tests of the services and of the `config` reply.
A systemd time limit bounds each run, and a run that reaches it counts as a hang.

*Table 113. The mutation controls of step 7a.*

| Mutant | What it changes | Tests that fail |
|---|---|---|
| `embed-not-recorded` | `embed` leaves `non_deterministic_ops()`. | 3: native replay fails, and the produce of an out-of-phlo call is not marked failed |
| `ann-not-recorded` | `ann` leaves `non_deterministic_ops()`. | 3 |
| `embed-replay-recomputes` | Replay asks the embedder again. | 2 |
| `ann-replay-recomputes` | Replay asks the mirror again. | 3 |
| `ann-checks-only-in-play` | Replay returns first, as design v3 had it. | 3: native replay has incomplete operations |
| `quota-dropped` | `embed` has no quota. | 1 |
| `non-finite-accepted` | A NaN element is a reply value. | 1 |
| `mirror-check-dropped` | A space without a mirror is asked anyway. | 1 |
| `k-unbounded` | `k` is not bounded by the capacity. | 1 |
| `channel-digest-of-the-space` | The mirror receives the digest of the space, not of the channel. | 1 |
| `malformed-id-kept` | The reply keeps an id that is not 32 bytes long. | 1 |
| `config-entry-dropped` | The `config` reply has no mirror entry. | 2: the unit test and the native test |

All 12 mutants fail tests, and the harness restores the files.
No mutant hangs, because each certification of the suite has its own time limit.

### 18.7 Running the step-7a checks

To run the checks of step 7a, do these steps from the repository root:

1. Run `cargo test -p rholang --lib --test reified_vectordb_chroma -- vectordb_ space_handle_methods_config`.
2. Run `python3 scripts/verify-reified/vectordb-services-mutants.py`, which must report `FAIL=0`.
3. Run `scripts/check-reified-rspaces-proofs.sh`, which must report `FAIL=0`.
4. Run `python3 scripts/verify-reified/check-footprint.py --policy p6` and `check-complete.py --phase p6`.

## 19. Phase P6, step 7b: the mirror journal and its publication

Step 7b makes each node keep a ChromaDB mirror of each VectorDB space that has the factory param `mirror` (design v3 4.9, [decision 58](decision-register.md#decision-58)).
The mirror is a hint index for `ann`: it never decides a COMM, and nothing in consensus reads it.
A deploy publishes its writes to the mirror only when its final result has no errors (M1, plan decision 66).
Every test of this step runs native play and native replay certification (plan decision 69).

### 19.1 The net writes of a transaction

Each plan of a mirrored space captures the stores that it stages on a data channel of the space (`MirrorCapture` in `spaces/mirror.rs`).
After the commit, the capture reads the receipt of the transaction in staging order.
A `Stored` receipt at a captured index stores its record under the id of its produce.
A `Fired` receipt removes each non-persistent produce of its COMM on a data channel of the space.
The incoming produce of a produce-triggered fire is never stored, so it removes nothing.

The first and the last event of a record id in one transaction decide its net write.

*Table 114. The net write of a record id in one transaction.*

| First event | Last event | Meaning | Write |
|---|---|---|---|
| remove | store | Rule E: a peek removes the record, and the transaction stores it again. | none |
| store | store | A new record. | upsert |
| remove | remove | A record leaves the collection. | delete |
| store | remove | A new record leaves in the same transaction. | none |

An upsert holds the record id, the embedding, the channel digest, the persistence and the document.
The record id is the 32-byte hash of the storage produce, the value that `ids=` takes.
Each element of the embedding is the exact value, rounded to `f64` and then to `f32`.
The document is the first element of the payload when that element is a string.
A record without an embedding is never eligible for a similarity query, so it is not mirrored.
A record with an element that is not finite as an `f32` value is also not mirrored.

The capture has no meter, no error path and no await.
So it never changes the outcome, the events or the charges of a transaction.

### 19.2 The journal and its publication

The Reified runtime of a reducer holds the mirror journal of the current evaluation (`reified/runtime.rs`).
The space job of each operation appends the net writes of its transaction to the journal.
So the scheduler orders the appends of one space as it orders the jobs of that space.
The journal keeps the last write of each record id of each collection.

Two lines in hot files settle the journal with the final result of a deploy.
A result `Ok` without errors publishes the journal as one batch for each collection, and every other result drops the journal.

*Table 115. The settle lines (approved by design v3 4.9 and plan decision 66).*

| File | Line | Paths that pass it |
|---|---|---|
| `rholang/src/rust/interpreter/interpreter.rs` | `reducer.reified.settle_mirror_journal(&evaluation_result);` at the end of `inj_attempt_inner` | native play, and the legacy play and replay paths until the legacy model leaves the tree |
| `rholang/src/rust/interpreter/rho_runtime.rs` | `self.reducer.reified.settle_mirror_journal(&result);` before `NativeReplayEnvironment::evaluate` returns its result | native replay certification |

Every root evaluation drops a journal that no result settled (`begin_evaluation_scope`, FR-5').
So an evaluation without a deploy result never publishes, and its writes never reach the next deploy.
Casper makes such evaluations for its exploratory deploys, through `RhoRuntime::inj`.
A native replay that the host-work check rejects returns before its settle line, so its journal is never published either.

Native replay publishes its own journal to the mirror of the node that replays.
So a validator keeps its mirror current from the blocks that it certifies.
A node that plays a deploy and then certifies the same deploy publishes its batch twice.
The writes are idempotent, so the mirror ends in the same state.
When other batches come between the two publications, the mirror can hold a removed record for a short time.
The ids of `ann` are only hints, and `ids=` selects no record that consensus state does not hold (Section 18.1).

### 19.3 The mirror writer

`MirrorWriter` in `reified/vectordb_services.rs` applies the batches of a node to its mirror.
A bounded queue of 1,024 batches feeds one task, and the task applies the batches in publication order.
For each batch, the task makes the collection if it does not exist, upserts the records and then deletes the ids.
A batch that fails is applied again from its start, with at most 3 attempts and pauses of 100 ms and 200 ms.

Publication never waits: it hands each batch to the queue with `try_send`.
A batch that finds the queue full, or that still fails after its attempts, marks its collection dirty.
The reconciler of step 7c repairs the dirty collections.
`flush` waits until the task has finished each batch that the queue took, for tests and for shutdown.
The unavailable services of a node have no writer, so their publication drops the batches.

The ChromaDB backend makes a collection only when the server does not hold it.
So an existing collection keeps its metadata, the watermark of the reconciler included.
A record carries the metadata keys `channel` and `persistent`.
A delete in a collection that the server does not hold succeeds, because that collection holds no record.
The sandbox compiles these calls with `cargo check -p rholang --features chromadb`, and the live leg of step 7c runs them against a ChromaDB server.

*Table 116. The distance of a mirror collection (`MirrorMetric`, the `hnsw:space` of ChromaDB).*

| Metric of the space | Distance of the collection |
|---|---|
| `cosine` (the default) | `cosine` |
| `dot` | `ip` |
| `euclidean` | `l2` |
| `manhattan`, `hamming`, `jaccard` | none: gate 9 of the factory refuses `mirror` |

Gate 9 also refuses `mirror` with the qualifiers `temp` and `seq`, so an ephemeral space never has a mirror.
Until P7 enables these qualifiers, gate 3 refuses them first with `QualifierUnavailable`.

### 19.4 Tests

*Table 117. The tests of step 7b.*

| Test | Location | What it checks |
|---|---|---|
| `mirror_capture_is_active_only_for_a_mirrored_vectordb_space` | `spaces/mirror.rs` | A space without `mirror`, a queue space and a metric that the mirror does not index capture nothing. |
| `mirror_capture_net_writes_follow_the_first_and_the_last_event` | the same | Each row of Table 114, a persistent record that a COMM matches, the incoming produce of a produce-triggered fire, a record without a vector, and a counter channel. |
| `mirror_capture_skips_a_non_data_channel_and_another_space` | the same | A public channel and a data channel of another space are not mirrored. |
| `mirror_record_rounds_its_embedding_and_keeps_a_string_document` | the same | The `f32` rounding of each element, a record whose element exceeds the `f32` range, the persistence, a list payload without a document, and the distance `l2`. |
| `mirror_metric_maps_the_metrics_that_chroma_indexes` | the same | Each row of Table 116. |
| `mirror_journal_keeps_the_last_write_of_each_record` | the same | The last write of a record id wins, each collection gives one batch in record-id order, and `take` and `clear` empty the journal. |
| `mirror_writer_applies_batches_in_publication_order` | `reified/vectordb_services.rs` | The order of the batches and of the calls of a batch, and a batch with deletes only. |
| `mirror_writer_makes_the_designed_attempts` | the same | The writer makes 3 attempts, the value of the design. The writer tests count with this literal, so a change of the constant fails them. |
| `mirror_writer_retries_a_failed_batch` | the same | A batch that succeeds at its last attempt leaves its collection clean. |
| `mirror_writer_marks_a_failed_batch_dirty` | the same | A batch that fails each attempt marks its collection dirty, the next batch is applied, and `take_dirty` empties the set. |
| `mirror_writer_marks_a_full_queue_dirty` | the same | With a stalled mirror and a queue of one batch, the batches that find the queue full mark their collections dirty at once. |
| `vectordb_services_default_to_unavailable` | the same | The unavailable mirror refuses each write, and the unavailable services have no writer. |
| `reified_factory_gate_9_needs_the_default_qualifier_and_an_indexed_metric` | `spaces/factory/constructor.rs` | Gate 9 for each qualifier and each metric, with and without `mirror`. |
| `mirror_publishes_a_successful_deploy` | `rholang/tests/reified_vectordb_chroma.rs` | A deploy publishes one upsert for each record with a vector, with its id, embedding, channel digest, persistence and document. A record without a vector and an unmirrored space are not mirrored. |
| `mirror_journal_drops_failed_deploy` | the same | A space error, an out-of-phlo deploy and a host-work rejection publish nothing, each one unit below the usage of a control deploy that publishes its upsert. |
| `mirror_rule_e_coalesces` | the same | A peek writes nothing, a consume of a non-persistent record deletes it, and a persistent record stays. |
| `mirror_temp_never_mirrored` | the same | The factory refuses `mirror` with `temp` and `seq`, and nothing reaches the mirror. |
| `mirror_never_blocks_deploy` | the same | With a stalled mirror and a full queue, three deploys succeed at once and the collection is dirty. With a mirror that refuses each write, the collection is dirty after 3 attempts. |
| `mirror_journal_keeps_the_last_write_of_a_deploy` | the same | A record that one transaction stores and a later transaction of the same deploy consumes leaves no record in the mirror. |
| `mirror_fired_send_is_never_mirrored` | the same | A send that fires a parked waiter never enters the collection, so nothing reaches the mirror. |
| `mirror_collection_distance_follows_the_metric` | the same | The collections of spaces with the metrics `cosine`, `dot` and `euclidean` have the distances `cosine`, `ip` and `l2`. |
| `mirror_exploratory_evaluation_publishes_nothing` | the same | An evaluation through `RhoRuntime::inj` publishes nothing, and the next deploy publishes only its own record. |
| `mirror_replay_publishes_its_own_journal` | the same | Native replay certification of two deploys gives the mirror of the replaying node the calls and the records of the mirror of play. |

In `mirror_journal_drops_failed_deploy`, each failing deploy stores a record and then peeks it.
The peek fires only after the store has committed, so the journal holds the upsert when the deploy fails.
At one unit of `VerificationBytes` below the usage of the control, the host-work check rejects the deploy after its last operation.
The evaluation then returns `Err(HostWorkRejected)`, and the test support plays it with `native_play_result`.

The fake mirror of the tests (`rholang/tests/support/backends.rs`) keeps the records of each collection and a log of the write calls.
A stalled fake mirror holds its writes until the test releases them, and a down fake mirror refuses them.

### 19.5 Negative controls

The harness `scripts/verify-reified/mirror-mutants.py` applies each mutant and restores the source files after it.
It runs the unit tests of the capture, of the journal, of the writer and of gate 9, and the native suite.
A systemd time limit bounds each run, and a run that reaches it counts as a hang.

*Table 118. The mutation controls of step 7b.*

| Mutant | What it changes | Tests that fail |
|---|---|---|
| `publish-with-errors` | A result with errors publishes its journal. | `mirror_journal_drops_failed_deploy` |
| `publish-on-err` | An evaluation that ends with an error publishes its journal. | `mirror_journal_drops_failed_deploy` |
| `no-play-settle` | The settle line of `interpreter.rs` is gone. | 8 |
| `no-replay-settle` | The settle line of `rho_runtime.rs` is gone. | `mirror_replay_publishes_its_own_journal` |
| `begin-scope-keeps-journal` | A root evaluation keeps a journal that no result settled. | `mirror_exploratory_evaluation_publishes_nothing` |
| `journal-not-appended` | The space job drops the writes of its transaction. | 8 |
| `rule-e-upserts` | A Rule E pair upserts its record. | `mirror_capture_net_writes_follow_the_first_and_the_last_event`, `mirror_rule_e_coalesces` |
| `new-then-removed-deletes` | A record that is new and leaves in the same transaction is deleted. | `mirror_capture_net_writes_follow_the_first_and_the_last_event` |
| `persistent-removed` | A COMM that matches a persistent record deletes it. | `mirror_capture_net_writes_follow_the_first_and_the_last_event`, `mirror_rule_e_coalesces` |
| `incoming-removed` | The incoming datum of a produce-triggered fire counts as a removal. | `mirror_capture_net_writes_follow_the_first_and_the_last_event`, `mirror_fired_send_is_never_mirrored` |
| `foreign-channel-removed` | A COMM on a channel that is not a data channel of the space removes a record. | `mirror_capture_skips_a_non_data_channel_and_another_space` |
| `non-finite-mirrored` | An embedding that is not finite as `f32` values is mirrored. | `mirror_record_rounds_its_embedding_and_keeps_a_string_document` |
| `document-dropped` | A mirror record never carries its document. | `mirror_capture_net_writes_follow_the_first_and_the_last_event`, `mirror_publishes_a_successful_deploy` |
| `persistence-dropped` | A mirror record never carries its persistence. | `mirror_record_rounds_its_embedding_and_keeps_a_string_document`, `mirror_publishes_a_successful_deploy` |
| `capture-without-mirror-param` | A VectorDB space without `mirror` is captured. | `mirror_capture_is_active_only_for_a_mirrored_vectordb_space`, `mirror_publishes_a_successful_deploy` |
| `journal-first-write-wins` | The journal keeps the first write of a record id. | `mirror_journal_keeps_the_last_write_of_each_record`, `mirror_journal_keeps_the_last_write_of_a_deploy` |
| `dot-as-cosine` | The metric `dot` gets the distance `cosine`. | `mirror_metric_maps_the_metrics_that_chroma_indexes`, `mirror_collection_distance_follows_the_metric` |
| `capture-index-shifted` | The captured stores shift by one staged index. | 6 |
| `writer-no-retry` | The writer makes one attempt only. | 4 |
| `failed-batch-not-dirty` | A batch that fails after its attempts leaves its collection clean. | `mirror_writer_marks_a_failed_batch_dirty`, `mirror_never_blocks_deploy` |
| `full-queue-not-dirty` | A batch that finds the queue full leaves its collection clean. | `mirror_writer_marks_a_full_queue_dirty`, `mirror_never_blocks_deploy` |
| `collection-not-ensured` | A batch writes to a collection that it never makes. | 12 |
| `deletes-skipped` | A batch never deletes. | 7 |

All 23 mutants fail tests, and the harness restores the files.
3 mutants fail only unit tests, because no program of the native suite reaches their case:

- `foreign-channel-removed`: a plan of a VectorDB space stages only on the data channels of its space.
  The family has no counter, and the allocator of index names runs outside the plans.
- `new-then-removed-deletes`: no transaction of the native suite stores a new record and removes it again.
- `non-finite-mirrored`: no record of the native suite has an element outside the `f32` range.

### 19.6 Running the step-7b checks

To run the checks of step 7b, do these steps from the repository root:

1. Run `cargo test -p rholang --lib --test reified_vectordb_chroma -- mirror_ vectordb_services gate_9`.
2. Run `python3 scripts/verify-reified/mirror-mutants.py`, which must report `FAIL=0`.
3. Run `cargo check -p rholang --features chromadb`.
4. Run `python3 scripts/verify-reified/check-footprint.py --policy p6` and `check-complete.py --phase p6`.


## 20. Phase P6, step 7c: the mirror reconciler, the docker pin and the live leg

Step 7c adds the reconciler of the mirror ([decision 58](decision-register.md#decision-58), [decision 61](decision-register.md#decision-61)).
On each finalized block, it repairs the channels that the journal of the node registered to the state of that block.
It reads Casper state only through read-only calls, and it writes only the ChromaDB mirror of the node (plan decision 70).
The step also pins the ChromaDB image, passes its environment to the nodes, and adds the live leg against a real server.

### 20.1 The channel registry

Each net write of a transaction knows its channel.
A store takes the channel of the captured store, and a removal takes the channel of the matched result of its COMM.
The journal keeps the channel digest and the key bytes of each channel that has a net write.
A batch carries them with the 32-byte space id.

The writer applies a batch in this order: it makes the collection, registers the channels, upserts the records and deletes the ids.
The registry lives in the metadata of the collection:

*Table 119. The metadata of a mirror collection.*

| Key | Value | Writer |
|---|---|---|
| `hnsw:space` | `cosine`, `ip` or `l2` | the writer, when it makes the collection |
| `space` | the lowercase hex of the 32-byte space id | the writer, at the first registration |
| `channel:<digest>` | the lowercase hex of the key bytes of the channel | the writer, once for each channel |
| `watermark` | the last finalized block that a pass applied | the reconciler |

ChromaDB `modify` replaces the whole metadata map.
So `ChromaMirror` holds one lock for each read-modify-write of metadata, and it caches the digests that it knows to be registered.
The writer and the reconciler of a node share one mirror, and the database of the node has no other writer.

A dirty collection keeps what its repair needs: the space, the distance and the channels of each batch that the writer did not apply.

### 20.2 A pass

The reconciler takes the newest finalized block of the node, and one pass runs at a time.
The pass below repairs a collection when its watermark is below the block, or when the writer marked it dirty.

```text
pass(L, R):                          -- L is the number of the block, R its post-state root
  D := take the dirty collections of the writer
  for each dirty collection c:       -- so a repair knows its channels
    make c with its distance, and register its channels
  for each collection c of the node:
    if watermark(c) >= L and c not in D: leave c
    else:
      for each registered channel (digest, key) of c:
        S := the records of the data channel of key at R, as mirror records
        M := the ids of the mirror records of the channel
        delete M - ids(S); upsert the records of S whose ids are not in M
      set watermark(c) := max(watermark(c), L)
  every dirty collection that the pass did not repair becomes dirty again
```

A failed call ends the repair of its collection without a new watermark, so the next pass repeats the collection.
The watermark never decreases, because a dirty collection can be repaired at an older block than a pass already applied.

Every pass reconciles every registered channel.
A pass sets the channels to the finalized view, so the optimistic writes of later blocks leave the mirror until those blocks are finalized.
`ann` reports the watermark, so a contract knows which finalized block the mirror reflects.

The registry also sets the limits of the repair:

- After an LFS bootstrap, the records of a channel that this node never journaled are absent until the channel changes.
- After a restart, the dirty state of the writer is lost. A failed batch is repaired only when its channels were registered before the failure, or when they change again.

### 20.3 The node and the docker environment

`setup.rs` gains one spawn call ([decision 61](decision-register.md#decision-61)).
The call passes two read-only closures to `mirror_reconciler::spawn`.
The first closure maps a block hash to the post-state root of the block from the block store.
The second closure gives a runtime at a root through `RuntimeManager::spawn_runtime` and `RhoRuntime::reset`.
`spawn` installs `node_services()`, and it starts the reconciler only when the services have a mirror writer.
A node without the `chromadb` feature, or without a ChromaDB endpoint, starts no task.

`docker/shard.yml` pins the server image by digest (server version 1.0.0).
Each node gets `CHROMA_ENDPOINT` (`http://chroma-db:8000`) and its own `CHROMA_DATABASE`, so nodes never write each other's mirror.
The environment of the node anchor became a map, because Compose merges the environment of a service with the anchor only in that form.
The resolved configuration differs from the base only by these entries and by the pinned image.
`ChromaMirror` creates the database of the node once, before its first call that reads or writes it.

### 20.4 Facts of the pinned server

A disposable container of the pinned image answered the calls of the backend as follows.

*Table 120. The behavior of the pinned ChromaDB server (REST API v2).*

| Call | Behavior |
|---|---|
| `modify` with `new_metadata` | It replaces the whole metadata map. |
| distance | It lives in the configuration of the collection, which `modify` never changes. |
| `get_or_create` on an existing collection | It keeps the stored metadata. |
| upsert | It accepts bool metadata and null documents. |
| `get` with `where` and an empty `include` | It returns the ids only. |
| delete of an absent id | It succeeds. |
| a missing collection | It answers 404. |

On this host, `docker run -p` fails, because iptables has no DNAT support in the running kernel.
The live leg therefore reaches the container on its bridge address, which needs no NAT rule.

### 20.5 Tests

*Table 121. The tests of step 7c.*

| Test | Location | What it checks |
|---|---|---|
| `mirror_reconciler_skips_a_collection_at_its_watermark` | `reified/mirror_reconciler.rs` | A collection at the watermark of the block makes no call. |
| `mirror_reconciler_sets_the_watermark_of_a_collection_without_channels` | the same | A collection without channels needs no runtime, and its watermark advances. |
| `mirror_reconciler_keeps_the_watermark_without_a_runtime` | the same | Without a runtime at the root, the repair stops before the watermark. |
| `mirror_reconciler_keeps_the_dirty_marks_of_a_refused_listing` | the same | A refused listing fails the pass, and every dirty collection stays dirty. |
| `mirror_reconciler_keeps_the_dirty_mark_of_a_collection_that_it_cannot_make` | the same | A dirty collection that the pass cannot make stays dirty, and the other collections are repaired. |
| `mirror_reconciler_repairs_a_dirty_collection_at_its_watermark` | the same | A dirty collection is repaired at its watermark, and the watermark never decreases. |
| `mirror_writer_registers_the_channels_of_a_batch` | `reified/vectordb_services.rs` | A batch registers its channels after it makes the collection and before its upserts. |
| `mirror_writer_keeps_what_a_repair_needs` | the same | A dirty collection keeps the space, the distance and the union of the channels of its failed batches. |
| `vectordb_services_registry_of_reads_the_metadata` | the same | The registry reads the space id, the channels and the watermark, ignores other keys, and refuses a malformed registry key. |
| `mirror_reconcile_removes_orphans` | `rholang/tests/reified_vectordb_chroma.rs` | A pass deletes an orphan, adds a missing record and sets the watermark. A pass at the watermark leaves the collection, and a failed pass keeps the watermark. |
| `mirror_restart_rescans_reports` | the same | After a restart, a pass repairs every channel that the metadata registry names. |
| `mirror_reconcile_repairs_a_dirty_collection` | the same | A failed batch leaves its collection dirty, and the next pass repairs it at its watermark. |
| `mirror_reconciler_follows_the_finalized_blocks` | the same | The reconciler maps the `BlockFinalised` events to post-state roots, and its passes reach the newest block that the node holds. |
| `chroma_live_mirror_calls` | `rholang/tests/reified_vectordb_chroma_live.rs` | Every call of `ChromaMirror` against the pinned server, the metadata read-modify-write and the database of the node included. |
| `chroma_live_publish_and_reconcile` | the same | A native deploy publishes its records to the server through the writer, and a pass repairs an orphan and a missing record. |
| `chroma_live_sbert_embedder` | the same | The SBERT model gives one finite vector of 384 values for each text. |

The native tests read the finalized state of a chain of native deploys (`ChainState` in `rholang/tests/support/native.rs`).
It spawns a runtime over the history of the chain and resets it to a root, as the runtime manager of a node does.
The live suite compiles only with the `chromadb` feature, and it fails when `CHROMA_ENDPOINT` is not set.
`scripts/verify-reified/chroma-live.sh` starts a disposable server of the pinned image, gives each run a new database, and keeps the model cache under `target/`.

### 20.6 Negative controls

The harness `scripts/verify-reified/mirror-mutants.py` now holds the mutants of steps 7b and 7c.
Six mutants of step 7b moved with the code of step 7c, because the removal loop, the record builder and the dirty marks changed.
The run after step 7c applies all 34 mutants.

*Table 122. The mutation controls of step 7c.*

| Mutant | What it changes | Tests that fail |
|---|---|---|
| `registry-not-written` | A batch never registers its channels. | 4 |
| `capture-channels-dropped` | The capture keeps no channel of its writes. | 5 |
| `orphans-kept` | A pass keeps the orphans. | `mirror_reconcile_removes_orphans`, `mirror_reconcile_repairs_a_dirty_collection` |
| `missing-not-added` | A pass never adds a missing record. | 4 |
| `watermark-not-advanced` | A pass never advances the watermark. | 4 |
| `watermark-decreases` | A pass can lower the watermark. | `mirror_reconciler_repairs_a_dirty_collection_at_its_watermark` |
| `current-collection-repaired` | A pass repairs a collection at its watermark again. | `mirror_reconciler_skips_a_collection_at_its_watermark`, `mirror_reconcile_removes_orphans` |
| `dirty-ignored` | A dirty collection at its watermark is not repaired. | `mirror_reconciler_repairs_a_dirty_collection_at_its_watermark`, `mirror_reconcile_repairs_a_dirty_collection` |
| `dirty-not-remarked` | A dirty collection that a pass did not repair loses its mark. | `mirror_reconciler_keeps_the_dirty_mark_of_a_collection_that_it_cannot_make`, `mirror_reconciler_keeps_the_dirty_marks_of_a_refused_listing` |
| `dirty-not-prepared` | A pass neither makes a dirty collection nor registers its channels. | `mirror_reconciler_keeps_the_dirty_mark_of_a_collection_that_it_cannot_make` |
| `events-ignored` | The event task maps no block to its post-state. | `mirror_reconciler_follows_the_finalized_blocks` |

All 34 mutants fail tests, the 23 mutants of step 7b among them, and the harness restores the files.
The ChromaDB backend runs only in the live leg, so no mutant of the harness changes it.
Table 136 of section 24.5 gives the run of step 8d, with the properties of that step.

### 20.7 Running the step-7c checks

To run the checks of step 7c, do these steps from the repository root:

1. Run `cargo test -p rholang --lib --test reified_vectordb_chroma -- mirror_ vectordb_services gate_9`.
2. Run `python3 scripts/verify-reified/mirror-mutants.py`, which must report `FAIL=0`.
3. Run `bash scripts/verify-reified/chroma-live.sh` on a host with Docker.
4. Run `cargo check -p node --features rholang/chromadb`.
5. Run `python3 scripts/verify-reified/check-footprint.py --policy p6` and `check-complete.py --phase p6`.


## 21. Phase P6, step 8a: the factory model, the VectorDB merges and the f64 mutant

Step 8a closes three obligations of design v3, section 6, and of the factory model.
It extends the Rocq model of the factories to the tables of the code, and it adds the property of the VectorDB params.
It adds the merge cases of the VectorDB family in native execution, with the soft bound of `capacity` of plan decision 71.
It also adds the f64-accumulation mutant of the numeric core.

### 21.1 The factory model

`SpaceFactory.v` now models the tables of the code of P6.
The model of P5 wave W2 had no PathMap rows, no alias `rho:space:PathMapSpace` and no PathMap schema, although the code had them since wave W3.
The model now has the six PathMap rows (ops 7 to 12) and the alias row (op 7).
It also has the two VectorDB rows (ops 6 and 25).
The three processes of P6 follow them: `rho:lang:vector` (op 35), `rho:vectordb:embed` (op 36) and `rho:vectordb:ann` (op 37).
Headlines 9 and 10 range over these rows, and headline 9 names the three processes of P6 as the other definitions.

Headline 14 (`vectordb_params_validated`) states the params of a VectorDB family:

- Exactly `hashmap:vectordb` and `vector:vectordb` take `capacity`, `dimensions`, `metric` and `mirror`, and they require `capacity` and `dimensions`.
- Every other family refuses the four names as unknown keys.
- `capacity` accepts the `GInt` values from 1 to 1,000,000, and `dimensions` the `GInt` values from 1 to 4,096.
- `metric` accepts the six metric names, and `mirror` accepts the name `chroma`.
- A call without `capacity`, or with `capacity` and without `dimensions`, fails with `MissingParam` on the first missing key in bytewise order.
- Gate 9 runs on the canonical params. It accepts a mirror exactly with the default qualifier and a metric that ChromaDB indexes: `cosine` (also without the param), `dot` or `euclidean`.

The first compile of the extended model measured a slow kernel check.
With the 23 rows of P5, the `Qed` of `definitions_incl` and of headline 9 took about 33 seconds each.
Without the six PathMap rows, it took 0.5 seconds.
A conversion strategy that unfolds the row tables last brings each `Qed` to milliseconds.
A strategy changes only the order of the unfolding, never what the kernel accepts.
The module now compiles in about 3 seconds, and its comment records the hypotheses that the measurement refuted.

### 21.2 The VectorDB merges

A VectorDB collection uses no counter (design v3 4.3), so its stores merge as the stores on a host channel do (M-1).
The merge suite (`rholang/tests/reified_merge_p5.rs`) now runs its node in one of two models of execution.
The P5 cases keep the legacy model, and the VectorDB cases run in native execution at the v6 limits (plan decision 69).

A store checks `capacity` against the view of its own transaction.
So two branches that each add a record to a collection one record below its capacity merge without a conflict.
The merged collection then holds one record more than its capacity.
No serial order gives that state, because the second deploy of a serial order fails with `CapacityExceeded`.

Plan decision 71 keeps this soft bound: a merge can exceed the capacity by at most the stores of the branch with fewer stores.
After an overflow, every store fails until removals bring the count below the capacity.
Headline 16 of `CounterConflict.v` (`capped_writes_soft_bound`) states the bound, the serial outcome and the check of a store.

### 21.3 Tests

*Table 123. The tests of step 8a.*

| Test | Location | What it checks |
|---|---|---|
| `prop_vectordb_params_validated` | `rholang/tests/reified_families.rs` | Headline 14: for every combination of the four params, the validation of both VectorDB families equals the oracle of the design. |
| `prop_vectordb_writes_commute` | `rholang/tests/reified_merge_p5.rs` | Headline 7 of `CounterConflict.v` for the VectorDB family below its capacity: the stores touch no counter, the branches do not conflict, and the merge is the state of both serial orders. |
| `prop_vectordb_capacity_soft_bound` | the same | Headline 16: from $`n`$ records and stores $`a`$ and $`b`$ that each fit a capacity $`c`$, the merge holds $`n + a + b \le c + \min(a, b)`$ records, and the serial order passes exactly when $`n + a + b \le c`$. |
| `space_collection_conflicts_vectordb_writes_merge` | the same | Two branches of stores merge into the state of both serial orders, and two takes of one record race on the record. |
| `space_collection_conflicts_vectordb_capacity_soft_bound` | the same | With a capacity of two, the merge holds three records. Then a store fails until two takes bring the count below the capacity. |

The two properties run 16 cases each, as every property of the merge suite does.
`prop_vectordb_capacity_soft_bound` also checks that its cases reach both sides of the bound.

### 21.4 Negative controls

*Table 124. The negative controls of step 8a.*

| Control | Kind | What it changes | Must fail in |
|---|---|---|---|
| `sf-p5-pathmap-alias-other-op` | Rocq mutant | The alias `PathMapSpace` names the op 0. | `alias_rows_facts` |
| `sf-p6-register-unwired` | Rocq mutant | A P6 row is registered without its dispatch arm. | `registered_rows` |
| `sf-p6-define-unregistered` | Rocq mutant | The factory of every P6 row is defined. | `definitions_incl` |
| `sf-p6-row-wrong-op` | Rocq mutant | `vector:vectordb` takes the op 24. | `rows_facts` |
| `sf-p6-vectordb-on-pathmap` | Rocq mutant | `pathmap:vectordb` gets the VectorDB schema. | `vectordb_schema` |
| `sf-p6-metric-required` | Rocq mutant | The VectorDB families require `metric`. | `vectordb_schema` |
| `sf-p6-capacity-zero` | Rocq mutant | `capacity` accepts 0. | `canonical_capacity_rule` |
| `sf-p6-capacity-bound` | Rocq mutant | The bound of `capacity` is 1,000,001. | `vectordb_params_validated` |
| `sf-p6-dimensions-bound` | Rocq mutant | The bound of `dimensions` is 4,097. | `vectordb_params_validated` |
| `sf-p6-metric-any-string` | Rocq mutant | `metric` accepts every string. | `canonical_metric_rule` |
| `sf-p6-mirror-any-string` | Rocq mutant | `mirror` accepts every string. | `canonical_mirror_rule` |
| `sf-p6-gate9-any-metric` | Rocq mutant | Gate 9 accepts a mirror with every metric name. | `check_mirror_spec` |
| `sf-p6-gate9-any-qualifier` | Rocq mutant | Gate 9 accepts a mirror with every qualifier. | `check_mirror_spec` |
| `sf-p6-gate9-no-default-metric` | Rocq mutant | Without the `metric` param, gate 9 has no metric. | `mirror_metric_default` |
| `sf-p6-indexed-manhattan` | Rocq mutant | ChromaDB indexes `manhattan` too. | `vectordb_params_validated` |
| `cc-capped-store-at-cap` | Rocq mutant | A store passes on a collection that holds the capacity. | `capped_pass_iff` |
| `cc-capacity-hard-claimed` | Rocq mutant | The claim that a merge never exceeds the capacity. | `capped_writes_soft_bound` |
| `dot-f64-accumulation` | Rust mutant | The dot product of dyadic vectors sums its terms in f64, with one rounding after each term. | 6 tests |

Five controls of P5 now cite the moved texts of the tables and of the schema.
All 41 controls of `SpaceFactory.v` and all 15 controls of `CounterConflict.v` fail where they must.
The f64 mutant fails `tensor_exact_dot_differs_from_f64_accumulation`, the reference vectors, the correctly rounded values and three exact-kernel tests.

### 21.5 Running the step-8a checks

To run the checks of step 8a, do these steps from the repository root:

1. Run `bash scripts/check-reified-rspaces-proofs.sh`, which must report `FAIL=0`.
2. Run `cargo test -p rholang --test reified_families --test reified_merge_p5`.
3. Run `python3 scripts/verify-reified/tensor-mutants.py`, which must report `FAIL=0`.
4. Run `python3 scripts/verify-reified/check-invariant-map.py`.
5. Run `python3 scripts/verify-reified/check-model-map.py --map formal/reified-rspaces-model-map-p5.json`.

## 22. Phase P6, step 8b: the model of the VectorDB discipline and its properties

Step 8b models the discipline of one VectorDB collection in Rocq, and it maps the properties of the VectorDB suite to that model.
The module `VectorDbDiscipline.v` models the driver: RECEIVE-VDB, OFFER-VDB, and the tail with LIST-CASCADE, as one transaction of a space job.
Two new properties test the bound of a transaction and the conservation of records.
Seven Rocq mutants and three new Rust mutants show that the proofs and the properties depend on their rules.

### 22.1 The model

The model has one collection: its records, and its parked waiters in DR-75 order.
A query is abstract: an eligibility and a selection order, with the three facts that `Similarity.v` proves for the order of `tensor/exact.rs`.
The order is a permutation of the eligible records, and it depends only on the multiset of the records.
An insertion keeps the order of the other records.
The corollary `vdb_parameters_valid` discharges these facts with an insertion sort by a total order, so the hypotheses are consistent.

A waiter binds in single mode or in list mode, and the claims are for waiters without a guard (decision 37).
Each loop of a plan has fuel, and headline S24 shows that no loop runs out of it.

Let $`R`$ be the records of the collection and $`W`$ its parked waiters.
Let $`c`$ be the round cap `FIRE_ROUND_CAP`, $`f`$ the fires of the transaction, and $`o`$ the number of owed second sends.
The plans compute the fuel of each loop from the state at its start:

```math
\mathit{fuel}_{\mathrm{receive}} = 1 + \lvert R \rvert, \qquad
\mathit{fuel}_{\mathrm{offer}} = 1 + \lvert W \rvert, \qquad
\mathit{fuel}_{\mathrm{tail}} = 1 + 2\,(c - f) + o .
```

Each step of the tail decreases $`2\,(c - f) + o`$.
An owed send uses up one second send.
A cascade fire adds one fire and at most one owed send, the lifted member of a peek.

*Table 125. The headlines of `VectorDbDiscipline.v`.*

| Id | Theorem | Statement |
|---|---|---|
| S21 | `vdb_join_removal_exposes_nothing` | A removal keeps the order of each similarity window. So a greedy search of single-mode binds that fails before a removal fails after it. The search of one bind is the selection of the driver (design v3 4.6). |
| S22 | `vdb_no_pending_match` | A transaction that ends leaves no parked waiter without a guard that can fire, in single mode and in list mode (LM-b, decision 66). |
| S23 | `vdb_list_needs_cascade` | A computed negative result: without LIST-CASCADE, a receive ends with a parked list waiter that can fire (CE-2 of the arbitration). With the tail, the same transaction leaves no pending match. |
| S24 | `vdb_txn_bounded` | For a cap above 0, a transaction makes at most $`c`$ fires, and no loop runs out of its fuel (I-21, decision 49). |
| S25 | `vdb_no_record_lost` | Rule E (decision 45): a record of the pre-state is in the post-state of a transaction that ends, or a fire of a waiter that does not peek spent it. |

The invariant map also maps the replay property of the suite to S26, the theorem `replay_reproduces_play_run` (H23) of `EventCorrelation.v`.
The VectorDB plans stage only through the substrate, so the replay of their transactions is an instance of H23.
The model map `formal/reified-rspaces-model-map-p6.json` maps 16 Rust items of the driver, the engine and the fire count to S21 to S26.

### 22.2 The properties

*Table 126. The properties of the VectorDB suite and their headlines.*

| Property | Headlines | What it checks |
|---|---|---|
| `prop_vectordb_driver_and_engine_agree` | S21 | The driver and the join engine fire the same continuations with the same values, and they leave the same records and waiters. |
| `prop_vectordb_no_pending_match` | S22, S23 | After each operation of a program without guards, a fresh receive with the pattern and the query of each parked waiter does not fire. |
| `prop_vectordb_list_is_ordered_and_complete` | S15 | A list holds $`\min(k, \lvert E \rvert)`$ values, and its delivered scores follow the selection order. |
| `prop_vectordb_replay_equal` | S26 | Replay of each program gives the outcomes of play, pairs every COMM and reaches the root of play. |
| `prop_vectordb_transactions_are_bounded` | S24 | Each transaction fires at most $`c`$ times. A flood of $`n \le 150`$ records or waiters drains in $`n`$ fires and $`\lfloor n / c \rfloor`$ stops at the round cap. |
| `prop_vectordb_records_are_conserved` | S25 | The last COMM that names a record that left the collection has no peeks. No record leaves when every receive peeks. |

Each property runs 64 cases with the fixed seed of the P5 runner.
The properties of agreement, of no pending match, of the bound and of conservation run each case on the driver and on the join engine.
The list property and the replay property run on the driver.
The map also names witness tests: the counterexamples CE-1 and CE-2, the carries at the round cap, and Rule E with embeddings.
The three join tests, the replay test and the restore of a member that a peek cascade lifts are witnesses too.

`prop_vectordb_transactions_are_bounded` follows the operation of each transaction of a chain.
A transaction that runs an operation stops at the round cap only after exactly $`c`$ fires.
A settle job starts its own count (decision 49).
It stops at the cap after $`c`$ fires of its own, or it resumes the stopped operation with the cause of that stop.

The generated programs stay below the cap, so each case also floods a collection with up to 150 records or parked receives.
One persistent operation then drains the flood.

The first version of this property claimed that a re-issue with the cause `RoundCap` always follows exactly $`c`$ fires.
Proptest refuted it with a flood of 64 parked receives in the join engine.
The settle job of the cap re-issued the persistent send with the cause `RoundCap` after 0 fires.
The engine owes a cascade search after every fire, and the driver owes it only when a record left.
So at the cap the engine continues in a settle job, which resumes the send with the cause of its stop.
Both behaviors satisfy S24, and the property now states the rule of the resume.

`prop_vectordb_records_are_conserved` compares the records before and after each operation of a program.
It identifies a record by the hash of its source produce and by its payload, its embedding and its persistence.
For each record that left, the last COMM of the operation that names its source produce must have no peeks.
That COMM is a fire of a waiter that does not peek, or its `K_retire`.
A `K_lift` also has no peeks, but the fire of the lifted record follows it, with the peeks of its waiter.

Each program runs in three variants.
The first variant is the program as generated.
In the second variant, every receive peeks, and then no record may leave.
The third variant is a drain: every send is not persistent, and every list waiter peeks.
After the program, the drain runs one plain receive without a peek for each operation, and each receive takes one record.
So each parked list waiter whose list is too long fires in a cascade when two eligible records remain.

The property reached its form in three measured steps.
The first version compared the values that the fires deliver, and that check was wrong.
A fire delivers the bindings of its pattern, so the pattern `[a, b]` delivers two values and not the record.
The second version accepted any COMM without peeks that names the record.
The mutant `driver-lift-not-owed` survived it, because a `K_lift` has no peeks.

A count over the 64 generated programs, with every list waiter as a peek, then showed a second gap.
Only 3 programs fired a parked list waiter after its own receive, and no program lifted a member for a peek waiter.
In the drain variant, 57 programs fire a parked list waiter later, with 102 such fires in the chains of receives.
The drain variant also makes 49 lifts that the fire of a peek waiter follows.
With the rule of the last COMM and the drain, the property fails under the mutant.
The witness test `space_vectordb_cascade_peek_restores_the_lifted_member` fails under it too, in its run on the driver.

### 22.3 Negative controls

*Table 127. The negative controls of step 8b.*

| Control | Kind | What it changes | Must fail in |
|---|---|---|---|
| `vdb-no-cascade` | Rocq mutant | The tail runs no LIST-CASCADE. | `first_cascade_none` |
| `vdb-peek-not-owed` | Rocq mutant | A peek fire of RECEIVE-VDB owes no second sends. | `receive_nl` |
| `vdb-lift-not-owed` | Rocq mutant | A cascade fire of a peek owes no second send for its lifted member. | `tail_nl` |
| `vdb-retire-unlogged` | Rocq mutant | An offer fire does not log the members that it retires. | `offer_nl` |
| `vdb-window-top1` | Rocq mutant | The similarity window of a join bind holds only its best record. | `jfill_mono` |
| `vdb-cont-on-persistent` | Rocq mutant | A fire whose persistent partner stays continues the loop. | `after_fire_cont` |
| `vdb-offer-never-fires` | Rocq mutant | OFFER-VDB fires no parked waiter. | `first_offer_none` |
| `cap-allows-one-more` | Rust mutant | A transaction fires $`c + 1`$ times before it stops. | `prop_vectordb_transactions_are_bounded` and 1 example test |
| `driver-lift-not-owed` | Rust mutant | A cascade fire of a peek in the driver owes no second send for its lifted member. | `prop_vectordb_records_are_conserved` and the witness `space_vectordb_cascade_peek_restores_the_lifted_member` |
| `engine-rule-e-dropped` | Rust mutant | A peek fire of the engine sends no record again. | `prop_vectordb_records_are_conserved` and `prop_vectordb_driver_and_engine_agree` |

The plan named a control that drops the claims of the join search.
Without claims, the search of one bind is `find`, and `find` fails on a filtered list when it fails on the list.
So every statement of the module stays true, and that control would test only the text of a proof.
`vdb-window-top1` replaces it, because a window of the best record makes the removal claim of S21 false.

The first run of `vdb-lift-not-owed` failed in `tail_inv`, whose statement stays true.
That proof restated the next transaction in full, so it depended on the text of the rule.
The lemma `TI_drop_set` now proves the invariant on the transaction of the goal, and the control fails in `tail_nl`.
The module records both lessons under "Failed strategies".

The final run of `vectordb-mutants.py` kills all nine Rust mutants, and it restores the source files.
Each new mutant fails the property of its headline: S24 for `cap-allows-one-more`, and S25 for the two Rule E mutants.
Table 100 gives the counts of the six older mutants in the same run.

### 22.4 Running the step-8b checks

To run the checks of step 8b, do these steps from the repository root:

1. Run `bash scripts/check-reified-rspaces-proofs.sh`, which must report `FAIL=0`.
2. Run `cargo test -p rholang --test reified_vectordb`.
3. Run `python3 scripts/verify-reified/vectordb-mutants.py`, which must report `FAIL=0`.
4. Run `python3 scripts/verify-reified/check-invariant-map.py --phase P6`.
5. Run `python3 scripts/verify-reified/check-model-map.py --map formal/reified-rspaces-model-map-p6.json`.

## 23. Phase P6, step 8c: the TLA+ model of the VectorDB discipline

Step 8c adds `VectorDb.tla`, a TLA+ model of one VectorDB collection.
TLC explores every schedule of a few operations, with the settle jobs and the re-issues at the round cap.
The Rocq model of step 8b ends a transaction at its first stop, so these continuations are new in this model.
Five Unsafe controls show that each invariant depends on its rule, and a new property tests atomic list fires in Rust.

### 23.1 The model

Each step of the model is one space transaction: a fresh send, a fresh receive, a re-issued operation, or a settle job.
No other transaction runs inside it, so the step computes the whole transaction, as in `JoinCascade.tla`.
The payload img scores above doc for every query.
The query all makes every record eligible, and the query img makes only the img records eligible, as a bound does.
The selection order is the better score first, then the lower id, as the DR-75 tie-break.
A plain receive reads every record in id order, the canonical order.

The plans follow the driver.
RECEIVE-VDB fires a receive on its selection or parks it, and a peek owes its removed members as second sends.
OFFER-VDB offers an element to the first parked waiter that it fires, and a list fire without a peek retires the other members.
The tail offers the owed second sends first and then runs LIST-CASCADE.
One fire count runs through the transaction, and `AfterFire` is `FireRounds::after_fire`.

A review of the first draft against the code found three differences, and the model now follows the code in each case:

1. When the element of the operation stops at a divergent pair, the tail still runs in the same transaction (`finish_tail`). Only a stop at the round cap with work owed stops the transaction at once.
2. The owed second sends form a stack: the last owe runs first, and one fire owes its members in delivery order (`Tail::owe_all`).
3. A settle job of the driver runs in the join engine (`classify`). It offers its pending second sends first and then always runs the cascade (`Engine::run`, joins plan section 4.10).

*Table 128. The invariants of `VectorDb.tla`.*

| Id | Invariant | Statement | Rocq | Property |
|---|---|---|---|---|
| W1 | `QuiescentNoPendingMatch` | With no job outstanding, no parked waiter can fire on the records (LM-b). | S22 | `prop_vectordb_no_pending_match` |
| W2 | `ListAtomic` | A list fire without a peek spends each member of its list that is not persistent, in the transaction of the fire. | none | `prop_vectordb_list_fires_are_atomic` |
| W3 | `PeekConservation` | Every record that was stored is stored, spent by a fire of a waiter that does not peek, or pending in a settle job (Rule E). | S25 | `prop_vectordb_records_are_conserved` |
| W4 | `CascadeTerminates` | Each step of each tail lowers $`2\,(c - f) + o`$ or stops the transaction, and no transaction runs out of its fuel. | S24 | `prop_vectordb_transactions_are_bounded` |

A transaction that runs out of its fuel commits nothing in the model.
It only records the failed check of W4, so no other invariant sees a partial transaction.

### 23.2 Configurations

*Table 129. The safe configurations of `VectorDb.tla`.*

| Configuration | What it covers | Distinct states | Depth |
|---|---|---|---|
| `MC_VectorDb_List` | Single and list waiters of the query all, with peeks and persistent sends. Five operations reach CE-1 and CE-2. | 64,597 | 8 |
| `MC_VectorDb_Persist` | Persistent receives and sends at $`c = 1`$: divergent pairs, re-issues and settle jobs. | 17,193 | 9 |
| `MC_VectorDb_Plain` | Plain receives and list waiters of the queries all and img, with the list lengths all, 1 and 3. | 1,677,464 | 8 |

A safe run passes only if it reaches the states that it claims to cover.
So temporary invariants that deny each state checked the persist configuration, and TLC violated each one within four steps.
The states are an outstanding settle job, an outstanding re-issued operation, and a settle job with a resume operation.
Two pending second sends, and a persistent waiter parked next to records, are states of the check too.

### 23.3 Negative controls

*Table 130. The Unsafe controls of `VectorDb.tla`.*

| Control | Rule it reverts | Refutes | Shortest trace |
|---|---|---|---|
| `VdbNoCascadeUnsafe` | The tail runs no LIST-CASCADE. | W1 | CE-2: 5 transactions |
| `VdbParkOnlyWhileEmptyUnsafe` | LM-a: a list waiter parks only while no record is eligible. | W1 | CE-1: 3 transactions |
| `VdbNoRetireUnsafe` | A list fire without a peek retires no other member. | W2 | 3 transactions |
| `VdbDropOwedUnsafe` | A peek fire owes no second sends. | W3 | 2 transactions |
| `VdbUncountedCascadeUnsafe` | A cascade fire does not count toward the fires. | W4 | 5 transactions |

Each control refutes exactly its invariant, and the complement run without that invariant finds no violation.
Design v3 names the first three controls.
The last two give W3 and W4 a control each, so that no invariant of the model is vacuous.

### 23.4 The property of atomic list fires

The first run of the step-8b harness showed a gap: only the example tests CE-1 and CE-2 killed the mutant `no-member-retire`.
`prop_vectordb_list_fires_are_atomic` now tests W2 on generated programs.
It gives each send a distinct payload, so the values that a list fire binds name the members of its list.
After each operation, a member of a list fire of a waiter that does not peek may stay stored only when it is persistent.
Each program also runs as a drain whose list waiters keep their peek flags, so cascade fires without a peek occur.
The property fails under `no-member-retire`, together with the two example tests.

The final run of `vectordb-mutants.py` with this property kills all nine Rust mutants and restores the source files.
Table 100 gives the counts of that run.

### 23.5 Running the step-8c checks

To run the checks of step 8c, do these steps from the repository root:

1. Run `bash scripts/check-reified-rspaces-tla-invariants.sh`, which must report `FAIL=0`.
2. Run `cargo test -p rholang --test reified_vectordb`.
3. Run `python3 scripts/verify-reified/vectordb-mutants.py`, which must report `FAIL=0`.
4. Run `python3 scripts/verify-reified/check-invariant-map.py --phase P6`.
5. Run `python3 scripts/verify-reified/check-model-map.py --map formal/reified-rspaces-model-map-p6.json`.


## 24. Phase P6, step 8d: the model of the mirror and the replies of `rho:lang:vector`

Step 8d adds two formal models and maps the properties of the mirror and of `rho:lang:vector` to them.
`ChromaMirror.tla` models the ChromaDB mirror of a VectorDB space on one node, and the replay of its deploys on another node.
`VectorOps.v` proves the kind and the rounding of the replies of `rho:lang:vector`.
Seven new properties test the capture, the journal, the writer, the reconciler and the recorded processes.

### 24.1 The model of the mirror

The records of a space are consensus state, and the mirror of a node is a copy of the records outside consensus.
Each step of the model is one action.
The actions are a deploy, an exploratory evaluation, a batch of the writer, a finalization, a reconciler pass and a replay.
A deploy maps the records of its pre-state to its post-state, and its block holds the post-state.
A deploy that calls `ann` asks the mirror and records the answer.
It then consumes the first stored record that the answer names, as the exact query `ids=` does.

The mirror path of the model follows the code in this order:

1. Each space job records the net writes of its transaction in the journal of the evaluation.
2. A final result with no errors publishes the journal, and any other result drops it (M1, decision 66).
3. The start of each root evaluation drops a journal that no result settled.
4. A publication never waits, and a full queue marks the collection dirty.
5. The writer applies each batch in order, and a batch that fails after its attempts marks the collection dirty.
6. A reconciler pass repairs each registered channel of a stale or dirty collection to the view of the newest finalized block.

Replay runs on another node, with a mirror in any state.
It uses the recorded answer of `ann`, and it makes the checks of `ann` as play does (decision 67).
The header of the module names the Rust code of each action, with 18 line ranges that `check-model-map.py` validates.

A deploy of the model is one transaction.
In the code, the journal keeps the last write of each record of a deploy.
So a record that one transaction stores and a later transaction removes leaves a delete.
That delete names an id that the mirror does not hold, and ChromaDB accepts it with no effect.

*Table 131. The invariants of `ChromaMirror.tla`.*

| Id | Invariant | Statement |
|---|---|---|
| W5 | `ConsensusIndependentOfMirror` | The post-state of each deploy is a function of its pre-state and its recorded answer only. |
| W6 | `AdvisoryRecorded` | Replay uses the recorded answer of `ann` and never asks its own mirror. |
| W7 | `EqualHostWork` | Play and replay make the same checks of an `ann` call, so they reserve the same host work for them (decision 67). |
| W8 | `PublishOnlyOnSuccess` | Every published write comes from a deploy whose final result has no errors (M1). |
| W9 | `NetWritesExact` | A published batch upserts exactly the records that its deploy added and deletes exactly the records that it removed. A Rule E pair writes nothing. |
| W10 | `PublishNeverWaits` | A publication never waits for the writer. |
| W11 | `ReconciledEqualsFinalizedView` | After a reconciler pass that finishes, the mirror holds the finalized view on every registered channel. |
| W12 | `WatermarkHonest` | The watermark of a collection never passes the block of the last pass that finished. |

### 24.2 Configuration and negative controls

`MC_ChromaMirror` has two record ids on two channels, three deploys, eight steps and a queue of one batch.
TLC explores 1,722,212 distinct states to the depth 9, and no invariant fails.
A queue of one batch is full after one publication, so the configuration reaches the dirty path of the writer.

*Table 132. The Unsafe controls of `ChromaMirror.tla`.*

| Control | Rule it reverts | Refutes |
|---|---|---|
| `MirrorWriteInTxnUnsafe` | The deploy writes the mirror inside its transaction, and a failed write aborts the deploy. | W5 |
| `UnrecordedAdvisoryUnsafe` | Replay asks its own mirror. | W6 |
| `AnnChecksOnlyInPlayUnsafe` | Replay returns the recorded answer before the checks. | W7 |
| `PublishFailedDeployUnsafe` | A deploy whose result has errors publishes its journal. | W8 |
| `BeginScopeKeepsJournalUnsafe` | The start of a root evaluation keeps the journal of an exploratory evaluation. | W8 |
| `RuleEUpsertUnsafe` | A peek upserts the record that it stored again. | W9 |
| `BlockingPublishUnsafe` | A full queue makes the publication wait. | W10 |
| `OrphansKeptUnsafe` | A pass keeps the orphans of a registered channel. | W11 |
| `WatermarkAfterFailureUnsafe` | A failed pass sets the watermark. | W12 |

Each control refutes exactly its invariant, and the complement run without that invariant finds no violation.

### 24.3 The replies of `rho:lang:vector`

`rho:lang:vector` computes `add`, `scale` and `dot` exactly, and it replies in the widest kind of its inputs (decision 66, design v3 section 4.7).
An integer input gives the integer kind, a rational or a fixed-point input gives the rational kind, and a float input gives the float kind.
`VectorOps.v` models a number as an integer, a rational, or a float with its exact value, in the rationals of the Rocq standard library.
The rounding to nearest is an abstract function $`\mathrm{rnd}`$, so the claims hold for every rounding.

*Table 133. The headlines of `VectorOps.v`.*

| Id | Theorem | Statement |
|---|---|---|
| S27 | `vector_replies_exact` | The reply of `add`, `scale` and `dot` has the widest kind of the inputs. It is the exact value for the integer and the rational kinds. For the float kind it is $`\mathrm{rnd}`$ of the exact value, so `dot` rounds its exact sum once (decision 54). |
| S28 | `root_midpoint_nearest` | Let $`r`$ be a representable reply with the neighbours $`l < r < h`$, and let $`s \ge 0`$ be the root of $`x`$. If the squares of the midpoints bound $`x`$, no representable value is nearer to $`s`$ than $`r`$. |

The bound of S28 is:

```math
\left(\frac{l + r}{2}\right)^{2} \le x \le \left(\frac{r + h}{2}\right)^{2},
```

where the lower bound applies only when $`(l + r)/2 > 0`$.
`prop_vector_ops_roots_are_correctly_rounded` checks these two bounds exactly for the reply of `norm`, with the adjacent `f64` values as $`l`$ and $`h`$.
So S28 turns the check of the property into the claim that the root is correctly rounded.

S27 also shows that one branch of `number_par` is unreachable.
That branch fails with `Inconsistent` when an integer reply has a value that is not an integer.
The invariant map adds `prop_vector_ops_dot_laws` to S6 and `prop_vector_ops_distance_laws` to S7 of `Similarity.v`.

### 24.4 The properties

*Table 134. The properties of step 8d.*

| Property | File | Covers | What it checks |
|---|---|---|---|
| `prop_vector_ops_agree_with_an_exact_oracle` | `reified_vector_ops.rs` | S27 | On integers, rationals and fixed-point decimals, `add`, `scale` and `dot` equal the exact values, in the widest kind of the inputs. |
| `prop_vector_ops_float_inputs_give_floats` | `reified_vector_ops.rs` | S27 | A float among the inputs gives float values. On dyadic inputs whose results are exact in `f64`, they equal the exact values. |
| `prop_vector_ops_dot_laws` | `reified_vector_ops.rs` | S6 | `dot` is symmetric, and one permutation of the coordinates of both vectors keeps its value. |
| `prop_vector_ops_distance_laws` | `reified_vector_ops.rs` | S7 | The distance is not negative, symmetric, and zero from a vector to itself. The norm of a vector is its distance from zero. |
| `prop_vector_ops_roots_are_correctly_rounded` | `reified_vector_ops.rs` | S28 | The reply of `norm` satisfies the midpoint bounds of S28. |
| `prop_mirror_capture_net_writes_are_the_change` | `spaces/mirror.rs` | W9 | The capture upserts each record with an embedding that a transaction added, and it deletes each record that the transaction removed. |
| `prop_mirror_journal_keeps_the_last_write` | `spaces/mirror.rs` | W9 | The journal keeps the last write of each record id, and it gives one batch for each collection. |
| `prop_mirror_writer_applies_the_accepted_batches` | `reified/vectordb_services.rs` | W10 | A full queue refuses a batch and never waits. The writer applies the batches that the queue took, in order, and it marks dirty each collection whose batch it did not apply. |
| `prop_mirror_reconciler_watermarks_follow_finished_repairs` | `reified/mirror_reconciler.rs` | W12 | A watermark changes only through a repair that finishes, to the larger of the block and the old watermark. A dirty collection that a pass does not finish stays dirty. |
| `prop_mirror_tracks_the_published_deploys` | `reified_vectordb_chroma.rs` | W8, W9 | After each step of a generated chain, the mirror holds the records that the state changes of the successful deploys give. |
| `prop_mirror_reconcile_repairs_the_finalized_view` | `reified_vectordb_chroma.rs` | W11 | A pass that fails keeps the mirror and the watermark, and a pass that finishes holds exactly the records of the finalized state. |
| `prop_vectordb_services_replay_reproduces_the_recorded_outputs` | `reified_vectordb_chroma.rs` | W5, W6, W7 | Native replay with other services reproduces play with a complete journal, and it asks no mirror and no embedder. |

The properties of `reified_vector_ops.rs` run 64 cases, and the suite properties run 16 cases, both with the fixed seed of the P5 runner.
The suite properties run in the latest execution model: native play and native replay certification (decision 69).
The four in-module properties run 64 cases with a fixed seed of their own.
The invariant map also names seven example tests of the suite as witnesses of W5, W6 and W8 to W12.

Four hypotheses of the first versions failed, and each one is recorded here with its cause.

1. The capture of a removed record without an embedding was expected to write nothing. It writes a delete, because the capture does not read the removed datum. The mirror never holds such a record, so the delete has no effect. The property now applies the rule of embeddings to upserts only.
2. An exploratory evaluation was expected to succeed. It can fail with `CapacityExceeded` on a full collection, and it then publishes nothing. The property accepts that failure.
3. The test chain plays a failed deploy without a revert, unlike the deploys of a block. So the property follows the state changes of the successful deploys only, and not the state of the chain.
4. Play and replay were expected to reserve the same total host work. Native replay certification also verifies the committed evidence, so it reserves more. For one `embed` call, replay reserved 77,826,946 verification bytes, and play reserved 8,869,621.

The fourth result does not contradict decision 67.
The checks of an `ann` call are journaled native operations, so replay must make each check of play, or the certification finds an incomplete journal.
`prop_vectordb_services_replay_reproduces_the_recorded_outputs` checks that completeness, and it does not compare the totals.

### 24.5 Negative controls

*Table 135. The Rocq controls of step 8d.*

| Control | What it changes | Must fail in |
|---|---|---|
| `vo-kjoin-narrower` | The join of two kinds takes the narrower kind. | `kjoin_rank` |
| `vo-dot-rounds-each-term` | `dot` rounds each product before the sum. | `vector_replies_exact` |
| `vo-faithful-root-bound` | The check bounds $`x`$ by $`h^{2}`$ instead of the square of the midpoint, which allows a faithful rounding. | `root_midpoint_nearest` |

*Table 136. The run of the mirror and services mutants in step 8d.*

| Harness | Mutant | Tests that fail | New properties that fail |
|---|---|---|---|
| mirror | `publish-with-errors` | 2 | `prop_mirror_tracks_the_published_deploys` |
| mirror | `publish-on-err` | 2 | `prop_mirror_tracks_the_published_deploys` |
| mirror | `no-play-settle` | 14 | `prop_mirror_reconcile_repairs_the_finalized_view`, `prop_mirror_tracks_the_published_deploys` |
| mirror | `no-replay-settle` | 1 | none |
| mirror | `begin-scope-keeps-journal` | 2 | `prop_mirror_tracks_the_published_deploys` |
| mirror | `journal-not-appended` | 14 | `prop_mirror_reconcile_repairs_the_finalized_view`, `prop_mirror_tracks_the_published_deploys` |
| mirror | `rule-e-upserts` | 3 | `prop_mirror_capture_net_writes_are_the_change` |
| mirror | `new-then-removed-deletes` | 2 | `prop_mirror_capture_net_writes_are_the_change` |
| mirror | `persistent-removed` | 3 | `prop_mirror_capture_net_writes_are_the_change` |
| mirror | `incoming-removed` | 3 | `prop_mirror_capture_net_writes_are_the_change` |
| mirror | `foreign-channel-removed` | 2 | `prop_mirror_capture_net_writes_are_the_change` |
| mirror | `non-finite-mirrored` | 1 | none |
| mirror | `document-dropped` | 2 | none |
| mirror | `persistence-dropped` | 2 | none |
| mirror | `capture-without-mirror-param` | 2 | none |
| mirror | `journal-first-write-wins` | 3 | `prop_mirror_journal_keeps_the_last_write` |
| mirror | `dot-as-cosine` | 2 | none |
| mirror | `capture-index-shifted` | 11 | `prop_mirror_reconcile_repairs_the_finalized_view`, `prop_mirror_tracks_the_published_deploys` |
| mirror | `writer-no-retry` | 4 | none |
| mirror | `failed-batch-not-dirty` | 5 | `prop_mirror_writer_applies_the_accepted_batches` |
| mirror | `full-queue-not-dirty` | 3 | `prop_mirror_writer_applies_the_accepted_batches` |
| mirror | `collection-not-ensured` | 19 | `prop_mirror_writer_applies_the_accepted_batches`, `prop_mirror_reconcile_repairs_the_finalized_view`, `prop_mirror_tracks_the_published_deploys` |
| mirror | `deletes-skipped` | 10 | `prop_mirror_writer_applies_the_accepted_batches`, `prop_mirror_tracks_the_published_deploys` |
| mirror | `registry-not-written` | 5 | `prop_mirror_reconcile_repairs_the_finalized_view` |
| mirror | `capture-channels-dropped` | 6 | `prop_mirror_reconcile_repairs_the_finalized_view` |
| mirror | `orphans-kept` | 3 | `prop_mirror_reconcile_repairs_the_finalized_view` |
| mirror | `missing-not-added` | 5 | `prop_mirror_reconcile_repairs_the_finalized_view` |
| mirror | `watermark-not-advanced` | 6 | `prop_mirror_reconciler_watermarks_follow_finished_repairs`, `prop_mirror_reconcile_repairs_the_finalized_view` |
| mirror | `watermark-decreases` | 2 | `prop_mirror_reconciler_watermarks_follow_finished_repairs` |
| mirror | `current-collection-repaired` | 2 | none |
| mirror | `dirty-ignored` | 3 | `prop_mirror_reconciler_watermarks_follow_finished_repairs` |
| mirror | `dirty-not-remarked` | 3 | `prop_mirror_reconciler_watermarks_follow_finished_repairs` |
| mirror | `dirty-not-prepared` | 2 | `prop_mirror_reconciler_watermarks_follow_finished_repairs` |
| mirror | `events-ignored` | 1 | none |
| services | `embed-not-recorded` | 4 | `prop_vectordb_services_replay_reproduces_the_recorded_outputs` |
| services | `ann-not-recorded` | 4 | `prop_vectordb_services_replay_reproduces_the_recorded_outputs` |
| services | `embed-replay-recomputes` | 3 | `prop_vectordb_services_replay_reproduces_the_recorded_outputs` |
| services | `ann-replay-recomputes` | 4 | `prop_vectordb_services_replay_reproduces_the_recorded_outputs` |
| services | `ann-checks-only-in-play` | 4 | `prop_vectordb_services_replay_reproduces_the_recorded_outputs` |
| services | `quota-dropped` | 1 | none |
| services | `non-finite-accepted` | 1 | none |
| services | `mirror-check-dropped` | 1 | none |
| services | `k-unbounded` | 1 | none |
| services | `channel-digest-of-the-space` | 1 | none |
| services | `malformed-id-kept` | 1 | none |
| services | `config-entry-dropped` | 2 | none |

All 34 mirror mutants and all 12 services mutants fail tests, and both harnesses restore the files.
In 30 runs, a new property of step 8d is among the failing tests.
Tables 118 and 122 record the runs of steps 7b and 7c, before these properties existed.

### 24.6 Running the step-8d checks

To run the checks of step 8d, do these steps from the repository root:

1. Run `bash scripts/check-reified-rspaces-proofs.sh`, which must report `FAIL=0`.
2. Run `bash scripts/check-reified-rspaces-tla-invariants.sh`, which must report `FAIL=0`.
3. Run `cargo test -p rholang --lib --test reified_vectordb_chroma --test reified_vector_ops`.
4. Run `python3 scripts/verify-reified/mirror-mutants.py`, which must report `FAIL=0`.
5. Run `python3 scripts/verify-reified/vectordb-services-mutants.py`, which must report `FAIL=0`.
6. Run `python3 scripts/verify-reified/check-invariant-map.py --phase P6`.
7. Run `python3 scripts/verify-reified/check-model-map.py --map formal/reified-rspaces-model-map-p6.json`.

## 25. Phase P6, step 8e: the documents of the VectorDB family

Step 8e writes the documents of phase P6 (design v3 section 7, tracker leaf `rrs-doc-vectordb-tensor-chroma`).
Each document states what the code of steps 1 to 8d does, and the citations name its lines.

*Table 137. The documents of step 8e.*

| Document | Part | Content |
|---|---|---|
| [specification.md](specification.md) | [3.9](specification.md#39-vectordb), [9](specification.md#9-system-processes-of-the-vectordb-family) and [10](specification.md#10-the-chromadb-mirror) | The rules VD-1 to VD-14 of the VectorDB discipline, SP-1 to SP-6 of the system processes, and MR-1 to MR-7 of the mirror. Table 9 gives the evidence of each rule. |
| [design.md](design.md) | [N15](design.md#n15-phase-p6-the-vectordb-family) and [N16](design.md#n16-phase-p6-the-chromadb-search-backend) | The number model, the records and the selection, the driver, the join engine, the factory, `rho:lang:vector`, native execution, the services of a node, the mirror path, and the recorded processes. |
| [syntax-contract.md](syntax-contract.md) | Params and queries at run time | The factory params of the family, the send param `embedding`, and the checks of a query with their codes. |
| [decision-register.md](decision-register.md) | Decisions 66 to 71 | The entries of the decisions of P6, and the amendments of decisions 56 and 57. |
| [glossary.md](glossary.md) | Part 2 | 20 terms, from the exact value to the advisory query. |
| [`../../rholang/21-reified-spaces.md`](../../rholang/21-reified-spaces.md) | VectorDB spaces | The language section on VectorDB spaces. Phase P10 completes the chapter. |
| [`../../rholang/chromadb.md`](../../rholang/chromadb.md) and [`10-system-channels.md`](../../rholang/10-system-channels.md) | The search backend, the processes | The mirror, `embed` and `ann`, the example of an advisory query, and how to run a node with the mirror. |

Six new diagrams show the selection (28), list fires and LIST-CASCADE (29), and the mirror (30).
They also show recorded embeddings (31), the advisory query (32), and the number model (33).
Each diagram restates the code that its header names, and the image text gives the whole figure.
The node services outside consensus get the color alias `$rrs_service`, so that no two concepts share a color.

The examples of chapter 21 and of `chromadb.md` are rows of the syntax corpus, as the documentation rows of P4.
The model check of the corpus test now also visits the cases of a `match`.
So the query in a case of the `ann` example has a model.

The lint reports no violation over the campaign documents and chapter 21, and the render check reproduces each SVG byte for byte.
The corpus test passes its four tests with 44 rows, and the first 40 rows keep their bytes.
The three examples of chapter 10 hold no Reified syntax, so they are no corpus rows.
Each of them parses with no error node in the tree-sitter grammar of the parser branch.
The footprint policy `p6` approves the two edits outside the campaign directory as replaced blocks.
They are the new section of `chromadb.md`, and the section and the three summary rows of chapter 10.

### 25.1 Running the step-8e checks

To run the checks of step 8e, do these steps from the repository root:

1. Run `python3 scripts/verify-reified/doc_lint.py`, which must report no violation.
2. Run `bash docs/rspace/reified-rspaces/diagrams/render.sh --check`.
3. Run `cargo test -p rholang --test reified_syntax_corpus`.

## References

<a id="ref-ch00"></a>[CH00] K. Claessen and J. Hughes, "QuickCheck: a lightweight tool for random testing of Haskell programs," in *Proceedings of the Fifth ACM SIGPLAN International Conference on Functional Programming (ICFP 2000)*, ACM, 2000, pp. 268–279. [doi:10.1145/351240.351266](https://doi.org/10.1145/351240.351266)

<a id="ref-ch88"></a>[CH88] T. Coquand and G. Huet, "The calculus of constructions," *Information and Computation*, vol. 76, no. 2–3, pp. 95–120, 1988. [doi:10.1016/0890-5401(88)90005-3](https://doi.org/10.1016/0890-5401(88)90005-3)

<a id="ref-dls78"></a>[DLS78] R. A. DeMillo, R. J. Lipton, and F. G. Sayward, "Hints on test data selection: help for the practicing programmer," *Computer*, vol. 11, no. 4, pp. 34–41, 1978. [doi:10.1109/C-M.1978.218136](https://doi.org/10.1109/C-M.1978.218136)

<a id="ref-kkt19"></a>[KKT19] I. Konnov, J. Kukovec, and T.-H. Tran, "TLA+ model checking made symbolic," *Proceedings of the ACM on Programming Languages*, vol. 3, no. OOPSLA, pp. 1–30, 2019. [doi:10.1145/3360549](https://doi.org/10.1145/3360549)

<a id="ref-lam94"></a>[Lam94] L. Lamport, "The temporal logic of actions," *ACM Transactions on Programming Languages and Systems*, vol. 16, no. 3, pp. 872–923, 1994. [doi:10.1145/177492.177726](https://doi.org/10.1145/177492.177726)

<a id="ref-pt"></a>[PT] The proptest authors, "Proptest," version 1.11.0, a property-testing framework for Rust. [proptest-rs.github.io/proptest](https://proptest-rs.github.io/proptest/)

<a id="ref-roc25"></a>[Roc25] The Rocq Development Team, "The Rocq Prover," Zenodo, 2025. The DOI names every archived release, and the newest archived release is 9.1.0. [doi:10.5281/zenodo.15149628](https://doi.org/10.5281/zenodo.15149628)

<a id="ref-yml99"></a>[YML99] Y. Yu, P. Manolios, and L. Lamport, "Model checking TLA+ specifications," in *Correct Hardware Design and Verification Methods (CHARME 1999)*, Lecture Notes in Computer Science, vol. 1703, Springer, 1999, pp. 54–66. [doi:10.1007/3-540-48153-2_6](https://doi.org/10.1007/3-540-48153-2_6)
