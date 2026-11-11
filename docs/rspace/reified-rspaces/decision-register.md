# Reified RSpaces decision register

This register is the normative record of the decisions that govern the Reified RSpaces campaign.
It restates the decisions of the realignment plan, which the user approved on 2026-10-04 [[PLAN](#ref-plan)].
It adds no decision of its own.
The [glossary](glossary.md) defines the terms, and the [README](README.md) lists the campaign documents and phases.

The [design record](design-record/README.md) keeps the design ledger of 2026-08-15 as history.
Where this register and the ledger differ, this register applies.

## How to use the register

- Cite an entry by its anchor, for example `decision-register.md#decision-18`, `#m-1`, or `#i-19`.
- Read the table of an entry for its date, its status, and its supersession links.
- Read the statement for the decision itself, and the reason for its rationale.

The plan quotes the words of the user for some decisions, and the entries keep these words in double quotes.
Decisions 19 to 65 override the earlier plan sections where they conflict.
The minimal-footprint revision (M-1 to M-5) overrides the target architecture of the plan where they conflict.

## Terms used in this register

- **Plan:** the realignment and completion plan [[PLAN](#ref-plan)].
- **Ledger:** the design ledger of 2026-08-15 in the [design record](design-record/2026-08-15-design-ledger.md). It is history, not a specification.
- **Baseline:** the node code without the campaign changes. The plan calls it HEAD. It was the commit `e748426` until the merge of [decision 44](#decision-44), and the commit `4d67b5ccc` from that merge to merge 1. Merge 1 moved it to `4c58a925f` ([decision 89](#decision-89)), and merge 2 moves it to `702496775` ([decision 90](#decision-90)).
- **Codex:** the earlier agent session that ran the campaign from 2026-10-01 to 2026-10-03 and diverged from the design.
- **Phase:** one of the plan steps P0 to P12. The [README](README.md#campaign-phases) lists them.
- **pgmcp:** the service that indexes the workspace and holds the tracker of the campaign.
- **Epic and leaf:** items of the pgmcp tracker. The epic is `reified-rspaces-mettatron-migration` (pgmcp id 5582).
- **Relay:** a user approval that the tracker records word for word: VPI-1 (verification policy), AAI-1 (acceptance amendment), TRI-1 (tracker dispositions), and SIG-F (F1r3lang readiness signal).
- **Arbitration:** an independent design review that the plan cites by item, for example H10.
- **Design pass labels:** in decision 19, D1 to D7 are items of the P1 design pass. C1 to C10 are items of the P2 pass, and UD-1 to UD-9 are items of the P3 pass.
- **P4 items:** A1 to A25 are the items of the P4 design pass and its arbitration. Section F of the P4 design report lists them. The report is outside the repository.
- **P5 labels:** P5a to P5e are the sub-phases of P5. F1 to F21 are the design choices that the P5 design report asks the user to approve. S1 to S7 and E1 to E5 are the corrections of the P5 readiness review. Both reports are outside the repository.
- **`K_retire`:** a single-channel continuation that the P5 agents synthesize to retire one stored record with a consume.
- **Fix rules:** FR-1 to FR-10 are the rules of the P1 fix passes, and FR-1', FR-5', FR-7', and FR-9' are their round-2 forms. The [substrate section of the design](design.md#substrate) lists them.
- **Ledger labels:** DELTA A is the ledger rule that spaces persist in the one RSpace, which owns checkpoint, replay, and reset. D1 to D13 name defects of the legacy branch.
- **Audit labels:** K1 to K17 name the change clusters of codex. For example, K5 is the removal of the COMM produce sort.
- **Contract item:** `rrs-reducer (6)` means item 6 of the original task contract `rrs-reducer`.
- **Crates:** `casper` is the consensus crate, `rspace++` holds RSpace, `models` holds the Par types, and `rholang` holds the interpreter.
- **EPathMap and EPM1:** EPathMap is the PathMap value of Rholang, written `{|…|}`. EPM1 is its trie-based representation on the feature/mettail branch.
- **FLT:** a Foreign Language Term, which is a scoped term of an installed guest language in F1r3lang.
- **GSLT:** a graph-structured lambda theory: the form in which F1r3lang defines a language, with its sorts, term constructors, equations and rewrites.
- **URI binder:** a `new` binder with a URN, such as ``new x(`rho:io:stdout`)``.
- **Base:** the branch `feature/cost-accounted-rho`, which this branch merges. Merge 1 took it at `4c58a925f` ([decision 89](#decision-89)), and merge 2 takes it at `702496775` ([decision 90](#decision-90)).
- **DR file:** the decision records of the base, `docs/casper/theory/cost-accounting-decision-records.md`. A cite such as "DR file, lines 12959-12961" names lines of this file, which merge 2 takes unchanged from `702496775`.
- **Labels of the base:** DR-75 to DR-135 are decision records in the DR file. A label such as DR-114 T7, DR-129 E1 or DR-135 R4 names an item of a record. The rule P2 of DR-114 and the test F7 of DR-129 are such items. P-1 and P-6 are properties of DR-132.
- **C4 labels:** C4 is the work item of merge 2 for owner refusals inside a space transaction. Its design report asks the questions Q1 and Q2. Its arbitration gives the findings F1 to F13 and the classification B1, by which the machinery continuations are system work.
- **Labels of decisions 117 to 121:** one design covers decisions 102 and 103 and the item C4. Its open points are Q-102-1 to Q-102-3, Q-103-1 to Q-103-4 and Q-C4-1. G1 to G10 name the defects and the gaps of its arbitration, and S1 to S6 name the reductions. R-A and R-B are the options of question Q-102-3. O-1 to O-5 are the open points of decision 112, and B-1 is an item of its plan. R0 and R1 are the captures of decision 120.
- **Machinery continuation:** a continuation that the Reified agent synthesizes to keep the protocol state of a space. `K(v)` takes the counter record (`spaces/counter.rs`), and `K_anchor` consumes the config record (`spaces/anchor.rs`, [decision 92](#decision-92)). `K_lift` lifts a stored record for a settle (`spaces/settle.rs`), and `K_retire` retires one stored record (`spaces/disciplines/scan.rs`).
- **Positional failure:** an error of a staging that depends on the state of the space at the place of its transaction. A later state can remove it. The errors are `CellOccupied`, `CapacityExceeded` and `NestedCommitGuard` (`spaces/errors.rs:350-354`). Such a transaction commits only its anchor ([decision 92](#decision-92)).
- **Oracle mode:** the mode of the File I/O FIPS in which each node owns its external store. Play calls the service once for each call and records the reply, and replay delivers the record. Nothing undoes an external write (DR-114).
- **Acronyms:** API is application programming interface, ABI is application binary interface, AST is abstract syntax tree, and CLI is command-line interface. TLA+ is the specification language of the TLA model checks.

## Index

| Entry | Title | Date | Status |
|---|---|---|---|
| [Decision 1](#decision-1) | Scope rule | 2026-10-03 | In force |
| [Decision 2](#decision-2) | MeTTaTron space held | 2026-10-03 | In force |
| [Decision 3](#decision-3) | F1r3lang ownership | 2026-10-03 | In force |
| [Decision 4](#decision-4) | Parser isolation | 2026-10-03 | In force |
| [Decision 5](#decision-5) | Realignment method | 2026-10-03 | In force |
| [Decision 6](#decision-6) | Milestone commits | 2026-10-03 | Fixture part superseded by decision 18. [Decision 112](#decision-112) needs the fixture commit again |
| [Decision 7](#decision-7) | Namespace tag 0xE0 | 2026-10-03 | In force |
| [Decision 8](#decision-8) | Agent-shaped API | 2026-10-03 | In force |
| [Decision 9](#decision-9) | Approved design proposals | 2026-10-03 | In force. Item 1 (the theory seam) implemented in P7 step 4, with the slot of decision 74 |
| [Decision 10](#decision-10) | Parser branch name | 2026-10-03 | In force |
| [Decision 11](#decision-11) | Head-only matching | 2026-10-04 | In force |
| [Decision 12](#decision-12) | Lane rule | 2026-10-04 | In force |
| [Decision 13](#decision-13) | Verification command defaults | 2026-10-04 | In force. Amended by [decision 137](#decision-137) |
| [Decision 14](#decision-14) | Rocq line-reference audit | 2026-10-04 | In force |
| [Decision 15](#decision-15) | Empty-state hash fixture | 2026-10-04 | Fixture commit superseded by decision 18. [Decision 112](#decision-112) changes the hash once |
| [Decision 16](#decision-16) | Tracker dispositions | 2026-10-04 | In force |
| [Decision 17](#decision-17) | Runtime space registration postponed | 2026-10-04 | In force |
| [Decision 18](#decision-18) | Marker-tagged tuples | 2026-10-04 | Superseded by [decision 112](#decision-112) on 2026-10-27. Merge 2 implemented decision 112. [Decision 81](#decision-81) approved one `casper/src` exception, and [decision 84](#decision-84) reverted it |
| [Decision 19](#decision-19) | P1 substrate | 2026-10-04 | In force, amended by decisions 30 and 36 |
| [Decision 20](#decision-20) | Tracker leftovers of TRI-1 | 2026-10-04 | In force |
| [Decision 21](#decision-21) | Corrected encoding table | 2026-10-04 | In force. [Decision 112](#decision-112) replaces its marker tuples with protobuf fields |
| [Decision 22](#decision-22) | P2 hooks | 2026-10-04 | In force. [Decision 112](#decision-112) amends its hooks |
| [Decision 23](#decision-23) | P3 parser design | 2026-10-04 | In force |
| [Decision 24](#decision-24) | Keywords by lookahead | 2026-10-04 | In force |
| [Decision 25](#decision-25) | Parser-branch commits (UD-9) | 2026-10-04 | In force |
| [Decision 26](#decision-26) | Completion goal | 2026-10-04 | In force |
| [Decision 27](#decision-27) | Activation and rollout out of scope | 2026-10-04 | In force |
| [Decision 28](#decision-28) | Repeated channels in a group | 2026-10-04 | In force. [Decision 103](#decision-103) replaces two clauses, and [decision 118](#decision-118) amends decision 103 |
| [Decision 29](#decision-29) | Joins into ordered collections in P5 | 2026-10-04 | In force. Implemented in P5 wave W3 |
| [Decision 30](#decision-30) | Peek fires re-offer the sent value | 2026-10-04 | In force |
| [Decision 31](#decision-31) | Every operation on an ordered collection bumps its counter | 2026-10-04 | In force |
| [Decision 32](#decision-32) | P4 plan-text changes | 2026-10-04 | In force. Its open P8 item is settled by decision 77. [Decision 106](#decision-106) replaces item A21 for `embed` and `ann` |
| [Decision 33](#decision-33) | assertEqual has arity 3 | 2026-10-04 | In force |
| [Decision 34](#decision-34) | P4 detail items | 2026-10-04 | In force. Item A15 amended by [decision 137](#decision-137) |
| [Decision 35](#decision-35) | P1 close-out scope: lean, verify at P4 | 2026-10-04 | In force. Its shared freeze moved to P11 by decision 75, and [decision 137](#decision-137) replaces the freeze with the host verification |
| [Decision 36](#decision-36) | Space replay reproduces play when a deploy runs out of phlo | 2026-10-05 | In force |
| [Decision 37](#decision-37) | Receive selection follows the F1r3lang rule from the F1r3lang merge on | 2026-10-05 | In force. Implemented in P12 |
| [Decision 38](#decision-38) | Priority-queue waiters wait in one queue per level | 2026-10-05 | In force |
| [Decision 39](#decision-39) | P5 design choices F1 to F17 and F20, without F21 | 2026-10-05 | In force, vetoable value of F9 |
| [Decision 40](#decision-40) | Space charging follows the cost-accounted-rho policy | 2026-10-05 | Retired by [decision 89](#decision-89). It was applied in P5 |
| [Decision 41](#decision-41) | Bag and set spaces order candidates by source hash | 2026-10-05 | Superseded by [decision 102](#decision-102) on 2026-10-27. It was applied in P5a |
| [Decision 42](#decision-42) | The queue agent's metering is aligned now | 2026-10-05 | Retired by [decision 89](#decision-89). It was implemented |
| [Decision 43](#decision-43) | A re-issued persistent operation keeps its first-round identity | 2026-10-05 | In force. Implemented |
| [Decision 44](#decision-44) | Merge of feature/cost-accounted-rho at 4d67b5ccc | 2026-10-05 | In force |
| [Decision 45](#decision-45) | Space operations pay the legacy storage charges of the host | 2026-10-06 | In force for Rule E, which is implemented. [Decision 90](#decision-90) removes Option P, as [decision 69](#decision-69) planned |
| [Decision 46](#decision-46) | Spaces fail closed in native execution | 2026-10-06 | Superseded by decision 47 on 2026-10-07 |
| [Decision 47](#decision-47) | Full native support for spaces is its own phase | 2026-10-06 | In force. Implemented on 2026-10-07. [Decision 89](#decision-89) retires its metered view reads |
| [Decision 48](#decision-48) | The work order after decision 43 | 2026-10-06 | In force |
| [Decision 49](#decision-49) | One fire count for each space transaction | 2026-10-06 | In force. Implemented in P5 wave W2 |
| [Decision 50](#decision-50) | Joined set collections cascade | 2026-10-07 | In force. Implemented in P5 wave W3 |
| [Decision 51](#decision-51) | A PathMap bag or set peek bumps the token when its second send changes state | 2026-10-07 | In force. Implemented in P5 wave W3 (Phase 2 of the PathMap engine) |
| [Decision 52](#decision-52) | Tensor logic and the ChromaDB backend join the VectorDB phase P6 | 2026-10-07 | In force. Tensor logic moved to P12 by decision 65 |
| [Decision 53](#decision-53) | The tensor-logic fragment TL_V | 2026-10-07 | Moved to P12 by decision 65 |
| [Decision 54](#decision-54) | The number model of tensor logic | 2026-10-07 | In force for P6 decisions. TL-only functions moved to P12 by decision 65 |
| [Decision 55](#decision-55) | The tensor-logic syntax | 2026-10-07 | Superseded by decision 62 on 2026-10-07 |
| [Decision 56](#decision-56) | The VectorDB vocabulary | 2026-10-07 | In force, amended by decisions 62 and 66 |
| [Decision 57](#decision-57) | The recorded processes `embed` and `ann` | 2026-10-07 | In force. Implemented in P6. Native clause superseded by decision 68. Decisions [105](#decision-105), [113](#decision-113) and [114](#decision-114) amend its replies and its builds |
| [Decision 58](#decision-58) | The ChromaDB mirror is a hint index | 2026-10-07 | In force. Implementation in P6. Decisions [98](#decision-98), [105](#decision-105) and [114](#decision-114) replace its publication, its failure code and its reconciler |
| [Decision 59](#decision-59) | The test policy for live services | 2026-10-07 | In force. Implementation in P6 |
| [Decision 60](#decision-60) | The VectorDB tie-break follows decision 41 | 2026-10-07 | Superseded by [decision 102](#decision-102) on 2026-10-27 |
| [Decision 61](#decision-61) | Footprint exceptions for the ChromaDB mirror | 2026-10-07 | In force. Implementation in P6. [Decision 114](#decision-114) retires its reconciler spawn, and [decision 109](#decision-109) amends the environment of the nodes |
| [Decision 62](#decision-62) | Tensor-logic equations are FLT terms of a TensorLogic GSLT | 2026-10-07 | In force. Implementation at P12, after SIG-F |
| [Decision 63](#decision-63) | The TensorLogic GSLT has an index-notation core and familiar sugar | 2026-10-07 | In force. Implementation at P12 |
| [Decision 64](#decision-64) | The TensorLogic surface is paper-first, functional and Rholang-flavored | 2026-10-07 | In force. Implementation at P12 |
| [Decision 65](#decision-65) | The TensorLogic language waits for F1r3lang and does not block VectorDB | 2026-10-07 | In force |
| [Decision 66](#decision-66) | The answers of the P6 design refresh | 2026-10-08 | In force. Native answer superseded by decision 68. [Decision 98](#decision-98) replaces the answer M1, and [decision 114](#decision-114) retires the reconciler answer |
| [Decision 67](#decision-67) | The checks of an `ann` call run in play and in replay | 2026-10-08 | Superseded by [decision 104](#decision-104) on 2026-10-27 |
| [Decision 68](#decision-68) | `embed` and `ann` leave the native ban | 2026-10-08 | In force. [Decision 89](#decision-89) does its last clause |
| [Decision 69](#decision-69) | Only the latest execution model | 2026-10-08 | In force |
| [Decision 70](#decision-70) | Reified RSpaces only | 2026-10-08 | In force. [Decision 114](#decision-114) retires the reconciler of its example |
| [Decision 71](#decision-71) | `capacity` is a soft bound under merges | 2026-10-08 | In force |
| [Decision 72](#decision-72) | The P7 design process | 2026-10-09 | In force |
| [Decision 73](#decision-73) | P7 design v2, Seq claims per channel, HashSet on units | 2026-10-09 | In force. Implemented in P7 steps 2 (the Seq claims) and 3 (the HashSet store). [Decision 91](#decision-91) amends the basis of its Seq claims |
| [Decision 74](#decision-74) | The config persists the theory identity as compact bytes | 2026-10-09 | In force. Implemented in P7 step 4 |
| [Decision 75](#decision-75) | One pgmcp freeze at P11 | 2026-10-09 | Superseded by [decision 137](#decision-137) on 2026-11-10 |
| [Decision 76](#decision-76) | The P8 design process | 2026-10-09 | In force |
| [Decision 77](#decision-77) | P8 design v2, the URNs of the internal ops, the space-view clause | 2026-10-10 | In force |
| [Decision 78](#decision-78) | One native replay test runs at the v6 host-work limits | 2026-10-10 | Retired by [decision 89](#decision-89). It was implemented in P8 step 1 |
| [Decision 79](#decision-79) | The conformance kit keeps the v6 host-work limits | 2026-10-10 | Retired by [decision 89](#decision-89). It was implemented in P8 step 5 |
| [Decision 80](#decision-80) | The P9 design process | 2026-10-11 | In force |
| [Decision 81](#decision-81) | Backport the DR-119 fix of bug 11004 | 2026-10-11 | Reverted by [decision 84](#decision-84). It was implemented in P9 step 0 |
| [Decision 82](#decision-82) | N4 fits three facts of Casper | 2026-10-11 | In force. [Decision 84](#decision-84) disabled its concurrent row, and [decision 108](#decision-108) restores it |
| [Decision 83](#decision-83) | The creation job records a seq space | 2026-10-12 | In force. Its second half is a recorded follow-up |
| [Decision 84](#decision-84) | Revert the DR-119 backport | 2026-10-12 | In force. [Decision 108](#decision-108) restores its concurrent row |
| [Decision 85](#decision-85) | P10 design v2 | 2026-10-12 | In force |
| [Decision 86](#decision-86) | The gate of the node command line waits for its repair | 2026-10-12 | In force. Amended by [decision 89](#decision-89) and [decision 136](#decision-136) |
| [Decision 87](#decision-87) | No new capacity param | 2026-10-12 | In force. Amended by [decision 89](#decision-89) |
| [Decision 88](#decision-88) | The legacy vector operations come with tensor logic | 2026-10-12 | In force |
| [Decision 89](#decision-89) | The base removed host-work metering, so Reified RSpaces removes its own | 2026-10-16 | In force. Decisions [96](#decision-96), [102](#decision-102), [105](#decision-105) and [107](#decision-107) amend it |
| [Decision 90](#decision-90) | The merge 2 plan for `feature/cost-accounted-rho` at `702496775` | 2026-10-25 | In force. [Decision 95](#decision-95) replaces its item on the test attributes of `session.rs`, and [decision 98](#decision-98) changes its mirror item |
| [Decision 91](#decision-91) | The seq rule uses causal histories | 2026-10-25 | In force |
| [Decision 92](#decision-92) | The replay order of space transactions uses ordinary events | 2026-10-26 | In force. [Decision 94](#decision-94) replaces its escalation re-check, and [decision 99](#decision-99) replaces its anchor payer |
| [Decision 93](#decision-93) | Racing tests stay multi-thread | 2026-10-26 | In force. [Decision 111](#decision-111) adds the current-thread runtime |
| [Decision 94](#decision-94) | Escalation to a join scope takes a two-phase lock | 2026-10-26 | In force |
| [Decision 95](#decision-95) | The space readiness check reaches the replay journal through the native epoch trait | 2026-10-26 | In force. [Decision 99](#decision-99) replaces its related correction |
| [Decision 96](#decision-96) | `AgentReadCoverage.v` is deleted, as the base deleted its host-work theories | 2026-10-27 | In force |
| [Decision 97](#decision-97) | The base branch decides the design, also for the earlier decisions | 2026-10-27 | In force |
| [Decision 98](#decision-98) | The mirror follows the oracle mode of the base (DR-114) | 2026-10-27 | In force |
| [Decision 99](#decision-99) | The machinery of a space runs as system work, and the firing deployment pays | 2026-10-27 | In force |
| [Decision 100](#decision-100) | A refused lift or retire parks inert | 2026-10-27 | In force |
| [Decision 101](#decision-101) | A receive with several members runs the item test of each member during the search, as the base does | 2026-10-27 | In force |
| [Decision 102](#decision-102) | Play picks among candidates at random, and replay follows the recorded pick (DR-129) | 2026-10-27 | In force |
| [Decision 103](#decision-103) | A join that names one channel more than once assigns data as the base does (DR-129 E1) | 2026-10-27 | In force. [Decision 118](#decision-118) amends it |
| [Decision 104](#decision-104) | `embed` and `ann` replay their records as the base driver does (DR-114) | 2026-10-27 | In force. [Decision 116](#decision-116) amends its mechanism clause |
| [Decision 105](#decision-105) | The failure replies of `embed` and `ann` use the base codes, and every reply must be plain data | 2026-10-27 | In force |
| [Decision 106](#decision-106) | A persistent send to `embed` or `ann` fails at the fire, as rule P2 of the base does | 2026-10-27 | In force |
| [Decision 107](#decision-107) | The seal of a re-stored record resolves with the region price (DR-135 R4) | 2026-10-27 | In force |
| [Decision 108](#decision-108) | The N4 row `n4_concurrent_space_blocks_agree` returns | 2026-10-27 | In force |
| [Decision 109](#decision-109) | Observers get no VectorDB services, as observers of the base get the no-op Chroma service | 2026-10-27 | In force |
| [Decision 110](#decision-110) | Bare floats keep the forms of the base | 2026-10-27 | In force |
| [Decision 111](#decision-111) | The racing tests also run on a current-thread runtime, as the base does | 2026-10-27 | In force |
| [Decision 112](#decision-112) | The Reified syntax uses protobuf fields, as the base carries its new syntax | 2026-10-27 | In force. Merge 2 implemented it |
| [Decision 113](#decision-113) | `embed` and `ann` exist only in a `chromadb` build, and the VectorDB space type exists in every build | 2026-10-27 | In force |
| [Decision 114](#decision-114) | The mirror is an oracle store with no repair, as the oracle mode of File I/O | 2026-10-27 | In force |
| [Decision 115](#decision-115) | The reply channel of `embed` and `ann` stays first, as the File I/O sugar requires | 2026-10-27 | In force |
| [Decision 116](#decision-116) | `embed` and `ann` keep their own handlers and take the DR-114 contract from one helper of the base | 2026-10-27 | In force |
| [Decision 117](#decision-117) | A positional failure after a random pick stages the operation again in DR-75 order (R-A) | 2026-10-28 | In force |
| [Decision 118](#decision-118) | The throughput rule of decision 117 also decides the join rules of decision 103 | 2026-10-28 | In force |
| [Decision 119](#decision-119) | The design of decisions 102, 103 and C4 takes the changes of the arbiter | 2026-10-28 | In force |
| [Decision 120](#decision-120) | The open points of decision 112 follow the base | 2026-10-28 | In force |
| [Decision 121](#decision-121) | Decision 112 also changes two Casper source files beyond the constant | 2026-10-28 | In force |
| [Decision 122](#decision-122) | The open points U1, U4 and U5 of C4 follow the base and dev | 2026-10-29 | In force |
| [Decision 123](#decision-123) | Consensus behavior that Reified needs comes from dev and base code behind traits | 2026-10-29 | In force |
| [Decision 124](#decision-124) | The determinism rules state record and replay, as the base does | 2026-10-29 | In force |
| [Decision 125](#decision-125) | A legacy replay test that fails is restated at the step where it fails | 2026-11-01 | In force |
| [Decision 126](#decision-126) | Behavior that varies by space kind goes through trait interfaces | 2026-11-01 | In force |
| [Decision 127](#decision-127) | The registry of space kinds and the VectorDB services generalize to arbitrary Reified RSpaces | 2026-11-01 | In force |
| [Decision 128](#decision-128) | The shared engine reaches kind-specific behavior through the existing traits | 2026-11-01 | In force |
| [Decision 129](#decision-129) | The finish after a deviating row follows the rule of the base for a refused tuple | 2026-11-03 | In force |
| [Decision 132](#decision-132) | Plan item C7.12 is withdrawn, so decision 43 stays in every kind | 2026-11-04 | In force |
| [Decision 133](#decision-133) | The unit loop commits the finish rows of a cut row, and native replay follows them | 2026-11-04 | In force |
| [Decision 134](#decision-134) | The end of a refused lift varies through the trait `RowEnd` | 2026-11-04 | In force |
| [Decision 135](#decision-135) | Three divergent SCAN chain cases leave the run | 2026-11-08 | In force |
| [Decision 136](#decision-136) | The node command line runs the examples, and an exploratory deploy reads their results | 2026-11-10 | In force |
| [Decision 137](#decision-137) | The campaign closes at claimed_done on the host verification, with no pgmcp run | 2026-11-10 | In force |
| [M-1](#m-1) | Counter meta-channels and merge check #1 | 2026-10-04 | In force, scope widened by decision 31 |
| [M-2](#m-2) | Space-owned record envelope | 2026-10-04 | In force |
| [M-3](#m-3) | Charging through the accounting observer | 2026-10-04 | In force, revised 2026-10-04. [Decision 89](#decision-89) retires its host-work clauses, and [decision 90](#decision-90) retires its legacy charges |
| [M-4](#m-4) | Replay consult through child modules | 2026-10-04 | Retired by [decision 90](#decision-90). The rigged replay modes retire, and `replay_rspace.rs` no longer declares `space_replay` |
| [M-5](#m-5) | Runtime-level cross-process harnesses | 2026-10-04 | In force |
| [I-1](#i-1) | Spaces are not ISpace implementors | 2026-10-04 | In force |
| [I-2](#i-2) | Single-channel handle forwarder | 2026-10-04 | In force, revised 2026-10-04, forwarder argument changed by decision 32 |
| [I-3](#i-3) | Self-describing space names | 2026-10-04 | In force |
| [I-4](#i-4) | Deploy-scoped temp and seq spaces | 2026-10-04 | In force, revised 2026-10-04 |
| [I-5](#i-5) | Metadata in the envelope | 2026-10-04 | In force |
| [I-6](#i-6) | Canonical COMM produce sort | 2026-10-04 | In force |
| [I-7](#i-7) | Bare floats in the parser | 2026-10-04 | Superseded by [decision 110](#decision-110) on 2026-10-27 |
| [I-8](#i-8) | Params only on factories and handles | 2026-10-04 | In force. [Decision 112](#decision-112) changes the carrier of the params |
| [I-9](#i-9) | Sequential kernels without ndarray | 2026-10-04 | In force |
| [I-10](#i-10) | Optional priority bound | 2026-10-04 | In force |
| [I-11](#i-11) | Per-space scheduler footprint | 2026-10-04 | Retired by [decision 90](#decision-90). It was the basis of decision 47. [Decision 92](#decision-92) takes its role in native replay |
| [I-12](#i-12) | Charging rules | 2026-10-04 | In force, revised 2026-10-04, noted 2026-10-07 by decision 47. [Decision 89](#decision-89) retires its host-work clauses, and [decision 90](#decision-90) removes its legacy clause |
| [I-13](#i-13) | Native replay fails closed for spaces | 2026-10-04 | Superseded by decision 47 on 2026-10-07 |
| [I-14](#i-14) | Unchanged empty-state hash | 2026-10-04 | In force, revised 2026-10-04. [Decision 112](#decision-112) changes the hash once |
| [I-15](#i-15) | Self-describing matchers | 2026-10-04 | In force |
| [I-16](#i-16) | No multi-channel join in synthesized machinery | 2026-10-04 | In force |
| [I-17](#i-17) | Config as a canonical Par record | 2026-10-04 | In force |
| [I-18](#i-18) | Fixed-arity system processes | 2026-10-04 | In force, revised 2026-10-04 |
| [I-19](#i-19) | Space id derivation | 2026-10-04 | In force, reason corrected by decision 32 |
| [I-20](#i-20) | Derived flags refreshed before the strict decode | 2026-10-05 | Retired by [decision 112](#decision-112) |
| [I-21](#i-21) | Fire-round cap of a space transaction | 2026-10-05 | In force, vetoable value |

Decisions 130 and 131 set the order of the work and change no design, so the register has no entry for them.

## User decisions 1 to 18

The plan records decisions 1 to 10 on 2026-10-03.
Its tracker restructuring dates decisions 11 to 18 on 2026-10-04.

### <a id="decision-1"></a>Decision 1: Scope rule

| Field | Value |
|---|---|
| Date | 2026-10-03 |
| Status | In force |
| Supersedes | None |
| Related | [Decision 2](#decision-2), [outer store](glossary.md#outer-store), [inner collection](glossary.md#inner-collection) |

**Statement.**

- The campaign implements the Reifying RSpaces FIPS [[FIPS-RRS](#ref-fips-rrs)].
- It includes every outer store of the FIPS: HashMap, PathMap with prefix-send semantics, Array (fixed and cyclic), Vector, and HashSet (seq).
- It includes every inner collection: bag, set, cell, queue, priority queue, stack, and bag with VectorDB.
- It also includes the qualifiers, `use` blocks, the space agent, factories, `getSpaceAgent`, and theories.
- All other Bag-related or PathMap-related work belongs to the agent on its own feature branch.
  This work includes ParBag (`PathMap<Multiplicity>`), PathMap-backed RSpace datum storage, AC (associative-commutative) matching, stack-safe Par codecs, and the `rrs-native-bag` Rholang Bag values.
  These items are not in the FIPS.

**User's words.** The plan quotes no words for this decision.

**Reason.** The FIPS defines the scope of the campaign. The excluded items are not in the FIPS, and the agent of the PathMap campaign owns them.

### <a id="decision-2"></a>Decision 2: MeTTaTron space held

| Field | Value |
|---|---|
| Date | 2026-10-03 |
| Status | In force |
| Supersedes | None |
| Related | [Decision 16](#decision-16), [decision 26](#decision-26) |

**Statement.**

- The MeTTaTron Reified RSpace is held.
- The hold covers all `mc-*` and `rrs-metta-*` items, `fn-bridge-deletion`, the N-M suites, and the MeTTa rows inside other tasks.

**User's words.** The plan quotes no words for this decision.

**Reason.** The plan records the hold as a user decision and states no separate reason. Only the user lifts the hold.

### <a id="decision-3"></a>Decision 3: F1r3lang ownership

| Field | Value |
|---|---|
| Date | 2026-10-03 |
| Status | In force |
| Supersedes | None |
| Related | [Decision 9](#decision-9), [decision 17](#decision-17), [F1r3lang](glossary.md#f1r3lang) |

**Statement.**

- The campaign does not own the F1r3lang implementation, which is the pgmcp epic `mettail-rholang-ddl-flt-rspace-epic` (id 5996).
- The campaign owns the Reified components that depend on F1r3lang, after the user says that F1r3lang is ready.
- Nobody merges the F1r3lang branch before that signal.

**User's words.** The plan quotes no words for this decision.

**Reason.** Another agent owns F1r3lang. The readiness signal SIG-F gates phase P12, which merges F1r3lang into the campaign branch.

### <a id="decision-4"></a>Decision 4: Parser isolation

| Field | Value |
|---|---|
| Date | 2026-10-03 |
| Status | In force |
| Supersedes | None |
| Related | [Decision 10](#decision-10), [decision 25](#decision-25) |

**Statement.**

1. Save the 104-path parser diff of codex as a patch.
   All 104 paths changed on 2026-10-01 between 15:46 and 20:54, inside the session of codex.
   The branch had no commits after 2026-08-08.
2. Create the worktree `../rholang-rs-reified-rspaces` on the new branch `feature/reified-rspaces-v3` from `02cef80`.
   Apply the patch there.
   The existing rholang-rs branches `feature/reified-rspaces` (`a2073f7`, the work of the user from December 2025) and `feature/reified-rspaces-v2` stay untouched.
3. Restore those 104 paths in the shared worktree `../rholang-rs-cost-accounting-transpiler` to HEAD.
   Eight workspaces consume that worktree by path, and the F1r3lang baseline reads it through `F1R3LANG_LEGACY_PARSER_ROOT`.
4. The node worktree uses an uncommitted local `[patch]` to the new parser worktree.
   Nobody commits the `[patch]`.

**User's words.** The plan quotes no words for this decision.

**Reason.** The shared parser worktree serves eight workspaces and the F1r3lang baseline. Edits there reach every consumer, so the Reified parser work moves to its own branch and worktree.

### <a id="decision-5"></a>Decision 5: Realignment method

| Field | Value |
|---|---|
| Date | 2026-10-03 |
| Status | In force |
| Supersedes | None |
| Related | [Design record](design-record/README.md) |

**Statement.**

- Back up first, and then rebuild cluster by cluster.
- The backup is a full binary patch plus a tarball of the untracked files, stored outside `/tmp`.
- Restore the contaminated files to HEAD.
  Then re-apply only the conforming clusters, and verify each cluster with tests.

**User's words.** The plan quotes no words for this decision.

**Reason.** A verified backup must exist before any destructive restore. Phase P0 of the plan therefore runs a restore drill before the restore.

### <a id="decision-6"></a>Decision 6: Milestone commits

| Field | Value |
|---|---|
| Date | 2026-10-03 |
| Status | In force. The fixture commit that this decision gated was not needed under [decision 18](#decision-18). [Decision 112](#decision-112) needs it again, and the user approved it. |
| Supersedes | None |
| Superseded by | Fixture part: [decision 18](#decision-18) |
| Related | [Decision 25](#decision-25), [I-14](#i-14) |

**Statement.**

- Create `[agent]`-prefixed commits at verified green milestones on `feature/reified-rspaces` (node) and `feature/reified-rspaces-v3` (parser).
- Do not push and do not merge.
- Do not commit the `[patch]`.
- Do not add Co-Authored-By lines, as the project `CLAUDE.md` requires.

**User's words.** The plan quotes no words for this decision.

**Reason.** The plan states no separate reason.

### <a id="decision-7"></a>Decision 7: Namespace tag 0xE0

| Field | Value |
|---|---|
| Date | 2026-10-03 |
| Status | In force |
| Supersedes | The ledger tag `0xF3` |
| Related | [Namespace tag](glossary.md#namespace-tag), [KIND](glossary.md#kind), [I-19](#i-19) |

**Statement.**

- The namespace tag `0xE0` replaces `0xF3`.
- Storage channels have the layout $`\mathtt{0xE0} \mathbin{\Vert} \mathit{KIND} \mathbin{\Vert} \mathit{space\_id}_{32} \; [\mathbin{\Vert} \text{canonical inner bytes}]`$.
- The KIND values are D `0x44` (data), C `0x43` (config), I `0x49` (path index), and A `0x41` (allocator).
  The KIND H is `0x48` (handle), and M is `0x4D` (reserved, because MeTTa is held).
- The design pass added the KINDs S, N, X, and B and the lowercase ephemeral forms. The [KIND table](glossary.md#kind) and the figure below give the full set.
- Factory and system-operation channels have the form $`[\mathtt{0xE0}, \mathit{idx}]`$.

![Namespace bytes: an id that starts with 0xE0 is either a two-byte id [0xE0, idx] or a three-part id 0xE0 ‖ KIND ‖ body. The left table lists every KIND with its byte, layout, persistence and writer. The right table lists the factory ids, the reserved id 0x20, the system ids 0x21 to 0x23, the internal operations 0x30 to 0x32, the syntax markers 0xD0 to 0xD4, and AGENT_TAG 0xFF.](diagrams/02-namespace-bytes.svg)

*Source: [diagrams/02-namespace-bytes.puml](diagrams/02-namespace-bytes.puml).*

**User's words.** The plan quotes no words for this decision.

**Reason.** `0xF3` collides with the MeTTaIL tag `MTL_NATIVE_SHIFT_CHANNEL_TAG`, and MeTTaIL claims the band `0xF0` to `0xF9`. A survey of the node, F1r3lang, feature/mettail, and MeTTaIL found no other user of `0xE0`.

### <a id="decision-8"></a>Decision 8: Agent-shaped API

| Field | Value |
|---|---|
| Date | 2026-10-03 |
| Status | In force |
| Supersedes | None |
| Related | [I-2](#i-2), [I-15](#i-15), [handle](glossary.md#handle), [user agent-backed space](glossary.md#user-agent-backed-space) |

**Statement.** The API follows the Agents FIPS of 2025-08-20 [[FIPS-AGENTS](#ref-fips-agents)], which the parser already implements as syntactic sugar. The decision has two parts.

Part (a): native spaces are agents.

- Factories are agent constructors. The FIPS form `for (s <- HMB!?("default", free Th()))` already has this shape.
- Each space gets `this = GPrivate(0xE0‖H‖space_id)` and a single-channel persistent receive on `this` in the default space.
- That receive forwards each call to the installed handle-op process ([I-2](#i-2)). It is consensus state, it survives restarts, and it replays through ordinary COMM events.
- The factory returns `bundle+{*this}`.
- Handles answer `space!gensym()`, `space!produce(ch, p)`, and `space!consume(matchers, arrow, body)`, plus introspection.

Part (b): user agent-backed spaces. The FIPS states that "any agent implementing the correct behavioral type can be used as an RSpace".

- The target is a `use` block or a `new x : agent` annotation whose value is not a native handle.
- The reducer turns `new` into `gensym`, a send into `produce`, and a receive into `consume`.
- Matchers are capabilities that forward to the installed matcher operation, and bodies are pure-`Par` receives ([I-15](#i-15)).

**User's words.** The plan quotes no words for this decision. The quotation above comes from the FIPS.

**Reason.** The FIPS lets any agent with the correct behavioral type act as an RSpace, and it reifies any RSpace into an agent [[FIPS-RRS](#ref-fips-rrs)].

### <a id="decision-9"></a>Decision 9: Approved design proposals

| Field | Value |
|---|---|
| Date | 2026-10-03 |
| Status | In force |
| Supersedes | None |
| Related | [M-3](#m-3), [I-1](#i-1), [theory](glossary.md#theory), [re-rooting](glossary.md#re-rooting) |

Each proposal gets a dated entry in the decision record and an update of the syntax contract.
The four proposals follow the words and the reason.

**User's words.** The plan quotes no words for this decision.

**Reason.** F1r3lang keeps installed handles, theorem-channel tokens, and epochs in process memory, and its admission certificates are not serializable. So the seam persists content identities and checks again on replay. The syntax layer prepares the frontend change of decision 9.2. The delivery rule follows the PathMap example of the FIPS [[FIPS-RRS](#ref-fips-rrs)].

#### <a id="decision-9-1"></a>Decision 9.1: F1r3lang-shaped theory seam

- Admission returns `Proven`, `Refuted`, or `Undetermined`, and `Undetermined` fails closed.
- Checks are budgeted, and they run at produce and at COMM. Their charges follow the revised [M-3](#m-3).
- Builtin theories are structural.
- The persisted theory reference is its content identity: language fingerprint, category, theorem id, checker ABI, and limit profile. It is never a process-local handle.
- On attach and on replay, the node checks again deterministically. It stores no certificates.

#### <a id="decision-9-2"></a>Decision 9.2: Frontend-neutral syntax layer

- The layer has parser-free Reified Par builders, a dual-frontend conformance corpus, and runtime tests that start from Par fixtures.
- The MeTTaIL Rholang 1.4 frontend replaces Tree-Sitter after the F1r3lang merge.

#### <a id="decision-9-3"></a>Decision 9.3: FIPS-exact PathMap prefix delivery

- Re-root EPathMap payloads under the suffix, for example `{|[2,"hi"]|}`.
- Deliver other payloads as `[suffix…, payload]`.
- An EPathMap adapter is in front of the delivery. The adapter is forward-compatible with the EPM1 representation of feature/mettail (EPathMap `serialized_paths = 8`, `trie_snapshot = 9`).
- PathMap semantics are defined logically. The path-element encoding aligns with EPM1 where EPM1 is stable.

#### <a id="decision-9-4"></a>Decision 9.4: getSpaceAgent

- `getSpaceAgent!?(space)` returns the factory capability Par.

### <a id="decision-10"></a>Decision 10: Parser branch name

| Field | Value |
|---|---|
| Date | 2026-10-03 |
| Status | In force |
| Supersedes | None |
| Related | [Decision 4](#decision-4) |

**Statement.** The parser branch is `feature/reified-rspaces-v3`.

**User's words.** The plan quotes no words for this decision.

**Reason.** The new name keeps the existing branches `feature/reified-rspaces` and `feature/reified-rspaces-v2` untouched ([decision 4](#decision-4)).

### <a id="decision-11"></a>Decision 11: Head-only matching

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force |
| Supersedes | The ledger wording "queue oldest-matching" |
| Related | [Head-only matching](glossary.md#head-only-matching), [M-1](#m-1) |

**Statement.**

- Only the head datum and the head continuation interact. The head of a queue has the minimum `ord`, and the head of a stack has the maximum `ord`.
- A non-matching head blocks later items.
- A priority queue checks its per-priority queues in ascending priority order, head-only within each level, as the legacy code does.
- The ledger wording "queue oldest-matching" is superseded.

**User's words.** The plan quotes no words for this decision.

**Reason.** The FIPS describes queues by their heads: "The tops of each queue have the potential to interact with each other" [[FIPS-RRS](#ref-fips-rrs)].

### <a id="decision-12"></a>Decision 12: Lane rule

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force |
| Supersedes | None |
| Related | [Decision 4](#decision-4) |

**Statement.**

- All code edits go in f1r3node-rust, the node worktree of the campaign.
- The only other edit location is the Reified grammar in the approved `rholang-rs-reified-rspaces` worktree.
- Nobody edits pgmcp, neither its code nor its configuration. Its `cpu_percent` setting stays as configured, and only the user changes it.
- The campaign uses pgmcp only through its tracker and verification tools, as the epic requires.
- The worktrees of other agents (F1r3lang, mettail, MeTTa-Compiler, and PathMap) are read-only.

**User's words.** "You are supposed to be editing f1r3node-rust"

**Reason.** The words of the user fix the edit location of the campaign.

### <a id="decision-13"></a>Decision 13: Verification command defaults

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force. Amended by [decision 137](#decision-137). |
| Supersedes | None |
| Related | [Decision 22](#decision-22) |

**Statement.**

- Allow a hash-pinned cache of third-party crates. Workspace crates always rebuild from the exact candidate.
- The exact-arithmetic receipt is a pure-Python `fractions.Fraction` reference. SageMath stays as an extra local gate leg.
- The node CLI verifies the examples, as the contract requires.
- The workspace regression gate skips the 27 pre-existing `#[ignore]` tests by exact name and records their reasons.
- The user still sees the exact recipe command list for the VPI-1 relay before registration.
  *Changed by [decision 137](#decision-137): pgmcp registers no recipe, so the VPI-1 relay is not asked.*

**User's words.** The plan records these defaults as "user-approved".

**Reason.** The plan states no separate reason.

### <a id="decision-14"></a>Decision 14: Rocq line-reference audit

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force |
| Supersedes | The earlier disposition that appended the Reified directory and removed no directory |
| Related | [Decision 12](#decision-12) |

**Statement.**

- In `scripts/audit/check-rocq-line-refs.sh`, remove the dead `formal/rocq/validator/theories` entry.
- Append `formal/rocq/reified_rspaces/theories` to the same list.
- Leave `check-cost-accounted-rho-proofs.sh` untouched for its owners.

**User's words.** The plan quotes no words for this decision.

**Reason.** The validator entry is dead since commit `1d325b996`. The appended entry brings the Reified theories under the line-reference audit.

### <a id="decision-15"></a>Decision 15: Empty-state hash fixture

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | The fixture commit is superseded by decision 18. The regression rule below is in force. [Decision 112](#decision-112) changes the hash once. |
| Superseded by | [Decision 18](#decision-18) |
| Related | [I-14](#i-14) |

**Statement.**

- `empty_state_hash_fixed` is not expected to change.
- The baseline test `empty_state_hash_should_be_the_same_as_hard_coded_cached_value` must stay green at every milestone.
- The second hard-coded hash in `runtime_spec.rs` must stay green too.
- If either test fails, the work stops and the user gets a report. Nobody edits the constants.

*Amended by [decision 112](#decision-112): the protobuf fields change every state root once, so merge 2 edits both constants once. The base did the same in commit `d367bc11a` (DR-135 item 12). After that edit, the rule applies again.*

**User's words.** The plan quotes no words for this decision.

**Reason.** Decision 18 leaves every ordinary hash unchanged, so the fixture commit of the earlier plan is not needed.

### <a id="decision-16"></a>Decision 16: Tracker dispositions

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force |
| Supersedes | None |
| Related | [Decision 2](#decision-2), [decision 20](#decision-20) |

**Statement.**

- The tracker resolves the superseded leaves of codex with `work_item_resolve` (`resolution: superseded`, with the `replacement_public_ids` of the successors). The TRI-1 relay of the user approval authorizes this step.
- Duplicates resolve with `resolution: duplicate`.
- `work_item_defer` defers held items. This excludes them from the completion roll-up of the epic, and `work_item_reinstate` restores them when the user lifts the hold.
- Prefer amending existing leaves.
- Create new leaves only for outputs that no leaf covers.
  These outputs are the port details and the features approved on 2026-10-03 and 2026-10-04.
- Another concurrent session wrote two feedback notes, "Review = gap-fill" and "No general Casper edits", for its cost-accounting work. The user stated on 2026-10-04 that they do not apply to this campaign.

**User's words.** The plan quotes no words for this decision.

**Reason.** The plan states no separate reason.

### <a id="decision-17"></a>Decision 17: Runtime space registration postponed

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force |
| Supersedes | None |
| Related | [Decision 3](#decision-3), [decision 9](#decision-9), [theory identity](glossary.md#theory-identity) |

**Statement.**

- Runtime registration and loading of custom Reified RSpaces is postponed. The epic has no such task today.
- The held MeTTaTron design of the ledger stays as it is.
- The tracker records the follow-up `rrs-fu-runtime-space-registration`, gated on the F1r3lang readiness signal (SIG-F).
- The design direction for the follow-up has five parts:
  - Syntax comes from installed MeTTaIL languages (parser images and foreign-language terms). See `mettail-rust/rholang-runtime/src/language_install.rs`.
  - Registration uses the Versioned Registry `rho:lib:…` namespace. See `f1r3node-rust-f1r3lang/rholang/src/rust/interpreter/registry/`.
  - Semantics come from a declarative space specification: outer store, discipline, matching relations, and admission theory. A generic, metered space agent interprets the specification.
  - Native engines such as MeTTaTron enter only as allow-listed host capabilities by URN (MeTTaIL-in-Rholang FIPS, Part V).
  - The open prerequisites are the purity and cross-language fuel obligations of the same FIPS, Part V.5.
- This campaign adds no hooks for registration. The theory seam already persists content identities, and the config record has a version tag.

**User's words.** "custom reified rspaces might require custom syntax which will not be a trivial change, so let's postpone it to later." For the design direction, the user asked to use "theories/modules from f1r3lang to define custom syntax and semantics for dynamically loaded reified rspaces".

**Reason.** Custom spaces can need custom syntax, which is not a small change.

### <a id="decision-18"></a>Decision 18: Marker-tagged tuples

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | Superseded by [decision 112](#decision-112) on 2026-10-27. Merge 2 implemented decision 112. Before, [decision 81](#decision-81) approved one `casper/src` exception, the backport of the DR-119 fix of bug 11004, and [decision 84](#decision-84) reverted it |
| Supersedes | The ledger K15 protobuf fields `Par.use_blocks = 15`, `New.space_types = 6`, `Send.params = 8`, `ReceiveBind.query_spec = 6`, and `Expr.e_free_body = 38` |
| Changes | The fixture parts of [decision 6](#decision-6), [decision 15](#decision-15), and [I-14](#i-14) |
| Related | [Decision 21](#decision-21), [marker tuple](glossary.md#marker-tuple), [recognized position](glossary.md#recognized-position) |

**Statement.**

- The Reified syntax is encoded as marker-tagged tuples, not as new protobuf fields.
- Precedents are the mergeable-tag channels `(tag, x)` of `reduce.rs::is_mergeable_channel` and the FLT head tags of F1r3lang, `GPrivate("mettail.term." ‖ F ‖ "." ‖ L)`.
- The result has no `.proto` change, unchanged hashes for ordinary terms, and an unchanged `empty_state_hash_fixed`.
- The baseline fixture test becomes a regression gate, and `casper/src` does not change at all.
- Decisions 6 and 15 and I-14 lose their fixture parts.
- [Decision 21](#decision-21) gives the corrected encoding table.

*Superseded by [decision 112](#decision-112) on 2026-10-27: the Reified syntax takes protobuf fields, as the base carries its new syntax. Every state root then changes once.*

**User's words.** The user chose "Marker tuples".

**Reason.** This is arbitration item H10. `Par` derives serde over all fields, and `stable_hash_provider::hash = Blake2b256(bincode(Par))`. A new field would change every channel, produce, and state hash. That change forks existing networks and forces a casper fixture change.

## User decisions 19 to 27

The P1, P2, and P3 design passes found places where the wording of the plan did not fit the code.
The user approved each recommended option on 2026-10-04, in the second and third rounds of decisions.
These decisions override the earlier plan sections where they conflict.

### <a id="decision-19"></a>Decision 19: P1 substrate

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force |
| Supersedes | The substrate description of the plan where it conflicts, for example raw `put`, `remove`, and `log` methods and a `replay()` method |
| Amended by | [Decision 30](#decision-30), which supersedes the storing of the incoming datum of a produce-side peek fire in the P1 design. [Decision 36](#decision-36), which moves the failure of an unpaired replay fire from staging to commit, so that D6 also covers `ReplayDivergence`. [Decision 45](#decision-45), whose Rule E retires the retention of peeked stored records (FR-10): every fire removes what the host removes, and the agent stores a peeked record again |
| Related | [M-4](#m-4), [space transaction](glossary.md#space-transaction), [sub-order](glossary.md#sub-order), [sweep](glossary.md#sweep), [substrate design](design.md#substrate) |

**Statement.**

- D1: The volatile sweep runs at the end of `ReducerCore::inj_with_observation` and `ReducerCore::inj` (`reduce.rs`), in P7. Every play, replay, reporting, and native-replay path passes there, under the evaluation guard.
- D2: `SpaceStoreHandle` is a struct over `Arc<dyn SpaceTxnHost<C, P, A, K>>`. `SpaceTxn` takes whole operations with an explicit outcome (`ProduceOutcome::{Store, Fire}`, `ConsumeOutcome::{Park, Fire}`) and `commit`.
  `SpaceTxn` has no raw `put`, `remove`, or `log` methods and no `replay()`. The traits are generic over `C, P, A, K`, because rspace++ cannot name `Par`.
- D3: Each operation of a transaction runs under its own sub-order $`\mathit{parent} \mathbin{+\!\!+} [(i, 2)]`$, where 2 is `SUB_OPERATION_TAG`. Native phlo recording rejects two operations under one order (`accounting/native_runtime/operations.rs:214`).
- D4: A native replay directive fails the space operation closed (`NativeReplayDirectiveUnsupported`).
- D5: `root_leaf_record_count` counts the records in the outer frame of the leaf (`get_data_proj_binary`). No history module changes.
- D6: Commit is prefix-atomic. If the observer rejects operation $`i`$, the operations before $`i`$ stay applied, as in the equivalent host program. The deploy fails, and the soft-checkpoint revert removes them.
- D7: `matches`, `was_repeated_enough_times`, and `get_comm_or_candidate` are re-stated over the staged view. All other M-4 helpers are reused.
  In play, the store steps `store_data`, `remove_matched_datum_and_join`, `store_waiting_continuation`, and `store_persistent_data` are private to sibling modules. So `space_play` re-states them over the public `HotStore` API.
- Correction: codex inverted six baseline peek tests, not five. They are `storage_actions_test.rs:323` and `:370`, `comm_observer_tests.rs:446`, and `replay_rspace_tests.rs:472`, `:545`, and `:607`.

**User's words.** "Approve (Recommended)", for the API, the rules, and the placement.

**Reason.** The P1 design pass found that the substrate wording of the plan did not fit the code.

### <a id="decision-20"></a>Decision 20: Tracker leftovers of TRI-1

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force |
| Supersedes | None |
| Related | [Decision 16](#decision-16), [decision 23](#decision-23) |

**Statement.**

- Unlink `rrs-persistence-substrate → rrs-pathmap-rspace-core`.
- Unlink `rrs-tests-verification-full-gates → rrs-tests-verification-recovery-migration`.
- Re-point the 19 edges that target canceled items to their successors.
- Amend the stale text about PathMap core, exclusive writes, and document paths.
- Apply the P3 parser-leaf changes:
  - Remove the Bag amendments.
  - Defer the bag-literal leaves.
  - Reduce `floats-bag-literals` to floats.
  - Supersede `rrs-ra-10-06` by `rrs-syntax-marker-encoding`.
  - Point `rrs-ra-10-09` at the builders.
  - Move the publish, version, and npm items to P11.

**User's words.** "Fix all (Recommended)"

**Reason.** The TRI-1 dispositions left these stale links and texts behind.

### <a id="decision-21"></a>Decision 21: Corrected encoding table

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force. [Decision 112](#decision-112) replaces its marker tuples with protobuf fields |
| Supersedes | The encoding table under "Frontend-neutral syntax layer" in the plan |
| Related | [Decision 18](#decision-18), [decision 27](#decision-27) (identifier rule), [canonical form](glossary.md#canonical-form) |

**Statement.**

- `x!(d…; p…, k=v…)` becomes the trailing data element `(PARAMS, [p…], {k: v…})`. The element is omitted when there are no params.
- Positional params keep source order, and named keys are sorted bytewise (`String::cmp`).
- `for (y <- ch ~ (q…; k=v…))` and a bare `~ e` (payload `[e]`) become the source `(QUERY, ch, [q…], {k: v…})`. Payload and params are never both empty.
- `new x_1[: s_1], … in P` with at least one annotation becomes `(NEWSPACE, [a_1…a_k], Par{news: [New]})`.
- Each annotation entry is `[]` (no annotation) or `[s_i]`, with one entry per plain binder, so $`k = \mathit{bind\_count} - \lvert \mathit{uri} \rvert`$.
- An annotation on a URI binder is rejected (`SpaceTypeOnUriBinding`).
- Empty sections are erased: `x!(a;)` encodes as `x!(a)`, and `~ (e;)` as `~ e`.
- Decoders reject every non-canonical form (`EmptyCanonicalForm`, `NonCanonical`).

The full encoding table after this decision:

| Source form | Encoding | Marker | Recognized position |
|---|---|---|---|
| `use s { P }` | `(USE, s, P)` in `Par.exprs` | USE = `[0xE0, 0xD0]` | process position |
| `new x_1[: s_1], … in P` with at least one annotation | `(NEWSPACE, [a_1…a_k], Par{news: [New{…}]})` in `Par.exprs` | NEWSPACE = `[0xE0, 0xD1]` | process position |
| `x!(d…; p…, k=v…)` | trailing data element `(PARAMS, [p…], {k: v…})`, omitted without params | PARAMS = `[0xE0, 0xD2]` | `eval_send` (last evaluated data element) |
| `for (y <- ch ~ (q…; k=v…)) P`, bare `~ e` as `[e]` | receive source `(QUERY, ch, [q…], {k: v…})` | QUERY = `[0xE0, 0xD3]` | `unbundle_receive` (evaluated bind source) |
| `free Th(args)` | the value `(FREE, "Th", [args…])` | FREE = `[0xE0, 0xD4]` | factories (inert data elsewhere) |

*Amended by [decision 112](#decision-112): the Reified syntax leaves these marker tuples and takes protobuf fields. The canonical rules of this entry stay, and [decision 120](#decision-120) settles the open points of the new encoding.*

**User's words.** "Approve corrected (Recommended)"

**Reason.** The P2 design pass (items C1 to C3) found that the earlier table did not fit the code.

### <a id="decision-22"></a>Decision 22: P2 hooks

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force. [Decision 112](#decision-112) amends its hooks |
| Supersedes | None |
| Related | [Decision 18](#decision-18), [recognized position](glossary.md#recognized-position) |

**Statement.**

- P2 lands all four recognition hooks: the process filter, the `generated_message_eval` arm, the `eval_send` hook, and the `unbundle_receive` hook.
- Until P4 enables them, the hooks fail closed with `RuntimeUnavailable`, `ParamsUnsupported`, and `QuerySpecUnsupported`.
- One shared predicate, `is_evaluated_process_expr`, replaces both filter copies in `util/mod.rs`.
- `accounting/` stays byte-identical. So the funding resolver `accounting/lexical.rs` fails closed (`BugFoundError`) on Reified terms. It has no production caller.
  *Changed by [decision 89](#decision-89) and [decision 90](#decision-90): `accounting/` is no longer byte-identical. The merges add the marker support of the funding resolver and of the estimate.*
- Two findings go to the cost-accounting owner: the filter twin in `lexical.rs` (H-1), and the demand analyses that do not see marker bodies (H-2).
- The plan applies the wording corrections C6 and C10 under "Frontend-neutral syntax layer".
- Fact C7: `space_factory_urn.rs` exists only in the backup.
- Fact C8: `models` has `doctest = false`, so the encoding examples live in tests.
- Fact C9: gates build in `RRS_SCRATCH`, never in the source tree or in `/tmp`.

*Amended by [decision 112](#decision-112): the Reified syntax leaves the marker tuples, so the four hooks read the protobuf fields of decision 112.*

**User's words.** "Approve (Recommended)"

**Reason.** The P2 design pass (items C4 and C5) found that the hook wording of the plan did not fit the code.

### <a id="decision-23"></a>Decision 23: P3 parser design

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force |
| Supersedes | None |
| Related | [Decision 24](#decision-24), [conservative extension](glossary.md#conservative-extension) |

**Statement.**

- UD-1: Keep the additive send grammar of codex, and rename `send_params_inputs` to `hyper_inputs`. The syntax trees of ordinary sends and the 57 existing tree-sitter corpus cases do not change.
- UD-3: The AST uses `Hyperparam` and `HyperparamList`.
- UD-4: `MalformedQuerySpec` covers a positional query param, the empty group `~ (;)`, and a query on a non-simple source (`!?`, `?!`, `!m`).
- UD-5: The parser checks syntax only. Value checks go to the shared validators in `models/src/rust/reified/` and to the node normalizers.
- UD-6: The parser reports errors by variant and span, with no `Display` text.
- UD-7: The crate stays at `0.1.x` until the P11 user gate.
- The new gate `ordinary_source_normalization_unchanged` checks every repository `.rho` and `.rhox` file. Each file parses and normalizes to identical bytes at the baseline and with the v3 parser (consensus checklist item 22).

**User's words.** "Approve (Recommended)"

**Reason.** The P3 design pass found that the parser wording of the plan did not fit the code.

### <a id="decision-24"></a>Decision 24: Keywords by lookahead

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force |
| Supersedes | None |
| Related | [Decision 23](#decision-23), [conservative extension](glossary.md#conservative-extension) |

**Statement.**

- This is item UD-2 of the P3 design pass.
- A tree-sitter external scanner (`rholang-tree-sitter/src/scanner.c`) emits the `use` keyword only when separators follow, then a space name, then `{`. A space name is an identifier outside the USE exclusion table, `_`, or `@` with a simple quoted process.
- The scanner emits the `free` keyword only when separators follow, then an identifier outside the FREE exclusion table, then `(`.
- The exclusion tables (USE 11 words, FREE 11 words, QUOTE 5 words) come mechanically from the baseline `grammar.json` (parser commit `6548aec`).
- These sequences are never valid at the baseline, so every program that parses today parses unchanged.

**User's words.** "Lookahead (Recommended)"

**Reason.** Contextual keywords keep the parser a conservative extension of the baseline parser.

### <a id="decision-25"></a>Decision 25: Parser-branch commits (UD-9)

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force |
| Supersedes | None |
| Related | [Decision 6](#decision-6), [decision 27](#decision-27) |

**Statement.**

- The `CLAUDE.md` of rholang-rs forbids `git add` and `git commit`, and decision 6 asks for milestone commits on `feature/reified-rspaces-v3`.
- The user resolved this conflict: `[agent]` milestone commits on the parser branch are approved.
- These commits use explicit pathspecs, with no push, no merge, and no publish.

**User's words.** The user chose "Commit per decision 6".

**Reason.** Decision 6 and the repository rule of rholang-rs conflicted, and only the user can resolve such a conflict.

### <a id="decision-26"></a>Decision 26: Completion goal

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force |
| Supersedes | None |
| Related | [Decision 2](#decision-2), [decision 3](#decision-3) |

**Statement.**

- The campaign completes all tasks end to end.
- It allows no deferrals, stubs, hacks, workarounds, fixmes, todos, pragmatic decisions, technical debt, or other forms of incompletion.
- An explicit request of the user overrides these constraints.
- When the campaign asks the user a question, it waits for the explicit answer of the user before it continues.
- The explicit holds stay in force: P12 waits for SIG-F, the MeTTaTron space stays held, and the PathMap Bag work stays with its owner.

**User's words.** "complete all tasks, end-to-end, with no deferrals, stubs, hacks, workarounds, fixmes, todos, pragmatic decisions, technical debt, or other forms of incompletion; anything I explicitly request overrides these constraints; if you ask me a question, await my explicit response before proceeding"

**Reason.** The words of the user set the completion criterion of the campaign.

### <a id="decision-27"></a>Decision 27: Activation and rollout out of scope

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force |
| Supersedes | None |
| Related | [Decision 21](#decision-21), [decision 25](#decision-25), [LFS](glossary.md#lfs) |

**Statement.**

- Activation and rollout are out of scope.
- The campaign adds no activation height, no scan script, no rollout procedure, and no rollout documentation.
- Staging the rollout is an operations task after the implementation is complete.

The plan keeps this technical background for the operations owners:

- Replay executes each deploy again from its source text.
- A pre-upgrade deploy that names a new URN passes admission, because the normalizer does not check URNs. It fails at run time (`util/mod.rs:278`), so an upgraded node would compute a different post-state for it.
- New nodes replay only above the LFS horizon (`initializing.rs:773-810`).
- The `header.version` of a block must equal the genesis version (`block_processor.rs:399`).

The plan also records two items under this decision:

- UD-9 resolved: the same resolution as [decision 25](#decision-25).
- Identifier rule: it implements the rule of [decision 21](#decision-21) that decoders reject every non-canonical form.
  - A param key or a theory name that a decoder accepts must be one that the frontend can produce.
  - `models/src/rust/reified/validation.rs` holds two constant word lists: the 26 words that a param key rejects (`REJECTED_PARAM_KEYS`), and the 18 words that a theory name rejects (`REJECTED_THEORY_NAMES`).
  - A P3b test checks every word-shaped string token of `grammar.js` against the parser.
  - `x!(d; w=1)` must parse as key `w` exactly when the validators accept `w`.
  - `free w()` must parse as a theory call exactly when the validators accept `w` as a theory name.
  - `Encoding.v` models the same two predicates.
  - The builtin theories `Nat`, `Int`, `String`, `Bool`, and `Any` must be accepted.

**User's words.** First "New networks only", and then "the better solution is to stage rollout once the implementation is complete, which is an ops problem unrelated to the current campaign".

**Reason.** The user classifies the rollout as an operations problem that is not part of this campaign.

## User decisions 28 to 51

The P4 design pass and its arbitration found more places where the plan did not fit the code.
The user approved each recommended option on 2026-10-04, as decisions 28 to 35.
On 2026-10-05, a P4 test, the P5 design pass, and its readiness review led to decisions 36 to 39.
These decisions override the earlier plan sections where they conflict, as decisions 19 to 27 do.

### <a id="decision-28"></a>Decision 28: Repeated channels in a group

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force. [Decision 103](#decision-103) replaces two clauses, and [decision 118](#decision-118) amends decision 103 |
| Supersedes | The rule of fix rule FR-1 that rejected a repeated channel (`DuplicateChannelInGroup`) |
| Related | [Decision 19](#decision-19), [canonical group](glossary.md#canonical-group), [store identity](glossary.md#store-identity) |

**Statement.**

- This is item A4 of the P4 arbitration.
- A channel group may name one channel more than once.
- Every group with two or more channels is non-decreasing by `stable_hash_provider::hash`, so a channel multiset has exactly one canonical order (fix rule FR-1').
- A group that names a channel $`k`$ times binds $`k`$ distinct records on it.
  *Replaced by [decision 103](#decision-103): a persistent record can bind several positions, as in the base ([decision 118](#decision-118)).*
- In a produce-side fire, the incoming datum binds exactly once.
  *Replaced by [decision 103](#decision-103): the data of a repeated channel bind as in the base, so a persistent incoming datum can bind several positions. [Decision 118](#decision-118) keeps the retry of the incoming datum at each of its positions (rule JN-9).*

**User's words.** "Allow, canonical order (Recommended)"

**Reason.** Runtime aliasing can create a join that names one channel twice.
The FIPS gives each pair of a matcher and a channel its own message, so the repeated channel binds distinct records.
*[Decision 103](#decision-103) found that the consume clause of the FIPS does not say this (`Reifying RSpaces.md:131-143`).*
A sorted list is the only non-decreasing arrangement of its elements, so the hot store never holds one channel multiset under two keys.
Two keys for one multiset would make the checkpoint refuse two actions on one history key.

### <a id="decision-29"></a>Decision 29: Joins into ordered collections in P5

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force. Implemented in P5 wave W3 by the [join engine](design.md#n12-wave-w3-of-p5-joins-of-one-space) |
| Supersedes | None |
| Related | [Decision 11](#decision-11), [decision 50](#decision-50), [head-only matching](glossary.md#head-only-matching), [counter meta-channel](glossary.md#counter-meta-channel) |

**Statement.**

- This is item A3 of the P4 arbitration.
- Queue, stack, and priority-queue spaces support joins across channels of one space (FIPS line 309).
- The design uses a continuation envelope with several `ord` values and a cascade that ends: it lifts a waiter and offers its data again.
- P5 implements these joins after its own design pass and its proofs of invariant preservation, cascade termination, and order independence.
- The P4 single-channel slice rejects such joins with `OrderedJoinUnsupported`, but only until P5 lands.
- P5 wave W3 implements the joins of one space for every discipline, and it retires `OrderedJoinUnsupported`.

**User's words.** "Implement in P5 (Recommended)"

**Reason.** The FIPS allows these joins.
A join into an ordered collection needs an envelope with several `ord` values and a cascade, and both need their own design and proofs.

### <a id="decision-30"></a>Decision 30: Peek fires re-offer the sent value

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force, consume-side clause amended 2026-10-06 by [decision 45](#decision-45) |
| Supersedes | The storing of the incoming datum of a produce-side peek fire in the P1 design (decision 19, P1 design §6) |
| Related | [Decision 19](#decision-19), [retention](glossary.md#retention), [re-offer](glossary.md#re-offer), [substrate design](design.md#s58-peek-re-offer-fr-10) |

**Statement.**

- This is item A1 of the P4 arbitration. Fix rule FR-10 implements it.
- When a send fires a peek waiter, the substrate does not store the incoming datum.
- The agent keeps the datum in flight, and it offers the datum to the next waiter as the next sub-operation.
- The re-offer has the produce identity (`Produce::create`), the observer sequence, the counter step, and the authority of the re-produce of the baseline.
- If no waiter matches, the re-offer stores the datum.
- A consume-side peek still leaves the stored record in place. [Decision 45](#decision-45) amends this clause: the COMM removes the record, and the same transaction stores it again, as the second send of the baseline does.
- The fire receipt does not mark the incoming datum as retained.

**User's words.** "Offer to next waiter (Recommended)"

**Reason.** With a peek waiter at the head of a queue, a stored copy of the datum can match the next waiter.
That pending match breaks the no-pending-match invariant of the space.
The re-offer equals the re-produce of the baseline reducer (`reduce.rs:1627-1689`), so the events and the observer calls stay equal to those of the host.

### <a id="decision-31"></a>Decision 31: Every operation on an ordered collection bumps its counter

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force |
| Supersedes | The "state-changing" scope of [M-1](#m-1) and its sentence "A peek changes no state and does not touch the counter" |
| Related | [M-1](#m-1), [counter meta-channel](glossary.md#counter-meta-channel), [merge](glossary.md#merge) |

**Statement.**

- This is item A2 of the P4 arbitration.
- Every operation on an ordered collection consumes the counter of the collection and produces it again.
- This rule also covers a peek and a fire that changes no state.
- Merges are then serializable for each collection.
- Two branches that only peek one queue also conflict.
- Each peek pays the three counter events: a Consume, a COMM, and a Produce.

**User's words.** "Bump every operation (Recommended)"

**Reason.** The P4 design gives two counterexamples that pass merge checks #1 to #3 when reads and fires that change no state skip the counter.
In the first, each branch peeks one queue and then dequeues the head of the other queue.
In the second, each branch fires a persistent waiter on one queue and stores a datum on the other queue.
No serial order of the two branches gives the merged state.
With a counter step on every operation, both cases race on the counters, and merge check #1 reports the race.

### <a id="decision-32"></a>Decision 32: P4 plan-text changes

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force. Its open P8 item is settled by [decision 77](#decision-77). [Decision 106](#decision-106) replaces item A21 for `embed` and `ann` |
| Supersedes | The forwarder argument `GByteArray(sid)` of [I-2](#i-2), and the claim of [I-19](#i-19) that `SpaceAlreadyExists` stops the `Factory!!(…)` re-produce loop |
| Related | [Decision 18](#decision-18), [decision 22](#decision-22), [internal operation](glossary.md#internal-operation), [handle](glossary.md#handle) |

**Statement.** The user approved four changes of the plan text, items A5, A6, A7, and A21 of the P4 design.

- A5: `canonical_inner_channel_bytes` is the bincode of the sorted Par, which is the preimage of the store identity. Protocol Buffers encodes `locally_free`, so prost bytes would split one store channel into two keys.
- A6: `HANDLE_OP` is bound to the URN `rho:reified:internal:handleOp`. The baseline binds every system definition to a URN (`rho_runtime.rs:1942-1948`), and `registry_lookup` serves any key.
- A6, continued: the operation accepts a call only with the unforgeable `this`. It must be a `GPrivate` of exactly 34 bytes with KIND H or h, and its config record `C(sid)` must exist.
- A6, continued: this replaces the `GByteArray(sid)` forwarder of I-2, which would leak the capability through `config` and `toByteArray`.
- A7: P2 implements the send hook as the same-signature swap of `self.produce(` for `self.reified_produce(`. The function `reified_produce` splits the trailing params first, and it fails closed as decision 22 says.
- A7, continued: the H2 arm is one line without a guard, of at most 100 columns, for example `eval_marker`. The reason is that H1 admits only marker tuples into process position.
- A21: a persistent send (`!!`) to a two-byte `0xE0` operation channel, or to an H or h handle, aborts the deploy. The two-byte channels are the factories, `getSpaceAgent`, `assertEqual`, `vector`, and the internal operations.
  *Changed by [decision 106](#decision-106): a persistent send to `embed` or `ann` passes the send. It fails at its fire with `SystemProcessShapeError`, by rule P2 of DR-114. The abort at the send stays for the other operations.*
- A21, continued: the baseline never cancels the re-produce sibling of a persistent send (`reduce.rs:1306-1392`, `1691-1741`). So the claim of I-19 that `SpaceAlreadyExists` stops the loop was wrong.

**User's words.** "Approve all four (Recommended)"

**Reason.** The P4 design pass found that the plan text did not fit the code in these four places.

### <a id="decision-33"></a>Decision 33: assertEqual has arity 3

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force |
| Supersedes | The two-argument call of the FIPS example, which is pseudo-code |
| Related | [I-18](#i-18), [getSpaceAgent](glossary.md#getspaceagent) |

**Statement.**

- This is item A12 of the P4 design.
- `rho:assert:assertEqual` has arity 3.
- The call `assertEqual!(ret, a, b)` replies on `ret` with a `GBool`, which is true when the sorted Par values of `a` and `b` are equal.

**User's words.** "Arity 3 with reply (Recommended)"

**Reason.** Every Reified system process has a fixed arity ([I-18](#i-18)), and a call needs a reply channel to return its result.
The two-argument call of the FIPS is pseudo-code.

### <a id="decision-34"></a>Decision 34: P4 detail items

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force. Item A15 amended by [decision 137](#decision-137). |
| Supersedes | None |
| Related | [Decision 32](#decision-32), [decision 33](#decision-33), [lock coverage](glossary.md#lock-coverage), [evaluation scope](glossary.md#evaluation-scope) |

**Statement.** The user approved the P4 detail items A8 to A11, A13 to A20, and A22 to A25. Section F of the P4 design report lists them.

- A8: no hunk for the peek result flag. Space fires never pass through the peek path of the reducer.
- A9: gates that fail closed in P4. Temp and seq spaces and `FREE` wait for P7, and user agents and matcher consumes wait for P8.
- A9, continued: P4 registers only the family `hashmap:queue`. P5 and P6 add the other families, and P5 adds the aliases.
- A10: a bounded chain of fires offers the datum again inside the transaction. A divergent pair, a persistent waiter and a datum that survives, starts one new intent for each round.
- A11: a record may have one par, with an empty payload. The plan said two or more.
- A13: a new Rocq module `SpaceFactory.v`, which the tracker leaves cite.
- A14: checker extensions, and a cumulative footprint policy `p4` that extends the policy `campaign`.
- A15: amendments of tracker leaves under the AAI-1 relay. The next items list them.
  *Amended by [decision 137](#decision-137): the AAI-1 relay is not asked. The tracker leaves keep their texts, and their closure reads them with these amendments.*
  - Real events: the canonical COMM sort (I-6), events of temp spaces (I-4), no activation boundary (decision 27), and filters through the module wrappers.
  - Replay consult: the case with several results moves to P6.
  - Private channels: bound to a URN.
  - Queue discipline: as A2 says, which is decision 31.
  - Space routing: the wording "derived maps".
  - Leaves `ra-04-01` to `ra-04-06`: the paths, the tag `0xF3`, the module `spaces/namespace.rs`, and the model `Namespace.v`.
  - Collection conflicts: crossed stacks in P5.
  - The TLA+ model of the handle unwrap: P8.
  - Head-only matching of stacks and priority queues: P5.
  - The alias of `getSpaceAgent`: P5.
- A16: the record of a handle produce uses the random state of the call.
- A17: the runtime captures the host at the start of `create_rho_env`.
- A18: R7 lands in P4, and it never packs arguments.
- A19: the code names are `RuntimeUnavailable` and `SpacesUnsupportedInNativeReplay`. [Decision 46](#decision-46) replaces the second name with `SpacesUnsupportedInNativeExecution`.
- A20: the record tags and the shape of the config record.
- A22: the errors of `getSpaceAgent` are typed replies.
- A23: the format of typed replies, and the rule "is a name".
- A24: data processes that a program sends inside `use` are not wrapped in a closure.
- A25: two branches that each create a new collection in one space conflict through the space counter `S(sid)`. This rule is conservative.

The P4 design also applies four compliance fixes without a new decision, because they follow recorded decisions:

- P4 locks every channel that an operation touches ([lock coverage](glossary.md#lock-coverage), fix rule FR-4).
- Agents use the metered matcher APIs, with a scan cap for each operation (I-12, M-3, arbitration item H8).
- P5 enables `QUERY`, because the receive of a priority queue needs it.
- Agents call `begin_evaluation_scope` at each root evaluation (fix rule FR-5'), and they never await between `begin` and `commit`.

**User's words.** "Approve (Recommended)"

**Reason.** The P4 design pass found these details while it designed the vertical slice of a queue space.
They follow the recorded decisions, or they fill details that the plan left open.

### <a id="decision-35"></a>Decision 35: P1 close-out scope

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force. Its shared freeze moved to P11 by [decision 75](#decision-75), and [decision 137](#decision-137) replaces the freeze with the host verification |
| Supersedes | The A5 realignment ledgers of the P1 close-out design |
| Related | [Decision 5](#decision-5), [decision 16](#decision-16), [verification.md](verification.md) |

**Statement.** The P1 close-out stays lean, and the pgmcp verification of P1 runs together with P4.

- The close-out finishes only the work that was in flight: the regression tests, the regression-side checkers, the closure checker and [verification.md](verification.md). P2 starts after the P1 commit.
- No realignment ledger is built. The seven ledger leaves close with existing evidence: the P0 backup manifest, the footprint pin of policy `p1-realign`, and the TRI-1 ledger in pgmcp.
- The six realignment leaves of the `SpaceStoreHandle` group are duplicates of the substrate leaf, which carries the same tests.
- The pgmcp verification runs of the P1 leaves happen in the same freeze as those of P4.
  *Changed by [decision 137](#decision-137): no freeze takes place. The final gate set runs the checks of the P1 and P4 leaves on a development host, and the leaves close at claimed_done.*

**User's words.** "Lean, verify at P4 (Recommended)", after "just make sure you remain focused and don't get lost in the woods" and "don't get stuck in perpetual support changes and never get around to the reified rspaces!"

**Reason.** The remaining P1 work was bookkeeping, not spaces. One shared freeze also saves a second freeze of the worktree.

### <a id="decision-36"></a>Decision 36: Space replay reproduces play when a deploy runs out of phlo

| Field | Value |
|---|---|
| Date | 2026-10-05 |
| Status | In force |
| Amends | [Decision 19](#decision-19): an unpaired replay fire fails at commit, after the observer calls of play, and no longer at staging. Rule D6 then also covers `ReplayDivergence`. |
| Related | [Unpaired fire](glossary.md#unpaired-fire), [pairing](glossary.md#pairing), [prefix atomicity](glossary.md#prefix-atomicity), [substrate design](design.md#s53-verify-fr-3), [the finding](findings/space-replay-out-of-phlo.md) |

**Statement.**

- The P4 test `space_queue_replay_equal` exposed the defect. Replay checked the COMM of each fire at staging, and it dropped the whole transaction at a missing COMM. So replay skipped charges that play made, and Casper rejected the honest block.
- In replay, a fire whose COMM the log does not hold is staged as unpaired, with no error at staging.
- At commit, the unpaired fire makes the observer calls of play in the same order. A rejected call returns the error and the cost of play.
- If every call accepts, the commit fails with `ReplayDivergence`, and the prefix stays applied (D6).
- The code changes only `rspace++/src/rspace/space_store_handle.rs` and `rspace++/src/rspace/replay_rspace/space_replay.rs`. Casper, the host, and the P4 agents do not change.
- Proofs and models: `EventCorrelation.v` restates conjunct 3 of H4 and adds the headline H23 `replay_reproduces_play_run`. `CheckpointReplay.tla` adds a phlo budget, the invariant T6 `ReplayOutcomeEqual`, and two Unsafe controls.
- The fix design of 2026-10-05 is outside the repository. The [finding](findings/space-replay-out-of-phlo.md) records the defect, its mechanism, and the fix.
- The related defect of host replay (pgmcp finding 9921) and the observation on a forged omission stay recorded for their owners. This campaign does not pursue them.

**User's words.** "Approve the fix (Recommended)". This decision amends decision 19. For the context, the user also said: "keep this campaign focused on Reified RSpaces!"

**Reason.** Casper compares the failed flag and then the cost of each replayed deploy.
Before the fix, a deploy that ran out of phlo inside a space transaction replayed to `ReplayDivergence` at a lower cost than play.
So any user could make validators reject an honest block with a phlo limit that runs out inside a space transaction.

With the fix, replay stages exactly the operations of play, and its commit makes the same calls on the same budget.
So the same call rejects, after the same prefix, and the cost is the same.
An unpaired fire never logs, removes, or unbinds anything, so each recorded COMM still pairs at most once, and a divergence still fails closed.

### <a id="decision-37"></a>Decision 37: Receive selection follows the F1r3lang rule from the F1r3lang merge on

| Field | Value |
|---|---|
| Date | 2026-10-05 |
| Status | In force. The rule takes effect in P12, after F1r3lang becomes the frontend of the node. |
| Supersedes | Correction S3 of the P5 readiness review |
| Related | [Decision 3](#decision-3), [decision 11](#decision-11), [decision 26](#decision-26), [F1r3lang](glossary.md#f1r3lang), [head-only matching](glossary.md#head-only-matching) |

**Statement.**

- **Timing.** The rule below is the target semantics of spaces. The register records it now, and the pgmcp leaf `rrs-f1r3lang-space-where-semantics` holds the notes. The campaign implements it only after F1r3lang is finalized and becomes the frontend of F1r3node, in P12, which waits for SIG-F.
- **Until then**, space receives use the current guard rule of this tree, as the host does. A consume takes the first spatial match of each bind and checks the guard once, and a refusal parks the waiter (`ops_consume.rs:250-301`). On the produce side, a guard refusal moves to the next waiter (`space_matcher.rs:351-360`). Head-only disciplines test only the head, so both rules agree for the queue, the stack, and the priority queue.
- **The set discipline under the current rule** (readiness review S3, option (a)). A guarded waiter can park while a stored member passes its guard. A later send of an equal member then fires the waiter with the incoming member. The agent then retires the stored members of that class with single-channel `K_retire` COMMs, so the class leaves the set. Only a `where` guard reaches this case. Under the rule of F1r3lang it cannot arise, because the consume selects the passing member.
- **The target rule (notes for P12).** F1r3lang records the guard rule of the host as its defect D1, and it fixes the rule. In the fixed rule, a `where` guard takes part in candidate selection, and a guard failure counts as a pattern non-match. A receive fires with the lexicographically least selection whose data match spatially and pass the guard, over the canonical candidate order. The repository `f1r3node-rust-f1r3lang` implements this search in lines 380 to 470 of `rspace++/src/rspace/space_matcher.rs`.
- At P12, spaces adopt the rule in their own agents, with no host change. Single-bind receives take the first candidate, in discipline order, that matches and passes the guard. Head-only disciplines test only the head, so a head that fails the guard blocks ([decision 11](#decision-11)). Joins use the complete search of F1r3lang, with its second repair. In that repair, a spatial dead end at a later bind backtracks to the next datum of an earlier bind. The `K_retire` step of the set then becomes unreachable for single-bind receives.
- Until P12, the join search of P5c is the algorithm of the baseline. It takes the first spatial match for each bind, and it checks the guard once. The plan section on PathMap delivery states the same rule.
- **Scope.** This campaign does not implement the where-clauses of F1r3lang: not the syntax, not the guard decider, and not the D1 fix of the host matcher. Those belong to F1r3lang ([decision 3](#decision-3)). Space agents only order their own candidate search, and they call the existing guard check of the injected matcher (`SpaceMatcher::admits`, `check_commit_metered`). No host, matcher, or normalizer code changes.

**User's words.** "F1r3lang rule, full (Recommended)", after "Review the where semantics in `[WORKSPACE_ROOT]/f1r3node-rust-f1r3lang/` and refine your question accordingly". Then "Take notes how the where-clause semantics should apply to the new Reified RSpaces but the semantics won't be implemented until F1r3lang is finalized and becomes the frontend of F1r3node." For the scope: "You are not responsible for implementing F1r3lang's where-clauses -- just so we're clear!" The register writes the workspace root of the path as `[WORKSPACE_ROOT]`.

**Reason.** F1r3lang treats the guard rule of the host as a defect.
Under that rule, a guard does not take part in candidate selection, so a guarded receive can park while a stored datum passes its guard.
Spaces adopt the corrected rule so that they agree with F1r3lang when F1r3lang becomes the frontend.
F1r3lang is not final yet, and another agent owns it, so the implementation waits for P12.

### <a id="decision-38"></a>Decision 38: Priority-queue waiters wait in one queue per level

| Field | Value |
|---|---|
| Date | 2026-10-05 |
| Status | In force |
| Supersedes | Correction S4 of the P5 readiness review |
| Related | [Decision 11](#decision-11), [decision 29](#decision-29), [I-10](#i-10), [head-only matching](glossary.md#head-only-matching), [waiter](glossary.md#waiter) |

**Statement.**

- The legacy branch keeps one data queue for each level, and it keeps its waiters in one bag. Level 0 is the highest level, and the default level is the lowest. Each level is first in, first out, and head-only.
- The legacy code is `PriorityQueueDataCollection` with `BagContinuationCollection`, in lines 1719 to 1725 of `system_processes.rs` on the legacy branch. This decision refines the waiter side.
- A receive with `priority=p` waits in the waiter queue of level `p`. A receive with no priority waits in an unrestricted waiter queue. It takes the first matching level head in ascending level order, as the legacy receive does.
- When one datum could fire waiters of both kinds, the older waiter fires, which is the waiter with the lower `ord`.
- The P5b design specifies the details, and it argues confluence again (readiness review S4).

**User's words.** "One per level, that's how they were implemented in the legacy feature branch which you may review for inspiration. This branch should be a revival and refinement of the legacy branch.", then "As proposed (Recommended)".

**Reason.** This branch revives and refines the legacy branch, which keeps one queue for each level.
The P5 readiness review found that the confluence argument of the cascade needs one head waiter for each head-only collection.
A waiter queue for each level gives each queue one head, and the order rule decides between the two kinds of waiters.

### <a id="decision-39"></a>Decision 39: P5 design choices F1 to F17 and F20, without F21

| Field | Value |
|---|---|
| Date | 2026-10-05 |
| Status | In force. The value of F9 is vetoable, as the cap of [I-21](#i-21) is. |
| Supersedes | None |
| Related | [Decision 28](#decision-28), [decision 29](#decision-29), [decision 37](#decision-37), [decision 38](#decision-38), [I-16](#i-16), [I-21](#i-21) |

**Statement.** The user approved the P5 design choices F1 to F17 and F20, and the user dropped F21. The P5 design report lists the choices, and the corrections S1 to S7 and E1 to E5 of its readiness review apply.

- F1: HashSet moves to P7.
- F2: the stack and the priority queue settle with lift-and-re-offer, the cascade of [decision 29](#decision-29).
- F3: set retirement is one consume over `[D;n]`, a group that names the data channel `D` of the collection `n` times ([decision 28](#decision-28)).
- F4: PathMap delivers through the prefix marker and single-channel retire COMMs.
- F5: the set key is the bincode of the sorted `Par`.
- F6: an unprioritized send goes to its own lowest level.
- F7: the `size` of an array is required, and `cyclic` is `false` by default.
- F8: a forged channel in an Array or Vector space aborts with `NotAnIndexName`.
- F9: the PathMap depth cap is 64, with the abort `PathTooDeep`. The value is vetoable.
- F10: at the round cap, the original operation is re-issued, as in P4.
- F11: a `new` inside `use` of an Array or Vector space runs one allocation job. The hot line `reduce.rs:2655` gains `.await`.
- F12: the tracker leaf `rrs-spaces-determinism-collision-ties` completes in P6.
- F13: the legacy Rocq modules get the dispositions that the P5 design report lists, with the changes of its readiness review. The port drops the axiom `FunctionalExtensionality`.
- F14: two set waiters are equal only when they are byte-identical.
- F15: a cell holds one record and one waiter.
- F16: PathMap bag and set peeks do not bump the token, which is the space counter that orders a PathMap space. [Decision 51](#decision-51) amends F16 for a peek whose second send changes state.
- F17: the `temp` and `seq` rows of the pattern-enforcement matrix complete in P7.
- F20: the `[D;n]` consume never parks, so [I-16](#i-16) holds.
- F21 is dropped: no `MixedArrowJoin` error code, because a program cannot reach it.

**User's words.** "Approve all (Recommended)", after the items were elaborated.

**Reason.** The P5 design pass found these choices where the plan and the FIPS leave details open, or where the plan does not fit the code.
The readiness review corrected the design, and it recommended each approved choice, some of them with changes.
A receive has one peek flag for all its binds, so no program can make mixed peeks, and F21 is not needed (readiness review S5).

### <a id="decision-40"></a>Decision 40: Space charging follows the cost-accounted-rho policy

| Field | Value |
|---|---|
| Date | 2026-10-05 |
| Status | Retired by [decision 89](#decision-89). P5 applied it. |
| Supersedes | The phrase "per-operation candidate-scan caps" of the plan sections "Bounded work" and I-12, and the matching words of [decision 34](#decision-34) |
| Related | [Decision 41](#decision-41), [decision 42](#decision-42), [I-12](#i-12), [I-21](#i-21), [M-3](#m-3) |

**Statement.**

- **Scans: no structural candidate-scan cap.** As in DR-88 of `feature/cost-accounted-rho`, an agent reserves every read and copy before it makes it. It reserves on the host-work budget of the deploy.
- It reserves through the metered matcher (`get_metered`, `check_commit_metered`) and the reservation helpers of `shared/src/rust/clone_backing.rs`.
- The dimensions map as on that branch: operations to VerificationOperations, scanned bytes to VerificationBytes, and backing bytes to SearchStateBytes.
- Nothing is inspected up front. A budget that runs out gives the deterministic error `HostWorkRejected`.
- `FIRE_ROUND_CAP` of [I-21](#i-21) stays, because it bounds the size of a transaction, not a scan.
- **PathMap delivery: the vehicle carries the payload.** The delivery record that is never stored is `[envelope(MARKER, src(d)), EList(p∖q), payload…]`, so its introduction bytes cover the payload, as the equivalent host produce does. Stored markers stay short.
- The campaign does not edit the accounting module ([M-3](#m-3)). In this tree the host-work budget is `None` in `inj_attempt`. So these charges bound nothing in production until the cost-accounting work activates the budget, as for host operations.

**User's words.** On the scan cap: "Use the policy of the latest feature/cost-accounted-rho branch changes (see: ~/Workspace/f1r3fly.io/f1r3node-rust/)". On the cost of a PathMap delivery: "Use the policy of the latest feature/cost-accounted-rho branch changes".

**Reason.** One charging policy for host operations and space operations keeps the cost of a space operation equal to the cost of its equivalent host program.
DR-47 of that branch charges a send its introduction whether the send is delivered or stored, so no charge depends on the arrival order.

### <a id="decision-41"></a>Decision 41: Bag and set spaces order candidates by source hash

| Field | Value |
|---|---|
| Date | 2026-10-05 |
| Status | Superseded by [decision 102](#decision-102) on 2026-10-27. P5a applied it, and [decision 89](#decision-89) amended its reason. |
| Supersedes | "Ascending record bytes" for the bag, and the key order of set classes, in the disciplines table of the plan |
| Related | [Decision 40](#decision-40), [decision 44](#decision-44) |

**Statement.** As in DR-75 of `feature/cost-accounted-rho`, the bag and set agents order their candidates by the hash of their source produce or consume.
They compare full digests only between equal source hashes, and the store index decides last.
The rule is the canonical order of `rspace++/src/rspace/candidate_order.rs`, which the host uses since the merge of [decision 44](#decision-44).

**User's words.** "Source-hash order (Recommended)".

**Reason.** The order reads only precomputed hashes for distinct sources, so its work does not grow with the payloads.
The host and the spaces use one order, so a space operation and its equivalent host program choose the same candidate.
*Amended by [decision 89](#decision-89): since DR-129, host play picks at random among the candidates, and replay keeps the canonical order as its own order.*
*The spaces keep the canonical order, because each choice of a space must be a function of its entries (DT-1 of the specification).*
*The tests that compare a space with its host program pin host play to the canonical choice.*

*Superseded by [decision 102](#decision-102) on 2026-10-27: the spaces also pick at random in play, as the host does since DR-129. Replay follows the recorded pick. The design of decision 102 restates the rule DT-1.*

### <a id="decision-42"></a>Decision 42: The queue agent's metering is aligned now

| Field | Value |
|---|---|
| Date | 2026-10-05 |
| Status | Retired by [decision 89](#decision-89). It was implemented with the model `AgentReadCoverage.v`. |
| Supersedes | The one-dimension `ScanMeter` of P4, which charged `SearchCandidates` for each matcher call |
| Related | [Decision 40](#decision-40), [I-12](#i-12), [M-3](#m-3) |

**Statement.** The P4 queue agent follows [decision 40](#decision-40) now.
`ScanMeter` takes the dimension mapping of `feature/cost-accounted-rho`.
The agent reserves its own envelope decodes, payload copies, clones, and tie-breaks before each one.
The change touches the committed P4 code (`reified/costs.rs` and `spaces/*`) and the tests that pin the old mapping.

**User's words.** "Align now (Recommended)".

**Note of decision 45.** The legacy charges of [decision 45](#decision-45) walk the encoded lengths of the staged operations and reserve no host work, as `ChargingRSpace` does.
A legacy deploy has no host-work budget, so such a reservation would have no effect.

**Reason.** After the merge of [decision 44](#decision-44), the matcher reserves its own reads with that mapping.
The old `ScanMeter` would drop the operations and the backing bytes that the matcher reserves, so its charges would differ from those of the host.

**The policy of the branch.** The rules come from the code of `feature/cost-accounted-rho` at `4d67b5ccc`:

- The mapping of `reserve_native_observation_work` sends operations to VerificationOperations, scanned bytes to VerificationBytes, and allocated bytes to SearchStateBytes.
- The metered matcher reserves each read that it makes, and its caller adds no charge for a call (DR-88).
- A caller copies only what it selects, and it reserves each copy first (DR-81, DR-82).
- A move out of a container reserves one read of the moved slot (DR-91).
- The branch reserves the encode of a random state. It does not charge a split or a decode of a random state.

**Implementation.** `ScanMeter` (`reified/costs.rs`) reserves with that mapping, and it skips every walk without a budget.
The record builders and decoders have metered forms (`spaces/record.rs`).
A builder reserves a copy of a template of its envelope, because every envelope of a kind has the shape of its template.
A head scan decodes each envelope, reserves each comparison, and copies only the head (`spaces/generic.rs`, `spaces/disciplines/queue.rs`).
The bump copies only its counter record and moves the copy into the fire (`spaces/counter.rs`).

The outcome after the commit only moves values out of its receipt, and the agent prepays that work at staging.

**Verification.** `AgentReadCoverage.v` proves that every operation reserves each read before it makes it, and that no reservation follows the commit.
It also proves that the charges do not depend on the list order of the store or on unread payloads.
Five mutants of the model and the properties `prop_agent_*` of `rholang/tests/reified_p4_props.rs` check these claims ([verification](verification.md#1110-metering-of-decision-42)).

### <a id="decision-43"></a>Decision 43: A re-issued persistent operation keeps its first-round identity

| Field | Value |
|---|---|
| Date | 2026-10-05 |
| Status | In force. Implemented in commit `bc7783417`. |
| Amends | [Decision 31](#decision-31) for re-issued operations: the counter still bumps in every round |
| Related | [I-21](#i-21), [decision 29](#decision-29) |

**Statement.** On `feature/cost-accounted-rho`, a persistent produce keeps one identity across its re-produces, and its introduction is charged once for each identity in each deploy.
So a persistent space operation keeps the `ord` of its first round when it is re-issued at a divergent pair or at `FIRE_ROUND_CAP`.
Thus it also keeps its record and its identity.
The counter still bumps in every round ([decision 31](#decision-31)).
A design pass decides where a re-issued element takes its FIFO place before the code changes.

**User's words.** On the cost of re-issued persistent operations: "refer to the feature/cost-accounted-rho branch".

**Reason.** The same identity in every round makes the introduction charge independent of the number of rounds, as on the host.

### <a id="decision-44"></a>Decision 44: Merge of feature/cost-accounted-rho at 4d67b5ccc

| Field | Value |
|---|---|
| Date | 2026-10-05 |
| Status | In force |
| Amends | The base revision of every regression baseline: HEAD names the node code at `4d67b5ccc` |
| Related | [Decision 40](#decision-40), [decision 41](#decision-41), [decision 42](#decision-42), [decision 43](#decision-43), [verification 8.8](verification.md#88-the-merge-of-featurecost-accounted-rho) |

**Statement.**

- The campaign merged `feature/cost-accounted-rho` at `4d67b5ccc` into `feature/reified-rspaces`. The merge base is `e748426`.
- The footprint digests, the protobuf schema pins, and the three ordinary corpora moved to `4d67b5ccc` ([verification 8.8](verification.md#88-the-merge-of-featurecost-accounted-rho)). The corpora were captured in a clean export of that commit.
- The merge adapts the Reified code to the branch in four places:
  - `matching.rs` passes the matched data by reference to `check_commit_metered`.
  - `schedule.rs` registers each space job task in the session registry of detached tasks.
  - The reply of a handle `produce` is the last branch of the outcome, because a dispatch now ends the participant that makes it.
  - The model of the scheduler paths in `EventCorrelation.v` follows the compacted split DR-84.
- Casper does not change, and the footprint confirms that every file outside the approved Reified edits equals `4d67b5ccc`.

**User's words.** "in fact, do you want to go ahead and merge its latest changes into this branch?", then "yes, merge it carefully".

**Reason.** Decisions 40 to 43 take their rules from that branch, and the branch changes the host that the spaces must match.
With the merge, the spaces and the host share one candidate order, one dispatcher, and one charging policy.
The tests compare them on one base.

### <a id="decision-45"></a>Decision 45: Space operations pay the legacy storage charges of the host

| Field | Value |
|---|---|
| Date | 2026-10-06 |
| Status | In force for Rule E, which is implemented. [Decision 90](#decision-90) removes Option P, as [decision 69](#decision-69) planned. |
| Amends | [Decision 30](#decision-30), its consume-side clause, and the retention rule FR-10 of [decision 19](#decision-19) |
| Related | [Decision 40](#decision-40), [M-3](#m-3) |

**Statement.** In a legacy deploy, each staged space operation pays the storage and event charges that `ChargingRSpace` makes for the equivalent host call.
The agent makes the pre-charge of an operation in its `before_observe` callback.
It carries the post-charges of the operation to the callback of the next operation.
It makes the post-charges of the last operation after the commit.

A peek is charged as the branch charges a peek on a channel.
A study of the branch at `393d6791a` found the rule of the host.
The COMM of a host peek removes the record, and the reducer sends the record again (`produce_peeks`).
The branch charges that second send in full, and never as a free retry (DR-47).
In legacy mode it is one more event fee, and in native mode it is a new introduction.

So a consume-side peek on a space removes the record in its COMM (Rule E).
The same transaction then stores the record again, with its envelope and its order.
The events and the charges then equal those of the branch in both modes.
This amends the consume-side clause of [decision 30](#decision-30) and the retention rule FR-10 of the substrate.

**User's words.** On peeks: "Charge how the feature/cost-accounted-rho branch charges peeks on bags/channels". On the study's rule: "Re-store as the host (Recommended)".

**Reason.** [Decision 40](#decision-40) makes space charging follow the policy of `feature/cost-accounted-rho`.
For a legacy deploy, that policy is the charges of `ChargingRSpace`, and the substrate of the spaces bypasses it today.

**Scope of Rule E.** The user decided the scope on 2026-10-06: "Apply everywhere (Recommended)".
The substrate keeps no record for any peek, also not the stored candidates of a produce-side peek join.
Every fire removes what the host removes.
The agent stores each removed non-persistent record of a peek again, in the same transaction.

**Implementation of Rule E.**

- Substrate: `check_peeks` replaces `peek_retention`, and it decides nothing about the removal. `removed_records` returns every stored non-persistent candidate of a fire.
- The play and the replay backends always call `remove_matched_datum_and_join` or `store_persistent_data`. `fired_receipt` keeps the persist flags of `wrap_result`.
- Agent: after the fire of a peek receive, `Plan::restore` stages a produce of the same record bytes, with the outcome Store and `persist = false`.
- The callback of the re-store registers the cost authority of the record (`Introduction::for_record`), and the re-store bumps no counter.
- One commit: the substrate change and the re-store land together, because `put_datum` does not deduplicate data. A re-store alone would store a second copy, and a removal alone would lose the record.

**Implementation of Option P.**

*Removed by [decision 90](#decision-90): DR-134 of the base deletes the legacy meter.*
*So the merge of `702496775` removes Option P with its chain, its model and its tests, as [decision 69](#decision-69) planned.*
*The list below describes Option P before that merge. Rule E stays in force.*

- `reified/legacy_charges.rs` computes the [legacy charges](glossary.md#legacy-charge) of each staged operation: `produce_charges` and `consume_charges` restate `ChargingRSpace` and its `handle_result`.
- `LegacyChain` stages every operation of a space job. The callback of an operation charges the carried post-charges of the operation before it, then registers the introduction, then charges the pre-charge.
- `LegacyChain::finish` charges the post-charges of the last operation after the commit, in the finish step of the prepared job.
- The chain is active only in a legacy deploy with a metered budget. The counter bump, the plans of the queue, the re-store, and the creation of a space all stage through it.
- The design is section [N10](design.md#n10-legacy-charges) of the design, with Figure N10.
  *Changed by [decision 90](#decision-90): section N10 now states the retirement of these charges, and the design has no Figure N10. The file [diagrams/17-legacy-charge-chain.puml](diagrams/17-legacy-charge-chain.puml) keeps the figure.*
- The evidence is section [11.14](verification.md#1114-legacy-charges-of-decision-45) of the verification dossier. It holds `LegacyCharges.v` (Q79 and Q80), the invariant `LegacyChargesFollowHost` of `CheckpointReplay.tla` (U13), the tests, and the controls.

### <a id="decision-46"></a>Decision 46: Spaces fail closed in native execution

| Field | Value |
|---|---|
| Date | 2026-10-06 |
| Status | Superseded by [decision 47](#decision-47) on 2026-10-07 |
| Amends | [I-13](#i-13), and the code names of item A19 of [decision 34](#decision-34) |
| Related | [Decision 44](#decision-44), [decision 47](#decision-47) |

**Statement.** Native play and native replay both reject the first space operation of a deploy, before any space work.
The error code is `SpacesUnsupportedInNativeExecution`, and it replaces `SpacesUnsupportedInNativeReplay`.
The change touches campaign files only, and it makes no edit in Casper.

**User's words.** "Land Stage 1 now (Recommended)".

**Reason.** Since the merge of [decision 44](#decision-44), proposer self-certification and validator replay run every offered deploy in the native replay environment.
That environment rejects every space operation, while native play ran it.
So the proposer did the play work of a space deploy and then quarantined the deploy.
One rule for play and replay makes the deploy fail in play, and native replay reproduces that failure.

### <a id="decision-47"></a>Decision 47: Full native support for spaces is its own phase

| Field | Value |
|---|---|
| Date | 2026-10-06 |
| Status | In force. Implemented on 2026-10-07. [Decision 48](#decision-48) moved the phase after P5. [Decision 78](#decision-78) adds one test edit to the accounting module. [Decision 89](#decision-89) retires the metered view reads. |
| Amends | [M-3](#m-3), for one accessor. It supersedes [decision 46](#decision-46) and [I-13](#i-13). |
| Related | [Decision 46](#decision-46), [decision 48](#decision-48), [I-11](#i-11), [I-12](#i-12) |

**Statement.** Spaces get full support on the native path in their own phase, after [decision 43](#decision-43).
The phase starts with a design pass, and then it delivers code, formal models, and tests.
It adds one accessor of about 20 lines to `accounting/native_runtime/checked_operations/trace/replay/session/execution.rs`.
That accessor is the only exception to [M-3](#m-3).

**User's words.** "Plan it after decision 43 (Recommended)".

**Reason.** The native path is the default of protocol version 6.
A space backend over the tickets of the native replay session needs the space store of the session, and the session keeps its store private.

**Process.** The user chose "Me + review agent (Recommended)" on 2026-10-07.
The main session refreshed the design, one read-only agent arbitrated it, and the main session checked the findings in code.
The user then approved design v2 ("Approve v2 (Recommended)").

**Implementation (2026-10-07).** Six steps implement design v2:

1. The substrate meters the space view in native execution (commit `9ddcaa127`).
   *Retired by [decision 89](#decision-89): the merge of `4c58a925f` removes the metered view reads, with the H27 headline and the RH-3 tests.*
2. The native replay backend of spaces replays each staged operation against its journal row (commit `9d9f31b7d`).
3. The accessor `space_store`, the bound state of the runtime and the bind call run spaces in native execution (commit `9390408c4`).
4. The integration suite certifies space deploys at the v6 limits (commit `bbe6d03d2`).
5. The formal models add H24 to H27 to `EventCorrelation.v` and the new module `NativeReplay.tla`.
6. The documents record the design, the specification, the verification, and the findings for the cost-accounting owner.

**TLA+ placement.** The user chose "New NativeReplay.tla (Recommended)" on 2026-10-07.
The models of design v2 section 5 are in a new module, so `CheckpointReplay.tla` and its 20 configurations stay unchanged.

**Epoch closure.** A probe of step 4 showed that a forged journal could hang native replay of a space deploy: 64 of 109 forgeries hung.
Commit `c6cb371ba` closes the epoch when a native space operation fails, except after a published denial.
*Retired by [decision 124](#decision-124): no failure closes the epoch now. A waiter of a failed row parks as a counted waiter of the base stall rule.*
The [findings](findings/native-execution-cost-accounting.md) record the same lost wake-up in the host path as F-47-4, with F-47-1 to F-47-3 and the measurement M-47-1.

### <a id="decision-48"></a>Decision 48: The work order after decision 43

| Field | Value |
|---|---|
| Date | 2026-10-06 |
| Status | In force |
| Amends | [Decision 47](#decision-47), its timing |
| Related | [Decision 43](#decision-43), [decision 45](#decision-45) |

**Statement.** The work continues in this order:

1. Commit [decision 43](#decision-43).
2. Implement and commit [decision 45](#decision-45), Rule E and Option P, in the shared agent and substrate.
3. Run P5 with parallel agents on separate files. Each discipline and each outer store gets its own batch, and one owner edits `generic.rs` and the family dispatch.
4. Run [decision 47](#decision-47), the native support, after P5. Until then, spaces fail closed in native execution ([decision 46](#decision-46)).

Each phase runs one full gate set, not each decision.
Before its commit, each decision runs its targeted tests, the formal legs that it changes, the checkers, and the document lint.
It also runs the empty-state-hash regression tests of I-14.

**User's words.** The user asked why one space took so much time, and then chose "Speed up as proposed (Recommended)".

**Reason.** About 184 production lines are specific to `hashmap:queue`: 131 in `queue.rs` and 53 in `hashmap.rs`.
About 12,550 lines are the shared framework that every family reuses.
The framework holds the substrate, the encoding, the agent framework, the factory, the handle, the routing, and the runtime.
So the other families can reuse the framework in parallel batches.

### <a id="decision-49"></a>Decision 49: One fire count for each space transaction

| Field | Value |
|---|---|
| Date | 2026-10-06 |
| Status | In force. Implemented in P5 wave W2 |
| Amends | [I-21](#i-21), its scope: the settles of an operation count on the fire count of its transaction |
| Related | [Decision 43](#decision-43), [decision 45](#decision-45), [decision 48](#decision-48) |

**Statement.**

- One space transaction makes at most `FIRE_ROUND_CAP` fires. The settle before the plan, the plan, the settle after the plan and a peek re-store count on one count.
- `Plan` holds the count, and `Cx::rounds` gives it to every driver (`rholang/src/rust/interpreter/spaces/generic.rs`).
- `open_plan` runs the settle before the plan. When that settle fills the transaction, the operation re-issues before its plan stages anything, with the order rule of [decision 43](#decision-43).
- A second send of a peek that meets a full transaction re-issues before it fires.
- A settle job is a transaction of its own, so it starts at the count 0.
- The step of a `K_lift` is the fire count of the transaction at the lift.

**User's words.** The STACK batch found that the settle before a stack plan, the plan and the settle after it each started a count of their own.
So one transaction could make $`3 \times \mathrm{FIRE\_ROUND\_CAP}`$ fires. Asked how to resolve this against I-21, the user answered "Go with the most principled solution".

**Reason.**

- I-21 bounds the fires of one transaction. Counts of their own gave a bound of three times the cap, which no rule states.
- One count states the bound of I-21 exactly. The shape of each transaction stays deterministic, so play and replay agree.
- A queue, a bag and a cell have no settle, so their transactions and the queue parity golden do not change.

**Evidence.** The test `space_stack_plan_after_a_full_settle_reissues_the_operation` shows the re-issue of a full transaction.
The property `prop_stack_settle_bounded_any_schedule` checks the bound on every schedule. The stack models `StackDiscipline.v` and `StackDiscipline.tla` and the restore check of `GenericRSpace.v` model the count.

### <a id="decision-50"></a>Decision 50: Joined set collections cascade

| Field | Value |
|---|---|
| Date | 2026-10-07 |
| Status | In force. Implemented in P5 wave W3 |
| Amends | The joins plan of [decision 29](#decision-29): its claim that a removal exposes no fire on a collection without a head |
| Related | [Decision 37](#decision-37), [decision 41](#decision-41), [decision 49](#decision-49), [join engine](design.md#n12-wave-w3-of-p5-joins-of-one-space) |

**Statement.**

- The [join engine](glossary.md#join-engine) runs its [cascade](glossary.md#cascade) for a set collection as for a collection of a queue, a stack, a cell or a priority queue.
- The cascade of a set has the parts of every other cascade: the lift and the re-offer, and the retirement of the bound classes.
- It also has the one fire count, the settle before the plan, and the carry at the cap.
- A bag keeps no cascade.
- The driver of one set collection does not change, and [decision 37](#decision-37) and [decision 41](#decision-41) do not change.

**User's words.** The JOINS-FORMAL batch found that a set class can move when a fire removes its first member.
Asked how to resolve this, the user answered "Cascade for sets (Recommended)".

**Reason.**

- A set class sits at the place of its first member in the canonical order of DR-75 ([decision 41](#decision-41)).
- A fire can remove a non-persistent first member while a persistent member of the class stays. The class then takes the place of the persistent member.
- A waiter can bind one set collection two or more times. Such a parked waiter without a guard can then have a match under the greedy search of [decision 37](#decision-37).
- Without a cascade, nothing fires that waiter, and the space keeps a pending match.
- A bag has no classes, and a removal only makes the pool of each bind smaller. So a removal never gives a bag waiter a match that it did not have.

**Evidence.** `set_join_class_shift_breaks_inv_scan` in `JoinCascade.v` proves the pending match of the set without a cascade, and `ref_fill_mono` proves the monotonicity of the bag.
The unit test `space_join_engine_set_cascade_fires_a_waiter_that_a_class_move_exposes` in `rholang/src/rust/interpreter/spaces/join.rs` shows the fire of the exposed waiter.
A set has no counter, so the engine keys each `K_lift` on a set by the source hash of the lifted record.
So each lift of a set has an identity of its own.

### <a id="decision-51"></a>Decision 51: A PathMap bag or set peek bumps the token when its second send changes state

| Field | Value |
|---|---|
| Date | 2026-10-07 |
| Status | In force. Implemented in P5 wave W3 (Phase 2 of the PathMap engine) |
| Amends | F16 of [decision 39](#decision-39) |
| Related | [Decision 45](#decision-45), [decision 31](#decision-31), [M-1](#m-1) |

**Statement.**

- A peek on a PathMap bag or set space bumps the token $`S(\mathit{sid})`$ exactly when its transaction changes state.
- Under Rule E, the peek COMM removes the delivered path marker.
- The second send of the datum is then offered to the waiters of its path and of each prefix of the path.
- When the second send fires a waiter that does not peek, it spends the datum and retires its record and its markers. That transaction bumps the token.
- When no such waiter fires, the second send restores the marker with its identity. That transaction changes no state, and it does not bump the token.

**User's words.** The PATHMAP-ENGINE batch asked how F16 applies under Rule E, and the user answered "Bump on change (Recommended)".

**Reason.**

- F16 predates Rule E, so it assumed that a peek leaves the datum and its markers in place.
- Under Rule E, the second send of a peek can spend the datum and retire markers under prefixes that other operations share.
- The token keeps two deploys of one block that write under one shared index channel in one chain ([M-1](#m-1)). A rejected chain then leaves no orphan marker and no missing marker.
- A peek whose marker returns changes no state, so it needs no bump.
- So concurrent pure reads of a bag or a set stay free of conflicts.

## User decisions 52 to 65

Decisions 53 to 61 come from the P6 design pass of decision 52. The report is the design record of these entries.
Its main terms are these:

- **TL_V** is the fragment of tensor logic that VectorDB spaces implement.
- **TEL** is the tensor equation language, the index notation of the paper, written as text.
- A **correctly rounded** value of a function is the exact real value, rounded to the nearest number with a declared precision.

### <a id="decision-52"></a>Decision 52: Tensor logic and the ChromaDB backend join the VectorDB phase P6

| Field | Value |
|---|---|
| Date | 2026-10-07 |
| Status | In force. Implementation in P6, after [decision 47](#decision-47). [Decision 65](#decision-65) moves the tensor-logic part to P12 |
| Amends | The deferred follow-ups `rrs-fu-tensor-ops` and `rrs-fu-external-backends` (ChromaDB part) of the design ledger |
| Related | [Decision 40](#decision-40), [decision 41](#decision-41), [decision 48](#decision-48), decisions 53 to 61 |

**Statement.**

- P6 implements tensor logic for VectorDB spaces, with a syntax and semantics that the user approved for intuition and familiarity.
- P6 also integrates ChromaDB, the third-party vector database of the node (`rho:chroma:*`).
- ChromaDB mirrors the records of a VectorDB space and answers advisory queries. The native engine decides every match.
- Text embeddings are computed once by the proposer and recorded with the datum. Replay reuses the recorded values.
- The legacy tensor support of the old feature branch is rewritten, not ported.

**User's words.**

- "for the vectordb reified rspace, there is the built-in vectordb and there's integration with a 3rd-party vectordb in f1r3node -- let's include support for the third-party vectordb (I don't recall the name) in the vectordb reified rspace support and tests including syntax and such."
- "I wasn't happy with the legacy support for tensor logic in the old feature branch."
- "if applicable, keep f1r3lang/gslt/oslf integration in mind."
- The user chose "Design pass now (Recommended)", "Mirror + recorded embeddings (Recommended)" and, for the design, "Approve all (Recommended)".

**Reason.**

- The FIPS asks to look into tensor logic for a vector database, and the paper is "Tensor Logic: The Language of AI" (Domingos, arXiv 2510.12269).
- A similarity match inside a COMM changes consensus state, so it must be deterministic. ChromaDB search is approximate and differs between nodes.
- So ChromaDB can only mirror records and give advisory answers, which replay reuses as recorded outputs.

### <a id="decision-53"></a>Decision 53: The tensor-logic fragment TL_V

| Field | Value |
|---|---|
| Date | 2026-10-07 |
| Status | Moved to P12 by [decision 65](#decision-65), after the F1r3lang readiness signal |
| Amends | none |
| Related | [Decision 52](#decision-52), [decision 37](#decision-37), [decision 29](#decision-29) |

**Statement.**

- TL-0: one tensor equation scores each record of one VectorDB collection. A selection projection (top-k, threshold or radius) picks the records of the COMM.
- TL-1: pure tensor programs run through `rho:lang:vector` (`eval`). They have no space effect.
- TL-2: a Boolean rule runs as a join within one space, or as a set-at-a-time program over an atomic snapshot of its relations.
- TL-3: embedding-space reasoning (superposition, embedded relations, similarity and a temperature) is built from TL-0 and TL-1.
- The inner collection picks the semiring of a rule: a set gives the step function, and a bag counts derivations.
- Excluded: learning and gradients, Tucker decomposition and GPU scaling, sampling, virtual indices, file tensors, and recursion inside one COMM.

**Reason.**

- A COMM must be atomic, bounded and deterministic. One non-recursive equation for each receive is all three.
- Each exclusion needs state between COMMs, randomness, input and output, or unbounded work, or it is a performance technique.
- A join rule is complete only under the F1r3lang guard rule of [decision 37](#decision-37), so TL-2 joins are complete at P12.

### <a id="decision-54"></a>Decision 54: The number model of tensor logic

| Field | Value |
|---|---|
| Date | 2026-10-07 |
| Status | In force for the exact decisions of P6. The functions that only tensor logic uses move to P12 ([decision 65](#decision-65)) |
| Amends | The wording of ledger item D-5 (exact mode) |
| Related | [I-9](#i-9), [decision 40](#decision-40) |

**Statement.**

- Every consensus decision of a VectorDB space uses exact rational arithmetic.
- Every supported number enters exactly. A float becomes its exact dyadic value, and NaN or an infinity fails with `NonFiniteValue`.
- A dot product of floats uses an exact long accumulator, and a cosine comparison uses exact squared forms.
- `exp`, `log`, `sqrt`, `tanh`, `cos`, `dist`, `sig` and `softmax` are correctly rounded at a declared precision (default 64 bits, from 8 to 256).
- A metered Ziv loop computes these values. A working width above $`4p+128`$ bits fails with `PrecisionExhausted`.
- Exact substitutes (`hardsig`, `relu`, `step` and others) stay available.
- Kernels are sequential slice kernels, with no ndarray ([I-9](#i-9)).

**Reason.**

- Exact arithmetic gives the same decision on every node.
- A correctly rounded value is deterministic by definition and monotone, and its implementation can change without a fork.
- The loop terminates, because these functions give irrational values at rational inputs other than the exact cases.

### <a id="decision-55"></a>Decision 55: The tensor-logic syntax

| Field | Value |
|---|---|
| Date | 2026-10-07 |
| Status | Superseded by [decision 62](#decision-62) on 2026-10-07 |
| Amends | none |
| Related | [Decision 18](#decision-18), [decision 21](#decision-21), [decision 24](#decision-24), [decision 17](#decision-17) |

**Statement.**

- A score query writes its equation in TEL, as the `score` param of the existing query form `~ (payload; params)`.
- `tensors` binds other names in an equation to values.
- Pure programs use the same language through `rho:lang:vector`, for example `vec!eval("B[i] maj= V[n, i]", {"V": vs})`.
- The Rholang grammar does not change, and no new marker tuple is needed.
- A literal equation is checked at normalization, with the offset of each error. A computed equation is checked at run time.
- At P12, TEL becomes an installed F1r3lang language with a typed form that lowers to the same syntax tree.

**Reason.**

- The approved query form keeps atomic and deterministic selection in the space agent.
- One language serves queries and programs. It follows the paper, and it contains einsum.
- An unchanged parser keeps decisions 23 and 24 and the dual-frontend corpus intact.
- The other options were index notation in the grammar, einsum signatures, Datalog strings, and capability methods only. The design report compares them on seven use cases.

### <a id="decision-56"></a>Decision 56: The VectorDB vocabulary

| Field | Value |
|---|---|
| Date | 2026-10-07 |
| Status | In force, amended by [decision 62](#decision-62): no `score` and no `tensors`, and by [decision 66](#decision-66): no `precision` in P6 |
| Amends | The query-param table of the [syntax contract](syntax-contract.md) |
| Related | [Decision 21](#decision-21), [decision 55](#decision-55) |

**Statement.**

- Query keys: `score`, `tensors`, `radius`, `scores`, `ids` and `precision`.
- Send key: `embedding`.
- Factory keys: `dimensions` (required, 1 to 4,096), `capacity` (required, 1 to 1,000,000), `precision` and `mirror`.
- Exclusive pairs: `metric` with `score`, and `threshold` with `radius`. They join the pair of `k` and `results`.
- `threshold` keeps its range from 0 to 1, except when the query has `score`. `threshold` also accepts fixed-point literals.

**Reason.** Each key serves one use case of the design, and each bound limits state or expression size, not a scan ([decision 40](#decision-40)).

### <a id="decision-57"></a>Decision 57: The recorded processes `embed` and `ann`

| Field | Value |
|---|---|
| Date | 2026-10-07 |
| Status | In force. Implemented in P6. The clause "both fail in native execution" is superseded by [decision 68](#decision-68) on 2026-10-08. Decisions [105](#decision-105), [113](#decision-113) and [114](#decision-114) amend its replies and its builds on 2026-10-27 |
| Amends | The op table of the namespace (two new ops) |
| Related | [Decision 52](#decision-52), [decision 46](#decision-46) |

**Statement.**

- `rho:vectordb:embed` uses the op `[0xE0, 0x24]` and the body reference 236. It replies `[true, vector, model]` or `[false, code, detail]`.
  *Changed by [decision 105](#decision-105): a failure reply is `[false, code, message]`, with a code and a fixed message of DR-114.*
- `rho:vectordb:ann` uses the op `[0xE0, 0x25]` and the body reference 237. It replies `[true, ids, distances, watermark]` or `[false, code, detail]`.
  *Changed by [decision 114](#decision-114) and [decision 105](#decision-105): the reply is `[true, ids, distances]`, with no watermark, and a failure reply uses a code of DR-114.*
- Both processes are in `non_deterministic_ops()`, so replay reuses their recorded outputs.
- Both exist in every build, and both fail in native execution.
  *Replaced by [decision 113](#decision-113): both exist only in a `chromadb` build. [Decision 68](#decision-68) superseded the native clause.*
- Every new reply has the reply channel first and the shape `[ok, ...]`, so `!?` and `try` apply.
- The marker range `0xD5` to `0xDF` is reserved for future syntax markers.

**Reason.**

- A recorded output gives replay the proposer's answer. A process that exists in every build lets every validator replay that answer.
  *[Decision 113](#decision-113) replaces the clause on every build, because the base registers its Chroma processes only with their feature.*
- `ann` returns ids, never payloads. Only the exact query param `ids=` can turn them into a decision, and the exact order still decides.

### <a id="decision-58"></a>Decision 58: The ChromaDB mirror is a hint index

| Field | Value |
|---|---|
| Date | 2026-10-07 |
| Status | In force. Implementation in P6. Decisions [98](#decision-98), [105](#decision-105) and [114](#decision-114) replace its publication, its failure code and its reconciler |
| Amends | none |
| Related | [Decision 52](#decision-52), [decision 45](#decision-45) |

**Statement.**

- The mirror of a VectorDB space is node-local and is never consensus state.
- The factory param `mirror="chroma"` turns it on. Temp and seq spaces cannot have a mirror.
- At the end of a successful deploy, the node publishes the writes of the deploy to the mirror. A failed deploy publishes nothing.
  *Replaced by [decision 98](#decision-98): a play publishes the writes of each commit at the commit, whatever the result of the deploy. Replay never publishes.*
- A peek re-store of Rule E writes nothing to the mirror.
- A reconciler reads the finalized state and repairs the mirror. It removes orphan records and adds missing ones.
  *Retired by [decision 114](#decision-114): nothing repairs the mirror, as in the oracle mode of File I/O.*
- When ChromaDB is unavailable, writes wait in a bounded buffer, and a deploy never waits. `ann` then replies `MirrorUnavailable`, and replay reuses that reply.
  *Changed by [decision 105](#decision-105): `ann` replies `EXT_FAILED` in place of `MirrorUnavailable`, and replay delivers that record. [Decision 114](#decision-114) drops a batch that finds the queue full.*

**Reason.** The native engine decides every match, so a stale mirror can only give weaker advice. It can never change consensus state.

### <a id="decision-59"></a>Decision 59: The test policy for live services

| Field | Value |
|---|---|
| Date | 2026-10-07 |
| Status | In force. Implementation in P6 |
| Amends | none |
| Related | [Decision 58](#decision-58) |

**Statement.**

- Sandbox tests use a fake ChromaDB backend with programmed faults, and a fake embedding model.
- A host-side live leg runs against a pinned ChromaDB image.
- When `RRS_LIVE_CHROMA=1` is set, an unreachable server fails the live leg. It never skips silently.

**Reason.** The verification sandbox has no network and no Docker. A silent skip would hide a broken integration, as the existing ChromaDB tests can.

### <a id="decision-60"></a>Decision 60: The VectorDB tie-break follows decision 41

| Field | Value |
|---|---|
| Date | 2026-10-07 |
| Status | Superseded by [decision 102](#decision-102) on 2026-10-27 |
| Amends | The VectorDB row of the discipline table ("Blake2b256(record), then full bytes") |
| Related | [Decision 41](#decision-41) |

**Statement.** Two VectorDB records with equal scores are ordered by the hash of their source produce, then by their full digest.

*Superseded by [decision 102](#decision-102) on 2026-10-27: play picks at random among records with equal scores, and replay follows the recorded pick.*

**Reason.** Bag and set spaces already use this order ([decision 41](#decision-41)), so one order serves every unordered discipline.

### <a id="decision-61"></a>Decision 61: Footprint exceptions for the ChromaDB mirror

| Field | Value |
|---|---|
| Date | 2026-10-07 |
| Status | In force. Implementation in P6. [Decision 114](#decision-114) retires its reconciler spawn, and [decision 109](#decision-109) amends the environment of the nodes |
| Amends | The plan statement "node: no change expected" |
| Related | [Decision 58](#decision-58) |

**Statement.**

- `node/src/rust/runtime/setup.rs` gains one task spawn for the mirror reconciler.
  *Retired by [decision 114](#decision-114): nothing repairs the mirror, so the spawn stays as a comment with a restore note.*
- `docker/shard.yml` pins the ChromaDB image by digest and passes its environment to the nodes.
  *Amended by [decision 109](#decision-109): the read-only node keeps no mirror database, because an observer installs the unavailable services.*

**Reason.** The reconciler runs on finalized blocks, and the node starts the existing tasks of that kind. A pinned image makes the live leg reproducible.

**Findings for other owners.** The design found ten defects in the existing `rho:chroma:*` integration (C-1 to C-10 of the design report).
The campaign records them as findings for their owner.

### <a id="decision-62"></a>Decision 62: Tensor-logic equations are FLT terms of a TensorLogic GSLT

| Field | Value |
|---|---|
| Date | 2026-10-07 |
| Status | In force. Implementation at P12, after the F1r3lang readiness signal |
| Amends | [Decision 55](#decision-55), the keys `score` and `tensors` of [decision 56](#decision-56), and the syntax choice of [decision 52](#decision-52) |
| Related | [Decision 17](#decision-17), [decision 18](#decision-18), decisions 63 to 65 |

**Statement.**

- A tensor-logic equation is a foreign-language term (FLT) of a TensorLogic GSLT of F1r3lang.
- P6 has no string syntax for equations: no `score` or `tensors` string params, and no string `eval` of `rho:lang:vector`.
- The GSLT gives the syntax and the typing. An allow-listed host capability evaluates the reflected term natively ([decision 17](#decision-17)).
- The FLT surface `tl:Category` lands at P12, after the readiness signal SIG-F, and it lowers to one frontend-neutral syntax tree.

**User's words.**

- "For the tensor logic syntax, wouldn't it be cleaner to go with a foreign language terms of a TensorLogic GSLT from F1r3lang? Would that be feasible or infeasible? The string expression syntax is going to be problematic."
- The user chose "FLT only, gated on SIG-F (Recommended)".

**Reason.**

- A string syntax needs its own lexer, its own error offsets and its own escapes, and P12 would then have to migrate it.
- The FLT lexer modes, the language installation and the reflection ABI exist only in F1r3lang. In the repository `mettail-rust`, they are lines 647 to 669 of `languages/src/rholang.rs` and the file `rholang-codegen/src/rho_net_lower.rs`. So the FLT form waits for the F1r3lang merge.
- The installed-theory intrinsics of F1r3lang have no rational or tensor arithmetic, so evaluation stays native.
- Rejected alternatives: an interim FLT bridge in the tree-sitter grammar, the string syntax of decision 55, and the F1r3lang merge first.

### <a id="decision-63"></a>Decision 63: The TensorLogic GSLT has an index-notation core and familiar sugar

| Field | Value |
|---|---|
| Date | 2026-10-07 |
| Status | In force. Implementation at P12 |
| Amends | none |
| Related | [Decision 62](#decision-62), [decision 64](#decision-64) |

**Statement.**

- The core is the index notation of the paper, a superset of einsum. Named indices are the arguments of a relation, so a product is a join and a sum is a projection.
- A specified numpy-like subset is sugar:
  - `@`, and elementwise arithmetic with the broadcasting rules of numpy
  - `.T`, `transpose(axes)`, `sum` and `max` with `axis=`, and `einsum`
  - `exp`, `sigmoid`, `relu` and `softmax(axis=)`
  - slices with constant bounds
- The GSLT defines each sugar form as a derived constructor with a rewrite into the core. Rocq proves that each rewrite keeps the denotation, so equal computations normalize to one core term.
- The documented differences from numpy are exact arithmetic, transcendental functions at a declared precision, no in-place change, and no shape that depends on data.

**User's words.**

- "Would it be good to support a numpy-like syntax for the TensorLogic GSLT?"
- The user chose "Index core + numpy sugar (Recommended)".

**Reason.**

- The index notation keeps the logic reading of an equation, and the sugar makes common programs familiar.
- One normal form keeps the reflected `Par` and the content identity of a theory canonical.
- Static shapes keep the static demand bound and the metering of [decision 40](#decision-40).

### <a id="decision-64"></a>Decision 64: The TensorLogic surface is paper-first, functional and Rholang-flavored

| Field | Value |
|---|---|
| Date | 2026-10-07 |
| Status | In force. Implementation at P12. The full grammar needs the approval of the user before code |
| Amends | The sugar of [decision 63](#decision-63) (refined) |
| Related | [Decision 62](#decision-62), [decision 65](#decision-65) |

**Statement.**

- The core follows section 3.1 of the paper:
  - equations such as `Y[i] = step(W[i] X[i])`, with joins by juxtaposition
  - the projections `max=` and `avg=`, element lists, slices and index arithmetic
  - the normalized index `p'.`, the Datalog forms, and the query `Y?`
- Primes are ASCII single quotes (`n'`, `M''`). The Unicode primes in user names normalize to them.
- `let` defines pure, non-recursive functions that the GSLT inlines. Recursion stays in equations over indices.
- `|` is the parallel composition of Rholang: `E1 | E2` means that both equations hold, and equations with one left side accumulate, as the paper states.
- Built-in operators, symbols, functions and methods are pure ASCII. User functions and operators may have Unicode names under UAX #31 and NFC, with the confusable checks of UTS #39.

**Correction (verified on 2026-10-07).** A construction hole of F1r3lang must be bound by an enclosing FLT pattern: lines 4720 to 4745 of `rholang-runtime/src/rholang_ast.rs` in the repository `mettail-rust`.
So tensor data reaches a term through the space: the agent binds reserved names, and user weights come from a named-tensor map param.

**User's words.**

- "For built-in operators, symbols, and methods we should use pure ASCII, but we should allow users to define unicode symbols and such for custom functions and things."
- "maybe something more functionally inspired and more aligned with Rholang and TensorLogic than numpy, but still with familiar syntax?"
- "I don't mean backticks, I mean single quotes for prime like you suggested"
- The user chose "Record it (Recommended)".

**Reason.** The paper is the authority for the semantics, and a functional, Rholang-flavored surface fits the host language. Pure functions and static shapes keep evaluation bounded.

### <a id="decision-65"></a>Decision 65: The TensorLogic language waits for F1r3lang and does not block VectorDB

| Field | Value |
|---|---|
| Date | 2026-10-07 |
| Status | In force |
| Amends | The P6 scope of [decision 52](#decision-52) and [decision 53](#decision-53) |
| Related | [Decision 62](#decision-62), [decision 17](#decision-17) |

**Statement.**

- The TensorLogic GSLT, its FLT surface, and the parts that serve only the language move to P12. P12 starts after the readiness signal SIG-F, with the OSLF-related work.
- These parts are the TL evaluator, its frontend-neutral syntax tree, `TensorLogic.v`, `TensorGSLT.v`, and the TL legs of Sage and Z3.
- P6 is the VectorDB space and the ChromaDB integration:
  - the built-in metrics with `k`, `threshold` and `radius`, and exact arithmetic
  - the vocabulary without the TL keys
  - `embed` and `ann`, the mirror, and the test policy
- The tracker moves the TL leaves of decision 52 to P12 before any P6 code.

**User's words.**

- "Also, the TensorLogic syntax will need to wait for the F1r3lang merge into this branch just like the OSLF-related Reified RSpace"
- "It should not block the VectorDB Reified RSpace work"

**Reason.**

- Code with no syntax to reach it would be dead until P12.
- The FIPS VectorDB requirement needs no tensor logic (`Reifying RSpaces.md:255-257`). The FIPS tensor-logic item is exploratory (`:259`), and the GSLT with FLT integration delivers it at P12.
- A query param is a `Par` ([decision 21](#decision-21)), and an FLT term reflects to a `Par`. So the P12 key for an equation is an additive change.

## User decisions 66 to 71

Decisions 66 to 71 come from phase P6 after the design refresh of decision 65.
The design record of decision 66 is design v3 (`.reified-rspaces-work/p6/design-v3.md`), with its arbitration.
Decisions 67 to 71 come from the implementation steps of P6.
Their main terms are these:

- **List mode** is the mode of a receive with the query param `k` or `results = "all"`. Its one bind binds the list of the first eligible records.
- **LIST-CASCADE** is the step that checks the parked list waiters again after a record leaves a collection, and fires the first one that can fire.
- The **mirror** of a VectorDB space is a copy of its records in ChromaDB. It is local to one node and never enters a state root.
- A **recorded process** is a system process whose reply the dispatcher records on the produce event of the reply, so replay produces the recorded reply.

### <a id="decision-66"></a>Decision 66: The answers of the P6 design refresh

| Field | Value |
|---|---|
| Date | 2026-10-08 |
| Status | In force. The native-execution answer is superseded by decision 68 on 2026-10-08. The reconciler sentence is corrected by decision 70 on 2026-10-08. [Decision 98](#decision-98) replaces the answer M1, and [decision 114](#decision-114) retires the reconciler answer, on 2026-10-27. |
| Amends | [Decision 56](#decision-56) (`precision` leaves P6), [decision 58](#decision-58) (the publication point) |
| Related | [Decision 37](#decision-37), [decision 45](#decision-45), [decision 49](#decision-49), [decision 50](#decision-50), [decision 57](#decision-57) |

**Statement.**

- **List mode parks and cascades (LM-b).**
  - A receive in list mode that does not fire parks on `[D]`, with its query in the policy slot of its envelope.
  - A later send that the list of the waiter takes fires the waiter over `[D]`, with the incoming datum.
  - When the waiter does not peek, the other non-persistent members of the list leave through `K_retire` COMMs in the same transaction.
  - After each removal, LIST-CASCADE checks the parked list waiters again, in the driver and in the join engine. It counts its fires with the fire count of [decision 49](#decision-49), and settle jobs carry the rest at the round cap.
- **The payload is the vector by default.**
  - A send without `embedding=` whose payload is one numeric vector uses that vector as its embedding.
  - A numeric list of the wrong length fails with `DimensionMismatch`.
  - Any other payload has no embedding, so no query with a vector selects it. `embedding=` overrides the payload.
- **`precision` leaves P6.** Delivered values are `f64` values, the rounding to nearest of the exact value. `precision` returns with tensor logic in P12.
- **`threshold` keeps its range.** The range is $`[0, 1]`$ for every similarity metric, `dot` included.
- **M1: the mirror journal settles with the final result.**
  - A deploy publishes its mirror journal only when its final evaluation result has no errors.
  - The two settle points are the result of `interpreter.rs`, after the host-work check, and the native path of `rho_runtime.rs`.
  - The `p6` footprint policy records these hot-file lines.
  - *Replaced by [decision 98](#decision-98): a play publishes the net writes of each commit at the commit. So the journal and its two settle points are retired.*
- **The reconciler repairs the journaled channels.**
  - The reconciler repairs the channels that the journal of the node touched. The collection metadata keeps them.
  - It repairs them to the view of the last finalized block.
  - After an LFS bootstrap, the mirror lacks the records that are older than the bootstrap, until they change again.
  - *Retired by [decision 114](#decision-114): nothing repairs the mirror. After an LFS bootstrap, the mirror holds no record of the imported state.*
- **`embed` and `ann` in native execution.** This answer kept the native ban of [decision 57](#decision-57), with a finding for the cost-accounting owner. [Decision 68](#decision-68) supersedes it.

**User's words.**

- "LM-b: park + cascade (Recommended)"
- "Payload is the vector (Recommended)"
- "Drop from P6 (Recommended)"
- "M1: final result (Recommended)"
- "Journaled channels (Recommended)"
- "Keep + file finding (Recommended)"

**Reason.**

- The rules LM-a and LM-c leave a waiter without a guard that could fire, and no step fires it. The counterexamples CE-1 and CE-2 of the arbitration show this.
- The rule LM-d never parks: a list receive with no list fails with a typed error, and list mode rejects `<=`. That rule changes the approved semantics of a receive.
- The FIPS sends a vector to the space, and a `for` consumes one vector (`Reifying RSpaces.md:255-257`). The design ledger desugars the default embedding as the payload.
- Without tensor logic, no value of the P6 vocabulary needs a declared precision.
- M1 makes the rule of [decision 58](#decision-58) hold as written: a failed deploy publishes nothing.
- The node knows the channels that its own journal touched, so a pass reads only those channels.

### <a id="decision-67"></a>Decision 67: The checks of an `ann` call run in play and in replay

| Field | Value |
|---|---|
| Date | 2026-10-08 |
| Status | Superseded by [decision 104](#decision-104) on 2026-10-27 |
| Amends | Design v3 section 4.9 ("replay returns the recorded output first") and arbitration amendment A8 with its note G9 |
| Related | [Decision 57](#decision-57), [decision 69](#decision-69) |

**Statement.**

- Play and replay both make the checks of an `ann` call against consensus state, in this order:
  1. The space is a native VectorDB space (`NotAVectorDbSpace`), with a mirror (`MirrorDisabled`).
  2. The query is a text, or a numeric vector of the dimensions of the space whose elements are finite as `f32` values (`DimensionMismatch`, `BadArgument`).
  3. The options are a map of the one key `k`, an `Int` in $`[1, \mathit{capacity}]`$ (`InvalidOption`).
- The config read and the decode of a numeric query reserve host work on the budget of the deploy, in both modes.
- Only the answers of the services are recorded: the embedding of a text query, the neighbours, and the watermark. Replay never asks a service.
- `embed` reads no consensus state, so it has no such checks.
- In native execution, each check is a journaled native operation. A native replay that skips a check fails its certification with an incomplete journal. The native replay tests of the suite therefore cover this rule ([decision 69](#decision-69)).

*Superseded by [decision 104](#decision-104) on 2026-10-27: play makes the checks of the call. Replay delivers the record with no check of the call, as the base driver does (DR-114). The reason of this decision, equal host work, lapsed with DR-131 ([decision 89](#decision-89)).*

**User's words.** "Checks in both modes (Recommended)"

**Reason.** Play read the config through a read-only space job, which reserves host work. Replay returned the recorded output first. So one call made play reserve 1,815,346 VerificationBytes, 11,909 VerificationOperations and 59,288 SearchStateBytes more than replay. That gap is a divergence of play and replay as soon as a deploy carries a budget.

### <a id="decision-68"></a>Decision 68: `embed` and `ann` leave the native ban

| Field | Value |
|---|---|
| Date | 2026-10-08 |
| Status | In force. [Decision 89](#decision-89) does its last clause. |
| Amends | [Decision 57](#decision-57) (the clause "both fail in native execution"), the native answer of [decision 66](#decision-66) |
| Related | [Decision 69](#decision-69) |

**Statement.**

- The body references 236 (`embed`) and 237 (`ann`) stay in `non_deterministic_ops()` and leave the `forbidden` list of `to_dispatch_table`.
- Both processes run in native execution as recorded processes. The hot-file edit of `interpreter/system_processes.rs` is two lines.
- The cost-accounting owner removes the whole `forbidden` list.
- When the record-and-replay mechanism for external services reaches this branch, both processes move to it. The campaign does not implement that mechanism.
  *Done by [decision 89](#decision-89): DR-114 reached the branch with the merge of `4c58a925f`, and both processes moved to it.*

**User's words.**

- "I am having the responsible agent remove the forbidden list altogether, it was added by the implementing agent to cut corners and will cause some real problems"
- "remove embed and ann from it as the list will be going away"
- "External service calls will use a record-and-replay mechanism akin to the File I/O FIPS as their replay mechanism, that is being implemented by the responsible agent now."

**Reason.** Native replay already reproduces a recorded reply. A ban would make the VectorDB backend unusable in the execution model that the campaign targets.

### <a id="decision-69"></a>Decision 69: Only the latest execution model

| Field | Value |
|---|---|
| Date | 2026-10-08 |
| Status | In force |
| Amends | [Decision 45](#decision-45) (Option P becomes transitional), and the test parts of [decision 47](#decision-47), [decision 67](#decision-67) and [decision 68](#decision-68) that name legacy execution |
| Related | Follow-up `rrs-fu-retire-legacy-model` (pgmcp 10934) |

**Statement.**

- All new and changed P6 code, tests and documents target only native funded execution (protocol v6), with native play and native replay certification.
- The legacy-only parts of the campaign stay until the merge of the latest feature/cost-accounted-rho removes the legacy model from this tree. These parts are Option P of [decision 45](#decision-45), `LegacyCharges.v` and the legacy-mode suites. That merge removes them.

**User's words.**

- "I'm not a fan of supporting both a legacy and nouveau model of execution, the latest changes to feature/cost-accounted-rho enforce just the latest model"
- "Latest model from now (Recommended)"

**Reason.** Two execution models double the tests and the proofs of each behavior, and the platform retires the legacy model.

### <a id="decision-70"></a>Decision 70: Reified RSpaces only

| Field | Value |
|---|---|
| Date | 2026-10-08 |
| Status | In force. [Decision 114](#decision-114) retires the reconciler of its example |
| Amends | The reconciler sentence of [decision 66](#decision-66), the N4 leaf of the tracker |
| Related | [Decision 12](#decision-12), [decision 18](#decision-18) |

**Statement.**

- No design plans a Casper change that is not directly Reified work.
- The campaign opens no question of finality, fork choice or the block lifecycle.
- It uses Casper state only through read-only interfaces where an approved design needs it. For example, the reconciler reads the finalized state through `RuntimeManager`.
  *Changed by [decision 114](#decision-114): the reconciler is retired, so this example no longer applies.*
- The campaign does not hunt, analyze, file or fix latent defects of other owners, unless they block the Reified work. The findings that it filed before stay as records.

**User's words.**

- "This does not seem directly related to Reified RSpaces, it is a Casper decision. I have emphasized before and I will do so again, stay in your own lane!"
- "This is not a Casper refinement campaign, this is a Reified RSpaces campaign"
- "This is also not a general purpose bug hunting safari"

**Reason.** The scope of the campaign is the Reifying RSpaces FIPS. Other owners maintain Casper and the other subsystems.

### <a id="decision-71"></a>Decision 71: `capacity` is a soft bound under merges

| Field | Value |
|---|---|
| Date | 2026-10-08 |
| Status | In force |
| Amends | none |
| Related | [M-1](#m-1), [decision 40](#decision-40) |

**Statement.**

- A VectorDB collection has no counter, and a store checks `capacity` against the view of its own transaction.
- A merge of concurrent branches can exceed `capacity` by at most the stores of the merged branches.
- On such a merged state, every store fails with `CapacityExceeded` until removals bring the count below `capacity`.
- Consensus stays deterministic, and the work of each scan stays metered ([decision 40](#decision-40)).

**User's words.** "Soft bound, documented (Recommended)"

**Reason.**

- In native execution, with `capacity = 2` and one stored record, two concurrent branches that each add a record merge with no conflict. The merged collection holds 3 records.
- An exact bound needs a counter bump on each store, and then all VectorDB writes into one collection serialize across branches. The plan keeps full merge concurrency for bag, set and VectorDB spaces.

## User decisions 72 to 84

### <a id="decision-72"></a>Decision 72: The P7 design process

| Field | Value |
|---|---|
| Date | 2026-10-09 |
| Status | In force |
| Amends | none |
| Related | [Decision 47](#decision-47), [decision 65](#decision-65) |

**Statement.**

- The main session drafts the P7 design against the current code, as for decisions 47 and 65.
- One fresh read-only Plan agent arbitrates the draft.
- The main session verifies the findings in code and presents the result to the user before any P7 code.

**User's words.** "Me + review agent (Recommended)"

**Reason.** P7 changes consensus-visible behavior (deploy-scoped spaces, Seq rules, theory admission), so an independent arbitration precedes the code.

### <a id="decision-73"></a>Decision 73: P7 design v2, Seq claims per channel, HashSet on units

| Field | Value |
|---|---|
| Date | 2026-10-09 |
| Status | In force. Implemented in P7 steps 2 (the Seq claims) and 3 (the HashSet store). [Decision 91](#decision-91) amends the basis of its Seq claims, and [decision 90](#decision-90) retires its clause on legacy execution |
| Amends | The plan sentence "the atomic Seq guard enforces exclusive use" (exclusive use holds for each channel) |
| Related | [I-4](#i-4), [decision 39](#decision-39) (F1, F17), [decision 47](#decision-47), [decision 69](#decision-69) |

**Statement.**

- The user approves P7 design v2, which applies the 26 amendments of the arbitration.
- Each channel of a seq space has its own causal-lineage claim. A use of a channel must descend from the previous use of that channel, else the deploy aborts with `SeqConcurrent`.
  *Amended by [decision 91](#decision-91): the causal history of the use decides the descent. A use descends from the previous use when that use is in its history (`SeqHistory`).*
- Operations that touch no channel of the space (`gensym`, `config`, `qualifier`, allocations) and internal jobs (re-issues, settle jobs) are not checked.
- The seq qualifier, and so every HashSet family, runs only in native execution. Legacy execution keeps `QualifierUnavailable` for it.
  *Amended by [decision 90](#decision-90) (C6, step D): spaces run only in native execution, so a factory call outside it fails with `RuntimeUnavailable` before every gate. Gate 3 reads no execution model, and `QualifierUnavailable` is retired.*
- The HashSet store is the default space restricted to empty messages. A send with data aborts with `PayloadNotEmpty`, and a receive with a binder aborts with `PatternNotEmpty`.

**User's words.**

- The design: "Approve v2 (Recommended)".
- The Seq rule, after the request "Elaborate on each of these options, what they imply, their pros and cons, their consistency with the FIPS, and their consistency with the existing behavior of the default bag-based RSpace. Then ask this question again.": "Per-channel claim (Recommended)".
- The HashSet store, after the request "Elaborate on the context and background of the question. Most likely, we should model this behavior after the existing behavior of the default bag-based RSpace. Ask the question again after giving me enough information to make an educated decision.": "Bag semantics on units (Recommended)".

**Reason.**

- The FIPS restricts each seq channel ("you cannot use it in a concurrent process"), and the channels of the default space are independent.
- Legacy host operations bypass the scheduler, so a causal-lineage claim is deterministic only in native execution.
  *Amended by [decision 91](#decision-91): DR-132 of the base removes the scheduler and its participant paths. A causal history depends only on the program and its recorded matches, so the claim is deterministic under racing evaluation.*
- An empty pattern already matches exactly an empty message in the default space, so the HashSet store needs no change to matching.

**Implementation.** P7 step 3 records the two codes of HS-8 exactly as design v3 states them ([specification 2.7](specification.md#27-hashset)):

- `PayloadNotEmpty`: a send with any value to a HashSet collection, `x!(Nil)` included (HS-2).
- `PatternNotEmpty`: a bind on a HashSet collection with a pattern, a binder or a remainder (HS-3). A wildcard or a literal pattern is a pattern too, because the FIPS allows only the empty pattern.

Both checks run before any staging, in the driver and in the join engine.

### <a id="decision-74"></a>Decision 74: The config persists the theory identity as compact bytes

| Field | Value |
|---|---|
| Date | 2026-10-09 |
| Status | In force. Implemented in P7 step 4 |
| Amends | TH-2 of P7 design v3 ("The config slot `"theory"` holds `Nil` or this map") |
| Related | [Decision 9](#decision-9) (item 1), [decision 40](#decision-40), [decision 47](#decision-47), [I-17](#i-17), [theory identity](glossary.md#theory-identity) |

**Statement.**

- The config slot `"theory"` holds `Nil` or one `GByteArray` with the canonical bytes of the identity of the theory.
- The bytes hold the eight fields of the identity in their order of declaration. A number is big endian, and a string or an arg comes after its length as a `u32`.
- The decoder accepts exactly the image of the encoder. A decoded identity encodes to its input bytes.
- The `config` reply of a handle expands the bytes to the identity map of TH-2, so a program sees the same map as before.
- The identity stays content only, in the one config record, with no new record kind.

**User's words.** "Compact bytes (Recommended)".

**Reason.**

- Every space operation reads the config through the strict metered decoder.
- The clone-backing cost model charges about 600 `VerificationBytes` for each bincode byte of that read.
- The TH-2 map has eight string keys and a nested limits map, which is about 22 `Par` nodes. One config read of a space with a builtin theory cost 2,495,724 `VerificationBytes` instead of 927,045: 169 % more on every operation.
- With that cost, many family programs with `free Nat()` exceeded the v6 replay limit of $`2^{28}`$ `VerificationBytes`.
- A slot of one `Par` costs about 47,600 `VerificationBytes` more for each read, which is 5 %.

**Evidence.** `theory_config_slot_is_one_compact_par` pins the slot form and bounds the cost of the read to a tenth more. `identity_bytes_roundtrip` (`TheoryAdmission.v`) proves the codec, and `prop_theory_identity_bytes_roundtrip` tests it.

### <a id="decision-75"></a>Decision 75: One pgmcp freeze at P11

| Field | Value |
|---|---|
| Date | 2026-10-09 |
| Status | Superseded by [decision 137](#decision-137) on 2026-11-10 |
| Amends | [Decision 35](#decision-35) (the shared freeze of P1 and P4), and the per-leaf verification of the plan's execution protocol |
| Related | [Decision 13](#decision-13), [decision 48](#decision-48), [verification.md](verification.md) |

**Statement.**

- No recipe registration, VPI-1 relay, AAI-1 amendment or verification run happens before phase P11.
- The campaign continues with P8 at once.
- At P11 one freeze registers the recipes and shows the user the exact recipe command list (VPI-1).
- The same freeze amends the acceptance criteria (AAI-1) and runs the verifier for every active leaf, together with the final gates.
- Until then, each phase records its evidence in pgmcp progress notes and in the dossier.

*Superseded by [decision 137](#decision-137): no freeze registers the recipes, and pgmcp runs no leaf. The final gate set runs on a development host, and the leaves close at claimed_done.*

**User's words.** "One freeze at P11 (Recommended)", after P7 step 6.

**Reason.**

- The recipes pin exact test names and pass counts.
- P8 to P10 add tests and change those counts, so an earlier freeze needs new amendments after each phase.
- Decision 35 moved the verification of P1 to a later freeze for the same reason.

### <a id="decision-76"></a>Decision 76: The P8 design process

| Field | Value |
|---|---|
| Date | 2026-10-09 |
| Status | In force |
| Related | [Decision 47](#decision-47), [decision 65](#decision-65), [decision 72](#decision-72), [I-15](#i-15), [I-16](#i-16), [decision 32](#decision-32) |

**Statement.**

- The main session drafts the design of P8 against the current code.
- P8 covers the spaces that user agents implement. Its parts are the translation of `new`, of a send and of a receive into calls of the agent, and agent-tagged names.
- Its other parts are `OP_MATCHER`, the body capability, `OP_SINK`, the reference agent and the conformance kit.
- The design also settles the open P8 item of decision 32: the URN binding of `MATCHER_OP` and `SINK_OP`.
- One fresh read-only Plan agent arbitrates the draft.
- The main session verifies its findings in code and presents the result to the user before any P8 code.

**User's words.** "Me + review agent (Recommended)".

**Reason.** The same process produced the approved designs of decision 47, P6 and P7.

### <a id="decision-77"></a>Decision 77: P8 design v2, the URNs of the internal ops, and the space-view clause

| Field | Value |
|---|---|
| Date | 2026-10-10 |
| Status | In force |
| Amends | [Decision 32](#decision-32) (its open P8 item), the plan sentence that the internal ops are "installed but never bound to a URN", and divergence 5 of the [design ledger](design-record/2026-08-15-design-ledger.md) |
| Related | [Decision 76](#decision-76), [I-15](#i-15), [I-16](#i-16), [glossary: internal operation](glossary.md#internal-operation) |

**Statement.**

- The P8 design v2 is approved: design v1 with the 16 amendments of the review and one change of the main session.
- `MATCHER_OP` and `SINK_OP` are bound to the URNs `rho:reified:internal:matcherOp` and `rho:reified:internal:sinkOp`, as `HANDLE_OP` is.
- The FIPS clause on "modified" and "unmodified" spaces is the scoping of a typing context.
- `new x : s` adds the channel `x` to `s`, typed by the theory of `s`, and the theory stays immutable.
- `getSpaceAgent` of a user agent replies `NotANativeSpace`, and a native handle accepts only the matchers that a translation built.

**User's words.**

- On the design: "So long as this aligns completely with the FIPS then it is okay with me".
- On the URNs: "You can go with your recommendations, but I don't remember these being mentioned in the FIPS. Where did they come from?"
- On the clause: "Reading 1 (Recommended)", after the user explained that the FIPS example concerns GSLT typing in F1r3lang.
- On the narrower points: "Keep and document (Recommended)".

**Reason.**

- The FIPS defines what a matcher does and that a method call has a return channel. It does not define how the node builds them.
- The semantics design pass of the plan (2026-10-04) introduced the two ops. A synthesized `match` would be unmetered, and `rho:io:devNull` takes one argument only.
- A node binds every installed definition to a URN. The ops reject every forged call, so a URN grants no capability.
- F1r3lang theories are immutable, and a change gives a new handle. A name from `new` is unforgeable and lexically scoped, so the reference semantics of a space gives the behavior of the typing reading.
- F1r3lang's theorem-channel kernel does not let a caller supply capture values. So a native space runs only the matchers whose pattern it can check.

### <a id="decision-78"></a>Decision 78: One native replay test runs at the v6 host-work limits

| Field | Value |
|---|---|
| Date | 2026-10-10 |
| Status | Retired by [decision 89](#decision-89). It was implemented in P8 step 1 |
| Amends | [Decision 47](#decision-47) (its sentence "That accessor is the only exception to M-3"), [M-3](#m-3), and item 21 of the consensus-preservation checklist, for one test edit |
| Related | [Decision 77](#decision-77), [I-12](#i-12), [host work](glossary.md#host-work) |

**Statement.**

- The accounting test `native_replay_authority_charge_is_schedule_independent` runs its native replay at the v6 host-work limits (`offered_funded_v6_host_work_limits`).
- The old budget of 100,000,000 units in each dimension stays in the test file as a comment.
- This edit and the accessor of decision 47 are the only changes to the accounting module. Production code does not change.

**User's words.** "v6 limits in the test (Recommended)".

**Reason.**

- Native replay installs every system process on the host-work budget of the deploy, so each install reserves host work.
- At P7, the replay of this test used 99,944,586 VerificationBytes of its budget of 100,000,000.
- The `MATCHER_OP` and `SINK_OP` installs of decision 77 reserve 515,355 VerificationBytes together. They crossed that limit, and the replay failed with `HostWorkRejected`. Either install alone crossed it.
- The installs of P4 to P7 had used the headroom. The cost-accounting branch has the same test and the same budget, without the Reified installs.
- The test checks that replay charges do not depend on the schedule. The v6 limits are the limits of certification in production, so the test keeps its purpose.

**Rejected alternatives.**

- Remove the two installs. This changes design v2 and decision 77, adds events to each agent call, and leaves only 55,414 units of headroom.
- Let the cost-accounting owner change the test. The step 1 commit then waits for a merge.

**Evidence.** With the edit, the test passes, so no v6 dimension binds.
A temporary print measured the VerificationBytes of the replay before its evaluation: 52,547,059 without the two installs and 53,062,414 with them.
The evaluation adds 47,397,527 in both cases.
The two installs take about 0.19 % of the v6 limit of 268,435,456 VerificationBytes.
The footprint policy `p8` approves the edit, and checklist items 11 and 21 name it from the milestone `p8`.

### <a id="decision-79"></a>Decision 79: The conformance kit keeps the v6 host-work limits

| Field | Value |
|---|---|
| Date | 2026-10-10 |
| Status | Retired by [decision 89](#decision-89). It was implemented in P8 step 5 |
| Amends | None |
| Related | [Decision 77](#decision-77), [Decision 78](#decision-78), [host work](glossary.md#host-work), [conformance kit](glossary.md#conformance-kit) |

**Statement.**

- Every deploy of the conformance kit plays natively and certifies by native replay at the v6 host-work limits.
- The persistent case of the kit drains one stored message at the consume, and then fires on one later send in each deploy.
- Chapter 21 states the structural cost of an agent space, and not the replay costs of this tree.

**User's words.** "Keep the limits for now, I am repairing the replay costs but it isn't complete yet".

**Reason.**

- Native replay charges much more host work than native play for programs with stored contracts.
- One call of a small stored contract costs 1.0 M VerificationBytes in play and 62.6 M in replay. The limit is 268.4 M.
- Play charges nothing for the size of a stored continuation. Replay charges a first load and an amount for each call that grow with it.
- The reference agent makes few calls for each operation, so every step of the kit certifies at the v6 limits.
- One case does not: a persistent consume that drains two stored messages in one deploy needs 282.1 M in replay, and 20.7 M in play.

**Rejected alternatives.**

- Certify the reference agent at a larger test budget. Production certifies at the v6 limits, so the kit would pass at a budget that production does not have.
- Merge the newer commits of the cost-accounting branch first. Their effect on these costs is not known, and the merge is a large step before P8 completes.

**Evidence.** The measurement is a progress note of the finding M-47-1 (pgmcp 10816).

### <a id="decision-80"></a>Decision 80: The P9 design process

| Field | Value |
|---|---|
| Date | 2026-10-11 |
| Status | In force |
| Related | [Decision 47](#decision-47), [decision 65](#decision-65), [decision 72](#decision-72), [decision 76](#decision-76), [M-5](#m-5) |

**Statement.**

- The main session drafts the design of P9 against the current code.
- P9 is the consensus integration: the replay and the merge of spaces across blocks, and the event coverage of space writes.
- P9 also adds the suites N1, N2, N4, N7 and N9.
- Its other parts are the export and the import of space state, and the ledger of the legacy tests.
- One fresh read-only Plan agent arbitrates the draft.
- The main session verifies its findings in code and presents the result to the user before any P9 code.

**User's words.** "Me + review agent (Recommended)".

**Reason.** The same process produced the approved designs of decision 47, P6, P7 and P8.

### <a id="decision-81"></a>Decision 81: Backport the DR-119 fix of bug 11004

| Field | Value |
|---|---|
| Date | 2026-10-11 |
| Status | Reverted by [decision 84](#decision-84) (2026-10-12). It was implemented in P9 step 0, and [decision 82](#decision-82) amended its effect on N4 |
| Amends | [Decision 18](#decision-18): one approved exception to "no `casper/src` change" |
| Related | [Decision 70](#decision-70), [decision 80](#decision-80), [M-1](#m-1) |

**Statement.**

- The branch carries the fix of bug 11004 from feature/cost-accounted-rho, commit `7ab849b56` (DR-119 of that branch).
- `SystemVault.rho` takes one creation lock for each bucket of a cursor scope, and an existing cost cursor takes no lock.
- Settlements on different cursor scopes then compose in a merge. Before the fix, every two paid offered settlements conflicted on one global lock.
- The files of that commit that this tree holds unchanged since the base `4d67b5ccc` are ported byte for byte.
- These files are `SystemVault.rho`, the policy snapshot and settlement tests, a design note, the Rocq module `CostCursorBuckets.v` and the TLA models of the cost cursors.
- The drifted files are adapted: `_CoqProject`, `PROOF_ARTIFACTS.md`, the three gate scripts, the scale claims, the conformance matrix and the decision records.
- The commit's changes to two `batch2` specs and to the offered API tests are not ported. They depend on test code that this tree does not hold.
- The footprint policy `p9`, the completeness phase `p9` and item 8 of the consensus checklist allow exactly these changes.

**User's words.** "Can you backport the fixes to Bug 11004 from feature/cost-accounted-rho?", and then "Backport DR-119 (Recommended)".

*Correction ([decision 84](#decision-84), 2026-10-12).* The backport was the main session's recommendation, and the user asked only whether a backport was possible.
The recommendation did not state that `SystemVault.rho` is a genesis contract (`standard_deploys.rs:206`), so the backport changed the genesis state of new networks.
The user: "I did not ask you to change Genesis, you asked me to change genesis."

**Reason.**

- Without the fix, two sibling blocks with paid offered deploys always conflict in the vault, whatever their spaces do.
- The N4 suite of P9 can then test the merge of two sibling blocks with spaces at the Casper level, with different payers.
- That is the only Casper-level check of [M-1](#m-1). The ported files are identical to the branch, so the next merge of that branch keeps them.
  *Amended by [decision 82](#decision-82) (2026-10-11): two payers cannot share the records of one space, so N4 tests two payers that each use their own spaces. N4 has no Casper-level check of M-1.*

**Rejected alternatives.**

- Keep N4 without the fix. The merge rows of spaces would stay only in the runtime rig.
- Merge all of feature/cost-accounted-rho first. That merge holds 541 commits, among them replay-cost work that its owner calls not complete.

**Evidence.** The policy snapshot and settlement tests, the cost-accounting Rocq and TLA legs of the new artifacts, the full `casper` suite and I-14 (verification.md, section 34).

### <a id="decision-82"></a>Decision 82: N4 fits three facts of Casper

| Field | Value |
|---|---|
| Date | 2026-10-11 |
| Status | In force. [Decision 84](#decision-84) disabled its concurrent row, and [decision 108](#decision-108) restores it |
| Amends | Section 8 of the P9 design v2 (alternating proposers), and the effect of [decision 81](#decision-81) on N4 (two payers on one space) |
| Related | [Decision 70](#decision-70), [decision 80](#decision-80), [M-1](#m-1) |

**Statement.**

- Node 0 proposes every block of the space chain of `n4_space_chain_propose_validate`. The other nodes validate each block.
- `n4_concurrent_space_blocks_agree` forks two sibling blocks from genesis. Two payers with distinct cursor buckets each create and use their own spaces.
- In that test, every node validates the merge block, and the merge rejects no deploy.
- N4 has no Casper-level row in which two siblings race on one space counter.
- The rholang merge suites and the models `CounterConflict.v` and `CounterConflict.tla` cover the guard of [M-1](#m-1).
- The campaign changes no Casper file for these facts.

*Amended by [decision 84](#decision-84) (2026-10-12).* This tree has no DR-119 fix again.
So the merge of two paid siblings rejects one of them, and the sender of the rejected sibling fails the merge block (gap G6).
`n4_concurrent_space_blocks_agree` is disabled, and the merge row takes two labeled payers.

*Amended by [decision 108](#decision-108) (2026-10-27).* The merged base changes the facts that disabled the row, so `n4_concurrent_space_blocks_agree` runs again.
The rows keep the form of this decision.

**User's words.** "Adjust N4 (Recommended)".

**Reason.** The probes of P9 step 1 found three facts of Casper (verification.md, section 34.2).

1. The leader rule. While a parent branch holds unfinalized user deploys, only the sender of the newest such block includes offered deploys. Another proposer takes a fallback that counts only legacy deploys, so it proposes nothing.
2. The funding rule. A deploy that peeks or takes a record that another payer funded has no feasible signed assignment. The proposer quarantines it.
3. Gap G6. Two sibling offers of one payer conflict on its cost cursor, and the merge rejects one. The node that owns the rejected copy's block then fails to validate the merge block.

So proposers cannot alternate, and two payers cannot share the records of one space.
The siblings of one payer conflict in the vault and break validation in this tree.
DR-116 of feature/cost-accounted-rho closes gap G6 after the base `4d67b5ccc`.

**Rejected alternatives.**

- Backport DR-116 and add a counter row with one payer. The cost cursor of that payer also conflicts, so the row cannot show that the space counter alone causes the rejection. The backport is a second Casper exception.
- Merge the latest feature/cost-accounted-rho first. The merge brings DR-116 and every other change since the base, and the counter row keeps the same confound.
- Finalize each space block before another node proposes. N4 then depends on Casper's finality, which the campaign does not test ([decision 70](#decision-70)).

**Evidence.** The probes R1, R2, R6, R7 and R8 of P9 step 1 (verification.md, section 34.2).

### <a id="decision-83"></a>Decision 83: The creation job records a seq space

| Field | Value |
|---|---|
| Date | 2026-10-12 |
| Status | In force. The second half of the first direction is recorded as a follow-up |
| Amends | The place of the record of VR-2 in the code. P7 design v3 VR-2 already put the record in the creation job |
| Related | [Decision 73](#decision-73), [decision 80](#decision-80), [I-4](#i-4) |

**Statement.**

- The creation job of a temp or seq space records the space in the book of the root evaluation after its commit, inside the job. A durable space and a failed creation record nothing.
- The factory no longer records the space. It replies after the job, as before.
- The SQ-2 check of a send stays in the reduction participant that evaluates the send. Every check reads the book before its participant dispatches.
- The scheduler runs no reduction participant while a job runs. So the book changes only between the steps of the participants.
- The checks that run between two jobs read one book. So the charge of each check is a function of the deploy, in native play and in native replay.

**User's words.** "Design pass, then fix (Recommended)", then "Drop second half (Recommended)", then "Make note of the second half in case we want to pursue it later".

**Reason.** P9 step 3 measured one deploy that created a seq space in parallel with other sends. Over 24 runs, play gave two host-work vectors (19 and 5 runs) with equal events, roots and phlo.

The factory recorded the space in its participant after the creation job.
Other participants of the same scheduling window read the book at the same time.
So a send of that window was checked or not by timing, and the charge of the check varied.
The `SeqSent` verdict did not vary, because a holder of a seq name follows the record.

Native replay runs under the same scheduler as play, so the fix makes both deterministic.

**Rejected alternatives.**

- Also run the SQ-2 check inside the ordered execution of each produce (the second half of the first direction). It adds no determinism today. It needs these changes:
  - an edit outside the Reified files, in the execution of `Intent::Produce` (`deterministic_reduction.rs`)
  - host-work plumbing in jobs
  - job-side copies of the two scan sites of user agents
  - `SeqSent` through `RSpaceError`

  It would guard against a future write of the book outside a job. The user asked to keep it as a follow-up: pgmcp item `rrs-fu-sq2-scan-in-ordered-produce`.
- Compare operation orders. The lexicographic order of participant paths does not follow causality, so a holder of a seq name could skip the check.
- Carry a causal lineage of seq spaces with each process. It is exact, but it needs a participant field, a carrier for stored bodies and a hook at the dispatch of a COMM.
- Check every send. The host work of every ordinary deploy would change.

**Evidence.** The ledger of the defect and the tests `prepare_create_records_ephemeral_spaces_after_commit`, `prop_seq_gate_constant_per_window` and `seq_scan_charge_is_schedule_independent` (verification.md, section 34.4.1). The invariant `ScanViewPerWindow` of `SpaceCoordination.tla` and its Unsafe control `RecordInWindowUnsafe`.

### <a id="decision-84"></a>Decision 84: Revert the DR-119 backport

| Field | Value |
|---|---|
| Date | 2026-10-12 |
| Status | In force. [Decision 108](#decision-108) restores its concurrent row |
| Reverts | [Decision 81](#decision-81) |
| Amends | [Decision 18](#decision-18) (its `casper/src` exception ends), and [decision 82](#decision-82) (its concurrent row) |
| Related | [Decision 70](#decision-70), [decision 80](#decision-80) |

**Statement.**

- One commit reverts P9 step 0 (`e46123f6e`). `SystemVault.rho`, its two tests, the cost-accounting documents, the formal artifacts and the three cost-accounting scripts equal the base `4d67b5ccc` again.
- The five files that the backport added are removed. They stay in the history and on feature/cost-accounted-rho.
- The entry of `SystemVault.rho` in the corpus of ordinary sources returns to its capture of 2026-10-05.
- The footprint policy `p9`, item 8 of the consensus checklist and the closure total return to their state before the backport.
- The completeness phase `p9` held only the files of the backport. It now holds the Reified changes of P9, because the checklist checks a phase for each milestone.
- `casper/src` matches the base again. The only Casper changes of the campaign are the N4 test files and their one `mod` line.
- `n4_concurrent_space_blocks_agree` is disabled, with its reason at the test. The merge row takes two labeled payers, and no bucket search chooses them.
  *Amended by [decision 108](#decision-108): the merged base changes the facts that disabled the row, so the row runs again.*

**User's words.** "I did not ask you to change Genesis, you asked me to change genesis.", then "Revert it if it does not directly relate to Reified RSpaces and is not a technical blocker. This campaign is only focused on completing support for Reified RSpaces, nothing else should be investigated, changed, or implemented unless it is a technical blocker.", and "If a change to casper is necessary for the correct implementation of Reified RSpaces, then that is okay, but do not randomize on unrelated work!"

**Reason.**

- DR-119 fixes a defect of the vault (bug 11004), not a defect of Reified RSpaces. Reified code never touches the vault.
- `SystemVault.rho` is a genesis contract (`standard_deploys.rs:206`), so the backport changed the genesis state of new networks.
- The backport was the main session's recommendation, and the recommendation did not state this effect.
- The fix is not a technical blocker of Reified RSpaces. The N4 acceptance needs equal blocks, roots, event logs and costs on two nodes.
  It also needs a merge across blocks, and invalid traces that fail without a panic.
- The chain row, the merge row with an empty main parent and the tamper row meet that acceptance without the fix (verification.md, section 34.10).
- Only the row of two concurrent paid siblings needs the fix. Without it, the vault conflict rejects one sibling, and its sender then fails the merge block (gap G6). That row tests Casper's merge of paid blocks, not spaces.
- The next merge of feature/cost-accounted-rho brings DR-119 and DR-116 through their owner. The reverted files equal the base, so that merge takes them with no conflict.

**Rejected alternatives.**

- Keep the backport. It is a change of genesis that Reified RSpaces do not need.
- Keep the concurrent row in the form of design v2, which checks agreement on the rejected sibling. Measured: the sender of the rejected sibling fails the merge block with "offered-funded recovery requires an envelope buffer" (gap G6).

**Evidence.** The N4 runs without the fix, before and after the adaptation, and the byte comparison of the reverted files with the base (verification.md, section 34.10).

## User decisions 85 to 95

### <a id="decision-85"></a>Decision 85: P10 design v2

| Field | Value |
|---|---|
| Date | 2026-10-12 |
| Status | In force |
| Related | [Decision 72](#decision-72), [decision 76](#decision-76), [decision 80](#decision-80) |

**Statement.**

- Phase P10 follows design v2 (`.reified-rspaces-work/p10/design-v2.md`). The main session drafted design v1, one read-only review agent arbitrated it, and the main session checked each fact of the review in code.
- `Checkpoint.v` is written as the plan and the leaf `rrs-fv-port-checkpoint` state it. It has a concrete store and a concrete `apply_operation`, and its premises hold for one concrete instance.
- The benchmarks keep the comparison before and after of the parent task: P10 restores the P0 archives from the backup.
- The Determinism section goes in `specification.md`, and P10 drafts the body of PR-C in pgmcp.
- Deviation D-A: the codex modules `Occurrence.v`, `DirectionalMatch.v`, `EventCoverage.v` and `SavedQueryPolicy.tla` are recorded with their successors, and each of their laws is checked against a successor.
- Deviation D-B: `MANIFEST.toml` stays the index of the examples, and the expected values are Rust data in the harness. The reason: rholang has no TOML parser, and campaign commits never change `Cargo.lock`.
  *Changed on 2026-11-11: two commits change `Cargo.lock`. The merge of [decision 90](#decision-90) brings the lock of the base, and commit `cf4bcb410` moves the three packages of `rholang-rs` to the parser pin `6548aec`. Deviation D-B stays.*
- This host has a Zen 3 CPU with no LBR, so the benchmarks record call graphs with `perf record --call-graph dwarf`.

**User's words.** "Approve v2 (Recommended)". The recommendation was the main session's.

**Reason.** The plan requires a design pass with one review agent for each phase (decisions 72, 76 and 80). The two deviations keep the plan's intent with no new dependency and no duplicate proofs.

### <a id="decision-86"></a>Decision 86: The gate of the node command line waits for its repair

| Field | Value |
|---|---|
| Date | 2026-10-12 |
| Status | In force. Amended by [decision 89](#decision-89) and [decision 136](#decision-136). |
| Amends | [Decision 13](#decision-13) (the timing of the gate of the node command line) |
| Related | [Decision 69](#decision-69) |

**Statement.**

- With offered funding active, the node refuses legacy deploys and rejects a block that holds one (`casper/src/rust/api/block_api.rs:712-716`, `validation_dispatcher.rs:124-130`).
  *Changed by DR-134 of the base, which merge 2 takes: the node refuses legacy deploys on every shard.*
- On this branch and on feature/cost-accounted-rho, `node deploy` builds a legacy deploy and `node eval` evaluates on the legacy path. Only the HTTP route `/deploy/offered` and the gRPC deploy service take offered deploys.
  *Changed by DR-134 of the base, which merge 2 takes: `node deploy` signs an owner-direct offer. `node eval` still evaluates on the legacy path ([decision 136](#decision-136)).*
- P10 verifies every example in the native harness, with native play and native-replay certification at the v6 limits.
  *Amended by [decision 89](#decision-89): the v6 limits have no host-work limits, so the harness certifies at the phlo limits only.*
- The gate of decision 13 is written against the repaired command line and runs every example after the repair reaches this branch. The campaign changes no file of `node/src` for it.
  *Amended by [decision 136](#decision-136): the gate runs each example with `node deploy`, `node propose` and `node show-block`, and an exploratory deploy reads the results.*

**User's words.** "I am repairing the node CLI with the cost-accounting agent"

**Reason.** The repair belongs to the owner of the offered protocol, and the user leads it.

### <a id="decision-87"></a>Decision 87: No new capacity param

| Field | Value |
|---|---|
| Date | 2026-10-12 |
| Status | In force. Amended by [decision 89](#decision-89). |
| Related | [Decision 39](#decision-39) (F7, the `size` of an Array), [decision 71](#decision-71) |

**Statement.**

- The legacy branch had `size` and `cyclic` on Array spaces and `capacity` on VectorDB collections. Both exist here, so the campaign adds no new param.
- The features of the legacy branch must exist here, but the API does not have to match the legacy API.
- The FIPS matrix records four passages with these dispositions. They are the main session's recommendation, and the user's answers leave them in place.
- Vector out of memory (FIPS line 209) is a deviation. The deploy fails deterministically when it exceeds its phlo or host-work budget, because memory exhaustion cannot be a consensus outcome.
  *Amended by [decision 89](#decision-89): there is no host-work budget, so the deploy fails on its phlo budget only.*
- The quantale semantics (lines 187 to 193, and 269) is informative. The FIPS states it as an open question, and the follow-up `rrs-fu-quantale-unification` routes it to the FIPS authors.
- Majority and the elementwise sigmoid (line 261) come at P12 with tensor logic (decisions 62 to 65, and decision 88).
- "Others" (lines 263 to 265) is informative. Its fixed-size versions exist where the legacy branch had them: the `size` of an Array and the `capacity` of a VectorDB collection.

**User's words.** "Your question is confusing if the legacy `size` parameter already exists, then why are you asking me about adding the redundant `capacity` parameter?", and "you don't have to mirror the API and interface from the legacy branch, but those specific features had been implemented"

**Reason.** The legacy features exist, so a new param would be redundant.

### <a id="decision-88"></a>Decision 88: The legacy vector operations come with tensor logic

| Field | Value |
|---|---|
| Date | 2026-10-12 |
| Status | In force |
| Related | [Decision 52](#decision-52), [decisions 62 to 65](#decision-62) |

**Statement.**

- The legacy `rho:lang:vector` had 18 operations. The current process has `add`, `scale`, `dot`, `norm` and `distance`.
- The other 16 legacy operations come with tensor logic in phase P12, after the readiness signal of F1r3lang.
- They are `cosine_similarity`, `sigmoid`, `temperature_sigmoid`, `softmax`, `heaviside`, `l2_normalize`, `majority`, `gram_matrix`, `top_k_similar`, `bind`, `unbind`, `bundle`, `permute`, `unpermute`, `hamming_similarity` and `resonance`.
- The example `vector_ops` ports the five current operations, and its disposition cites this decision.

**User's words.** "Keep them for P12", then "I didn't say they needed to exist before their dependencies were ready ...". The main session had recommended adding them in P10.

**Reason.** The legacy operations depend on the work of tensor logic, which depends on F1r3lang. A legacy feature must exist when its dependencies are ready.

### <a id="decision-89"></a>Decision 89: The base removed host-work metering, so Reified RSpaces removes its own

| Field | Value |
|---|---|
| Date | 2026-10-16 |
| Status | In force. Decisions [96](#decision-96), [102](#decision-102), [105](#decision-105) and [107](#decision-107) amend it |
| Retires | [Decision 40](#decision-40), [decision 42](#decision-42), the metered view reads of [decision 47](#decision-47), [decision 78](#decision-78) and [decision 79](#decision-79), and the host-work clauses of [I-12](#i-12) |
| Amends | The reason of [decision 41](#decision-41), the last clause of [decision 68](#decision-68), [decision 86](#decision-86) (the harness certifies without host-work limits) and [decision 87](#decision-87) (a deploy fails on its phlo only) |
| Related | DR-131 of `feature/cost-accounted-rho` (commit `75de2dc39`, its records in `4c58a925f`), and DR-101, DR-114, DR-121, DR-128 and DR-129 of that branch |

**Statement.**

- A space operation meters only what the equivalent host operation meters: its COMMs (compute) and the bytes that it stores (storage). It uses the same observer calls (the phlo clause of [I-12](#i-12)).
- Nothing else is metered: no read, copy, decode, sort, scan, match, guard or similarity work.
- The merge of `feature/cost-accounted-rho` at `4c58a925f` deletes the host-work metering of the space substrate and of the rholang space code in one merge commit. The commit message gives the command that restores it.
  *Amended by [decision 96](#decision-96): merge 2 also deletes the host-work model `AgentReadCoverage.v`, and six host-work headlines of other modules become comments.*
- These parts stay:
  - the phlo clause of I-12
  - the fire-round cap of [I-21](#i-21), which is a liveness bound
  - the canonical order of [decision 41](#decision-41), which is semantics
    - *Replaced by [decision 102](#decision-102): the spaces pick at random in play, as the host does since DR-129.*
  - the protocol size limits
  - the native replay backend of spaces, with its accessor and bind call ([decision 47](#decision-47))
  - the bench harness of P10 step S8.
- [Decision 74](#decision-74) keeps its compact config bytes, because they are a state format. Only its reason lapses.

**User's words.** "The host work metering has been removed from feature/cost-accounted-rho, you should git-commit any outstanding changes and then git-merge from feature/cost-accounted-rho and revise your plan accordingly. There are two classes of metering in cost-accounted rholang/f1r3lang: compute and storage. Compute metering is done purely over COMMs and storage metering is computed over the required bytes. That is it, there is not supposed to be any other forms of metering."

**Reason.** The base has no host-work budget after DR-131, so a space reservation has no meter. One metering model keeps the phlo of a space operation equal to the phlo of its host program.

**The merge adaptations.** The merge also brings five rules of the base, and the Reified code follows each of them.

- The stack-safe funding resolver of the base (`accounting/lexical.rs`) counts a USE or NEWSPACE marker tuple as a term, as the reducer does (hook H1). So it fails closed on a marker at a resolving position (finding H-1). Since DR-121, a receive body is not a resolving position, so a marker there resolves.
- DR-128: the read rule of a space channel (`readable_channel`) raises `BundlePermissionError`, which the failure classifier maps to a user failure.
- DR-101: the re-store of a peeked record (`Introduction::for_record`) resolves the residue seal of the record in its deployment, as `produce_peeks` does. A seal that does not resolve is `Inconsistent`.
  *Amended by [decision 107](#decision-107): the seal also resolves with the region price of the budget, as `produce_peeks` does (DR-135 R4).*
- DR-114: `embed` and `ann` move to the record-and-replay mechanism of native execution ([decision 68](#decision-68)). A reply takes record room, a reply that does not fit becomes `OutputTooLarge`, and a replayed call without its record is a bug.
  *Changed by [decision 105](#decision-105): a reply that does not fit becomes `EXT_OUTPUT_TOO_LARGE`, the code of the base.*
- DR-129: host play picks at random again, and the spaces keep the canonical order of decision 41. A test that compares a space with a host program pins host play to the canonical choice. A test that replays a host log draws a fresh host replay.
  *Replaced by [decision 102](#decision-102): the spaces also pick at random in play, and replay follows the recorded pick (DR-129).*

**The answers of 2026-10-25.** The user answered the open questions of the merge on 2026-10-25.

- User failures (task 14): "Classify them as user failures (Recommended)".
  A deterministic space error that the program causes becomes the interpreter error `SpaceUserError` (`interpreter/errors.rs`).
  The failure classifier maps it to a user failure, as DR-128 does for `BundlePermissionError` (`accounting/economic_failure.rs`).
  `Inconsistent` stays a platform failure.
- The funding resolver: "Why would you recommend fail-closed and keep reified rspaces broken? What needs to be done in the base branch to support these properly?"
  The answer is the marker support of [decision 90](#decision-90) (R-1, R-2 and E-1).
  The base gets no Reified knowledge, only the shared predicate at two more sites and one arm of the estimate.
- The footprint data and the Python checkers: "do not commit them to the main repo", then "Untrack, keep local (Recommended)".
  Commit `6bf10a721` untracks the 23 Python scripts with their tests and fixtures, `footprint-policies.json` and the two head-file-digest files.
  The local copies stay, and `.git/info/exclude` hides them.
- The merge order: "Commit merge 1, then merge (Recommended)".
  Merge 1 is commit `b60e2e558`, and [decision 90](#decision-90) plans merge 2.

### <a id="decision-90"></a>Decision 90: The merge 2 plan for `feature/cost-accounted-rho` at `702496775`

| Field | Value |
|---|---|
| Date | 2026-10-25 |
| Status | In force. [Decision 95](#decision-95) replaces its item on the test attributes of `session.rs`. [Decision 98](#decision-98) changes its mirror item. |
| Retires | [I-11](#i-11), Option P of [decision 45](#decision-45) (as [decision 69](#decision-69) planned), and the legacy clause of [I-12](#i-12) |
| Amends | Item D3 of [decision 19](#decision-19) (the sub-order) and fix rule FR-5' of [decision 34](#decision-34). Both are unreachable after DR-132, and the merge comments them out. |
| Correction | *The merge kept both as live code, and only tests reach them ([design S5.7](design.md#s57-parent-tracking-fr-5)).* |
| Related | DR-132 to DR-135 of `feature/cost-accounted-rho`, [decision 73](#decision-73), [decisions 91 to 95](#decision-91), design v2 of merge 2 (pgmcp artifact 1086) |

**Statement.** The user approves design v2 of merge 2.
The merge takes the base at `702496775`, which has 781 commits after `4c58a925f`.
Four decision records of the base change Reified RSpaces:

- DR-132: racing evaluation replaces the deterministic scheduler, and each journal row takes an operation name.
- DR-133: the dev-parity trim.
- DR-134: the node takes only offered deploys, and the legacy meter is deleted.
- DR-135: owners fund their processes with held prices.

**Overlap with the base.** Since `4c58a925f`, 18 files changed on both sides, and no file has two fixes of one defect.
The merge reuses the base mechanisms that now do Reified work:

- the drain of `inj_with_observation`, which replaces `enter_boundary` for the volatile sweep
- the counted readiness wait of native replay, which now also covers space transactions
- `run_parallel_dispatches`, which `dispatch_space_outcome` calls for every dispatch
- the owner hooks of DR-135, which give the owner checks of space rows
- the I5 guard `native_evaluation_spawns_only_counted_tasks`, which now also scans the Reified directories.

The stack-safe sorter of `702496775` reaches the Reified code through `ParSortMatcher`, and this branch has no copy of the sorter.

**Base edits.** The user approves these edits of base files, which are the minimum that Reified RSpaces needs:

1. The conflict resolutions keep the base text of `reduce.rs` and `rho_runtime.rs` and add only the Reified fields, hooks and calls.
2. `session.rs` loses three `#[cfg(test)]` attributes of the replay accessor. [Decision 95](#decision-95) replaces this item.
3. `session/execution.rs` gets one `mod` line for the Reified readiness check.
4. `lexical.rs` uses the shared marker predicate at two more sites (R-1).
5. `estimate.rs` gets one arm and one new reason (E-1).
6. The test `racing_evaluation.rs` adds the Reified directories to the I5 guard.
7. With [decision 91](#decision-91), the spawn helper and the fired-body dispatch carry causal histories.

**Required changes.** These changes follow from the base, with no choice:

- Space jobs run inline in the counted task, so `reified/schedule.rs` goes with its five callers.
- The space backends adopt the operation names of DR-132. The observed body and the finish run inside `named`.
- Option P goes ([decision 69](#decision-69)): `reified/legacy_charges.rs`, the legacy chain, `LegacyCharges.v`, the Legacy mode of `CheckpointReplay.tla`, the legacy-mode tests and the `legacy_budget` scenario.
- The test literals of `CostRegion` take `price` and `caller`, and three child walks match `held_price: _`.
- The funding resolver and the estimate support the markers:
  - R-1: the shared predicate also counts the markers in the schedule of `visit` and in its test oracle.
  - R-2: a marker body is never walked, so the resolver claims no name inside it. R-2 needs no code.
  - E-1: the estimate treats a marker as unmodeled demand with the new reason `SpaceConstruct`, so the gate holds the signed limit.
- User failures map to `SpaceUserError` (the answers of 2026-10-25 in [decision 89](#decision-89)).
- The volatile sweep runs once after the drain, and `enter_boundary` has no successor.
- The mirror journal records the writes of a VectorDB space inside the commit, so the publication order is the commit order.
  *Changed by [decision 98](#decision-98): the commit itself publishes its writes in play, so the journal is retired. The publication order stays the commit order.*
- The live ChromaDB script uses the cache variable of rust-bert, because the dev-parity trim of the base returns to the embedder of dev.
- DR-135 moves the Reified pins of bytes, costs and roots: 9 bytes for each region and 1 byte for each signed term. Each moved pin is classified before it changes.

**Decided by existing rules.**

- The replay order of space transactions: no option that only Reified RSpaces has is correct. The readiness wait extends the counted park of the base, and [decision 92](#decision-92) gives the order rule.
- Owner funding: each staged row runs the owner checks of the base inside its own row. A refusal gives the host outcome, so a produce stores and a consume parks (SM-8, and item D6 of [decision 19](#decision-19)).
- One execution model ([decision 69](#decision-69)): the legacy-mode suites move to native play with native replay certification. Outside native execution, every space entry point fails closed, as the grant issuance of the base does.
- FR-5' and the sub-order are unreachable after DR-132, and the merge comments them out with a reason.
  *Correction: the merge kept both as live code, and only tests reach them ([design S5.7](design.md#s57-parent-tracking-fr-5)).*

**Order of work.** The conflicts and the compile come first.
Then come the models, each with negative controls: `NativeReplay.tla`, `SpaceCoordination.tla` and the owner rows.
The code transcribes the models.
Then come the resolver and the estimate, the user failures, the tests, the documents and all suites.
The merge commit waits for the consent of the user.

**User's words.** "Approve the plan (Recommended)". The directives of the user on the same day are these:

- "restrict any changes to concensus to the absolute minimum necessary for reified rspaces, you are not tasked with fixing concensus bugs or making improvements to casper"
- "If the base branch fixes any of the same bugs as this branch, take the base branch's versions and amend them as needed for reified rspaces"
- "pgmcp may be freely used as a memory server and for effectively infinite context"

**Reason.** After DR-132 to DR-135, the base has mechanisms for most of the Reified work. Their reuse keeps the edits of base files to the minimum that the directives ask for.

### <a id="decision-91"></a>Decision 91: The seq rule uses causal histories

| Field | Value |
|---|---|
| Date | 2026-10-25 |
| Status | In force |
| Amends | The basis of the Seq claims of [decision 73](#decision-73): causal histories replace the participant paths of the scheduler |
| Related | DR-132 of `feature/cost-accounted-rho`, [decision 90](#decision-90), [I-4](#i-4) |

**Statement.**

- [Decision 73](#decision-73) based QF-15 ("each channel has one chain of uses") on the participant paths of the scheduler. DR-132 removes these paths.
- Each task, stored record and waiter now carries the uses that happened before it, as a clock for each seq channel (`SeqHistory` in `reified/seq.rs`).
- A use is admitted when the last use of its channel is in its history (`SeqHistory::knows`).
- A fired body takes the union of the histories of its participants.
- So the verdict depends only on the program and the recorded matches, and play and replay agree under racing evaluation.
- The rule of decision 73 and the FIPS sentence "it is not used in a concurrent process" stay.
- The spawn helper and the fired-body dispatch of the base carry the histories (base edit 7 of [decision 90](#decision-90)).

**Model.** `SpaceCoordination.tla` checks two invariants together.
`HistIsCausalPast` says that each kept history is the causal past of its participant.
`NoFalseRefusal` says that the history of a refused use misses the last use of its channel.
With both invariants, a refused use is concurrent with the last use of its channel, so no refusal is false.

The control `NoMergeUnsafe` (a fired body does not take the union) refutes `HistIsCausalPast`.
The control `NoInheritUnsafe` (a new task does not keep the history of its parent) also refutes `HistIsCausalPast`.
The control `ClaimsNeedContextUnsafe` (the first use of a channel is refused) refutes `NoFalseRefusal`.

**User's words.** "Causal histories (Recommended)".
One other option was a refusal of seq in protocol 6, which removes every HashSet family.
The last option was a rule without QF-15, which allows concurrent uses against the FIPS.

**Reason.** A causal history is a function of the program and of its recorded matches, not of the thread schedule.
So each play and its replay give the same verdict.
The hypothesis to test is that the existing seq tests keep their verdicts.

### <a id="decision-92"></a>Decision 92: The replay order of space transactions uses ordinary events

| Field | Value |
|---|---|
| Date | 2026-10-26 |
| Status | In force. [Decision 94](#decision-94) replaces its escalation re-check. [Decision 99](#decision-99) replaces its anchor payer. |
| Replaces | The role of [I-11](#i-11) in native replay |
| Related | [Decision 31](#decision-31), [decision 47](#decision-47), [decision 90](#decision-90), [M-1](#m-1), DR-114 and DR-132 of `feature/cost-accounted-rho` |

**Question and answer.** The user asked "what is the most Casper-faithful mechanism? I do not want to turn Casper into a new protocol, how does it handle similar replay semantics on dev and feature/cost-accounted-rho?".
The answer was this:

- Dev rigs the replay space with the event log of the block, so a COMM fires only as recorded (`replay_rspace.rs:413-490`).
- Every order-dependent choice of a Rholang program is a COMM, and every read of shared state is a consume or a peek.
- The base adds the operation journal of DR-132, with rows by source and predecessors by channel.
- The base also adds native records for choices that are not RSpace events: G3 refusals, DR-114 calls and DR-113 grants.

**Statement.**

- Each row of a space transaction names the counter channel `S(sid)` of its space in its footprint, as one more join group.
  The predecessor rule of the base then replays the transactions of one space in play order.
  The recorder takes the footprint from the caller (`accounting/native_runtime/operations.rs`).
- The first row of a transaction must be unique to its job (finding CE1).
  A transaction records its read with an anchor before its other rows when it has no such first row:
  - a stage-time failure that depends on the state of the space
  - a step that changes nothing
  - a settle with no row of its own
  - a first row that another job can also stage.
- An anchor is one consume without a peek on the persistent config record `C(sid)` of the space. Its COMM changes no state.
- Merge check 1 ignores persistent produces, so anchors cause no merge conflicts.
- A positional failure (`CellOccupied`, `CapacityExceeded` or `NestedCommitGuard`) discards its staged operations. The transaction then commits the anchor alone and returns the error.
- Native replay checks the readiness of the first row of a space transaction only.
  A missing first row makes the transaction park and stage again. A missing or unfit later row fails the commit closed at that row.
- Rejected: a new journal record, which needs a new record kind in wire version 4 and a replay concept that dev does not have.
- Accepted cost: an anchored transaction pays one consume introduction and one COMM, as the counter events of [decision 31](#decision-31) do.
  The estimate covers anchors, because every space route makes the estimate incomplete. So the gate holds the signed limit.

**Design details.** The implementation design (pgmcp artifact 1106) fixes these points:

- Allocations and config reads also lock `S(sid)`, so their rows name it too.
- An anchor pays with the authority of its operation, as the counter events of decision 31 do (`spaces/counter.rs`). [Decision 95](#decision-95) records this correction.
  *Replaced by [decision 99](#decision-99): an anchor carries the residue seal of DR-101, so the running deployment pays for it.*
- The identity of an anchor derives from the random state of its job (`RandomPurpose::Anchor`).
- The anchors live in the new module `spaces/anchor.rs`.
- The model `cost_accounted_rho/NativeReplayStall.tla` gets the anchor rows in its section for space transactions.

**Arbitration.** An independent review (agent `ae11054e91fb3310e`, "approve with amendments") adds these items:

- CE1: the first row of each transaction is unique to its job.
- A failure discards its staged operations and stages the anchor in the same transaction.
- The escalation re-check compares exact channel sets. [Decision 94](#decision-94) replaces this item.
- `NestedCommitGuard` gets an anchor.
- CE2: an owner refusal inside a space commit stalls replay, because `outcome_fits` has no case for `SkippedIntroduction`. The owner checks of space rows fix it ([decision 90](#decision-90)).

**Model.** `SpaceReplayOrder.tla` checks four invariants on every play order: `NoStall`, `NoFailClosed`, `Agreement` and `ExactRows`.
The controls `NoFootprintKeyUnsafe` and `NoAnchorRowlessUnsafe` refute `NoStall`.
The controls `AnchorAppendedUnsafe` and `UnknownSourceFailsUnsafe` refute `NoFailClosed`.
The racing tests `space_racing_replay_*` of `rholang/tests/reified_native_spaces.rs` check the four invariants on the code.

**User's words.** "Ordinary events (Recommended)".

**Reason.** Dev and the base replay every order-dependent choice through ordinary events.
An anchor is such an event, so the replay of spaces adds no concept to the protocol.

### <a id="decision-93"></a>Decision 93: Racing tests stay multi-thread

| Field | Value |
|---|---|
| Date | 2026-10-26 |
| Status | In force. [Decision 111](#decision-111) adds the current-thread runtime |
| Amends | The rules DT-3 and DT-5 and the row N9 of the [specification](specification.md), for racing evaluation |
| Related | DR-132 of `feature/cost-accounted-rho` (commit `97d020154`), [decision 90](#decision-90), [decision 92](#decision-92) |

**Statement.**

- No test uses a single-thread runtime to fix an interleaving.
  *Amended by [decision 111](#decision-111): the racing tests also run on a current-thread runtime, as the base does. No test depends on that runtime to pass.*
- The rule of the base applies (DR-132, commit `97d020154`): "racing evaluation gives arrival-order logs, so no test compares whole logs across runs".
- Each run replays its own record instead, and its outcome must be an allowed outcome.
- The goldens of ordinary execution keep every field that every interleaving gives: roots, leaf bytes, state dumps, costs and the COMM structure.
- Three multi-thread runs of the clean base vary only in four fields, which become checks of each run:
  - the arrival order of three parallel sends (R1)
  - the order of the event kinds (R4, R8 and R9)
  - the length of the log (R9)
  - the operation count of the persistent-send and persistent-receive programs (R6).
- The equality tests of two runs become replay checks of each run with allowed outcomes. These are N9, the determinism suite, the rehydrate tests, the examples and the serial properties of P4.
- The specification restates the rules DT-3 and DT-5 and the row N9 for racing evaluation.

**Implementation.** The racing tests of [decision 92](#decision-92) run on runtimes with 2 and 8 workers, three runs each, and each run certifies its own play.
The tests follow the form of the base property P-1 (`racing_native_play_agrees_with_native_replay`), without its current-thread runtime.
*Changed by [decision 111](#decision-111): the racing tests run on the runtimes `[0, 2, 8]`, where 0 is the current-thread runtime.*

**User's words.** "Whoah, whoah, whoah, whoah! Why are you mentioning single-threading?!? Rholand and F1r3node are inherently concurrent!"
The main session had proposed to check goldens on a single-thread runtime.

**Reason.** Rholang and F1r3node are concurrent.
A single-thread runtime shows one interleaving only, but consensus must hold for every interleaving.

### <a id="decision-94"></a>Decision 94: Escalation to a join scope takes a two-phase lock

| Field | Value |
|---|---|
| Date | 2026-10-26 |
| Status | In force |
| Replaces | The escalation re-check of [decision 92](#decision-92), which dropped the transaction, began again over the union and classified again |
| Related | `produce_lock` of dev and of `feature/cost-accounted-rho` (`rspace++/src/rspace/locks.rs`, with F1 of DR-132 in the base), [lock coverage](glossary.md#lock-coverage) |

**Statement.**

- Phase A: a collection transaction locks its own keys, with `C(sid)`. It classifies the operation and reads the join scope under phase A.
- Phase B: the transaction then locks every key of the operation and of the scope in one sorted call. This is the form of `produce_lock` in dev and in the base.
- The scope stays valid, because every space transaction locks `C(sid)`. So the phase A lock on `C(sid)` blocks every other transaction of the space between the scope read and the phase B lock.
- No deadlock, by the argument of dev: every transaction takes phase A before phase B. It never waits for phase A while it holds phase B, and it takes each phase in one sorted call.
- Lock coverage stays exact on keys, so no outcome depends on lock stripes.
- Scope: the begin API of the Reified substrate (`space_store_handle.rs`, `space_play.rs`, `space_replay.rs` and `native_session/space.rs`) and `prepare_collection_op`. No base file changes.

**Model.** `JoinScopeLock.tla` checks `NoStaleScope`, `NoFalseRefusal` and `NoDeadlock` for two transactions of one space and one host produce, under every stripe map.
`EscalationRebeginUnsafe` (the code before this decision) and `PhaseAWithoutConfigUnsafe` refute `NoStaleScope`.
`PhaseBSplitUnsafe` and `PhaseAAfterPhaseBUnsafe` refute `NoDeadlock`, and `CoverageByHostUseUnsafe` refutes `NoFalseRefusal`.

**User's words.** "Two-phase lock, as dev (Recommended)". The recommendation was the main session's.
The user had asked "What were the results of your design and whether it needs to be amended to more closely align with dev and feature/cost-accounted-rho?"

**Reason.** The host produce already locks in two phases.
The same form keeps the scope of an escalation valid without a retry loop, and the argument of dev shows that it cannot deadlock.

### <a id="decision-95"></a>Decision 95: The space readiness check reaches the replay journal through the native epoch trait

| Field | Value |
|---|---|
| Date | 2026-10-26 |
| Status | In force. [Decision 99](#decision-99) replaces its related correction |
| Replaces | The item of [decision 90](#decision-90) that removes three `#[cfg(test)]` attributes of `session.rs` |
| Related | [Decision 31](#decision-31), [decision 92](#decision-92), DR-132 and DR-135 of `feature/cost-accounted-rho` |

**Statement.**

- The base trait `NativeOperationEpoch` (`rspace++/src/rspace/replay_rspace/native_epoch.rs`) gets two methods with default bodies that fail closed: `space_transaction_ready` and `wait_space_change`.
- `SessionEpoch` implements them through its replay (`session/operations.rs`).
- The space session calls them through `session.epoch`, as `native_session/space.rs` does for the other epoch calls.
- The three `#[cfg(test)]` attributes of the `replay` accessor of `session.rs` stay, so the accessor is test-only, as in the base.
- The base files that change are `native_epoch.rs` and `session/operations.rs`.

**Related correction.** The rules decide this point without a question.
An anchor of [decision 92](#decision-92) pays with the authority of its operation, as the counter events of [decision 31](#decision-31) do (`spaces/counter.rs`).
Owners pay for their processes (DR-135). The suggestion of the design agent that the running deployment pays is not adopted.

*Replaced by [decision 99](#decision-99) on 2026-10-27: the machinery continuations, the anchor included, carry the residue seal of DR-101. So the running deployment pays for them, as DR-101 item 2 says.*

**User's words.** "Epoch trait, as base (Recommended)". The recommendation was the main session's.

**Reason.** The space session already reaches the replay through the epoch trait for its other calls.
Two defaulted methods keep the replay accessor test-only and change no behavior of the base.

## User decisions 96 to 137

The merge 2 review of 2026-10-27 aligned the earlier decisions with the [base](#terms-used-in-this-register) at `702496775` ([decision 97](#decision-97)).
Decisions 96 to 121 record its results and the answers of the user on 2026-10-27 and 2026-10-28.
Decisions 122 to 124 record the results of 2026-10-29.
Decisions 125 to 129 and 132 to 137 record the answers of the user from 2026-11-01 to 2026-11-10.
These decisions override the earlier decisions where they conflict.
Each entry names the decisions and the clauses that it replaces, and the old entries keep their text with a note.

### <a id="decision-96"></a>Decision 96: `AgentReadCoverage.v` is deleted, as the base deleted its host-work theories

| Field | Value |
|---|---|
| Date | 2026-10-27 |
| Status | In force |
| Amends | [Decision 89](#decision-89): step M2 of its plan retired the headline H27, but no other host-work theory |
| Related | [Decision 42](#decision-42), [decision 97](#decision-97), DR-88 and DR-131 of the base (commit `75de2dc39`) |

**Statement.**

- Decision 89 retired the queue metering of decision 42, and `AgentReadCoverage.v` is its Rocq model.
- All 9 headlines of the module are laws of host-work reservations, and no module imports it.
- DR-131 of the base deleted its host-work theories, among them `CandidateReadCoverage.v` of DR-88, which `AgentReadCoverage.v` restates.
- Merge 2 deletes the file. The command `git show 6bf10a721:formal/rocq/reified_rspaces/theories/AgentReadCoverage.v` restores it.
- `_CoqProject`, the gate count and the negative controls keep the module as comments with a restore note.

**The same rule for six more headlines.** Six headlines of other modules restate the rule of `AgentReadCoverage.v` for their own traces.
They are `settle_trace_covered`, `alloc_trace_covered`, `set_key_trace_covered`, `scan_trace_covered`, `level_scan_trace_covered` and `prefix_scan_trace_covered`.
The base deleted every theory of its host-work meter, so these headlines go too, as H27 went ([decision 97](#decision-97)).
With H27 and the 9 headlines of the module, the gate then expects $`306 - 9 - 1 - 6 = 290`$ headlines.

Each module keeps the section as a comment that starts with a note of decision 96.
To restore a section, remove that note and the closing marker of its comment.
The negative controls of the six headlines go with the headlines.

**User's words.** "Delete it, as base did (Recommended)". The recommendation was the main session's, from the rule of [decision 97](#decision-97).

**Reason.** The module and the six headlines prove laws of a meter that no longer exists.
The base deleted the same kind of theory, so a kept theory would only check rules that no code follows.

### <a id="decision-97"></a>Decision 97: The base branch decides the design, also for the earlier decisions

| Field | Value |
|---|---|
| Date | 2026-10-27 |
| Status | In force |
| Amends | Every earlier decision that the base models in another way. Decisions 98 to 121 name each change |
| Related | [Decision 70](#decision-70), [decision 90](#decision-90) |

**Statement.**

- For each design point, the campaign finds the mechanism of the base and adopts it. The sources are the decision records of the base and its code at `702496775`.
- The record of each decision names the rule of the base with its file and line.
- When the base models a recorded decision in another way, the base decides the point again, with no question. The new decision names what it replaces.
- A question goes to the user when the base has no mechanism for the point.
- A question also goes to the user when the mechanism of the base conflicts with explicit words of the user or with another standing directive.

**Review.** Four read-only agents compared decisions 1 to 96, the I-rows and the M-rows with the base.
The main session checked each reported divergence against `702496775` before it recorded decisions 98 to 121 (pgmcp note 25218 and the later notes).

**User's words.**

- "Align with the base branch as closely as possible -- let that be your guide while making design decisions and ask me if the base branch truly does not model the issue"
- "that goes for all previously answered questions that are better answered by aligning with the base feature/cost-accounted-rho branch"

**Reason.** Reified RSpaces builds on the base.
A mechanism of the base needs no second design, and one rule on both sides keeps the differences from the base small.

### <a id="decision-98"></a>Decision 98: The mirror follows the oracle mode of the base (DR-114)

| Field | Value |
|---|---|
| Date | 2026-10-27 |
| Status | In force |
| Replaces | The M1 answer of [decision 66](#decision-66), the publication sentence of [decision 58](#decision-58), the mirror item of the required changes of [decision 90](#decision-90), and the former rules MR-2 (the journal) and MR-3 (the settle, and the publication of native replay) of the [specification](specification.md#10-the-chromadb-mirror) |
| Related | [Decision 97](#decision-97), [decision 114](#decision-114), DR-114 of the base |

**Base facts.**

- A play runtime calls the services of the node once for each recorded call, and every replay keeps the no-op services (`runtime_manager.rs:939-951`).
- The batch pass gives the play services to the play role and the no-op services to the replay role (`offered_batch.rs:420-429`).
- The dry run of an exploratory deploy spawns the play runtime (`dry_run.rs:185`).
- The reporting runtime gets the no-op services (`node/src/rust/runtime/setup.rs:568`).
- An oracular file operation echoes its recorded reply in replay (`io/mod.rs:42-46`, `fs_handler.rs:8-10, 55-58`).
- DR-114 says: "Nothing external is undone on rejection or orphaning" (DR file, lines 12959-12961).

**Statement.**

- A play publishes the net writes of each commit of a mirrored space at the commit, before the commit releases its stripes. It publishes whatever the final result of its deploy.
- Only the store mode `Play` publishes. The modes `Replay`, `Reporting` and `NativeReplay` never publish.
- A mirror write gives no reply to the program, so nothing is recorded, as for a print (DR-114 T7).
- `embed` and `ann` stay recorded processes (rule SP-5), and replay produces their recorded replies.
- A failed deploy, a dry run and the play of a block that consensus does not keep leave their writes in the mirror. The mirror of a node gets only the writes of the deploys that the node plays. [Decision 114](#decision-114) decides that nothing repairs these effects.

**Code.** `ReifiedRuntime::run_op` publishes the batch of a commit in play, before it drops the hold of the commit (`reified/runtime.rs:355-425`).
The journal, `settle_mirror_journal`, the clear of `begin_evaluation_scope` and the two settle calls of `interpreter.rs` and `rho_runtime.rs` stay as comments with their restore notes.

**Model.** `ChromaMirror.tla` retires `PublishOnlyOnSuccess` with its controls `PublishFailedDeployUnsafe` and `BeginScopeKeepsJournalUnsafe`.
It adds `ReplayNeverPublishes`, which the control `ReplayPublishesUnsafe` refutes.
It also adds `OnePublicationPerCommit`, which the control `SettleAlsoPublishesUnsafe` refutes (pgmcp notes 25208 and 25218).

**User's words.** "For external dependencies like Chroma DB, shouldn't they be written to immediately (during play, I guess?) and then recorded so replay merely replays their recordings? Isn't that how the File I/O API works? What are your thoughts?"

**Reason.** The base calls an external service only in play, and it never undoes the call.
The mirror is such a service, so the same rule applies, and no replay writes the mirror.

### <a id="decision-99"></a>Decision 99: The machinery of a space runs as system work, and the firing deployment pays

| Field | Value |
|---|---|
| Date | 2026-10-27 |
| Status | In force |
| Replaces | The related correction of [decision 95](#decision-95), by which an anchor pays with the authority of its operation, and the anchor payer of [decision 92](#decision-92) |
| Related | [Decision 31](#decision-31), [decision 100](#decision-100), DR-101 item 2 and DR-135 R1 of the base |

**Base facts.**

- A continuation whose seal has only Unit regions runs a system body (`rholang/src/rust/interpreter/dispatch.rs:52-56`, `residue.rs:38`).
- A system body has no owner (`owner_funding.rs:1211-1218`). So the payer of the running deployment pays at the budget price (`owner_funding.rs:1546-1569`).
- DR-101 item 2 says: "The deployment that fires the body pays for the work of the body". DR-135 R1 keeps this residue mode for system bodies.

**Statement.** The [machinery continuations](#terms-used-in-this-register) `K(v)`, `K_anchor`, `K_lift` and `K_retire` carry the residue seal of DR-101.
So the running deployment pays for their introductions and their shares of a COMM, also inside the body of an owner.
An owner still pays for its own records, continuations and fired bodies, at its own price.
This is classification B1 of the C4 arbitration, for its finding F2.

**User's words.** "Align with the base feature/cost-accounted-rho branch as closely as possible"

**Reason.** The machinery keeps shared protocol state and never runs a body, as the registry bodies of DR-101 do.
The base runs such work as system work, so the same rule needs no new payer.
Decision 31 predates the owner funding of DR-135, so it did not decide this point.

### <a id="decision-100"></a>Decision 100: A refused lift or retire parks inert

| Field | Value |
|---|---|
| Date | 2026-10-27 |
| Status | In force |
| Related | [Decision 39](#decision-39) (the settle and the retires of the P5 design), [decision 99](#decision-99), [lift](glossary.md#lift), [settle](glossary.md#settle) |

**Statement.**

- Under [decision 99](#decision-99), an owner can refuse only a `K_lift` or a `K_retire`, through the owner share of the user record that it consumes.
- A refused lift or retire parks its continuation inert on the channel of the collection.
- Every reader of the waiters of a collection skips the two machinery shapes. It still fails on any other waiter that it cannot decode.
- A refused lift books the settle pair of the record and its waiter (finding F5). So the settle does not propose that pair again in the same run.
- A run without refusals changes no bytes.

**User's words.** "Park it inert (Recommended)". The recommendation was the main session's, option A of question Q1 of the C4 design report, narrowed by B1.

**Reason.** Option A keeps the rule of DR-135 that the run continues after a refusal, and it changes no consensus bytes when nothing is refused.
A failed deploy (option C) would let the shortfall of one owner fail the deploy of another party.
Ground patterns (option B) would change every machinery event and still need the reader filter.

### <a id="decision-101"></a>Decision 101: A receive with several members runs the item test of each member during the search, as the base does

| Field | Value |
|---|---|
| Date | 2026-10-27 |
| Status | In force |
| Related | [Decision 50](#decision-50), [decision 66](#decision-66) (list mode), DR-135 R5 and R6 of the base |

**Base facts.**

- The item test of an owner runs right after the pattern test of each datum (`space_matcher.rs:444-449`).
- A refused tuple rolls back, and the search tries the next candidate (`space_matcher.rs:351-360`, `ops_consume.rs:228-240, 265-278`).

**Statement.** A set class receive and a VectorDB list receive run the item test of each member during the search.
A refused tuple keeps all its members stored, and the receive waits, as on a miss.
This is option A of question Q2 of the C4 design report, which the arbitration recommends (finding F3).

**User's words.** "Align with the base feature/cost-accounted-rho branch as closely as possible."

**Reason.** The base tests each datum during the search, so a refusal acts as a miss and the search goes on.
The same rule keeps every park of a collection at one bind, so the join groups of the collection never change.

### <a id="decision-102"></a>Decision 102: Play picks among candidates at random, and replay follows the recorded pick (DR-129)

| Field | Value |
|---|---|
| Date | 2026-10-27 |
| Status | In force |
| Replaces | [Decision 41](#decision-41), [decision 60](#decision-60), and the clause of [decision 89](#decision-89) that keeps the canonical order |
| Related | [Decision 11](#decision-11), [decision 117](#decision-117), [decision 119](#decision-119), DR-75, DR-129 and DR-132 of the base |

**Base facts.**

- Play shuffles the candidates with `rand::rng()` (`ops_produce.rs:552-752`, `ops_consume.rs:313-325`).
- Replay keeps the order of DR-75 as its internal order (`candidate_order.rs:132`, `native_candidate.rs:95, 153`).
- DR-129 says: "Every replay role follows the recorded pick" (DR file, lines 16143-16145).
- DR-132 says: "The record decides every order-dependent choice" (DR file, lines 16833-16834).

**Statement.**

- A bag space, a set space, a join window and a tie of two equal VectorDB scores pick at random in play.
- Replay finds the recorded pick, and it does not compute a canonical one.
- The ordered disciplines (queue, stack and priority queue) keep their order, because their head is the only candidate ([decision 11](#decision-11)). The base has no ordered collections.

**Requirement check.** Row RR-22 of the FIPS asks for the multiset semantics of the default space: "Bags have the default multiset semantics" (`Reifying RSpaces.md:229-231`).
No row of the FIPS requires a canonical pick.

**Design.** A Plan agent designs decisions 102 and 103 together with C4, model first.
The design covers the rules DT-1, BG-1, ST-3, VD-4 and JN-6, the proofs `selection_total_order` and `topk_deterministic`, the set cascade and the pin tests.
[Decision 119](#decision-119) records the changes of the arbiter.

**Basis.** [Decision 97](#decision-97), with the user decision of 2026-10-12 that DR-129 records: "restored dev's randomized candidate order".
The user words of decision 89 concern metering only (the statement of [decision 89](#decision-89)).

**Reason.** The reason of decision 41 was one order for the host and the spaces.
Since DR-129 the host picks at random, so the same reason now asks for random picks in the spaces.

### <a id="decision-103"></a>Decision 103: A join that names one channel more than once assigns data as the base does (DR-129 E1)

| Field | Value |
|---|---|
| Date | 2026-10-27 |
| Status | In force. [Decision 118](#decision-118) amends it |
| Replaces | The clauses of [decision 28](#decision-28) "binds $`k`$ distinct records" and "the incoming datum binds exactly once", with the rules JN-3 and JN-9 |
| Related | [Decision 102](#decision-102), [decision 119](#decision-119), DR-129 E1 and DR-135 R6 of the base |

**Base facts.**

- One greedy pass in pattern order runs over a list that puts the incoming datum first and then orders by source hash (`space_matcher.rs:25-64, 489-541`).
- A persistent datum stays in the list, so it can bind several positions (`space_matcher.rs:452-460`).
- The pass never retries the incoming datum at a later position.
- Under owner funding, the tuple must hold the incoming datum (R6, `space_matcher.rs:355-360`).

**Statement.** A join that names one channel more than once assigns its data as the base does.
The rest of decision 28 stays: a group may repeat a channel, and the canonical group order (FR-1') holds.

**Requirement check.** The reason of decision 28 reads the FIPS as one message for each pair of a matcher and a channel.
The consume clause of the FIPS does not say this (`Reifying RSpaces.md:131-143`), and the bag clause asks for the default semantics (lines 229-231).

**Basis.** [Decision 97](#decision-97).

**Reason.** The base already assigns the data of a repeated channel, and its replay finds the same assignment from the record.

### <a id="decision-104"></a>Decision 104: `embed` and `ann` replay their records as the base driver does (DR-114)

| Field | Value |
|---|---|
| Date | 2026-10-27 |
| Status | In force. [Decision 116](#decision-116) amends its mechanism clause |
| Replaces | [Decision 67](#decision-67) |
| Related | [Decision 57](#decision-57), [decision 105](#decision-105), [recorded process](glossary.md#recorded-process), DR-114 and DR-135 item 10 of the base |

**Base facts.** `native_recorded_call` checks the arity (`interpreter/system_processes.rs:1356-1435`).
Replay then rejects an empty record with `BugFoundError`, and a reply that is not plain data (DR-135 item 10), and it delivers the record.
Replay makes no check against consensus state.

**Statement.** Play makes the checks of the call, and replay delivers the record with no check of the call.
The mechanism clause planned the base form of `is_native_external_process`. [Decision 116](#decision-116) replaces it with one shared helper of the base.

**Model.** `ChromaMirror.tla` retires `EqualHostWork` with its control `AnnChecksOnlyInPlayUnsafe`.

**Basis.** [Decision 97](#decision-97).

**Reason.** The reason of decision 67 was equal host work in play and in replay, and that reason lapsed with DR-131 ([decision 89](#decision-89)).
The base replays a recorded reply with no check, so two nodes with other services compute the same state.

### <a id="decision-105"></a>Decision 105: The failure replies of `embed` and `ann` use the base codes, and every reply must be plain data

| Field | Value |
|---|---|
| Date | 2026-10-27 |
| Status | In force |
| Replaces | The failure codes of the former rules SP-3 and SP-4 of the [specification](specification.md), the failure reply `[false, code, detail]` of [decision 57](#decision-57), `MirrorUnavailable` of [decision 58](#decision-58), and `OutputTooLarge` of the DR-114 adaptation of [decision 89](#decision-89) |
| Related | [Decision 104](#decision-104), [decision 116](#decision-116), DR-114 item 4 and DR-135 item 10 of the base |

**Base facts.**

- DR-114 item 4 gives the codes `EXT_BAD_ARG`, `EXT_FAILED` and `EXT_OUTPUT_TOO_LARGE` (DR file, lines 12840-12852, `interpreter/system_processes.rs:675-680`).
- Their messages are fixed: "<process>: invalid argument", "<process>: external service failed" and "<process>: external output exceeds the record room" (`interpreter/system_processes.rs:722-728`).
- DR-135 item 10 applies always (DR file, lines 17706-17715). Play replaces a reply that is not plain data with the paid `EXT_FAILED` reply (`native_played_reply`, `interpreter/system_processes.rs:736-758`).

**Statement.**

- A failed check of the arguments or of consensus state gives `EXT_BAD_ARG`.
- A service that fails or gives a bad answer gives `EXT_FAILED`.
- A reply past the record room gives `EXT_OUTPUT_TOO_LARGE`.
- `rho:lang:vector` keeps its codes, because it calls no service (rule SP-2).

**Basis.** [Decision 97](#decision-97).

**Reason.** One failure vocabulary serves every external service of the node.
A fixed message never carries the text of the provider, because "provider errors can echo masked keys" (`interpreter/system_processes.rs:668-670`).

### <a id="decision-106"></a>Decision 106: A persistent send to `embed` or `ann` fails at the fire, as rule P2 of the base does

| Field | Value |
|---|---|
| Date | 2026-10-27 |
| Status | In force |
| Replaces | The clause A21 of [decision 32](#decision-32) and of rule SP-1 for the body references 236 and 237 |
| Related | [Decision 104](#decision-104), DR-114 and DR-132 of the base |

**Base facts.** Rule P2 raises `SystemProcessShapeError` for a persistent send to a recorded process (`interpreter/reduce.rs:1306-1322`, DR file, line 12892).
It runs at the fire in native execution, before any service call.
`is_native_recorded_op` already holds the body references 236 and 237 (`interpreter/system_processes.rs:622-646`).

**Statement.**

- Rule A21 no longer judges `embed` and `ann`, so `check_persistent_send` lets a persistent send to them pass (`reified/system_channels.rs:152-170`).
- Rule A21 keeps its abort at the send for the other Reified operations. The base has no rule for a persistent send to a process that is not recorded.
- DR-132 calls that loop a defect, and it leaves the loop as it is (DR file, lines 17156-17165).

**Basis.** [Decision 97](#decision-97).

**Reason.** The base already fails a persistent send to every recorded process, before any service call.
A second rule for the same send would give two errors for one fault.

### <a id="decision-107"></a>Decision 107: The seal of a re-stored record resolves with the region price (DR-135 R4)

| Field | Value |
|---|---|
| Date | 2026-10-27 |
| Status | In force |
| Amends | The re-store of a peeked record in the DR-101 adaptation of [decision 89](#decision-89) |
| Related | [Decision 45](#decision-45), DR-101 and DR-135 R4 of the base |

**Base facts.** `produce_peeks` resolves the residue seal with `.with_price(budget.region_price())` (`interpreter/reduce.rs:1652-1670`, DR file, lines 17648-17655).

**Statement.** `Introduction::for_record` adds the same price (`spaces/record.rs:1630-1638`).
Its doc comment claims parity with `produce_peeks`, and the price makes the claim true.

**Basis.** [Decision 97](#decision-97), with the words of decision 45: "Re-store as the host (Recommended)" and "Apply everywhere (Recommended)".

**Reason.** The resolved region must equal the region that the deployment opened, and a second registration of one identity must carry the same authority.
So without the price, a re-store of such a record in the same deployment can fail with `EventIdentityConflict`.
This is finding F10 of the C4 arbitration, and it is an inference.

### <a id="decision-108"></a>Decision 108: The N4 row `n4_concurrent_space_blocks_agree` returns

| Field | Value |
|---|---|
| Date | 2026-10-27 |
| Status | In force |
| Amends | [Decision 84](#decision-84), which disabled the row |
| Related | [Decision 82](#decision-82), DR-116, DR-119, DR-134 and DR-135 of the base |

**Base facts.** The facts of Casper that [decision 82](#decision-82) fitted changed:

- DR-116 item 8 counts offered envelopes in the fallback (DR file, lines 13323-13325, `block_creator.rs:2863-2867`).
- DR-134 item 9 puts several offers in one block (DR file, line 17418).
- DR-135 lets owners fund their stored processes (DR file, line 17562).
- DR-116 removed the error of the envelope buffer (`interpreter_util.rs:2254-2262`).

**Statement.** The row runs again in `casper/tests/reified/n4.rs`, and it probes these facts on the merged tree.
Decision 108 changes no Casper source file.

**Basis.** [Decision 97](#decision-97). The record of decision 84 says that the row "can return with the merge of that branch" ([verification 34.10](verification.md#3410-the-revert-of-the-dr-119-backport)).

**Reason.** The defects that disabled the row are fixed in the base, so the row can check the agreement of concurrent space blocks again.

### <a id="decision-109"></a>Decision 109: Observers get no VectorDB services, as observers of the base get the no-op Chroma service

| Field | Value |
|---|---|
| Date | 2026-10-27 |
| Status | In force |
| Amends | The environment of the nodes in [decision 61](#decision-61) |
| Related | [Decision 98](#decision-98), [decision 113](#decision-113), [decision 114](#decision-114) |

**Base facts.** `for_observer` disables the services of an observer "for security", and it gives the observer the no-op Chroma service (`external_services.rs:46-57`).
`for_node_type` chooses by the validator key (`external_services.rs:74-86`, `node/src/rust/runtime/setup.rs:452, 474`).

**Statement.** Only a validator installs `node_services()`.
An observer installs the unavailable services, so it has no embedder, no mirror and no writer (`node/src/rust/runtime/setup.rs:1058-1061`).
An observer never plays, so it never publishes ([decision 98](#decision-98)).
The read-only node of `docker/shard.yml` keeps no mirror database.

**Basis.** [Decision 97](#decision-97).

**Reason.** The base disables the external services of an observer "for security".
An observer never plays, so it has no use for an embedder or a mirror.

### <a id="decision-110"></a>Decision 110: Bare floats keep the forms of the base

| Field | Value |
|---|---|
| Date | 2026-10-27 |
| Status | In force |
| Replaces | [I-7](#i-7), and the sentence "A bare float needs a digit after its dot" of the [syntax contract](syntax-contract.md) |
| Related | [Decision 23](#decision-23) |

**Base facts.** `preprocess_floats` accepts `5.`, `5.e3` and `.5` (`compiler/compiler.rs:17-50, 63`).
The vector `test-vectors/ordinary-float-forms-v1.json` was captured at `702496775`, and it records these forms as `ok`.

**Statement.** The node keeps `preprocess_floats`, so every float form of the base stays valid.
The planned retirement of I-7 at the final parser pin is dropped.

**Basis.** [Decision 97](#decision-97).

**Reason.** The base accepts these forms, and a deploy that is valid on the base must stay valid here.

### <a id="decision-111"></a>Decision 111: The racing tests also run on a current-thread runtime, as the base does

| Field | Value |
|---|---|
| Date | 2026-10-27 |
| Status | In force |
| Amends | [Decision 93](#decision-93) (`RACING_WORKERS`, `rholang/tests/reified_native_spaces.rs:2120-2122`), and the restatements of the rules DT-3 and DT-5 and the row N9 |
| Related | DR-132 of the base |

**Base facts.** The properties P-1 and P-6 run on a current-thread runtime and on runtimes with 2 and 8 workers.
The sources are the DR file (lines 17145-17156), `random_candidate_order.rs:690-699` and `owner_replay.rs:516`.

**Statement.**

- Each racing test runs on the runtimes `[0, 2, 8]`, where 0 is the current-thread runtime.
- Every run replays its own record.
- No test depends on the current-thread runtime to pass.

**User's words.** "Add it, as base (Recommended)". The recommendation was the main session's.

**Reason.** The current-thread runtime adds one more schedule to the runs.
No test depends on it, so the reason of decision 93 still holds: consensus must hold for every interleaving.

### <a id="decision-112"></a>Decision 112: The Reified syntax uses protobuf fields, as the base carries its new syntax

| Field | Value |
|---|---|
| Date | 2026-10-27 |
| Status | In force. Merge 2 implemented it ([decision 120](#decision-120)) |
| Replaces | [Decision 18](#decision-18) |
| Amends | [Decision 6](#decision-6) (its fixture commit), [decision 15](#decision-15), [decision 21](#decision-21), [decision 22](#decision-22), [I-8](#i-8) and [I-14](#i-14) |
| Retires | [I-20](#i-20), in merge 2 |
| Related | [Decision 121](#decision-121), DR-135 item 12 of the base |

**Base facts.** The base adds protobuf fields for its syntax (`RhoTypes.proto:48-49, 73`, commit `d367bc11a`).
Every state root then changes once (DR-135 item 12).
That commit changed `empty_state_hash_fixed` and called the change "a coordinated state-format upgrade".

**Statement.** The Reified syntax leaves the marker tuples and takes protobuf fields.
The question named the `casper/src` change of `empty_state_hash_fixed`, so the answer approves that edit.
A Plan agent designs the change, and [decision 120](#decision-120) settles its open points.

**User's words.** "Protobuf fields, as base".

**Reason.** The base carries its new syntax in protobuf fields and accepts one change of every state root.
The same form gives the Reified syntax the encoding rule of every other syntax of the node.

### <a id="decision-113"></a>Decision 113: `embed` and `ann` exist only in a `chromadb` build, and the VectorDB space type exists in every build

| Field | Value |
|---|---|
| Date | 2026-10-27 |
| Status | In force |
| Replaces | The clause "Both exist in every build" of [decision 57](#decision-57), and the former rule MR-7 of the [specification](specification.md#10-the-chromadb-mirror) |
| Related | [Decision 52](#decision-52), [decision 109](#decision-109) |

**Facts.**

- The only embedder of `embed` is SBERT from `rust-bert`, and the only index of `ann` is the Chroma mirror.
- Both crates come only with the feature `chromadb` (`rholang/Cargo.toml:100`).
- The base registers its Chroma processes only with the feature (`interpreter/rho_runtime.rs:1513-1514, 1604-1605`).

**Answer to "why not".** The main session had two reasons for every build: a replay never calls a service, and the mock suite ran by default.
These reasons do not justify a divergence from the base.

**Statement.**

- The registration of `embed` and `ann` and their channel rows depend on the feature (`P6_PROCESSES`, `reified/system_channels.rs:488-493`).
- The VectorDB factory, its exact queries, the query param `ids=` and its config stay in every build.
- The tests of `embed` and `ann` need `--features chromadb`.

**User's words.** "I don't quite follow. If embed and ann are chroma-specific then shouldn't they be gated behind its feature flag? If not, why not? Also, the VectorDB Reified RSpace type is not coupled with ChromaDB and it, itself, should not be gated by the chroma feature flag."

**Reason.** The two processes need the crates of the feature, as the Chroma processes of the base do.
The VectorDB space type needs neither crate, so it stays in every build.

### <a id="decision-114"></a>Decision 114: The mirror is an oracle store with no repair, as the oracle mode of File I/O

| Field | Value |
|---|---|
| Date | 2026-10-27 |
| Status | In force |
| Replaces | The reconciler sentence of [decision 58](#decision-58), the watermark of the reply of `ann` in [decision 57](#decision-57), the reconciler spawn of [decision 61](#decision-61), the reconciler answer of [decision 66](#decision-66), and the reconciler example of [decision 70](#decision-70) |
| Retires | The reconciler, the watermark, the dirty marks of the writer and the channel registration of the former rules MR-4 to MR-6 |
| Related | [Decision 98](#decision-98), [decision 109](#decision-109), [decision 115](#decision-115), DR-114 of the base |

**Facts.**

- The oracle mode of the File I/O FIPS lets a node own its store. The other nodes check the processing, not the accuracy (`2026-02-06-File-IO.md:35-37`).
- The FIPS defers the sync mode (`2026-02-06-File-IO.md:39-44`).
- The File I/O code of the base undoes no file content after a failure or an orphan, in either mode. Only the descriptor table rolls back (`io/handle_table.rs:915-919, 1041-1054`).
- DR-114 runs the Chroma writes in play only, and "Nothing external is undone on rejection or orphaning" (DR file, lines 12801 and 12959-12961).
- The prediction-markets paper asks for a non-repudiable commitment, not replicated determinism (`prediction_markets_cost_accounted_rho_20260629.tex:354-389`).

**Statement.**

- Only a play writes the mirror ([decision 98](#decision-98)), and nothing repairs it.
- The writes of failed deploys, dry runs and dropped blocks stay, and the records of other proposers never arrive.
- Retired: the [reconciler](glossary.md#reconciler) (former rule MR-5, `mirror_reconciler.rs` and its spawn in `node/src/rust/runtime/setup.rs`).
- Retired: the [watermark](glossary.md#watermark) (former rule MR-6, and the last field of the reply of `ann`).
- Retired: the [dirty marks](glossary.md#dirty-collection) of the writer, and the channel registration that only the reconciler read (former rule MR-4).
- A batch that finds the queue full, or that fails after its attempts, is dropped.
- Kept: the bounded queue of the writer, its attempts, and the rule of decision 58 that a publication never waits (rule MR-4).

**Code.** The line `pub mod mirror_reconciler;` stays as a comment with a restore note, and the file stays ([decision 120](#decision-120)).

**Model.** `ChromaMirror.tla` retires `ReconciledEqualsFinalizedView` and `WatermarkHonest` with their controls `OrphansKeptUnsafe` and `WatermarkAfterFailureUnsafe`.

**User's words.**

- "Model external service integration like Chroma DB after Mike's File I/O FIPS and implementation and any related publications (../publications/). If the desired behavior is not clear from the File I/O API, then ask again."
- "Oracle, no repair (Recommended)". The recommendation was the main session's.

**Reason.** The File I/O FIPS and DR-114 give one rule for an external store: play writes it, and nothing undoes the write.
The mirror is such a store, and only the query param `ids=` turns its advice into a decision, so consensus needs no repair.

### <a id="decision-115"></a>Decision 115: The reply channel of `embed` and `ann` stays first, as the File I/O sugar requires

| Field | Value |
|---|---|
| Date | 2026-10-27 |
| Status | In force |
| Amends | None. It confirms rule SP-1 of the [specification](specification.md) |
| Related | [Decision 57](#decision-57), [decision 114](#decision-114), [decision 116](#decision-116) |

**Base facts.**

- The user call form of File I/O puts the reply channel first.
- The desugaring of a send-receive puts the temporary name first: "x <- name!(args) becomes new temp in { name!(temp, args) | x <- temp }" (`p_input_normalizer.rs:174`).
- A method send puts the temporary name first and the method literal second (the `SourceDesc::SM` form of the parser).
- The native File I/O layer takes its acknowledgement last (`io/handler_trait/dispatch.rs`, `File.rho` and the signature table of DR-114).
- Only genesis uses that layer, because `eval_new` filters the `rho:io:fs:native:` names from user deploys (`io/mod.rs:94-108`, `interpreter/reduce.rs:2606-2627`).

**Statement.** `embed` and `ann` are user processes, so rule SP-1 stays: the reply channel comes first.
The reply shapes `[true, ...values]` and `[false, code, msg]` of the base stay (`io/response.rs:1-6`).
The evidence is in pgmcp note 25265 (items 1 and 2), and the plan of the mirror cluster confirms it.

**User's words.** "Settle design decisions about external services like chroma db with the file i/o api everywhere possible"

**Reason.** A user program calls `embed` and `ann` with the same call forms as the user calls of File I/O.
Those forms put the reply channel first.

### <a id="decision-116"></a>Decision 116: `embed` and `ann` keep their own handlers and take the DR-114 contract from one helper of the base

| Field | Value |
|---|---|
| Date | 2026-10-27 |
| Status | In force |
| Amends | The mechanism clause of [decision 104](#decision-104). The behavior of decision 104 stays |
| Related | [Decision 105](#decision-105), [decision 115](#decision-115), DR-114 of the base |

**Base facts.**

- The File I/O handlers keep state (`FsProcesses`), their own closures and their own dispatcher (`fs_handlers_to_definitions`, `interpreter/rho_runtime.rs:1637-1644`).
- They are not in `is_native_external_process`.
- `embed` and `ann` keep state in the same way.
- The checks of `ann` in play need the Reified runtime and its agent, and `SystemProcesses` has no Reified runtime (`interpreter/system_processes.rs:1099-1114`).
- So the driver `native_recorded_call` of the base cannot serve them.

**Statement.** The two processes keep their closures and take the contract of DR-114:

- the check of the arity
- in replay, `BugFoundError` for an empty record and for a reply that is not plain data
- in replay, the delivery of any other record, with no check of consensus state
- in play, the checks, the service call, `native_played_reply` and the codes of [decision 105](#decision-105).

**Mechanism (option A1).** A new helper of the base, `native_replayed_reply`, holds the two checks of replay (`interpreter/system_processes.rs:763-781`).
`native_recorded_call` and the two Reified handlers call it.
The codes `EXT_BAD_ARG` and `EXT_FAILED` and the helper `native_failure_reply` become `pub(crate)`.
`is_native_external_process` keeps its Reified exclusion, with a new reason (`interpreter/system_processes.rs:650-666`).

**User's words.** The directive of [decision 115](#decision-115), with [decision 97](#decision-97).

**Reason.** The File I/O processes show the form of the base for processes with state: their own handlers.
One shared helper gives the two Reified handlers the exact replay checks of the base.

### <a id="decision-117"></a>Decision 117: A positional failure after a random pick stages the operation again in DR-75 order (R-A)

| Field | Value |
|---|---|
| Date | 2026-10-28 |
| Status | In force |
| Related | [Decision 37](#decision-37), [decision 92](#decision-92), [decision 94](#decision-94), [decision 102](#decision-102), [decision 118](#decision-118) |

**User's words.** The user answered question Q-102-3 three times:

1. "Why is it that the base branch does not model similar behavior? Is it that the behavior does not need to be modeled? Answer me and ask the question again."
2. "If it is a guarded join, why not take the elements of the join as they arrive to maximize throughput?"
3. "Go with your recommendation so-long as it maximizes parallelism/concurrency, throughput, and minimizes blocking time. Otherwise, go with the option that does. F1r3node is inherently concurrent and maximizing parallelism/concurrency and throughput and minimizing blocking time and ultimately time-to-completion are first-class design and architectural concerns. Never serialize or oversimplify a solution for convenience, but keep solutions minimalistic while achieving their goals. These are not paradoxical as Occam's Razor states solutions should be kept as simple as possible but no simpler!"

**Base facts.**

- An operation of the base pays its introduction before the search (`RejectedIntroduction`, `rspace++/src/rspace/replay_rspace/native_directive.rs:168, 262`).
- A refused guard tries the next continuation (`space_matcher.rs:357-360`).
- A store of the base has no bound and no later charge, so it cannot fail. Replay checks `Stored` with E3-a (`native_candidate.rs:86-89`).
- So the base has no positional failure.

**Reified facts.**

- Bounded collections (the VectorDB capacity and the cell) and theory leases (TH-9) can fail a staging.
- [Decision 92](#decision-92) then commits only the anchor (`prepare_failure`, `spaces/generic.rs:4958-4983`).
- In production, only a guarded join in a bounded VectorDB space can fail in this way, after a pick among tied scores.

**Statement.**

- A [positional failure](#terms-used-in-this-register) is `CellOccupied`, `CapacityExceeded` or `NestedCommitGuard` (`spaces/errors.rs:350-354`).
- When a staging after a random pick fails with a positional failure, play stages the operation again in DR-75 order.
- That staging commits, or it fails with its own error through `prepare_failure`.
- The replay already stages an operation without a recorded pick in DR-75 order. So the retry needs no new rule in replay (S5 of [decision 119](#decision-119)).

**Comparison by the criteria of the user.**

- R-B (DR-75 order for the score ties of guarded joins) fixes the order of ties. Concurrent guarded joins then pick the same record and conflict at merge.
- The guard search of [decision 37](#decision-37) grows with the product of the candidate counts, and it has no phlo charge (DR-131).
- That search runs under the space token `C(sid)` in play and in replay ([decision 94](#decision-94)). So it adds blocking time to every deploy of the space.
- R-A keeps the random picks, adds one staging only after a positional failure, and adds no work to replay.

**Kept.** The guard rule of decision 37 stays at P12 (pgmcp note 25365).

**Reason.** R-A meets the criteria of the user best: it keeps concurrent picks apart, and it adds work only on a rare failure path.

### <a id="decision-118"></a>Decision 118: The throughput rule of decision 117 also decides the join rules of decision 103

| Field | Value |
|---|---|
| Date | 2026-10-28 |
| Status | In force |
| Amends | [Decision 103](#decision-103): the fact of the base "the pass never retries the incoming datum at a later position" is not a Reified rule |
| Replaces | Option A of the arbiter for question Q-103-3, and the comment-out of the set cascade by the arbiter (question Q-103-4) |
| Related | [Decision 50](#decision-50), [decision 117](#decision-117), [decision 119](#decision-119) |

**Statement.**

- Kept: the retry of the incoming datum at each of its positions (rule JN-9), rule JN-14 and invariant V26 as a whole.
- Kept: the set cascade (rule JN-10).
- Each kept mechanism removes a pending match that would block a waiter.
- Adopted from the base: a persistent entry is never claimed, so it can bind several positions (`space_matcher.rs:452-460`). This rule adds fires.
- Replay: the repeated collections of a join replay with the same deterministic lists of E1.

**User's words.** Words 3 of [decision 117](#decision-117).

**Reason.** Each kept mechanism fires a match that would otherwise wait, so it adds throughput and removes blocking time.
The base rule for persistent entries also adds fires.

### <a id="decision-119"></a>Decision 119: The design of decisions 102, 103 and C4 takes the changes of the arbiter

| Field | Value |
|---|---|
| Date | 2026-10-28 |
| Status | In force |
| Related | [Decisions 99 to 103](#decision-99), [decision 111](#decision-111), [decision 117](#decision-117), [decision 118](#decision-118) |

**Fix of the blocking defect G1.**

- `class_candidates` puts the representative first (`join.rs:6223-6286`), and a fire of width 1 dispatches only the first result (`spaces/generic.rs:2854-2965`).
- A COMM names its produces as a sorted set (`event.rs:56-61`), so replay cannot find a random representative.
- Rule: every member, the representative included, takes its place in DR-75 order, and the fire dispatches the first.
- The rule goes into `join.rs`, so the PathMap store gets it too. Rule ST-5 is restated.

**Reductions.**

- S1: `recorded_comms` and `admits` also serve native replay, with one query for the end of the record.
- S2: one method of a base trait, with a default body, returns an `Option`. The record holds no identities of refusals.
- S3: the E3-a check of the base, word for word. The risk K6 of the design plan, the proof `stored_check_sound` and the invariant `StoredIsHonest` are dropped.
- S4: one shuffle for each pick with `rand::rng()`, with no seed and no rank.
- S5: the replay half of R-A needs no new rule.
- S6: the model M1 of the design plan is dropped.
- G7: `select_slot` maps `Unavailable` to `None`.

**Gaps to close.**

- G2: [decision 118](#decision-118) restates it. V26 stays whole, and its bounds count the persistent records at several positions.
- G3: the lane window takes the incoming datum first.
- G4: a test of a persistent loop.
- G5: peeks under Rule E with two waiters.
- G6: a scenario of `SpaceReplayOrder.tla` for the risk K8 of the design plan.
- G8: the cost.
- G9: a theory as a veto in joins.
- G10: the case of the cell, which is empty.

**Open points that the base decides.**

- Q-102-1: DR-75 order for the members of a list and for ties.
- Q-102-2: random picks.
- Q-C4-1: the anchor first for a deviation job, and the scenario for K8.
- Q-103-1: the incoming datum first, then the head order.
- Q-103-2: rule R6 in every mode.

**Tests.**

- the runtimes `[0, 2, 8]` of [decision 111](#decision-111)
- the analog of the test F7 of DR-129: a vetoed bag join on `[c, c]` restores every datum, in 64 runs
- the analog of the racing test of DR-132
- the analog of the test F6 of DR-129, with a check that it is not vacuous
- in that test, each random family shows at least 2 picks, and each head-only family shows exactly 1
- a negative test with a forged `Stored`
- equal charges, and run counts with bounds on the misses.

**Basis.** [Decision 97](#decision-97). The main session checked each load-bearing claim of the arbiter on the tree (pgmcp note 25284).

**Reason.** The defect G1 blocks the replay of a random set member, so its fix is required.
The reductions remove parts of the design that the base already provides.

### <a id="decision-120"></a>Decision 120: The open points of decision 112 follow the base

| Field | Value |
|---|---|
| Date | 2026-10-28 |
| Status | In force |
| Related | [Decision 112](#decision-112), [decision 114](#decision-114), [decision 121](#decision-121) |

**Statement.**

- O-1, the transport of params: an `Expr` arm `e_params_body = 45` of the type `SendParams`. Only the reducer writes it, for factories and handles. An internal PARAMS tuple would keep marker code live.
- O-2, Reified forms in patterns: the normalizer rejects them, as the base rejects its cost syntax in a pattern (`pattern_guard.rs:1-78`). No source of the 44 rows of the corpus has such a form in a pattern.
- The forms of O-2 are a use block, a free theory, params on a send, a bind with a query and an annotated name declaration.
- O-3, the estimate: params take the size rule of the datum (`estimate.rs:1716-1739`), and a query takes the rule of its bind (`estimate.rs:1856-1887`). The escape check runs on their names, and the present classes stay.
- O-4: `Send` gets no `reserved 4;`. The statement changes no byte, and no rule asks for it.
- O-5, the branch of `new`: `eval_new` runs the fs-native filter of the base on every `new`. Then it branches to `eval_new_space` for an annotated `new`.
- The base filters every `new` "so user code cannot bypass Fs.rho's sandbox" (`interpreter/reduce.rs:2606-2627`). Today `eval_new_space` resolves URIs through `allocate_new_bindings` without the filter (`reducer_hooks.rs:1351-1360`). A test proves that an annotated `new` with an fs-native URN fails.

**Captures.** R0 is the merged working tree right before the edits of decision 112, and R1 is the same tree after them.
No other edit comes between the two captures.
The precedent is `reified-queue-parity-v1.json`, which records `"captured_at_revision": "merge of 702496775"`. So the merge commit does not have to come first.

**The retirement of the mirror reconciler.** The line `pub mod mirror_reconciler;` stays as a comment with a restore note, and the file stays (option R2).
This follows the user rule "Never "disable" code by deleting it!".

**Order of work.** The protobuf change comes after the other merge decisions, as one block.
Then R0 holds every other change, and the masked comparison of the v1 and v2 vectors shows only the effects of decision 112.
Before that block, a change that alters a vector captures it again in place, as the merge did.
[Decision 121](#decision-121) decides item B-1 of the plan.

**Basis.** [Decision 97](#decision-97).

**Reason.** The base already answers each open point with a mechanism of its own, so the protobuf change needs no new rule.

### <a id="decision-121"></a>Decision 121: Decision 112 also changes two Casper source files beyond the constant

| Field | Value |
|---|---|
| Date | 2026-10-28 |
| Status | In force |
| Amends | [Decision 112](#decision-112), which approved only the change of `empty_state_hash_fixed` in `casper/src` |
| Related | [Decision 70](#decision-70), [decision 120](#decision-120) |

**Statement.**

- `casper/src/rust/util/rholang/costacc/vault_payer.rs`: `single_private_name` adds `&& par.use_blocks.is_empty()`.
- The test of the base lists every field of `Par`, its own new fields included (`cost_signed_terms`, `cost_stacks` and `conditionals`). Without the line, a `Par` with one private name and a use block would become a valid payer.
- `casper/src/rust/util/rholang/costacc/direct_wallet_funding/execution/gate/tests.rs`: the literals of `Send` (line 910), `ReceiveBind` (lines 920-926) and `New` (lines 935-941 and 1088-1094) name the new empty fields. Then the Casper tests compile.
- No other file of `casper/src` changes, except for `empty_state_hash_fixed` ([decision 112](#decision-112)).
- Each edit keeps the old text as a comment with a "Changed by decision 112" note.

**User's words.** "Approve both edits (Recommended)". The recommendation was the main session's.

**Reason.** The test of the base asks for every field of `Par`, so a new field needs its line.
The literals of the test name every field, so the tests compile only with the new fields.

### <a id="decision-122"></a>Decision 122: The open points U1, U4 and U5 of C4 follow the base and dev

| Field | Value |
|---|---|
| Date | 2026-10-29 |
| Status | In force |
| Related | [Decision 90](#decision-90), [decision 91](#decision-91), [decision 92](#decision-92), [decision 100](#decision-100), [decision 117](#decision-117), [decision 119](#decision-119), [decision 123](#decision-123) |

**Base facts** at `702496775`. The snapshot S_r of a row r is the owner ledger at the budget start of r.

- B1: a row tests its candidates in the random order of play. An item failure at S_r is a miss. A refused tuple is recorded, and the search tries the next continuation (`NativeOwnerRefusal.tla:46-62`, `rspace/accounting.rs:248-277` and `native_runtime/operations.rs:364-365`).
- B2: a refused consume waits (`ops_consume.rs:272-274`).
- B3: a COMM removes its matched data in the same locked operation (`ops_produce.rs:374-377`).
- B4: replay is a function of the record. It follows the recorded pick (`ops_produce.rs:553`, DR-129), and it reads no live ledger (`NativeOwnerRefusal.tla:85`). Its Stored check skips the continuations that can veto (E3-a, `rspace/match.rs:26-33`).

**Statement.**

- U1 is option K of the arbiter, by B3. The retire group of the actual fire commits in the transaction of the fire, right after the fire row. A refused retire parks inert ([decision 100](#decision-100)).
- U4 is option (B) of the arbiter, the finish with no mark, by B4. After a deviating row, play finishes with the same function of the actual receipt that replay computes.
  - Where the plan stages nothing more after the actual outcome, play ends with the return of the plan.
  - Where the plan would stage more, play stops. It builds the re-issue as replay builds it at the stage call that answers `RecordEnded`.
- CE1 of [decision 92](#decision-92) decides every anchor. Rule CR-19 applies to a re-issue that a stop creates, because the first row of that re-issue is not unique to its job.
- U5 is yes, by the replay half of [decision 117](#decision-117) and by [decision 91](#decision-91). R-A also applies to `SeqConcurrent` after a random pick. The abort of a seq space stays.
- U2 stays option (a) ([decision 90](#decision-90)). U3 stays option (i), as in the base (`space_matcher.rs:452-460`).
- The base rules reject three options:
  - re-planned rows in the same transaction after the deviating row, because they need a resumption of the plan, and the plan code is straight-line
  - end anchors, because they add one COMM to each transaction
  - a replay probe, because it is unsound when the job commits nothing.

**Process.** A Plan agent writes the plan of C4. An independent arbiter checks each version of the plan before the implementation starts.

**User's words.** For U1: "What is the closest behavior on dev or feature/cost-accounted-rho? If there is similar behavior, align with it!" For U4: "… align with dev and feature/cost-accounted-rho, all the edge cases should already be modeled!" For U5: "… align with dev and feature/cost-accounted-rho!" Then: "Align everything related to casper/consensus as closely as possible with dev and feature/cost-accounted-rho, there should be no need to reinvent the wheel!"

**Basis.** [Decision 97](#decision-97). The evidence is pgmcp note 25521.

**Reason.** The base already models each case. B3 decides U1, B4 decides U4, and decisions 117 and 91 decide U5. So C4 needs no new mechanism for these points.

### <a id="decision-123"></a>Decision 123: Consensus behavior that Reified needs comes from dev and base code behind traits

| Field | Value |
|---|---|
| Date | 2026-10-29 |
| Status | In force |
| Related | [Decision 108](#decision-108), [decision 112](#decision-112), [decision 121](#decision-121), [decision 122](#decision-122) |

**Statement.**

- The owner checks, the refusal records, the candidate order, and the Stored and refusal checks of native replay come from base functions behind traits.
- Native RSpace and the Reified disciplines then run the same code. Native behavior stays identical, and the existing base tests and models show it.
- The targets are these base functions:
  - the produce and consume searches with their admissions (`ops_produce.rs`, `ops_consume.rs` and `MatchAdmission`)
  - the commit steps (`process_match_found`)
  - the replay selection (`native_candidate.rs` and `native_candidate/selection.rs`).
- The comments of the Reified copies in `space_play.rs` and `native_session/space.rs` say that they mirror these functions. These copies call the generalized code instead.
- A Reified-only mechanism stays only where no base trait can carry it, with its reason. The finish after a deviating row ([decision 122](#decision-122)) is the expected case.
- No consensus source file changes beyond [decision 112](#decision-112) and [decision 121](#decision-121). No consensus test changes unless Reified needs the change. The offer to restore the alternating proposers of the N4 chain row ([decision 108](#decision-108)) is withdrawn.

**User's words.** "you seem really invested in casper / consensus for some reason, even though it has very little to do with reified rspaces; if you are insistent on doing things with casper / consensus, then you should work on generalizing the dev / feature/cost-accounted-rho behavior with traits and reusability patterns!"

**Basis.** [Decision 97](#decision-97).

**Reason.** One code path for native and Reified rows keeps their behavior equal by construction. The base tests and models then check the shared code, and no Reified copy can drift from it.

### <a id="decision-124"></a>Decision 124: The determinism rules state record and replay, as the base does

| Field | Value |
|---|---|
| Date | 2026-10-29 |
| Status | In force |
| Amends | The rules DT-1, DT-4 and DT-7 of the [specification](specification.md) and the control SC-18 of the [security document](security.md), for racing evaluation |
| Related | [Decision 89](#decision-89), [decision 92](#decision-92), [decision 93](#decision-93), [decision 111](#decision-111), DR-132 of `feature/cost-accounted-rho` (commit `97d020154`) |

**Base facts** at `702496775`.

- The consensus proposal: "Under the random pick, two honest executors can reach different roots. Since 2026-10-13, §7 therefore records and replays (§7.1)." (`docs/casper/theory/cost-accounting-consensus-proposal.md:706`)
- DR-132: "The record decides every order-dependent choice: the DR-113 grants, the DR-114 room race and the DR-129 pick." (`docs/casper/theory/cost-accounting-decision-records.md:16833-16834`)
- Theorem 4 of DR-132: "All replay schedules of one journal and one term reach the same verdict." (`docs/casper/theory/cost-accounting-decision-records.md:16975-16976`)

**Statement.**

- Racing play decides which entries each operation sees. The journal of the deploy records that interleaving ([decision 92](#decision-92)), and native replay makes each choice of play again.
- Rule DT-1 holds for the entries that an operation sees. No such choice depends on the order of a list in memory, on the order of a hash map, or on the timing of threads.
- Rule DT-4: the cost of a deploy is a function of the deploy, its pre-state and its journal. Replay of the journal reproduces that cost (rule NE-3).
- Rule DT-7: for one journal, a restarted node pays what a node without a restart pays, and both reach the same root.
- Control SC-18: every choice of a space depends only on the entries that it sees and on the deploy. Replay makes each choice of play, whatever its timing. The test `space_racing_replay_agrees_with_play` joins the evidence of the control.
- No failure closes the epoch of a replay session. A failed operation ends with its own error, and a waiter of its row parks as a counted waiter of the base stall rule. A forged record ends the replay with `Incomplete` or `Stalled`.
- [Decision 93](#decision-93) restates the rules DT-3 and DT-5 for racing evaluation. [Decision 89](#decision-89) retires the host-work sentences of the rules DT-4, DT-5, DT-7 and NE-2, and the host-work row of Table 12.

**Measured.** Two plays of one deploy from one pre-state can reach different roots in this branch too.
Block 2 of the stored PathMap family runs a take on the prefix `a` and a store on the path `[b, 3]`.
These operations are two transactions of one space.

In 20 diagnostic runs of `space_lazy_attach`, the warm play and the cold play reached different roots in some runs.
The roots differed exactly in the runs where the two plays did not log the same events in any order.
The deliveries were the same in every run, and every play certified.
The evidence is in pgmcp notes 25608, 25618 and 25622.

**Basis.** [Decision 97](#decision-97).

**Reason.** The base already models racing determinism by record and replay. So the determinism rules state the same model, and Reified adds no mechanism for it.

### <a id="decision-125"></a>Decision 125: A legacy replay test that fails is restated at the step where it fails

| Field | Value |
|---|---|
| Date | 2026-11-01 |
| Status | In force |
| Amends | None. It sets when a legacy test moves to native replay under [decision 90](#decision-90) |
| Related | [Decision 69](#decision-69), [decision 90](#decision-90), [decision 118](#decision-118) |

**Statement.**

- Merge 2 restates a test of the legacy replay modes at the step where the test fails.
- That step moves the test to native replay through the harness of `rholang/tests/support/native.rs`, or it retires the test.
- The harness has the functions `native_play`, `certify` and `assert_reproduces`.
- The modes `SpaceMode::Replay` and `SpaceMode::Reporting` stayed unchanged until [decision 90](#decision-90) (C6, step D) retired them.
- The test `pathmap_replay_equal` of `rholang/tests/reified_pathmap.rs` now certifies each PathMap case through native replay.
- The test `join_seams_keyed_differential` of `rholang/tests/reified_join_props.rs` stays as a comment, under a label that names this decision.

**User's words.** "Restate when they fail (Recommended)". The option read: a legacy replay test that fails is restated at the step where it fails (native replay, or retired).

**Reason.** Merge 2 retires the legacy modes anyway, so a test that fails there takes its planned restatement at once.
The legacy modes then need no repair.
The golden vector and the stored logs of `join_seams_keyed_differential` predate [decision 118](#decision-118), and its replay half needs a mode that merge 2 retires.

### <a id="decision-126"></a>Decision 126: Behavior that varies by space kind goes through trait interfaces

| Field | Value |
|---|---|
| Date | 2026-11-01 |
| Status | In force |
| Amends | None |
| Related | [Decision 123](#decision-123), [decision 127](#decision-127), [decision 128](#decision-128), [decision 134](#decision-134) |

**Statement.**

- Behavior that varies by space kind goes behind a method of an existing trait.
- The method has a default for the common case and an override for each kind that differs.
- A new trait comes only when no existing trait can carry the variation. `RowEnd` of [decision 134](#decision-134) is such a trait.
- New code in a shared layer names no space kind and adds no branch on a kind.
- The shared layers are `rspace++/src`, `casper/src`, the native runtime under `rholang/src/rust/interpreter/accounting/` and the shared engine in `spaces/generic.rs`, `spaces/join.rs`, `spaces/record.rs`, `spaces/counter.rs` and `spaces/settle.rs`.
- The existing traits are these:
  - `OuterStore` and `Discipline` in `spaces/generic.rs`
  - `JoinSource` in `spaces/join.rs`, `ScanDiscipline` in `spaces/disciplines/scan.rs` and `SpaceTheory` in `spaces/theory/mod.rs`
  - `SpaceTxn`, `SpaceTxnHost`, `TxnStore`, `TxnBackend`, `RowSearch` and `RowItemRule` in `rspace++/src/rspace/space_store_handle.rs`.

**User's words.** "You are generalizing the code you absolutely must amend for reified rspaces to use abstraction and good engineering / design / architectural patterns that will scale to new reified rspaces, right? Things like traits and such are being used as interfaces, right?" and "Are the refactors you've made outside of the reified rspace implementations, themselves, already generalized to arbitrary reified rspaces including those that have not been implemented yet?"

**Reason.** A kind that does not exist yet then needs no edit of a shared layer.
[Decision 127](#decision-127) applies the rule to the registry of kinds, and [decision 128](#decision-128) applies it to the shared engine.

### <a id="decision-127"></a>Decision 127: The registry of space kinds and the VectorDB services generalize to arbitrary Reified RSpaces

| Field | Value |
|---|---|
| Date | 2026-11-01 |
| Status | In force |
| Amends | None |
| Related | [Decision 109](#decision-109), [decision 126](#decision-126), [decision 128](#decision-128) |

**Statement.**

- A new Reified space kind plugs in with trait implementations and one registration. It edits no shared layer unless it brings a new join window or bound type ([decision 128](#decision-128)).
- The registration is one `#[distributed_slice(SPACE_KINDS)]` static of the type `SpaceKind` in `spaces/families.rs`, as an FS handler registers in `FS_HANDLERS`.
- `SpaceKind` names the families of the kind, its system processes and the installer of its external services.
- The installer takes `is_validator`, so an observer installs the unavailable services ([decision 109](#decision-109)).
- The closed enums are gone: `SpaceOuter` and `SpaceInner` of `models/src/rust/space_factory_urn.rs` are name newtypes, and `Family::new` builds a `Family` struct.
- The built-in families stay in the const `Family::WIRED`, and `Family::registered` chains them with the families of the registered kinds.
- In production the slice holds one entry, `VECTORDB_KIND` of `reified/vectordb_services.rs`, the installer of the VectorDB services.
- The library test `space_kind_registry_holds_the_wired_families` pins this state: without a plug-in, the registry is the wired families in order.
- `check_space_kinds` checks the registry in the form of the base check of `CONSENSUS_FOLD`: unique URNs, unique free ops and the factory of each family.
- `setup_node_program` runs that check at node start, before any store opens, and a broken registry stops the start.
- The test `rholang/tests/reified_kind_plugin.rs` registers a kind with a new outer store, a new discipline, a recorded system process and an installer of services.

**User's words.** "they need to be generalized -- use a plan agent to generalize them and stick to the minimalism I have enforced: only change what is necessary for the completion of the reified rspaces including their generalized integration into all of f1r3node, but only change what is absolutely necessary and keep the behavior aligned with dev and feature/cost-accounted-rho (change as little as possible while completing my requirements)".

**Reason.** The closed enums of the kinds and the VectorDB runtime services named specific kinds, so a new kind needed edits of shared code.
The base and dev register their FS handlers through a distributed slice (`FS_HANDLERS`), so the registry takes the same form.

### <a id="decision-128"></a>Decision 128: The shared engine reaches kind-specific behavior through the existing traits

| Field | Value |
|---|---|
| Date | 2026-11-01 |
| Status | In force |
| Amends | None |
| Related | [Decision 126](#decision-126), [decision 127](#decision-127), [decision 134](#decision-134) |

**Statement.**

- The engine spots that named a kind go behind defaulted methods of the existing traits `Discipline` and `OuterStore`, with the same behavior.
- `OuterStore` gives the routed layout and the random-tail names as defaults, so `HashMapOuter` of `spaces/outer/hashmap.rs` declares only its name.
- `Discipline` gives the steps of the lanes window and of the similarity window. The defaults fail closed for a discipline without the window.
- The modules `spaces/disciplines/lanes.rs` and `spaces/disciplines/similarity.rs` own the window types, and the kind modules re-export them under their old names.
- The associated type `JoinSource::Discipline` names the discipline of a source, and the engine calls the steps of the window through it.
- `families::is_policy_key` gives the query keys of a parked waiter in place of the constant `VDB_QUERY_KEYS`.
- A new join window or a new side bound needs an edit of the engine, because it is new engine semantics. `JoinWindow` and `SideBound` of `spaces/generic.rs` are these types.

**User's words.** "Route via traits (Recommended)".

**Reason.** The verification of [decision 127](#decision-127) found engine spots that still named a kind.
The existing traits carry these spots, so the engine no longer names a kind, and the step adds no new trait and no new mechanism.

### <a id="decision-129"></a>Decision 129: The finish after a deviating row follows the rule of the base for a refused tuple

| Field | Value |
|---|---|
| Date | 2026-11-03 |
| Status | In force |
| Amends | None. It keeps U4 of [decision 122](#decision-122) |
| Related | [Decision 122](#decision-122), [decision 123](#decision-123), [decision 133](#decision-133) |

**Statement.**

- A row deviates when its commit gives an outcome other than the staged one, for example after an owner refusal.
- The base decides the follow-up of a refused tuple inside the same locked operation (rule B1 of [decision 122](#decision-122)).
- The finish after a deviating row follows that rule.
- The parts of the finish that read the store run read-only at the commit of the row, under the same commit hold.
- These parts are the settle of the stack and of the priority queue, the cascade of a join and the list tail of VectorDB.
- Each part answers through `RowSearch::tail_read`, and its default answers `TailRead::Ends`.
- The receipt carries the answer in the field `tail_read` of `RowDeviation`, beside the field `vetoing`.
- The parts that read nothing stay pure functions of the receipt: `after_fire` with the fire count, and the restore rule of a scan.

**User's words.** "dev and feature/cost-accounted-rho might not have this specific case, but I would be surprised if they don't have a similar case for the default bag-based rnode; review again and ask again only if it is not solved by dev or feature/cost-accounted-rho; dev is mirrored to `the mirror of dev`".

**Reason.** The base decides this follow-up under the lock of the operation, so merge 2 needs no new rule.
The question did not return to the user.
A form that stops after the deviating row is unsound, because play would record a job that replay never runs.

### <a id="decision-132"></a>Decision 132: Plan item C7.12 is withdrawn, so decision 43 stays in every kind

| Field | Value |
|---|---|
| Date | 2026-11-04 |
| Status | In force |
| Amends | None. It keeps [decision 43](#decision-43) |
| Related | [Decision 43](#decision-43), [I-21](#i-21) |

**Statement.**

- C7.12 was an item of the plan of merge 2. It made a re-issued offer keep its `ord` when the element was not persistent and the fire did not spend it.
- The user withdrew the item, so a re-issued element that is not persistent takes a fresh `ord` in every kind.
- `offer_head` of `spaces/generic.rs` and `offer_pq` of `spaces/disciplines/priority_queue.rs` keep `reissued_ord: op.persistent.then_some(ord)`.
- `resume` of `spaces/join.rs` keeps `reissued_ord: produce.persistent.then_some(ord)`.
- The three tests of the item stay as comments under the label "Disabled by decision 132". They are `reissue_keeps_the_ord_of_an_unspent_element`, `priority_queue_reissue_keeps_the_ord_of_an_unspent_element` and `join_reissue_keeps_the_ord_of_an_unspent_element`.
- The test `space_queue_nonpersistent_reissue_takes_a_fresh_ord` of `spaces/generic.rs` and the property `prop_queue_nonpersistent_reissue_fresh_ord` of `rholang/tests/reified_p4_props.rs` pin the rule of [decision 43](#decision-43).

**User's words.** "Withdraw C7.12 (Recommended)".

**Reason.** The plan item changed every run, also a run without owner funding.
The theorem `nonpersistent_first_ord_reentry_breaks_quiescent_inv` of `HeadOnlyDisciplines.v` proves the fault: a non-persistent element that keeps its first `ord` breaks the quiescent invariant at a deploy boundary.

### <a id="decision-133"></a>Decision 133: The unit loop commits the finish rows of a cut row, and native replay follows them

| Field | Value |
|---|---|
| Date | 2026-11-04 |
| Status | In force |
| Amends | None |
| Related | [Decision 122](#decision-122), [decision 123](#decision-123), [decision 124](#decision-124), [decision 129](#decision-129), [decision 134](#decision-134) |

**Statement.**

- The commit of a space transaction can stop at a row, for example after an owner refusal. That row is the cut row.
- The row search of a cut row builds its finish rows at the commit (`RowSearch::finish_rows` and `FinishRow`).
- The default of `finish_rows` answers that the row owes no finish.
- The unit loop of `rspace++/src/rspace/space_store_handle.rs` stages each finish row and commits it after the group of the cut row.
- A PathMap join gives these rows: the restore of each owed marker, then the bump of the token when one is due (`PrefixSource::finish_rows`).
- In native replay, the unit of a cut row also holds the later staged rows whose sources follow its finish rows (`follows_finish`).
- After a record end, the last row with a search can be the cut row when its later rows follow its finish rows (`tail_candidate`).
- A run without owner funding has no row search, so it does no new work.

**User's words.** "Extend the unit loop (Recommended)".

**Reason.** A cut PathMap row owes the restores of its markers and the bump of its token.
Without these rows, a later read of a datum whose marker was not restored fails with `Inconsistent` (`locate_marker`).
The change adds one defaulted method of `RowSearch`, one arm of the unit loop and one rule of native replay.

### <a id="decision-134"></a>Decision 134: The end of a refused lift varies through the trait `RowEnd`

| Field | Value |
|---|---|
| Date | 2026-11-04 |
| Status | In force |
| Amends | None |
| Related | [Decision 123](#decision-123), [decision 126](#decision-126), [decision 128](#decision-128), [decision 129](#decision-129), [decision 133](#decision-133) |

**Statement.**

- A join can see its cascade lift row refused at the commit. The row then ends with the stop of the engine at the lift.
- `StagedRowSearch<D: Discipline, E = ()>` of `spaces/generic.rs` takes an end `E: RowEnd`.
- The trait `RowEnd` has the methods `tail_read`, `tail_reissue` and `finish_rows`.
- The impl for `()` gives the base answers: `TailRead::Ends`, no re-issue and no finish rows.
- Each join kind writes only its end: `KeyedLiftEnd` in `spaces/join.rs` and `PrefixLiftEnd` in `spaces/pathmap/join.rs`.
- `JoinSource::lift_search` has no default, so the build of a new join kind fails until the kind gives its own end.
- No production type forwards the methods of `RowSearch` or wraps the search.

**User's words.** "Go with the most principled and generalized solution, use a plan agent if needed".

**Reason.** The row search already varies its tail read through the type parameter `D`.
A second parameter with a default varies the end in the same way.
`RowEnd` is one new trait, because no existing trait can carry the end: `RowSearch` would need forwards or a second box.

### <a id="decision-135"></a>Decision 135: Three divergent SCAN chain cases leave the run

| Field | Value |
|---|---|
| Date | 2026-11-08 |
| Status | In force |
| Amends | None |
| Related | [Decision 43](#decision-43), [decision 102](#decision-102), [decision 124](#decision-124) |

**Statement.**

- The chain cases `T-C4-21 SCAN set S1` and `T-C4-21 SCAN bag B1` and the set re-pick Return case (`send_repick_return_case`) leave the run of `rholang/tests/reified_native_owner_rows.rs`.
- The three cases stay in that file as comments, under a label that names this decision.
- The cases S2, S3, B2 and B3 stay in the run.
- The production code does not change.
- The unit tests of the re-issue with the cause `ReissueCause::DivergentPair` in `spaces/disciplines/scan.rs` and `spaces/generic.rs` keep the rule covered.

**User's words.** "Retire the 3 cases (Recommended)".

**Reason.** Each retired case runs a persistent send against a persistent receive on a kind without a counter.
The recorder of the base treats each round of that pair as a free retry (`try_record_retry` in `accounting/native_runtime/recording.rs`), so phlo cannot end the run.

### <a id="decision-136"></a>Decision 136: The node command line runs the examples, and an exploratory deploy reads their results

| Field | Value |
|---|---|
| Date | 2026-11-10 |
| Status | In force |
| Amends | [Decision 86](#decision-86) (the method of the gate of the node command line) |
| Related | [Decision 13](#decision-13), [decision 59](#decision-59), [decision 113](#decision-113) |

**Statement.**

- `node eval` cannot run the examples. The REPL service of the node evaluates on the legacy path, and that path never starts native execution. So each entry point of a host space returns `RuntimeUnavailable`.
- The script `scripts/verify-reified/node-cli-examples.sh` runs the examples on a standalone node. Each file of an example is one deploy and one block, in the order of the manifest.
- For each file, the script runs `node deploy`, `node propose` and `node show-block`. The block must hold one deploy, with the signature that `node deploy` printed.
- The node command line has no command that reads the data at a name. So the gRPC method `exploratoryDeploy`, a dry run at the state of a block, reads the results. The campaign changes no file of `node/src`.
- A block stores only the flag `errored` of a deploy. So the code of an error example comes from a dry run of its last file at the pre-state of its block.
- Phase A runs the 68 examples that need no service on the default build of the node. Phase B runs `vectordb-embed` and `vectordb-mirror` on a build with the feature `rholang/chromadb` and a disposable ChromaDB server.
- The node embeds with its SBERT model, and the harness of the examples uses a fake embedder. So the gate checks the shape of the reply of `vectordb-embed`: `[true, v, "all-MiniLM-L6-v2"]`, where `v` holds 384 values of type `Float32`.

**User's words.** "Shape check on the chromadb node (Recommended)".
The question stated the method of this entry, with the second build and the live ChromaDB server.

**Reason.** The deploy path is the path of a user of the node, and each file becomes a block of the node.
The exploratory deploy reads on the native path without a change to the node.
The vector of the real embedder depends on the model, so only its shape is a stable expectation.

### <a id="decision-137"></a>Decision 137: The campaign closes at claimed_done on the host verification, with no pgmcp run

| Field | Value |
|---|---|
| Date | 2026-11-10 |
| Status | In force |
| Supersedes | [Decision 75](#decision-75) (the one pgmcp freeze at P11) |
| Amends | [Decision 13](#decision-13) (the VPI-1 relay), [decision 34](#decision-34) (item A15) and [decision 35](#decision-35) (the shared freeze) |
| Related | [Decision 136](#decision-136), [verification.md](verification.md), [pgmcp](glossary.md#pgmcp) |

**Statement.**

- pgmcp registers no recipe, and its verifier runs no leaf of the campaign. So the relays VPI-1 and AAI-1 are not asked.
- The final gate set runs each check of the campaign once, on a development host, on the frozen candidate. Section 35 of [verification.md](verification.md) records the results.
- A leaf closes at the tracker status claimed_done when each requirement and each verification step of the leaf has passed. No leaf moves to the status verified.
- The requirements of a leaf are its tracker text with the changes of the register decisions. No acceptance amendment writes these changes into the tracker.
- The closure report in the tracker gives the host result of each gate, and it checks each result against the frozen candidate.
- A tracker change that needs the authority of the user (TRI-1) is asked only when an approved disposition cannot close a leaf.
- This rule completes decision 131 of the plan, of 2026-11-03. That decision runs the full verification once, when each item of the campaign is claimed_done.

**User's words.** "Host gates, stay claimed_done".
On the verifier of pgmcp: "Verify everything yourself, there's no need to re-run the same verification within pgmcp; claimed_done, so long as you completed all the requirements and verification steps, is sufficient for me."

**Reason.** pgmcp sets an item to verified only from the runs of its own verifier, and it refuses imported results.
The verifier builds in a RAM-backed `/tmp`.
Its snapshot holds no git-ignored file, so the checkers that stay on the development host cannot run in it.
The route through the verifier needed about 245 runs and the relays VPI-1 and AAI-1.
The host gates run the same commands once, on the same candidate.

## Minimal-footprint revision: M-1 to M-5

On 2026-10-04 the user asked to "only modify what is required for reified rspaces and certainly not touch casper unless it is absolutely required … that was the purpose of the realignment".
The revision overrides the architecture details of the plan wherever they conflict.

### <a id="m-1"></a>M-1: Counter meta-channels and merge check #1

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force, scope widened by [decision 31](#decision-31) |
| Supersedes | The ledger decisions "write_exclusive + check #4" and "ord = max+1, no separate counter" (with M-2) |
| Replaces | `Produce.write_exclusive`, `ProduceEventProto.writeExclusive = 8`, merge check #4, the `EventLogIndex` changes, the event converter mapping, and the replay flag verification |
| Footprint | Zero changes in `casper/src`, the rspace++ merger, the event structs, and the protobuf events |
| Related | [Counter meta-channel](glossary.md#counter-meta-channel), [merge](glossary.md#merge), [decision 11](#decision-11) |

**Statement.**

- Cross-branch conflicts for ordered collections come from sequence-counter meta-channels and the existing merge check #1 (`rspace++/src/rspace/merger/merging_logic.rs::conflicts`).
- Each ordered collection gets one version counter $`S(\mathit{sid}, \mathrm{key}(c))`$. The collection is one queue, stack, or cell channel $`c`$, or one whole priority queue.
- The counter datum is wrapped in an envelope, never a bare number.
- Every state-changing operation touches the counter: each write, each removal (also a delivery to a persistent waiter), and each waiter park or removal.
  *Widened by [decision 31](#decision-31) to every operation on an ordered collection, also a peek and a fire that changes no state.*
- Each such operation consumes the counter datum and produces it again incremented, through a synthesized Consume, COMM (with empty `peeks`), and Produce.
- A write stores the counter value as the `ord` of its record ([M-2](#m-2)).
- A peek changes no state and does not touch the counter.
  *Superseded by [decision 31](#decision-31): every operation on an ordered collection touches the counter, also a peek and a fire that changes no state.*
- One counter covers a whole priority queue, because a dequeue reads that the higher levels are empty.
- The first operation on a collection creates its counter by consuming the space counter $`S(\mathit{sid})`$. The factory creates $`S(\mathit{sid})`$ in the creation block of the space.
- Bag, set, and VectorDB use no counters, so they commute exactly as ordinary host channels do.
- Each state-changing operation on an ordered collection emits three extra events: the counter Consume, COMM, and Produce.
  *Since [decision 31](#decision-31), every operation on an ordered collection emits these three events.*
- Formal verification: a `CounterConflict` TLA model and a standalone Rocq module replace the `ExclusiveWrite` and check-#4 artifacts.
- The Rocq module is `formal/rocq/reified_rspaces/theories/CounterConflict.v`.
- The Rocq module restates check #1 and proves `ordered_writes_race_on_counter` and `unordered_writes_commute`. The `merge_algebra` theories and `check-merge-algebra-ALL.sh` do not change.

**User's words.** The section quotation above.

**Reason.** The plan gives three arguments.

*Order fabrication.* Without a counter, branch A could enqueue $`d_1`$ and then $`d_3`$ in one deploy while branch B enqueues $`d_2`$.
A tie-broken merge could then yield $`d_1, d_2, d_3`$, which no serialization of whole deploys produces.

*Crossed stacks.* Removals must touch the counter too.
Branch A pushes $`X`$ onto stack $`s_1`$ and pops $`U`$ from $`s_2`$.
Branch B pushes $`Y`$ onto $`s_2`$ and pops $`T`$ from $`s_1`$.
Let $`c_1`$ and $`c_2`$ be the counter data of $`s_1`$ and $`s_2`$.

Without counters on removals, the destroyed sets $`\{c_1, U\}`$ and $`\{c_2, T\}`$ are disjoint, and checks #1 to #3 stay silent.
But A then B makes B pop $`X`$, and B then A makes A pop $`Y`$.
Neither order yields the merged state.

*Within-block chains.* Inside one block, `block_index.rs` groups deploys into chains only through `merging_logic::depends`.
With the counter, the second deploy on a collection consumes the counter that the first deploy produced, so both deploys join one chain.
For ordinary channels, the within-block case is a latent baseline defect.
It goes to the casper owners as a finding and is not fixed here.

The independent arbitration pass of 2026-10-04 confirmed M-1 for writes. It refuted the earlier claim that removals need no counter, and the plan folds that correction into M-1.

### <a id="m-2"></a>M-2: Space-owned record envelope

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force |
| Supersedes | The ledger decision "ord = max+1, no separate counter" (with M-1) |
| Replaces | The `Datum` and `WaitingContinuation` fields `ord` and `meta`, the record trailer codec, the serializer and native-mirror changes, the hot-store identity extension, and `hash_consume_with_meta` |
| Footprint | Zero host struct or serializer changes. Leaf bytes keep the existing format. |
| Related | [Record envelope](glossary.md#record-envelope), [I-5](#i-5) |

**Statement.**

- Space records carry their metadata in a space-owned envelope inside the record payload.
- The `ListParWithRandom.pars` of a datum holds the envelope `(ord, priority, embedding)` plus the payload.
- The body of a parked continuation holds the envelope `(ord, query policy)` plus the original body.
- Only space agents read these records. Ordinary Rholang cannot reach the namespaced `0xE0` channels.
- Agents unwrap the envelope before matching, delivery, and dispatch.
- Consume identity already differs for each receive, because the random state of the continuation enters `hash_consume`.

**User's words.** The section quotation above.

**Reason.** The envelope keeps every host data structure, serializer, and identity unchanged.

### <a id="m-3"></a>M-3: Charging through the accounting observer

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force, revised 2026-10-04. [Decision 45](#decision-45) adds the legacy charges of a legacy deploy. [Decision 47](#decision-47) adds one accessor, and [decision 78](#decision-78) adds one test edit. [Decision 89](#decision-89) retires the host-work clauses and decision 78, and [decision 90](#decision-90) retires the legacy charges of Option P. |
| Replaces | `observe_space_work` on the accounting observer, edits to `accounting/costs.rs`, and the earlier `reserve_primitive` plan |
| Footprint | Zero edits to the cost-accounting module and the observer trait |
| Related | [I-12](#i-12), [phlo](glossary.md#phlo), [host work](glossary.md#host-work) |

**Statement.**

- Phlo comes from the existing RSpace accounting-observer protocol.
- `SpaceTxn` drives the protocol for every staged produce, consume, and COMM, exactly as the host RSpace does, in play and replay.
- At the baseline, only `Comm`, `RSpaceProduce`, and `RSpaceConsume` carry consensus cost (`accounting/mod.rs::consensus_cost_unit`).
- `MeteredMachine::reserve_primitive` is diagnostic only, with cost 0, so it cannot charge anything.
- Matching, attach, and rehydration work use the existing host-work (liveness) budget. The tools are the metered methods of the injected matcher (`get_metered`, `check_commit_metered`) and `reserve_host_work`.
  *Retired by [decision 89](#decision-89): the base removed host-work metering, so a space operation meters only its COMMs and the bytes that it stores.*
- The node charges host work before the work, independent of cache state.
  *Retired by [decision 89](#decision-89).*
- The Reified host-work cost functions live in a new `reified/costs.rs`.
  *Retired by [decision 89](#decision-89).*
- In a legacy deploy, each staged operation also pays the [legacy charges](glossary.md#legacy-charge) of its equivalent host operation ([decision 45](#decision-45), Option P).
  *Retired by [decision 90](#decision-90): the merge of `702496775` removes Option P.*

**User's words.** The section quotation above.

**Reason.** The observer protocol already charges every RSpace operation, so the campaign needs no edit of the cost-accounting module.

### <a id="m-4"></a>M-4: Replay consult through child modules

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | Retired by [decision 90](#decision-90) (C6, step D). The rigged replay modes of the spaces retire, so `ReplayRSpace` no longer implements `SpaceTxnHost`. `replay_rspace.rs` comments out `mod space_replay;`, and the file stays on disk. |
| Replaces | The new `replay_consult.rs` refactor |
| Footprint | A one-line `replay_rspace.rs` hunk |
| Related | [Decision 19](#decision-19), [pairing](glossary.md#pairing) |

**Statement.**

- Replay consult reuses the existing private `ReplayRSpace` helpers: `matches`, `was_repeated_enough_times`, `get_comm_or_candidate`, `remove_bindings_for`, the produce counters, and the accounting helpers.
- The `SpaceTxn` implementation reaches the helpers from a child module that one line in `replay_rspace.rs` declares.
- The change needs no visibility change and no extraction refactor.

**User's words.** The section quotation above.

**Reason.** A child module can use the private items of its parent module, so no shared code moves.

### <a id="m-5"></a>M-5: Runtime-level cross-process harnesses

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force |
| Replaces | `casper/tests/reified/n2_cross_process.rs` and `restart_rehydrate.rs` |
| Footprint | Fewer casper test files |
| Related | [M-1](#m-1) |

**Statement.**

- The cross-process harness (N2) and the restart and rehydrate harness run at the rholang runtime level, from the runtime root after a checkpoint.
- They do not run through the `RuntimeManager` of casper.

**User's words.** The section quotation above.

**Reason.** The runtime level is enough for these harnesses, so casper gains fewer test files.

## Internal design decisions: I-1 to I-21

The plan marks these decisions as vetoable.
Plan approval on 2026-10-04 adopted I-1 to I-19, and the decision record dates each one.
The plan adds I-20 and I-21 on 2026-10-05, after the P2 and P4 work found the need for them.
The column "Amends" of the plan names the contract text that each decision changes.

### <a id="i-1"></a>I-1: Spaces are not ISpace implementors

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force |
| Amends | `rrs-rspace-interface (1)` to `(4)` |
| Related | [Space agent](glossary.md#space-agent), [space transaction](glossary.md#space-transaction) |

**Statement.** Space agents are not `ISpace` implementors. They use `SpaceStoreHandle` and `SpaceTxn`. The proposed `ISpace` changes (`suffix_key`, `priority`, `consume_with_query_specs`) are dropped.

**Reason.** One substrate owns checkpoint, replay, and reset (DELTA A). The required `produce_guarded` of F1r3lang then needs no new implementors on the Reified side. The `ISpace` surface stays smaller for the merge.

### <a id="i-2"></a>I-2: Single-channel handle forwarder

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force, revised 2026-10-04. [Decision 32](#decision-32) changes the forwarder argument. |
| Supersedes | The earlier join handle `[this, descriptor]` |
| Amends | None (new API) |
| Related | [Handle](glossary.md#handle), [I-16](#i-16), [merge](glossary.md#merge) |

**Statement.** A handle is a single-channel persistent receive on $`\mathit{this} = H(\mathit{sid})`$. Its body is an ordinary `ParBody` forwarder `OPS!(GByteArray(sid), args)` to the installed handle-op process. The `cost_authority` of the continuation is `None`.

*Changed by [decision 32](#decision-32), item A6: the forwarder passes the unforgeable channel `this`, as `OPS!(this, args)`, and not `GByteArray(sid)`. The method `config` reveals the space id, so a forwarder argument that holds only the space id could be forged.*

**Reason.**

- ScalaBodyRef handlers receive no channel (`dispatch.rs`), so the instance state must reach the handler another way.
- The earlier join `[this, descriptor]` is rejected. Merge check #3 (`block_index.rs::produce_touches_pre_state_join`) marks every produce on a channel with a pre-state join of length greater than 1.
- Such a produce conflicts with every concurrent branch, so every cross-block call would conflict.
- A forwarder costs one extra COMM per call. It gives the merge profile of an ordinary contract and needs no dispatcher change.
- The alternative was a dispatcher hook that passes channels to native bodies. It was rejected for its footprint.

### <a id="i-3"></a>I-3: Self-describing space names

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force |
| Amends | `rrs-spaces-port-core`, the item "channels stamped" |
| Related | [KIND](glossary.md#kind) |

**Statement.** Names created in a space are self-describing: they have the KIND N.

**Reason.** In-memory stamps are lost on reset, and DELTA A requires cross-block use.

### <a id="i-4"></a>I-4: Deploy-scoped temp and seq spaces

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force, revised 2026-10-04 |
| Supersedes | The earlier checkpoint filter |
| Amends | `rrs-spaces-port-core` and `rrs-consensus-replay (5)` |
| Related | [Volatile channel](glossary.md#volatile-channel), [sweep](glossary.md#sweep), [decision 19](#decision-19) |

**Statement.** Temp and seq spaces use the lowercase ephemeral KINDs and live for one deploy. Their operations emit ordinary events and accounting observations. At the end of the deploy, the runtime sweeps their data, continuations, and join entries through `SpaceTxnHost::sweep_volatile`.

**Reason.**

- The checkpoints of `ispace_impl.rs` and `replay_rspace.rs` do not change. The earlier checkpoint filter is dropped, and the arbitration showed that it would have missed replay.
- Suppressed events would make ephemeral operations phlo-free, because phlo comes only from observed RSpace operations.
- The FIPS rule "never written to disk" holds, because events carry only hashes and the state never reaches a checkpoint.
- Deploy scope does not depend on block packing.

### <a id="i-5"></a>I-5: Metadata in the envelope

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force |
| Amends | `rrs-persistence-substrate (1)` |
| Related | [M-2](#m-2) |

**Statement.** The priority of a priority queue, the embeddings, and the parked query policy live in the space-owned envelope ([M-2](#m-2)). There is no `ProduceEventProto.priority` and no `ConsumeEventProto.querySpecs`.

**Reason.** Host structs, serializers, and identities stay unchanged.

### <a id="i-6"></a>I-6: Canonical COMM produce sort

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force |
| Amends | `rrs-rspace-interface (5)` |
| Related | [COMM event](glossary.md#comm-event) |

**Statement.** Every COMM keeps the canonical produce sort. Deterministic ranking derives the delivery order of k results again.

**Reason.** The removal of the sort by codex (K5) broke the replay of legacy blocks.

### <a id="i-7"></a>I-7: Bare floats in the parser

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | Superseded by [decision 110](#decision-110) on 2026-10-27 |
| Amends | Keeps `rrs-parser-grammar (5)` |
| Related | [Decision 23](#decision-23) |

**Statement.** The tree-sitter parser implements bare floats as the contract says, which is a small change. `preprocess_floats` retires at the final pin.

*Superseded by [decision 110](#decision-110) on 2026-10-27: the node keeps `preprocess_floats`, so every float form of the base stays valid. The retirement at the final pin is dropped.*

**Reason.** The design agent suggested a deferral. The plan overrides that suggestion to avoid a known workaround.

### <a id="i-8"></a>I-8: Params only on factories and handles

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force. [Decision 112](#decision-112) changes the carrier of the params |
| Amends | `rrs-reducer (6)` |
| Related | [Decision 18](#decision-18), [inert](glossary.md#inert) |

**Statement.** Params are allowed on system channels only for factory and handle channels. The `PARAMS` tuple of [decision 18](#decision-18) carries them.

*Amended by [decision 112](#decision-112): the field `Send.params` carries the params in place of the `PARAMS` tuple. The reducer forwards them in a new `Expr` arm (O-1 of [decision 120](#decision-120)). The rule of this entry stays.*

**Reason.** Constructor keyword arguments need params. Every other system channel rejects params.

### <a id="i-9"></a>I-9: Sequential kernels without ndarray

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force |
| Amends | `rrs-spaces-port-core (7)` |
| Related | [Inner collection](glossary.md#inner-collection) |

**Statement.** The ndarray dependency is dropped. Kernels are sequential slice kernels.

**Reason.** Sequential kernels satisfy the no-threaded-accumulation gate trivially.

### <a id="i-10"></a>I-10: Optional priority bound

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force |
| Amends | The ledger note on D13 |
| Related | [Head-only matching](glossary.md#head-only-matching) |

**Statement.** The `priorities` parameter of a priority queue is an optional upper bound. There is no storage for each level.

**Reason.** The bound removes the legacy unbounded `ensure_capacity` denial-of-service issue.

### <a id="i-11"></a>I-11: Per-space scheduler footprint

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | Retired by [decision 90](#decision-90). [Decision 92](#decision-92) takes its role in native replay. |
| Amends | `rrs-reducer (2)` |
| Related | [Space transaction](glossary.md#space-transaction) |

**Statement.** The scheduler footprint of a space operation is a per-space key.

**Reason.** A per-space key is safe with joins and prefixes. A later benchmark decides any refinement.

**Note of decision 47 (2026-10-07).** The no-deadlock argument of native replay rests on this key (design v2, section 4.5).
The key keeps the transactions of one space in one conflict group, in operation order.
So every predecessor of a space row is complete when the row begins.
`NativeReplay.tla` checks `NoBlockedReadiness` with this key alone, without the native key [2, 0].

**Retirement (decision 90, 2026-10-25).** DR-132 of the base replaces the deterministic scheduler with racing evaluation, so no scheduler footprint exists.
Native replay of space transactions loses the ordering premise of this key, and the note of decision 47 lapses with it.
`NativeReplay.tla` no longer checks `ScheduleSpec`, which stays in the model as a comment.

[Decision 92](#decision-92) takes the role of the key in native replay. Each row of a space transaction names the counter channel `S(sid)` of its space.
The predecessor rule of the base then orders the transactions of one space, and `SpaceReplayOrder.tla` checks that replay does not stall.

### <a id="i-12"></a>I-12: Charging rules

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force, revised 2026-10-04. [Decision 89](#decision-89) retires its host-work clauses, and [decision 90](#decision-90) removes its legacy clause. |
| Amends | `rrs-reducer (7)` |
| Related | [M-3](#m-3), [phlo](glossary.md#phlo), [host work](glossary.md#host-work) |

**Statement.** Charging follows the revised [M-3](#m-3):

- Phlo: the node observes every space produce, consume, and COMM, durable or ephemeral, exactly as a host RSpace operation.
- Liveness: attach and rehydration charge host work before decoding. The node reads the record count first, through `root_leaf_record_count`.
- Matching and candidate work use the host-work budget of the existing metered matcher.
- The node charges similarity and guard costs per candidate before it materializes the candidate.
- Charges never depend on cache state.
- The Reified host-work cost functions live in the new `reified/costs.rs`.
- Legacy deploys: each staged space operation pays the legacy charges of `ChargingRSpace` for its equivalent host call ([decision 45](#decision-45), Option P).

*Retired by [decision 89](#decision-89): DR-131 of the base removes host-work metering.*
*So the clauses on liveness, matching, similarity and guard costs, and the host-work cost functions lapse.*
*The phlo clause stays: a space operation meters its COMMs and the bytes that it stores. Its charges still never depend on cache state.*
*`reified/costs.rs` now holds only the fire-round cap of [I-21](#i-21), which is a liveness bound.*
*Removed by [decision 90](#decision-90): DR-134 of the base deletes the legacy meter, so the clause on legacy deploys lapses with Option P.*

**Reason.** The rule needs no edit of the cost-accounting module. `reserve_primitive` is diagnostic only at the baseline, so it is not a charging path.

**Note of decision 47 (2026-10-07).** In native execution, a space transaction reads its view through [metered view reads](glossary.md#metered-view-read).
Each first view load charges its key and its value only (P5 of design v2, headline H27).
The charge is the same on a cache hit and on a miss.
A cold load reads the history through the native reader with a meter that accepts every reservation, so the reader adds no charge.

### <a id="i-13"></a>I-13: Native replay fails closed for spaces

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | Superseded by [decision 47](#decision-47) on 2026-10-07. It was amended on 2026-10-06 by [decision 46](#decision-46). |
| Amends | None |
| Related | [Decision 19](#decision-19), [decision 47](#decision-47) |

**Statement.** Spaces fail closed in native execution.
Native play and native replay reject the first space operation of a deploy before any space work, with `SpacesUnsupportedInNativeExecution`.
The 2026-10-04 text covered native replay only, with the code `SpacesUnsupportedInNativeReplay`.

**Reason.** The 2026-10-04 reason was that only tests use `create_native_replay_env`.
That premise is false since the merge of [decision 44](#decision-44): proposer self-certification and validator replay run every offered deploy there.
One rule for play and replay makes a native space deploy fail in play, and native replay reproduces the failure.
[Decision 47](#decision-47) plans the full native support.

### <a id="i-14"></a>I-14: Unchanged empty-state hash

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force, revised 2026-10-04 by [decision 18](#decision-18). [Decision 112](#decision-112) changes the hash once |
| Amends | None |
| Related | [Decision 6](#decision-6), [decision 15](#decision-15) |

**Statement.** `empty_state_hash_fixed` does not change ([decision 18](#decision-18)), and its baseline test is a regression gate at every milestone. Milestone commits exclude only `Cargo.toml` (the `[patch]`) and `Cargo.lock`.

*Amended by [decision 112](#decision-112): `empty_state_hash_fixed` changes once, at the commit of decision 112, as in commit `d367bc11a` of the base. After that commit, its test is a regression gate again.*

**Reason.** Marker tuples leave every ordinary hash unchanged.

### <a id="i-15"></a>I-15: Self-describing matchers

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force |
| Amends | The user-agent API |
| Related | [Decision 8](#decision-8), [user agent-backed space](glossary.md#user-agent-backed-space) |

**Statement.**

- User-agent matchers are self-describing X ids. The single-channel persistent receive of a matcher forwards to the installed `OP_MATCHER`.
- The operation runs the metered matching of the injected `Match` implementation on the original `BindPattern`.
- Bodies are pure-`Par` receives with `EMap` patterns that hold the original continuation.

**Reason.**

- Matching semantics and host-work metering equal those of a native receive.
- A pure-`Par` `match` was rejected, because `eval_match` is unmetered.
- Forwarders need no dispatcher change.
- A wrapper agent can pass its matchers to a native handle, which decodes them from the id.

**Amendment (2026-10-10, P8 design v2 MO-1, [decision 77](#decision-77)).** The forwarder of a matcher calls `MATCHER_OP!(m, args)` with `args = [ret, proc]`, so the op has the arity 2, as `HANDLE_OP` takes `(this, args)`. The plan wrote the call as `OP_MATCHER!(m, ret, proc)`.

### <a id="i-16"></a>I-16: No multi-channel join in synthesized machinery

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force |
| Amends | None |
| Related | [I-2](#i-2), [join](glossary.md#join), [merge](glossary.md#merge) |

**Statement.** Synthesized machinery never uses a multi-channel join. Handles, matchers, bodies, and the `new` resume term use single-channel or nested receives.

**Reason.** Merge check #3 would make every later produce on such channels conflict with all concurrent branches. User-written joins keep the baseline join semantics.

### <a id="i-17"></a>I-17: Config as a canonical Par record

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force |
| Amends | None |
| Related | [KIND](glossary.md#kind), [decision 18](#decision-18) |

**Statement.** The space config is a canonical `Par` record on $`C(\mathit{sid})`$, not a new protobuf message.

**Reason.** The decision needs no new `.proto` file and no `models/build.rs` change. The record is self-describing and has a version tag.

### <a id="i-18"></a>I-18: Fixed-arity system processes

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force, revised 2026-10-04 |
| Amends | None |
| Related | [Factory](glossary.md#factory) |

**Statement.** Every Reified system process has a fixed arity. The reducer hook packs variadic factory arguments into one list.

**Reason.** `system_process_install` drops remainder bindings (`free_count = arity`). Packing avoids any change to that shared function.

### <a id="i-19"></a>I-19: Space id derivation

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force. [Decision 32](#decision-32) corrects the reason. |
| Amends | None |
| Related | [Space id](glossary.md#space-id), [decision 7](#decision-7) |

**Statement.** The space id is derived from the random state of the creating call:

```math
\mathit{sid} = \mathrm{next}\left(\mathrm{split}_{\mathtt{SPACE\_ID}}\left(\mathrm{split}_{\mathtt{0xE0}}\left(r_{\mathit{call}}\right)\right)\right)
```

A second creation of the same `sid` aborts with `SpaceAlreadyExists`.

**Reason.** The `sid` is unique for each creating message, so storage channels never cross branches. The abort stops the `Factory!!(…)` re-produce loop deterministically.

*Corrected by [decision 32](#decision-32), item A21: the abort alone does not stop the loop. The baseline never cancels the re-produce sibling of a persistent send. A persistent send to a factory or to a handle aborts the deploy instead.*

### <a id="i-20"></a>I-20: Derived flags refreshed before the strict decode

| Field | Value |
|---|---|
| Date | 2026-10-05 |
| Status | Retired by [decision 112](#decision-112) |
| Amends | [Decision 21](#decision-21), the rule "decoders reject every non-canonical form" |
| Related | [Decision 18](#decision-18), [decision 22](#decision-22), [canonical form](glossary.md#canonical-form), [marker tuple](glossary.md#marker-tuple) |

**Statement.**

- Before the strict decode, the reducer hooks compute again every `connective_used` flag that the encoder derives.
- The derived flags belong to the marker tuple, to its `EList` and `EMap` containers, and to the holder `Par` values.
- The containers are the lists and maps of PARAMS and QUERY, the annotation list of NEWSPACE, and its entries `[]` and `[s]`.
- The holders are the `Par` values of PARAMS and QUERY, and the `Par{news: [New]}` of NEWSPACE.
- Each derived flag is the disjunction of the flags of its children. The refresh never changes a value.
- Hook H4 applies the same refresh before it fails closed with `QuerySpecUnsupported`.
- The decoder still accepts only the exact canonical image of its encoder ([decision 21](#decision-21)).
- P4 batch 4C implements the rule. Batch 4E-F refines `Encoding.v`: its note on the abstraction function and the premises of substitution.

*Retired by [decision 112](#decision-112): the new protobuf messages have no derived flags, and no decoder reads a derived flag. So the refresh has nothing to refresh.*

**Reason.**

- A P2 finding: the evaluation and the substitution of the baseline keep the flag `connective_used` of a tuple, a list, or a map. They keep it also when the children of the container change.
- The evaluation keeps the flags at `rholang/src/rust/interpreter/reduce.rs:4130-4152`, `4154-4175`, and `4137-4157`.
- The substitution keeps them at `rholang/src/rust/interpreter/substitute.rs:1573-1594`, `1596-1615`, and `1515-1538`.
- Ordinary data can use connectives, as in `x!(Int)`. Without the refresh, a legal `use` body could fail closed as `NonCanonical`.
- Only the top level of a marker tuple can hold a stale flag, so the refresh costs time linear in the width of the tuple.
- To ignore the flags instead would weaken the contract of the decoder.

### <a id="i-21"></a>I-21: Fire-round cap of a space transaction

| Field | Value |
|---|---|
| Date | 2026-10-05 |
| Status | In force. The value is vetoable: the user may choose another cap. |
| Amends | Item A10 of the P4 design ("a bounded chain of fires"), arbitration item H8, and [I-12](#i-12) (work bounds for each operation) |
| Related | [Decision 34](#decision-34), [decision 43](#decision-43), [I-12](#i-12), [re-issue](glossary.md#re-issue), [space intent](glossary.md#space-intent) |
| Amended by | [Decision 49](#decision-49): the settles of an operation count on the fire count of its transaction |

**Statement.**

- One space transaction runs at most `FIRE_ROUND_CAP = 64` fire rounds (`rholang/src/rust/interpreter/reified/costs.rs:17`).
- At the cap, the agent commits the staged prefix and re-issues the rest of the operation as a new `Intent::Space`. This is the re-issue path of A10.
- A re-issued persistent operation keeps the `ord` of its first round, and it re-enters at its place ([decision 43](#decision-43)).

**Reason.**

- A10 and H8 require a bound on the fires of one operation, but they give no value.
- The bound is deterministic, so play and replay agree.
- The bound changes the output only for a chain of more than 64 fires. There it sets the transaction boundaries and the counter bumps.

## References

<a id="ref-fips-agents"></a>[FIPS-AGENTS] M. Stay, "Agents," F1r3fly Improvement Proposal, 2025-08-20. [FIPS repository, approved/2025-08-20-Agents/Agents.md](https://github.com/F1R3FLY-io/FIPS/blob/main/approved/2025-08-20-Agents/Agents.md)

<a id="ref-fips-rrs"></a>[FIPS-RRS] M. Stay and L. G. Meredith, "Reifying RSpaces," F1r3fly Improvement Proposal, 2025-09-26. [FIPS repository, approved/2025-09-26-Reifying-RSpaces](https://github.com/F1R3FLY-io/FIPS/blob/main/approved/2025-09-26-Reifying-RSpaces/Reifying%20RSpaces.md)

<a id="ref-plan"></a>[PLAN] "Reified RSpaces: realignment and completion plan (takeover from codex)," approved by the user on 2026-10-04. Session plan file `review-pgmcp-epic-rrs-realignment-twinkly-wind.md` in `[HOME]/.claude/plans/`, outside the repository. The epic `reified-rspaces-mettatron-migration` (pgmcp id 5582) cites it in each leaf.
