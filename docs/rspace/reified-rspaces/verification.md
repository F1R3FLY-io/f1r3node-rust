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
12. [References](#references)

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
6. **Total.** The count of closed headlines is 115 over 11 modules. P1 has 23 and `Encoding.v` has 15. The six P4 modules have 67 (sections [11.3](#113-rocq-models-of-p4), [11.11](#1111-native-execution-of-decision-46), [11.12](#1112-re-entry-of-decision-43) and [11.13](#1113-peek-re-store-of-decision-45)). `AgentReadCoverage.v` has 8 (section [11.10](#1110-metering-of-decision-42)), and `LegacyCharges.v` has 2 (section [11.14](#1114-legacy-charges-of-decision-45)). `Encoding.v` has 12 headlines of P2 and 3 of I-20.
7. **Negative controls.** The 75 mutants fail in their expected declarations. Sections [6.1](#61-rocq-mutants) and [11.5](#115-negative-controls-of-p4) hold 29 and 29 of them. Sections [11.10](#1110-metering-of-decision-42), [11.11](#1111-native-execution-of-decision-46), [11.12](#1112-re-entry-of-decision-43), [11.13](#1113-peek-re-store-of-decision-45) and [11.14](#1114-legacy-charges-of-decision-45) hold 5, 1, 3, 3 and 5. The 11 pristine modules compile first.

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
| H1 | `commit_trace_eq_host` (`EventCorrelation.v:5472`) | A commit of the staged operations makes exactly the observer calls, events, status, and replay bindings of the host run of the same operations. | None | `prop_space_txn_play_trace_equals_host`, `prop_space_txn_repeated_channel_groups_equal_host` |
| H2 | `commit_state_eq_host` (`EventCorrelation.v:5530`) | The committed store equals the host store: equal continuations, joins, and counters, and each data list a permutation of the host list. Since [decision 45](decision-register.md#decision-45), the statement has no premise on peeks. | None | `prop_space_txn_play_root_equals_host`, `prop_space_txn_repeated_channel_groups_equal_host` |
| H3 | `counter_timing_play_replay` (`EventCorrelation.v:6171`) | The play counter closure and the replay rule give the same COMM. A rejected replay fire rolls its increment back exactly, and the staged counter view advances as the host counter does. | None | `prop_counter_timing_play_equals_replay` |
| H4 | `verify_sound_complete` (`EventCorrelation.v:6261`) | `verify` pairs a COMM exactly when the log holds the trigger event and an unpaired recorded occurrence remains, and `recorded_comms` reports the same copies. A replay fire without its trigger event stages unpaired, with no pairing and no change of the bindings (conjunct 3, `stage_without_trigger_unpaired`). An unpaired fire never commits: it emits no event and no effect, and it keeps the bindings and the store (conjunct 6, `unpaired_fire_inert`). | None | `prop_space_txn_replay_divergence_typed_at_commit`, `prop_space_txn_replay_requires_trigger_event` |
| H5 | `replay_pairs_each_comm_once` (`EventCorrelation.v:6465`) | `rig` sets up the binding invariant from the play log. A replay transaction never pairs a COMM more often than the log records it, and its commit removes exactly the committed pairings. | None | `prop_space_txn_replay_pairs_play_log`, `prop_space_txn_repeated_channel_groups_equal_host`, `prop_space_txn_peek_restore` |
| H6 | `view_commit_coherent` (`EventCorrelation.v:5557`) | The staged view is the committed store. The commit stops at the first rejected operation, and the store then equals the view after the operations before it. | None | `prop_space_txn_view_equals_committed_store` |
| H7 | `joins_lockstep_preserved` (`EventCorrelation.v:5580`) | Staging and commit keep the join entries in lockstep with the waiting continuations. | None | `prop_space_txn_joins_lockstep` |
| H8 | `sub_order_between` (`EventCorrelation.v:8029`) | A sub-order sorts after its parent and before every later scheduler order. The sub-orders of one parent strictly increase, and no scheduler order equals a sub-order. | None | `prop_sub_orders_contiguous_and_disjoint` |
| H9 | `abort_inert` (`EventCorrelation.v:7130`) | A dropped transaction, also after access errors, and a commit after a store swap change no store, binding, observer call, event, or generation. The checkpoint then gives the same history map. | None | `prop_space_txn_abort_inert`, `prop_space_txn_non_canonical_inputs_fail_closed` |
| H10 | `coverage_total` (`EventCorrelation.v:6053`) | For every committed operation, an event of the same operation covers each store change, and every COMM follows its trigger event. | None | `prop_event_coverage_holds_for_generated_commits` |
| H11 | `peek_reproduces_trace_eq_host` (`EventCorrelation.v:5807`) | With the re-offer (FR-10) and the re-stores ([decision 45](decision-register.md#decision-45)) as the next operations, a commit makes the observer calls, events, status, and bindings of the host run, whose reducer sends the peeked data again, and the stores are equal. A fire never stores a datum, its receipt carries the stored persist flags, and the re-stores are exactly the sends of `produce_peeks`, each with the identity of its record. | None | `prop_space_txn_peek_restore` |
| H12 | `reporting_trace_is_replay_trace` (`EventCorrelation.v:7215`) | Reporting stages and commits exactly as replay does, and its host operations are those of replay. | None | `prop_space_txn_reporting_equals_replay`, `prop_space_txn_peek_restore` |
| H17 | `non_canonical_fails_closed` (`EventCorrelation.v:7072`) | A group that is not non-decreasing fails with `NonCanonicalChannelOrder` before any store or cache effect. A non-decreasing group passes, also when it repeats a channel, and the sorted list is the only canonical member of its permutation class. | None | `prop_space_txn_non_canonical_inputs_fail_closed`, `prop_space_txn_repeated_channel_groups_equal_host` |
| H19 | `directive_fails_closed` (`EventCorrelation.v:7255`) | In replay and reporting, a replay directive rejects the operation with no change of store, bindings, events, or effects, and the commit stops there. | None | `prop_space_txn_replay_directive_fails_closed` |
| H20 | `parent_order_single_txn` (`EventCorrelation.v:8302`) | `sub_order` is injective. A second commit under one parent in one evaluation scope fails before any effect, and a scope reset, a store swap, and a session change each reset the tracker. | None | `prop_space_txn_parent_order_single_commit` |
| H21 | `view_identity_is_store_identity` (`EventCorrelation.v:8423`) | View lookups and the installed guard respect the store identity, and the staged view agrees with the committed store. A view keyed by the Rust ordering disagrees for two channels with one store identity. | None | `prop_space_txn_store_identity_ignores_ord_only_fields`, `prop_space_txn_par_identity_ignores_locally_free` |
| H23 | `replay_reproduces_play_run` (`EventCorrelation.v:7805`) | Play and replay stage the same operations from the same view, and replay is rigged with the commit log of play. Then replay stages exactly the operations of play, and its commit makes the observer calls of play without the directive queries, the events of play, the status of play, and the store of play. So an operation that play rejects, for example for lack of phlo, fails replay at the same call, after the same prefix (decision 36). | None. The statement takes two premises: the observer ignores the directive queries, and it gives no directive. `replay_premises_satisfiable` shows the observer of a phlo limit that meets both. | `prop_space_txn_replay_reproduces_play_rejection` |

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
Table 45 of section [11.10](#1110-metering-of-decision-42) holds 5, and Table 47 of section [11.11](#1111-native-execution-of-decision-46) holds 1.
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

- `correlation_rejects_datum_without_produce_event` (`space_store_handle.rs:3069`) and `correlation_rejects_comm_before_trigger` (`space_store_handle.rs:3085`) show that the event correlation of the commit rejects a defect.
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

*Table 13. The checker self-tests (last run: 273 tests, 0 failures).*

| Module | Tests | What the tests check |
|---|---|---|
| `test_checkers` | 103 | The consensus-checklist checker (21), the footprint checker (28), the completeness checker (11), the Rust citation rule (10) and module (4), the ORD phase of the invariant-map checker (8), the production scan (7), the model-map checker for a phase other than P1 (9), and the P1 ids H23 and T6 of decision 36 in both map checkers (5). |
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
| `space_store_handle_receipt_keeps_the_host_flags` | `space_store_handle.rs:3440` | FR-10 | H11 | The receipt keeps the flag of the host for every result, and results that do not line up with the candidates fail closed. |
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
- **Native execution.** Native play and native replay reject a space deploy at one point, before any space work (Q67, section [11.11](#1111-native-execution-of-decision-46)).
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
| Q7 | `op_ids_pairwise_distinct` (`Namespace.v:966`) | The op ids of the factories, the system processes, and the markers are pairwise distinct. | `prop_op_ids_pairwise_distinct` |
| Q8 | `decode_encode_reified_id` (`Namespace.v:633`) | The decoder inverts every constructor, and it accepts only their images. | `prop_decode_reified_id_roundtrip_and_sound` |
| Q9 | `space_and_collection_counters_distinct` (`Namespace.v:784`) | The space counter differs from every collection counter. | `prop_space_and_collection_counters_distinct` |
| Q10 | `name_id_routes_to_sid` (`Namespace.v:995`) | A name id decodes to its space. | `prop_name_id_routes_to_sid` |
| Q11 | `canon_respects_par_eq` (`Namespace.v:1035`) | Channels that are equal as `Par` values have one collection key. | `prop_canonical_inner_bytes_respects_par_eq` |

#### 11.3.3 Headlines of RecordEnvelope.v

RecordEnvelope.v models the record envelopes of [decision M-2](decision-register.md#m-2): the datum, the continuation, the counter, and the config record.

*Table 27. The 8 headlines of RecordEnvelope.v.*

| Id | Theorem | Statement | Property tests |
|---|---|---|---|
| Q12 | `datum_unwrap_wrap` (`RecordEnvelope.v:591`) | Unwrapping a wrapped datum gives the order and the payload back. | `prop_datum_envelope_roundtrip` |
| Q13 | `datum_wrap_injective` (`RecordEnvelope.v:599`) | The datum envelope is injective in the order and the payload. | `prop_datum_envelope_injective` |
| Q14 | `datum_unwrap_sound` (`RecordEnvelope.v:610`) | The datum decoder accepts only the image of the encoder. | `prop_datum_envelope_decoder_sound` |
| Q15 | `continuation_unwrap_wrap` (`RecordEnvelope.v:632`) | Unwrapping a wrapped continuation gives the order and the body back. | `prop_continuation_envelope_roundtrip` |
| Q16 | `counter_and_config_roundtrip` (`RecordEnvelope.v:994`) | The counter and config records round-trip, the config as its canonical map. | `prop_counter_and_config_records_roundtrip` |
| Q17 | `envelope_kinds_disjoint` (`RecordEnvelope.v:1037`) | The tags of the four record kinds are pairwise distinct. | `prop_envelope_kinds_disjoint` |
| Q18 | `no_reified_datum_is_single_number` (`RecordEnvelope.v:1161`) | No Reified datum is a single number, so the merger never treats it as a number channel. | `prop_no_reified_datum_is_single_number` |
| Q19 | `unknown_version_rejected` (`RecordEnvelope.v:1103`) | A tag with another version or kind byte is rejected. | `prop_envelope_unknown_version_rejected` |

#### 11.3.4 Headlines of CounterConflict.v

CounterConflict.v restates merge check 1 of the baseline, and it proves the races and the serializability of the counter rule ([decision 31](decision-register.md#decision-31)).

*Table 28. The 9 headlines of CounterConflict.v.*

| Id | Theorem | Statement | Property tests |
|---|---|---|---|
| Q20 | `check1_race_iff` (`CounterConflict.v:337`) | Check 1 reports a race exactly for a shared non-persistent event that is not mergeable in both logs. | `prop_check1_restatement_matches_merger` |
| Q21 | `ordered_writes_race_on_counter` (`CounterConflict.v:743`) | Two branches that write one queue race on its counter. | `prop_queue_writes_race_on_counter` |
| Q22 | `ordered_reads_race_on_counter` (`CounterConflict.v:757`) | A peek bumps the counter too, so two branches that read one queue race. | `prop_queue_reads_race_on_counter` |
| Q23 | `first_ops_race_on_space_counter` (`CounterConflict.v:772`) | Two branches whose first operations open collections of one space race on the space counter. | `prop_first_ops_race_on_space_counter` |
| Q24 | `crossed_collections_race` (`CounterConflict.v:786`) | Branches that use two queues in crossed orders race on both counters. | `prop_crossed_collections_race` |
| Q25 | `same_block_ops_chain` (`CounterConflict.v:946`) | Two deploys of one block that use one queue form one chain. | `prop_same_block_queue_ops_chain` |
| Q26 | `unordered_writes_commute` (`CounterConflict.v:1484`) | Writes to a collection without a counter never race, and the merge holds both serial orders. | `prop_counter_free_writes_commute` |
| Q27 | `merge_without_race_is_serializable` (`CounterConflict.v:1322`) | Without a race, the merged state and the observations are those of a serial run. | `prop_nonracing_queue_branches_serializable` |
| Q28 | `counter_free_peek_admits_write_skew` (`CounterConflict.v:1596`) | A peek without the counter admits the write skew of a peek and a dequeue, as a computed counterexample. | `prop_counter_free_peek_admits_write_skew` |

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
| Q38 | `classify_space_ref_total_deterministic` (`SpaceRefs.v:449`) | Every space reference meets exactly one of the five rules of `classify_space_ref`. | `prop_classify_space_ref_total` |
| Q39 | `handle_requires_write_flag` (`SpaceRefs.v:480`) | A reference names a native space only as a bare handle or as a writable bundle of one. | `prop_handle_requires_write_flag` |
| Q40 | `reified_non_handle_not_a_space_ref` (`SpaceRefs.v:511`) | A Reified id that is not a handle is `NotASpaceRef`, never an agent. | `prop_reified_non_handle_not_a_space_ref` |
| Q41 | `route_total_deterministic` (`SpaceRefs.v:600`) | A route fails only on the key, and it depends on the context only through its native space. | `prop_route_total` |
| Q42 | `route_rules` (`SpaceRefs.v:624`) | The routing rules R1 to R5. | `prop_route_rules` |
| Q43 | `space_name_route_ignores_ctx` (`SpaceRefs.v:645`) | A space name routes to its own space under every context. | `prop_space_name_route_ignores_ctx` |
| Q44 | `r7_route_ignores_ctx` (`SpaceRefs.v:688`) | The R7 backend agrees with the reducer on every channel that is not forged. | `prop_r7_route_ignores_ctx` |
| Q45 | `ordinary_channels_route_default` (`SpaceRefs.v:710`) | The channels of an ordinary program route to the default space, and the fast path is exact. | `prop_ordinary_channels_route_default` |
| Q46 | `use_wrapper_restores_ctx` (`SpaceRefs.v:806`) | A body that the reducer stores under a context runs in that context when it fires. | `prop_use_wrapper_restores_ctx` |
| Q47 | `handle_dispatch_total` (`SpaceRefs.v:972`) | The handle process drops a call on a non-handle, aborts on a dangling handle, and replies only on a reply name. | `prop_handle_dispatch_total` |

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
| Q54 | `persistent_op_call_aborts` (`SpaceFactory.v:709`) | A persistent send to an op that takes calls, or to a handle, aborts before the call. | `prop_persistent_op_call_aborts` |
| Q55 | `space_already_exists_is_deterministic_abort` (`SpaceFactory.v:794`) | A creation over an existing config record aborts in play and in replay alike, and a commit applies all three operations or none. | `prop_space_already_exists_is_deterministic_abort`, `prop_prepare_commit_atomic` |

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
| `sf-vector-not-a-call-op` | SpaceFactory.v | The op `vector` takes no calls, so a persistent send to it does not abort. | `persistent_op_call_aborts` |
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
| Counters | `prop_check1_restatement_matches_merger` (`reified_p4_props.rs:2072`) and `prop_queue_writes_race_on_counter` to `prop_counter_free_peek_admits_write_skew` (8) | `reified_p4_props.rs:5441-5865` | Q20 to Q28 |
| Queue | `prop_queue_candidates_head_only` to `prop_legacy_scan_breaks_head_blocks` (9) | `reified_p4_props.rs:3016-3465` | Q29 to Q37, Q62 and Q63 for `prop_queue_plan_permutation_invariant`, and U10 for `prop_queue_loop_bounded` |
| Metering | `prop_agent_reservation_cut_rejects_without_effect` to `prop_agent_reserves_its_own_work_beyond_the_matcher` (3) | `reified_p4_props.rs:4513-4629` | Q59 to Q61, Q64 to Q66 |
| References and routing | `prop_classify_space_ref_total` to `prop_use_wrapper_restores_ctx` (9) | `reified_p4_props.rs:6220-6508` | Q38 to Q46 |
| Factories | `prop_family_index_bijective` to `prop_space_already_exists_is_deterministic_abort` (8) | `reified_p4_props.rs:6747-7116` | Q48 to Q55 |
| TLA+ invariants | `prop_queue_no_pending_match_any_schedule` to `prop_handle_calls_merge_like_contracts` (8) | `reified_p4_props.rs:7382-7958` | U1 to U8, and U12 |
| Leaf extras | `prop_handle_dispatch_total`, `prop_get_space_agent_returns_family_capability`, `prop_qualifier_parse_roundtrip`, `prop_prepare_commit_atomic` (4) | `reified_p4_props.rs:8367-8640` | Q47 to Q51, Q55 |
| Re-entry (decision 43) | `prop_queue_reissue_keeps_first_round_ord` to `prop_queue_persistent_reissue_charged_once` (8) | `reified_p4_props.rs:3773-4349` | Q68 to Q77, U1 to U3, and U9 to U12 |
| Peek re-store (decision 45) | `prop_queue_restore_realizes_plan` (1) | `reified_p4_props.rs:4701` | Q78 |
| Legacy charges (decision 45, Option P) | `prop_legacy_charges_equal_charging_rspace`, `prop_legacy_chain_log_is_host_order`, `prop_legacy_replay_stops_with_play` (3) | `reified_legacy_charges.rs:254-908` | Q79 and Q80, U13 for the last two |
| Native execution | `reified_native_play_rejects_a_space_deploy_before_any_space_work`, `reified_native_replay_reproduces_the_rejection_of_play`, `reified_native_host_program_plays_and_replays_unchanged` (3) | `reified_native_fail_closed.rs:287-320` | Q67 |
| I-20 | `marker_forging_grants_no_capability` (`reified_marker_recognition.rs:660`), `reified_encoding_roundtrip` (`reified_encoding.rs:158`), `reified_decode_commutes_with_substitution` (`reified_marker_recognition.rs:1188`) (3) | Two test files of P2 | Q56 to Q58 |

*Table 41. The 27 witness tests of phase P4.*

| Test | Location | Covers |
|---|---|---|
| `reified_marker_flags_change_derived_flags_only` | `reified/reducer_hooks.rs:1527` | Q56 |
| `reified_marker_flags_use_body_with_connective_send_is_canonical` | `reified/reducer_hooks.rs:1306` | Q57 |
| `reified_marker_flags_variable_bound_to_connective_is_canonical` | `reified/reducer_hooks.rs:1354` | Q58 |
| `space_already_exists_aborts` | `reified/factory.rs:192` | Q55 |
| `persistent_sends_abort_on_every_call_op_and_every_handle` | `reified/system_channels.rs:207` | Q54 |
| `forged_channels_route_to_the_context_space` | `reified/route.rs:283` | Q42 |
| `agent_tagged_names_route_to_the_agent` | `reified/route.rs:312` | Q42 |
| `space_names_route_intrinsically` | `reified/route.rs:269` | Q43 |
| `ordinary_unforgeable_names_route_to_the_default_space` | `reified/route.rs:348` | Q45 |
| `handle_under_a_bundle_without_write_flag_is_not_writable` | `reified/scope.rs:198` | Q39 |
| `other_reified_ids_are_not_space_refs_and_other_values_are_agents` | `reified/scope.rs:212` | Q40 |
| `wrap_body_restores_the_context_as_a_use_tuple` | `reified/scope.rs:257` | Q46 |
| `scan_meter_copy_equals_clone_backing_copy_and_cleanup` | `reified/costs.rs:388` | Q59 |
| `space_matching_reserves_only_what_the_matcher_reads` | `spaces/matching.rs:227` | Q59 |
| `space_counter_bump_cut_budget_rejects_without_effect` | `spaces/counter.rs:740` | Q61 |
| `space_queue_peek_restores_the_head_record` | `spaces/generic.rs:1843` | Q78 |
| `space_queue_peek_of_a_persistent_record_restores_nothing` | `spaces/generic.rs:1916` | Q78 |
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
| `rholang/tests/reified_native_fail_closed.rs` | 3 | Native play and native replay of a space deploy and of a host program (section [11.11](#1111-native-execution-of-decision-46)) |
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
| 19 | Namespaced channels never mergeable | `reified_namespaced_channels_never_mergeable` (`reified_queue.rs:1886`), step `reified-queue` | A tuple channel with a mergeable tag routes to the space inside a `use` block, and only the control channel outside is mergeable. No channel with the tag `0xE0` is mergeable. |

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
| `prop_agent_reservation_cut_rejects_without_effect` | `reified_p4_props.rs:4513` | The exact usage of an operation commits. One unit less in any charged dimension fails with `HostWorkRejected`, logs no event, and leaves the collection unchanged (Q59 to Q61, Q66). |
| `prop_agent_charge_independent_of_unread_payloads` | `reified_p4_props.rs:4564` | Padding of up to 4 KiB on the entries that an operation does not meet leaves its usage unchanged (Q64). |
| `prop_agent_reserves_its_own_work_beyond_the_matcher` | `reified_p4_props.rs:4629` | A forwarding meter records the reservations of the matcher, and the agent reserves more in each charged dimension (Q65). |
| `prop_queue_plan_permutation_invariant` | `reified_p4_props.rs:3414` | Two list orders of one collection give equal usage in every dimension (Q62, Q63). |
| `prop_queue_candidates_head_only` | `reified_p4_props.rs:3016` | A counting matcher bounds the matcher calls, because no charge counts them now, and the usage of `SearchCandidates` is zero. |
| `scan_meter_maps_amounts_to_the_branch_dimensions` | `reified/costs.rs:335` | A reservation charges VerificationOperations, VerificationBytes, and SearchStateBytes, and no other dimension. |
| `scan_meter_accepts_exact_credit_and_rejects_each_smaller_dimension` | `reified/costs.rs:359` | The exact credit suffices, and one unit less in any dimension rejects. |
| `scan_meter_copy_equals_clone_backing_copy_and_cleanup` | `reified/costs.rs:388` | A reserved copy charges the triple of the copy walk of the branch (Q59). |
| `scan_meter_skips_walks_without_a_budget` | `reified/costs.rs:422` | Without a budget, every reservation passes, and a metered encode gives the bytes of bincode. |
| `metered_matching_reserves_on_the_branch_dimensions` | `reified/costs.rs:444` | The metered matcher charges the three dimensions and no `SearchCandidates`. |
| `space_matching_reserves_only_what_the_matcher_reads` | `spaces/matching.rs:227` | `bind` adds nothing to the reservations of the matcher, and an unguarded `admits` charges one operation, whatever the size of the body (Q59). |
| `legacy_mapping_dropped_operations_and_backing` | `spaces/matching.rs:278` | Negative control: the old mapping of the meter kept only the scanned bytes of the matcher. |
| `space_counter_bump_reserves_its_work_on_the_branch_dimensions` | `spaces/counter.rs:716` | A bump charges the three dimensions and no `SearchCandidates`, and a zero budget rejects it. |
| `space_counter_bump_cut_budget_rejects_without_effect` | `spaces/counter.rs:740` | A cut budget rejects a bump before its commit, with no event and no change of the counters (Q61). |

Decision 42 changes no state, no event, and no outcome of an operation.
So the models of sections [11.3](#113-rocq-models-of-p4) and [11.4](#114-tla-models-of-p4) do not change.
The suites of Table 42 pass with their earlier assertions.

### 11.11 Native execution of decision 46

[Decision 46](decision-register.md#decision-46) makes spaces fail closed in native execution.
Native play and native replay reject the first space operation of a deploy with `SpacesUnsupportedInNativeExecution`, before any space work.
The headline Q67, `native_execution_fails_closed` of `SpaceRefs.v`, states the rule.
A space entry in native play equals the same entry in native replay, for every staged list and every replay state.
Both stage no operation and give one error.
Outside native execution, a host runtime runs the job unchanged.

*Table 47. The Rocq mutant of decision 46.*

| Id | What the mutant changes | Must fail in |
|---|---|---|
| `sr-native-play-runs-spaces` | A host runtime runs space jobs in native play, as before the decision. | `native_execution_fails_closed` |

*Table 48. The tests of decision 46.*

| Test | Location | What it checks |
|---|---|---|
| `reified_native_play_rejects_a_space_deploy_before_any_space_work` | `reified_native_fail_closed.rs:287` | A native play of a factory call fails with the code, and no channel of a space enters the hot store (Q67). |
| `reified_native_replay_reproduces_the_rejection_of_play` | `reified_native_fail_closed.rs:297` | The native replay environment replays the play journal: the same error, a complete journal, the phlo usage of play, and equal authority events and byte observations (Q67). |
| `reified_native_host_program_plays_and_replays_unchanged` | `reified_native_fail_closed.rs:320` | A program without spaces plays and replays natively without an error, with a complete journal and equal usage (Q67). |
| `host_runtime_is_available_outside_native_execution` | `reified/runtime.rs:441` | A host runtime runs space jobs when its budget runs no native deploy. |

**The hazard without the rule.** A run of the suite with the guard of `ReifiedRuntime::host` removed (`target/verification/reified-rspaces/d46/mutant-no-native-guard.log`) shows it.
Native play then ran the space operation without an error, and native replay failed with `native replay has incomplete operations`.
That is the divergence that the proposer's self-certification turned into a quarantine before the decision.

### 11.12 Re-entry of decision 43

[Decision 43](decision-register.md#decision-43) makes a re-issued persistent operation keep the order of its first round.
The element re-enters at its place.
It is at the head of its side when that side is empty or its order precedes the head.
Otherwise it rests behind the head and fires nothing.
`operation_ord` (`spaces/generic.rs:865-877`) checks the kept order against the counter.
`GenericSpace::at_head` (`spaces/generic.rs:672-697`) places the element, and `QueueDiscipline::precedes` (`spaces/disciplines/queue.rs:63-72`) compares two orders.

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
| `space_queue_divergent_pairs_reissue_after_one_round` | `spaces/generic.rs:2002` | A divergent pair re-issues with the order of its first round in every round, on both sides, and each round bumps the counter. |
| `space_queue_round_cap_reissues_the_rest` | `spaces/generic.rs:2059` | A capped send and a capped receive re-issue with the order of their first round, and the rest rests at that order. |
| `space_queue_reissue_keeps_the_first_round_ord_and_identity` | `spaces/generic.rs:2174` | Every round of a capped persistent send stages the same record, so every produce event on the data channel is equal (Q69). |
| `space_queue_reissue_reenters_ahead_of_younger_records` | `spaces/generic.rs:2215` | A younger datum stored between two rounds stays behind the re-entry, which fires the head waiter. |
| `space_queue_reissue_stores_behind_an_older_reentry` | `spaces/generic.rs:2246` | A re-entry that finds an older entry at the head of its side rests at its place and fires nothing (Q73). |
| `space_queue_shadowed_pair_resolves_at_reentry` | `spaces/generic.rs:2286` | Between two rounds the heads match while the send in flight precedes the head record, and the next round resolves the pair (Q70). |
| `space_queue_nonpersistent_reissue_takes_a_fresh_ord` | `spaces/generic.rs:2331` | A datum that is not persistent re-issues with no order and joins the tail at its new order (Q77). |
| `space_queue_reissued_receive_keeps_its_first_round_ord` | `spaces/generic.rs:2361` | A capped persistent receive re-enters ahead of a younger waiter and parks at its first order. |
| `space_queue_reentry_rejects_an_inconsistent_order` | `spaces/generic.rs:2395` | A kept order outside the orders of the counter, or equal to the order of a stored entry, fails with `Inconsistent` and changes nothing. |
| `space_queue_replay_reproduces_the_reentries` | `spaces/generic.rs:2484` | Replay stages the transactions of a divergent pair and of a capped re-entry, pairs the three equal COMMs of the pair, and reaches the root of play. |
| `space_queue_reissue_persistent_send` | `reified_queue.rs:1043` | In a deploy, a capped persistent send rests at its first order, and its produce events on the data channel are equal. |
| `space_queue_reissue_reenters_ahead_of_younger_records` | `reified_queue.rs:1114` | In a deploy, a body of the first round sends a younger datum, and the second round re-enters ahead of it and fires the head waiter. |
| `space_queue_reissue_keeps_one_produce_identity` | `reified_queue.rs:1163` | A capped persistent send has one introduction row for its one identity, and every observed event has its row. |
| `space_queue_divergent_pair_exhausts_phlo` | `reified_queue.rs:1270` | A divergent pair repeats one produce identity and one COMM, and the counter bumps of its rounds spend the phlo. |
| `prop_queue_loop_bounded` | `reified_p4_props.rs:3305` | A re-issue carries the first order exactly when the operation is persistent (Q74). |
| `prop_queue_reissue_keeps_first_round_ord` | `reified_p4_props.rs:3773` | In runs with re-entries between other operations, every re-issue keeps the first order of a persistent element, every step bumps the counter once, and $`\mathrm{wf}'`$ holds (Q68, Q69). |
| `prop_queue_shadow_invariant_with_inflight` | `reified_p4_props.rs:3820` | After every step of a run with re-entries SHADOW holds, and a step with no persistent element in flight leaves no pending match (Q70, Q71). |
| `prop_queue_fifo_with_reentries` | `reified_p4_props.rs:3862` | In runs with re-entries, the records that are not persistent leave in increasing order, each older than every such record that stays (Q72). |
| `prop_queue_nonpersistent_reissue_fresh_ord` | `reified_p4_props.rs:3922` | A send that fires 64 or more peek waiters re-issues unchanged, and its re-entry stores at most its record at the order of its new bump (Q77). |
| `prop_queue_reentry_head_blocks` | `reified_p4_props.rs:4085` | A re-entry behind an older head of its side fires nothing and rests at its kept order, and a fire meets the head of the other side (Q73, Q74). |
| `prop_queue_reentry_permutation_invariant` | `reified_p4_props.rs:4166` | Two list orders of one collection give the same re-entry: outcomes, events, entries, and host work (Q75). |
| `prop_fresh_ord_reissue_loses_first_round_place` | `reified_p4_props.rs:4263` | The kept order re-enters ahead of a younger datum and fires the persistent waiter again, and a fresh order rests behind it and fires nothing (Q76). |
| `prop_queue_persistent_reissue_charged_once` | `reified_p4_props.rs:4349` | On a runtime, a persistent send or receive that re-issues has one data-channel identity and one introduction row, and it rests at its first order (Q69). |

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
`check_peeks` (`rspace++/src/rspace/space_store_handle.rs:866-881`) only admits the peek set, and `removed_records` (`rspace++/src/rspace/space_store_handle.rs:939-972`) returns every stored non-persistent candidate of a fire.
`fired_receipt` (`rspace++/src/rspace/space_store_handle.rs:2499-2535`) keeps the persist flags of the host's `wrap_result`.
The play and replay backends always call `remove_matched_datum_and_join` or `store_persistent_data`.

The agent then makes a [re-store](glossary.md#re-store).
`GenericSpace::receive` copies a peeked head record that is not persistent (`rholang/src/rust/interpreter/spaces/generic.rs:812-817`).
Right after the fire, `Plan::restore` (`rholang/src/rust/interpreter/spaces/generic.rs:353-375`) stages a produce of the same record bytes with the outcome Store and `persist = false`.
`Introduction::for_record` (`rholang/src/rust/interpreter/spaces/record.rs:847-860`) gives the re-store the cost authority of the record, as `produce_peeks` does.
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
| H2 | `commit_state_eq_host` (`EventCorrelation.v:5530`) | The committed store equals the host store, with no premise on peeks. |
| H11 | `peek_reproduces_trace_eq_host` (`EventCorrelation.v:5807`) | A transaction with the re-stores right after a peek fire commits with the calls, events, status, bindings, and store of the host program, whose reducer sends the peeked data again. A fire never stores a datum, and the re-stores are exactly the sends of `produce_peeks`. |
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
| `space_store_handle_receipt_keeps_the_host_flags` | `space_store_handle.rs:3440` | The receipt keeps the flag of the host, and results that do not line up fail closed. |
| `space_queue_peek_restores_the_head_record` | `spaces/generic.rs:1843` | A queue peek logs the consume, the COMM, and the re-store, and the re-store has the identity of the first produce. |
| `space_queue_peek_restore_registers_the_record_authority` | `spaces/generic.rs:1876` | The re-store registers the authority of the record, and the peek registers its own. |
| `space_queue_peek_of_a_persistent_record_restores_nothing` | `spaces/generic.rs:1916` | A peek of a persistent record removes nothing and stores nothing again. |
| `space_queue_observer_parity` | `reified_queue.rs:1336` | In deploys, the space events of each queue step equal the host events of the same program, the re-store after a peek included. |
| `prop_queue_restore_realizes_plan` | `reified_p4_props.rs:4701` | Over runs that peek often, a peek keeps the records, its receipt keeps the persist flag, and the re-store follows the COMM with the identity of the record (Q78). |

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
| `prop_legacy_charges_equal_charging_rspace` | `reified_legacy_charges.rs:254` | Generated host programs run on the real `ChargingRSpace`. For each call, the charges of the equivalent staged operation equal the new log entries, value and label. All 8 shapes of call occur (Q79). |
| `prop_legacy_chain_log_is_host_order` | `reified_legacy_charges.rs:800` | Generated transactions with generated budgets make the legacy log and the stop of the host program. All 6 kinds of transaction occur, the stops in the commit and in the tail included (Q79). |
| `prop_legacy_replay_stops_with_play` | `reified_legacy_charges.rs:908` | Replay with the log of play makes the legacy log, the stop, and the store of play (Q80). |
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

## References

<a id="ref-ch00"></a>[CH00] K. Claessen and J. Hughes, "QuickCheck: a lightweight tool for random testing of Haskell programs," in *Proceedings of the Fifth ACM SIGPLAN International Conference on Functional Programming (ICFP 2000)*, ACM, 2000, pp. 268–279. [doi:10.1145/351240.351266](https://doi.org/10.1145/351240.351266)

<a id="ref-ch88"></a>[CH88] T. Coquand and G. Huet, "The calculus of constructions," *Information and Computation*, vol. 76, no. 2–3, pp. 95–120, 1988. [doi:10.1016/0890-5401(88)90005-3](https://doi.org/10.1016/0890-5401(88)90005-3)

<a id="ref-dls78"></a>[DLS78] R. A. DeMillo, R. J. Lipton, and F. G. Sayward, "Hints on test data selection: help for the practicing programmer," *Computer*, vol. 11, no. 4, pp. 34–41, 1978. [doi:10.1109/C-M.1978.218136](https://doi.org/10.1109/C-M.1978.218136)

<a id="ref-kkt19"></a>[KKT19] I. Konnov, J. Kukovec, and T.-H. Tran, "TLA+ model checking made symbolic," *Proceedings of the ACM on Programming Languages*, vol. 3, no. OOPSLA, pp. 1–30, 2019. [doi:10.1145/3360549](https://doi.org/10.1145/3360549)

<a id="ref-lam94"></a>[Lam94] L. Lamport, "The temporal logic of actions," *ACM Transactions on Programming Languages and Systems*, vol. 16, no. 3, pp. 872–923, 1994. [doi:10.1145/177492.177726](https://doi.org/10.1145/177492.177726)

<a id="ref-pt"></a>[PT] The proptest authors, "Proptest," version 1.11.0, a property-testing framework for Rust. [proptest-rs.github.io/proptest](https://proptest-rs.github.io/proptest/)

<a id="ref-roc25"></a>[Roc25] The Rocq Development Team, "The Rocq Prover," Zenodo, 2025. The DOI names every archived release, and the newest archived release is 9.1.0. [doi:10.5281/zenodo.15149628](https://doi.org/10.5281/zenodo.15149628)

<a id="ref-yml99"></a>[YML99] Y. Yu, P. Manolios, and L. Lamport, "Model checking TLA+ specifications," in *Correct Hardware Design and Verification Methods (CHARME 1999)*, Lecture Notes in Computer Science, vol. 1703, Springer, 1999, pp. 54–66. [doi:10.1007/3-540-48153-2_6](https://doi.org/10.1007/3-540-48153-2_6)
