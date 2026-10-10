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
- **Baseline:** the node code without the campaign changes. The plan calls it HEAD. It was the commit `e748426` until the merge of [decision 44](#decision-44), and it is the commit `4d67b5ccc` since that merge.
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
- **Acronyms:** API is application programming interface, ABI is application binary interface, AST is abstract syntax tree, and CLI is command-line interface. TLA+ is the specification language of the TLA model checks.

## Index

| Entry | Title | Date | Status |
|---|---|---|---|
| [Decision 1](#decision-1) | Scope rule | 2026-10-03 | In force |
| [Decision 2](#decision-2) | MeTTaTron space held | 2026-10-03 | In force |
| [Decision 3](#decision-3) | F1r3lang ownership | 2026-10-03 | In force |
| [Decision 4](#decision-4) | Parser isolation | 2026-10-03 | In force |
| [Decision 5](#decision-5) | Realignment method | 2026-10-03 | In force |
| [Decision 6](#decision-6) | Milestone commits | 2026-10-03 | Fixture part superseded by decision 18 |
| [Decision 7](#decision-7) | Namespace tag 0xE0 | 2026-10-03 | In force |
| [Decision 8](#decision-8) | Agent-shaped API | 2026-10-03 | In force |
| [Decision 9](#decision-9) | Approved design proposals | 2026-10-03 | In force. Item 1 (the theory seam) implemented in P7 step 4, with the slot of decision 74 |
| [Decision 10](#decision-10) | Parser branch name | 2026-10-03 | In force |
| [Decision 11](#decision-11) | Head-only matching | 2026-10-04 | In force |
| [Decision 12](#decision-12) | Lane rule | 2026-10-04 | In force |
| [Decision 13](#decision-13) | Verification command defaults | 2026-10-04 | In force |
| [Decision 14](#decision-14) | Rocq line-reference audit | 2026-10-04 | In force |
| [Decision 15](#decision-15) | Empty-state hash fixture | 2026-10-04 | Fixture commit superseded by decision 18 |
| [Decision 16](#decision-16) | Tracker dispositions | 2026-10-04 | In force |
| [Decision 17](#decision-17) | Runtime space registration postponed | 2026-10-04 | In force |
| [Decision 18](#decision-18) | Marker-tagged tuples | 2026-10-04 | In force |
| [Decision 19](#decision-19) | P1 substrate | 2026-10-04 | In force, amended by decisions 30 and 36 |
| [Decision 20](#decision-20) | Tracker leftovers of TRI-1 | 2026-10-04 | In force |
| [Decision 21](#decision-21) | Corrected encoding table | 2026-10-04 | In force |
| [Decision 22](#decision-22) | P2 hooks | 2026-10-04 | In force |
| [Decision 23](#decision-23) | P3 parser design | 2026-10-04 | In force |
| [Decision 24](#decision-24) | Keywords by lookahead | 2026-10-04 | In force |
| [Decision 25](#decision-25) | Parser-branch commits (UD-9) | 2026-10-04 | In force |
| [Decision 26](#decision-26) | Completion goal | 2026-10-04 | In force |
| [Decision 27](#decision-27) | Activation and rollout out of scope | 2026-10-04 | In force |
| [Decision 28](#decision-28) | Repeated channels in a group | 2026-10-04 | In force |
| [Decision 29](#decision-29) | Joins into ordered collections in P5 | 2026-10-04 | In force. Implemented in P5 wave W3 |
| [Decision 30](#decision-30) | Peek fires re-offer the sent value | 2026-10-04 | In force |
| [Decision 31](#decision-31) | Every operation on an ordered collection bumps its counter | 2026-10-04 | In force |
| [Decision 32](#decision-32) | P4 plan-text changes | 2026-10-04 | In force. Its open P8 item is settled by decision 77 |
| [Decision 33](#decision-33) | assertEqual has arity 3 | 2026-10-04 | In force |
| [Decision 34](#decision-34) | P4 detail items | 2026-10-04 | In force |
| [Decision 35](#decision-35) | P1 close-out scope: lean, verify at P4 | 2026-10-04 | In force. Its shared freeze moved to P11 by decision 75 |
| [Decision 36](#decision-36) | Space replay reproduces play when a deploy runs out of phlo | 2026-10-05 | In force |
| [Decision 37](#decision-37) | Receive selection follows the F1r3lang rule from the F1r3lang merge on | 2026-10-05 | In force. Implemented in P12 |
| [Decision 38](#decision-38) | Priority-queue waiters wait in one queue per level | 2026-10-05 | In force |
| [Decision 39](#decision-39) | P5 design choices F1 to F17 and F20, without F21 | 2026-10-05 | In force, vetoable value of F9 |
| [Decision 40](#decision-40) | Space charging follows the cost-accounted-rho policy | 2026-10-05 | In force. Applied in P5 |
| [Decision 41](#decision-41) | Bag and set spaces order candidates by source hash | 2026-10-05 | In force. Applied in P5a |
| [Decision 42](#decision-42) | The queue agent's metering is aligned now | 2026-10-05 | In force. Implemented |
| [Decision 43](#decision-43) | A re-issued persistent operation keeps its first-round identity | 2026-10-05 | In force. Implemented |
| [Decision 44](#decision-44) | Merge of feature/cost-accounted-rho at 4d67b5ccc | 2026-10-05 | In force |
| [Decision 45](#decision-45) | Space operations pay the legacy storage charges of the host | 2026-10-06 | In force. Rule E and Option P implemented |
| [Decision 46](#decision-46) | Spaces fail closed in native execution | 2026-10-06 | Superseded by decision 47 on 2026-10-07 |
| [Decision 47](#decision-47) | Full native support for spaces is its own phase | 2026-10-06 | In force. Implemented on 2026-10-07 |
| [Decision 48](#decision-48) | The work order after decision 43 | 2026-10-06 | In force |
| [Decision 49](#decision-49) | One fire count for each space transaction | 2026-10-06 | In force. Implemented in P5 wave W2 |
| [Decision 50](#decision-50) | Joined set collections cascade | 2026-10-07 | In force. Implemented in P5 wave W3 |
| [Decision 51](#decision-51) | A PathMap bag or set peek bumps the token when its second send changes state | 2026-10-07 | In force. Implemented in P5 wave W3 (Phase 2 of the PathMap engine) |
| [Decision 52](#decision-52) | Tensor logic and the ChromaDB backend join the VectorDB phase P6 | 2026-10-07 | In force. Tensor logic moved to P12 by decision 65 |
| [Decision 53](#decision-53) | The tensor-logic fragment TL_V | 2026-10-07 | Moved to P12 by decision 65 |
| [Decision 54](#decision-54) | The number model of tensor logic | 2026-10-07 | In force for P6 decisions. TL-only functions moved to P12 by decision 65 |
| [Decision 55](#decision-55) | The tensor-logic syntax | 2026-10-07 | Superseded by decision 62 on 2026-10-07 |
| [Decision 56](#decision-56) | The VectorDB vocabulary | 2026-10-07 | In force, amended by decisions 62 and 66 |
| [Decision 57](#decision-57) | The recorded processes `embed` and `ann` | 2026-10-07 | In force. Implemented in P6. Native clause superseded by decision 68 |
| [Decision 58](#decision-58) | The ChromaDB mirror is a hint index | 2026-10-07 | In force. Implementation in P6 |
| [Decision 59](#decision-59) | The test policy for live services | 2026-10-07 | In force. Implementation in P6 |
| [Decision 60](#decision-60) | The VectorDB tie-break follows decision 41 | 2026-10-07 | In force. Implementation in P6 |
| [Decision 61](#decision-61) | Footprint exceptions for the ChromaDB mirror | 2026-10-07 | In force. Implementation in P6 |
| [Decision 62](#decision-62) | Tensor-logic equations are FLT terms of a TensorLogic GSLT | 2026-10-07 | In force. Implementation at P12, after SIG-F |
| [Decision 63](#decision-63) | The TensorLogic GSLT has an index-notation core and familiar sugar | 2026-10-07 | In force. Implementation at P12 |
| [Decision 64](#decision-64) | The TensorLogic surface is paper-first, functional and Rholang-flavored | 2026-10-07 | In force. Implementation at P12 |
| [Decision 65](#decision-65) | The TensorLogic language waits for F1r3lang and does not block VectorDB | 2026-10-07 | In force |
| [Decision 66](#decision-66) | The answers of the P6 design refresh | 2026-10-08 | In force. Native answer superseded by decision 68 |
| [Decision 67](#decision-67) | The checks of an `ann` call run in play and in replay | 2026-10-08 | In force |
| [Decision 68](#decision-68) | `embed` and `ann` leave the native ban | 2026-10-08 | In force |
| [Decision 69](#decision-69) | Only the latest execution model | 2026-10-08 | In force |
| [Decision 70](#decision-70) | Reified RSpaces only | 2026-10-08 | In force |
| [Decision 71](#decision-71) | `capacity` is a soft bound under merges | 2026-10-08 | In force |
| [Decision 72](#decision-72) | The P7 design process | 2026-10-09 | In force |
| [Decision 73](#decision-73) | P7 design v2, Seq claims per channel, HashSet on units | 2026-10-09 | In force. Implemented in P7 steps 2 (the Seq claims) and 3 (the HashSet store) |
| [Decision 74](#decision-74) | The config persists the theory identity as compact bytes | 2026-10-09 | In force. Implemented in P7 step 4 |
| [Decision 75](#decision-75) | One pgmcp freeze at P11 | 2026-10-09 | In force |
| [Decision 76](#decision-76) | The P8 design process | 2026-10-09 | In force |
| [Decision 77](#decision-77) | P8 design v2, the URNs of the internal ops, the space-view clause | 2026-10-10 | In force |
| [Decision 78](#decision-78) | One native replay test runs at the v6 host-work limits | 2026-10-10 | In force. Implemented in P8 step 1 |
| [Decision 79](#decision-79) | The conformance kit keeps the v6 host-work limits | 2026-10-10 | In force. Implemented in P8 step 5 |
| [M-1](#m-1) | Counter meta-channels and merge check #1 | 2026-10-04 | In force, scope widened by decision 31 |
| [M-2](#m-2) | Space-owned record envelope | 2026-10-04 | In force |
| [M-3](#m-3) | Charging through the accounting observer | 2026-10-04 | In force, revised 2026-10-04 |
| [M-4](#m-4) | Replay consult through child modules | 2026-10-04 | In force |
| [M-5](#m-5) | Runtime-level cross-process harnesses | 2026-10-04 | In force |
| [I-1](#i-1) | Spaces are not ISpace implementors | 2026-10-04 | In force |
| [I-2](#i-2) | Single-channel handle forwarder | 2026-10-04 | In force, revised 2026-10-04, forwarder argument changed by decision 32 |
| [I-3](#i-3) | Self-describing space names | 2026-10-04 | In force |
| [I-4](#i-4) | Deploy-scoped temp and seq spaces | 2026-10-04 | In force, revised 2026-10-04 |
| [I-5](#i-5) | Metadata in the envelope | 2026-10-04 | In force |
| [I-6](#i-6) | Canonical COMM produce sort | 2026-10-04 | In force |
| [I-7](#i-7) | Bare floats in the parser | 2026-10-04 | In force |
| [I-8](#i-8) | Params only on factories and handles | 2026-10-04 | In force |
| [I-9](#i-9) | Sequential kernels without ndarray | 2026-10-04 | In force |
| [I-10](#i-10) | Optional priority bound | 2026-10-04 | In force |
| [I-11](#i-11) | Per-space scheduler footprint | 2026-10-04 | In force. Basis of decision 47 |
| [I-12](#i-12) | Charging rules | 2026-10-04 | In force, revised 2026-10-04, noted 2026-10-07 by decision 47 |
| [I-13](#i-13) | Native replay fails closed for spaces | 2026-10-04 | Superseded by decision 47 on 2026-10-07 |
| [I-14](#i-14) | Unchanged empty-state hash | 2026-10-04 | In force, revised 2026-10-04 |
| [I-15](#i-15) | Self-describing matchers | 2026-10-04 | In force |
| [I-16](#i-16) | No multi-channel join in synthesized machinery | 2026-10-04 | In force |
| [I-17](#i-17) | Config as a canonical Par record | 2026-10-04 | In force |
| [I-18](#i-18) | Fixed-arity system processes | 2026-10-04 | In force, revised 2026-10-04 |
| [I-19](#i-19) | Space id derivation | 2026-10-04 | In force, reason corrected by decision 32 |
| [I-20](#i-20) | Derived flags refreshed before the strict decode | 2026-10-05 | In force |
| [I-21](#i-21) | Fire-round cap of a space transaction | 2026-10-05 | In force, vetoable value |

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
| Status | In force. The fixture commit that this decision gated is no longer needed ([decision 18](#decision-18)). |
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
| Status | In force |
| Supersedes | None |
| Related | [Decision 22](#decision-22) |

**Statement.**

- Allow a hash-pinned cache of third-party crates. Workspace crates always rebuild from the exact candidate.
- The exact-arithmetic receipt is a pure-Python `fractions.Fraction` reference. SageMath stays as an extra local gate leg.
- The node CLI verifies the examples, as the contract requires.
- The workspace regression gate skips the 27 pre-existing `#[ignore]` tests by exact name and records their reasons.
- The user still sees the exact recipe command list for the VPI-1 relay before registration.

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
| Status | The fixture commit is superseded by decision 18. The regression rule below is in force. |
| Superseded by | [Decision 18](#decision-18) |
| Related | [I-14](#i-14) |

**Statement.**

- `empty_state_hash_fixed` is not expected to change.
- The baseline test `empty_state_hash_should_be_the_same_as_hard_coded_cached_value` must stay green at every milestone.
- The second hard-coded hash in `runtime_spec.rs` must stay green too.
- If either test fails, the work stops and the user gets a report. Nobody edits the constants.

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
| Status | In force |
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
- D3: Each operation of a transaction runs under its own sub-order $`\mathit{parent} \mathbin{+\!\!+} [(i, 2)]`$, where 2 is `SUB_OPERATION_TAG`. Native phlo recording rejects two operations under one order (`accounting/native_runtime/operations.rs:265`).
- D4: A native replay directive fails the space operation closed (`NativeReplayDirectiveUnsupported`).
- D5: `root_leaf_record_count` counts the records in the outer frame of the leaf (`get_data_proj_binary`). No history module changes.
- D6: Commit is prefix-atomic. If the observer rejects operation $`i`$, the operations before $`i`$ stay applied, as in the equivalent host program. The deploy fails, and the soft-checkpoint revert removes them.
- D7: `matches`, `was_repeated_enough_times`, and `get_comm_or_candidate` are re-stated over the staged view. All other M-4 helpers are reused.
  In play, the store steps `store_data`, `remove_matched_datum_and_join`, `store_waiting_continuation`, and `store_persistent_data` are private to sibling modules. So `space_play` re-states them over the public `HotStore` API.
- Correction: codex inverted six baseline peek tests, not five. They are `storage_actions_test.rs:323` and `:370`, `comm_observer_tests.rs:470`, and `replay_rspace_tests.rs:472`, `:545`, and `:607`.

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
| Status | In force |
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

**User's words.** "Approve corrected (Recommended)"

**Reason.** The P2 design pass (items C1 to C3) found that the earlier table did not fit the code.

### <a id="decision-22"></a>Decision 22: P2 hooks

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force |
| Supersedes | None |
| Related | [Decision 18](#decision-18), [recognized position](glossary.md#recognized-position) |

**Statement.**

- P2 lands all four recognition hooks: the process filter, the `generated_message_eval` arm, the `eval_send` hook, and the `unbundle_receive` hook.
- Until P4 enables them, the hooks fail closed with `RuntimeUnavailable`, `ParamsUnsupported`, and `QuerySpecUnsupported`.
- One shared predicate, `is_evaluated_process_expr`, replaces both filter copies in `util/mod.rs`.
- `accounting/` stays byte-identical. So the funding resolver `accounting/lexical.rs` fails closed (`BugFoundError`) on Reified terms. It has no production caller.
- Two findings go to the cost-accounting owner: the filter twin in `lexical.rs` (H-1), and the demand analyses that do not see marker bodies (H-2).
- The plan applies the wording corrections C6 and C10 under "Frontend-neutral syntax layer".
- Fact C7: `space_factory_urn.rs` exists only in the backup.
- Fact C8: `models` has `doctest = false`, so the encoding examples live in tests.
- Fact C9: gates build in `RRS_SCRATCH`, never in the source tree or in `/tmp`.

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
- A pre-upgrade deploy that names a new URN passes admission, because the normalizer does not check URNs. It fails at run time (`util/mod.rs:234`), so an upgraded node would compute a different post-state for it.
- New nodes replay only above the LFS horizon (`initializing.rs:775-812`).
- The `header.version` of a block must equal the genesis version (`block_processor.rs:316`).

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

## User decisions 28 to 47

The P4 design pass and its arbitration found more places where the plan did not fit the code.
The user approved each recommended option on 2026-10-04, as decisions 28 to 35.
On 2026-10-05, a P4 test, the P5 design pass, and its readiness review led to decisions 36 to 39.
These decisions override the earlier plan sections where they conflict, as decisions 19 to 27 do.

### <a id="decision-28"></a>Decision 28: Repeated channels in a group

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force |
| Supersedes | The rule of fix rule FR-1 that rejected a repeated channel (`DuplicateChannelInGroup`) |
| Related | [Decision 19](#decision-19), [canonical group](glossary.md#canonical-group), [store identity](glossary.md#store-identity) |

**Statement.**

- This is item A4 of the P4 arbitration.
- A channel group may name one channel more than once.
- Every group with two or more channels is non-decreasing by `stable_hash_provider::hash`, so a channel multiset has exactly one canonical order (fix rule FR-1').
- A group that names a channel $`k`$ times binds $`k`$ distinct records on it.
- In a produce-side fire, the incoming datum binds exactly once.

**User's words.** "Allow, canonical order (Recommended)"

**Reason.** Runtime aliasing can create a join that names one channel twice.
The FIPS gives each pair of a matcher and a channel its own message, so the repeated channel binds distinct records.
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
The re-offer equals the re-produce of the baseline reducer (`reduce.rs:986-1021`), so the events and the observer calls stay equal to those of the host.

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
| Status | In force. Its open P8 item is settled by [decision 77](#decision-77) |
| Supersedes | The forwarder argument `GByteArray(sid)` of [I-2](#i-2), and the claim of [I-19](#i-19) that `SpaceAlreadyExists` stops the `Factory!!(…)` re-produce loop |
| Related | [Decision 18](#decision-18), [decision 22](#decision-22), [internal operation](glossary.md#internal-operation), [handle](glossary.md#handle) |

**Statement.** The user approved four changes of the plan text, items A5, A6, A7, and A21 of the P4 design.

- A5: `canonical_inner_channel_bytes` is the bincode of the sorted Par, which is the preimage of the store identity. Protocol Buffers encodes `locally_free`, so prost bytes would split one store channel into two keys.
- A6: `HANDLE_OP` is bound to the URN `rho:reified:internal:handleOp`. The baseline binds every system definition to a URN (`rho_runtime.rs:1578-1584`), and `registry_lookup` serves any key.
- A6, continued: the operation accepts a call only with the unforgeable `this`. It must be a `GPrivate` of exactly 34 bytes with KIND H or h, and its config record `C(sid)` must exist.
- A6, continued: this replaces the `GByteArray(sid)` forwarder of I-2, which would leak the capability through `config` and `toByteArray`.
- A7: P2 implements the send hook as the same-signature swap of `self.produce(` for `self.reified_produce(`. The function `reified_produce` splits the trailing params first, and it fails closed as decision 22 says.
- A7, continued: the H2 arm is one line without a guard, of at most 100 columns, for example `eval_marker`. The reason is that H1 admits only marker tuples into process position.
- A21: a persistent send (`!!`) to a two-byte `0xE0` operation channel, or to an H or h handle, aborts the deploy. The two-byte channels are the factories, `getSpaceAgent`, `assertEqual`, `vector`, and the internal operations.
- A21, continued: the baseline never cancels the re-produce sibling of a persistent send (`reduce.rs:722-788`, `1023-1044`). So the claim of I-19 that `SpaceAlreadyExists` stops the loop was wrong.

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
| Status | In force |
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
| Status | In force. Its shared freeze moved to P11 by [decision 75](#decision-75) |
| Supersedes | The A5 realignment ledgers of the P1 close-out design |
| Related | [Decision 5](#decision-5), [decision 16](#decision-16), [verification.md](verification.md) |

**Statement.** The P1 close-out stays lean, and the pgmcp verification of P1 runs together with P4.

- The close-out finishes only the work that was in flight: the regression tests, the regression-side checkers, the closure checker and [verification.md](verification.md). P2 starts after the P1 commit.
- No realignment ledger is built. The seven ledger leaves close with existing evidence: the P0 backup manifest, the footprint pin of policy `p1-realign`, and the TRI-1 ledger in pgmcp.
- The six realignment leaves of the `SpaceStoreHandle` group are duplicates of the substrate leaf, which carries the same tests.
- The pgmcp verification runs of the P1 leaves happen in the same freeze as those of P4.

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
- **Until then**, space receives use the current guard rule of this tree, as the host does. A consume takes the first spatial match of each bind and checks the guard once, and a refusal parks the waiter (`ops_consume.rs:111-153`). On the produce side, a guard refusal moves to the next waiter (`space_matcher.rs:193-200`). Head-only disciplines test only the head, so both rules agree for the queue, the stack, and the priority queue.
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
- F11: a `new` inside `use` of an Array or Vector space runs one allocation job. The hot line `reduce.rs:1650` gains `.await`.
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
| Status | In force. P5 applies it. |
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
| Status | In force. P5a applies it. |
| Supersedes | "Ascending record bytes" for the bag, and the key order of set classes, in the disciplines table of the plan |
| Related | [Decision 40](#decision-40), [decision 44](#decision-44) |

**Statement.** As in DR-75 of `feature/cost-accounted-rho`, the bag and set agents order their candidates by the hash of their source produce or consume.
They compare full digests only between equal source hashes, and the store index decides last.
The rule is the canonical order of `rspace++/src/rspace/candidate_order.rs`, which the host uses since the merge of [decision 44](#decision-44).

**User's words.** "Source-hash order (Recommended)".

**Reason.** The order reads only precomputed hashes for distinct sources, so its work does not grow with the payloads.
The host and the spaces use one order, so a space operation and its equivalent host program choose the same candidate.

### <a id="decision-42"></a>Decision 42: The queue agent's metering is aligned now

| Field | Value |
|---|---|
| Date | 2026-10-05 |
| Status | In force. Implemented with the model `AgentReadCoverage.v`. |
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
| Status | In force. Rule E and Option P are implemented. |
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

- `reified/legacy_charges.rs` computes the [legacy charges](glossary.md#legacy-charge) of each staged operation: `produce_charges` and `consume_charges` restate `ChargingRSpace` and its `handle_result`.
- `LegacyChain` stages every operation of a space job. The callback of an operation charges the carried post-charges of the operation before it, then registers the introduction, then charges the pre-charge.
- `LegacyChain::finish` charges the post-charges of the last operation after the commit, in the finish step of the prepared job.
- The chain is active only in a legacy deploy with a metered budget. The counter bump, the plans of the queue, the re-store, and the creation of a space all stage through it.
- The design is section [N10](design.md#n10-legacy-charges) of the design, with Figure N10.
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
| Status | In force. Implemented on 2026-10-07. [Decision 48](#decision-48) moved the phase after P5. [Decision 78](#decision-78) adds one test edit to the accounting module. |
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
2. The native replay backend of spaces replays each staged operation against its journal row (commit `9d9f31b7d`).
3. The accessor `space_store`, the bound state of the runtime and the bind call run spaces in native execution (commit `9390408c4`).
4. The integration suite certifies space deploys at the v6 limits (commit `bbe6d03d2`).
5. The formal models add H24 to H27 to `EventCorrelation.v` and the new module `NativeReplay.tla`.
6. The documents record the design, the specification, the verification, and the findings for the cost-accounting owner.

**TLA+ placement.** The user chose "New NativeReplay.tla (Recommended)" on 2026-10-07.
The models of design v2 section 5 are in a new module, so `CheckpointReplay.tla` and its 20 configurations stay unchanged.

**Epoch closure.** A probe of step 4 showed that a forged journal could hang native replay of a space deploy: 64 of 109 forgeries hung.
Commit `c6cb371ba` closes the epoch when a native space operation fails, except after a published denial.
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
| Status | In force. Implemented in P6. The clause "both fail in native execution" is superseded by [decision 68](#decision-68) on 2026-10-08 |
| Amends | The op table of the namespace (two new ops) |
| Related | [Decision 52](#decision-52), [decision 46](#decision-46) |

**Statement.**

- `rho:vectordb:embed` uses the op `[0xE0, 0x24]` and the body reference 236. It replies `[true, vector, model]` or `[false, code, detail]`.
- `rho:vectordb:ann` uses the op `[0xE0, 0x25]` and the body reference 237. It replies `[true, ids, distances, watermark]` or `[false, code, detail]`.
- Both processes are in `non_deterministic_ops()`, so replay reuses their recorded outputs.
- Both exist in every build, and both fail in native execution.
- Every new reply has the reply channel first and the shape `[ok, ...]`, so `!?` and `try` apply.
- The marker range `0xD5` to `0xDF` is reserved for future syntax markers.

**Reason.**

- A recorded output gives replay the proposer's answer. A process that exists in every build lets every validator replay that answer.
- `ann` returns ids, never payloads. Only the exact query param `ids=` can turn them into a decision, and the exact order still decides.

### <a id="decision-58"></a>Decision 58: The ChromaDB mirror is a hint index

| Field | Value |
|---|---|
| Date | 2026-10-07 |
| Status | In force. Implementation in P6 |
| Amends | none |
| Related | [Decision 52](#decision-52), [decision 45](#decision-45) |

**Statement.**

- The mirror of a VectorDB space is node-local and is never consensus state.
- The factory param `mirror="chroma"` turns it on. Temp and seq spaces cannot have a mirror.
- At the end of a successful deploy, the node publishes the writes of the deploy to the mirror. A failed deploy publishes nothing.
- A peek re-store of Rule E writes nothing to the mirror.
- A reconciler reads the finalized state and repairs the mirror. It removes orphan records and adds missing ones.
- When ChromaDB is unavailable, writes wait in a bounded buffer, and a deploy never waits. `ann` then replies `MirrorUnavailable`, and replay reuses that reply.

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
| Status | In force. Implementation in P6 |
| Amends | The VectorDB row of the discipline table ("Blake2b256(record), then full bytes") |
| Related | [Decision 41](#decision-41) |

**Statement.** Two VectorDB records with equal scores are ordered by the hash of their source produce, then by their full digest.

**Reason.** Bag and set spaces already use this order ([decision 41](#decision-41)), so one order serves every unordered discipline.

### <a id="decision-61"></a>Decision 61: Footprint exceptions for the ChromaDB mirror

| Field | Value |
|---|---|
| Date | 2026-10-07 |
| Status | In force. Implementation in P6 |
| Amends | The plan statement "node: no change expected" |
| Related | [Decision 58](#decision-58) |

**Statement.**

- `node/src/rust/runtime/setup.rs` gains one task spawn for the mirror reconciler.
- `docker/shard.yml` pins the ChromaDB image by digest and passes its environment to the nodes.

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
| Status | In force. The native-execution answer is superseded by decision 68 on 2026-10-08. The reconciler sentence is corrected by decision 70 on 2026-10-08. |
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
- **The reconciler repairs the journaled channels.**
  - The reconciler repairs the channels that the journal of the node touched. The collection metadata keeps them.
  - It repairs them to the view of the last finalized block.
  - After an LFS bootstrap, the mirror lacks the records that are older than the bootstrap, until they change again.
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
| Status | In force |
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

**User's words.** "Checks in both modes (Recommended)"

**Reason.** Play read the config through a read-only space job, which reserves host work. Replay returned the recorded output first. So one call made play reserve 1,815,346 VerificationBytes, 11,909 VerificationOperations and 59,288 SearchStateBytes more than replay. That gap is a divergence of play and replay as soon as a deploy carries a budget.

### <a id="decision-68"></a>Decision 68: `embed` and `ann` leave the native ban

| Field | Value |
|---|---|
| Date | 2026-10-08 |
| Status | In force |
| Amends | [Decision 57](#decision-57) (the clause "both fail in native execution"), the native answer of [decision 66](#decision-66) |
| Related | [Decision 69](#decision-69) |

**Statement.**

- The body references 236 (`embed`) and 237 (`ann`) stay in `non_deterministic_ops()` and leave the `forbidden` list of `to_dispatch_table`.
- Both processes run in native execution as recorded processes. The hot-file edit of `interpreter/system_processes.rs` is two lines.
- The cost-accounting owner removes the whole `forbidden` list.
- When the record-and-replay mechanism for external services reaches this branch, both processes move to it. The campaign does not implement that mechanism.

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
| Status | In force |
| Amends | The reconciler sentence of [decision 66](#decision-66), the N4 leaf of the tracker |
| Related | [Decision 12](#decision-12), [decision 18](#decision-18) |

**Statement.**

- No design plans a Casper change that is not directly Reified work.
- The campaign opens no question of finality, fork choice or the block lifecycle.
- It uses Casper state only through read-only interfaces where an approved design needs it. For example, the reconciler reads the finalized state through `RuntimeManager`.
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

## User decisions 72 to 78

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
| Status | In force. Implemented in P7 steps 2 (the Seq claims) and 3 (the HashSet store) |
| Amends | The plan sentence "the atomic Seq guard enforces exclusive use" (exclusive use holds for each channel) |
| Related | [I-4](#i-4), [decision 39](#decision-39) (F1, F17), [decision 47](#decision-47), [decision 69](#decision-69) |

**Statement.**

- The user approves P7 design v2, which applies the 26 amendments of the arbitration.
- Each channel of a seq space has its own causal-lineage claim. A use of a channel must descend from the previous use of that channel, else the deploy aborts with `SeqConcurrent`.
- Operations that touch no channel of the space (`gensym`, `config`, `qualifier`, allocations) and internal jobs (re-issues, settle jobs) are not checked.
- The seq qualifier, and so every HashSet family, runs only in native execution. Legacy execution keeps `QualifierUnavailable` for it.
- The HashSet store is the default space restricted to empty messages. A send with data aborts with `PayloadNotEmpty`, and a receive with a binder aborts with `PatternNotEmpty`.

**User's words.**

- The design: "Approve v2 (Recommended)".
- The Seq rule, after the request "Elaborate on each of these options, what they imply, their pros and cons, their consistency with the FIPS, and their consistency with the existing behavior of the default bag-based RSpace. Then ask this question again.": "Per-channel claim (Recommended)".
- The HashSet store, after the request "Elaborate on the context and background of the question. Most likely, we should model this behavior after the existing behavior of the default bag-based RSpace. Ask the question again after giving me enough information to make an educated decision.": "Bag semantics on units (Recommended)".

**Reason.**

- The FIPS restricts each seq channel ("you cannot use it in a concurrent process"), and the channels of the default space are independent.
- Legacy host operations bypass the scheduler, so a causal-lineage claim is deterministic only in native execution.
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
| Status | In force |
| Amends | [Decision 35](#decision-35) (the shared freeze of P1 and P4), and the per-leaf verification of the plan's execution protocol |
| Related | [Decision 13](#decision-13), [decision 48](#decision-48), [verification.md](verification.md) |

**Statement.**

- No recipe registration, VPI-1 relay, AAI-1 amendment or verification run happens before phase P11.
- The campaign continues with P8 at once.
- At P11 one freeze registers the recipes and shows the user the exact recipe command list (VPI-1).
- The same freeze amends the acceptance criteria (AAI-1) and runs the verifier for every active leaf, together with the final gates.
- Until then, each phase records its evidence in pgmcp progress notes and in the dossier.

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
| Status | In force. Implemented in P8 step 1 |
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
| Status | In force. Implemented in P8 step 5 |
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
| Status | In force, revised 2026-10-04. [Decision 45](#decision-45) adds the legacy charges of a legacy deploy. [Decision 47](#decision-47) adds one accessor, and [decision 78](#decision-78) adds one test edit. |
| Replaces | `observe_space_work` on the accounting observer, edits to `accounting/costs.rs`, and the earlier `reserve_primitive` plan |
| Footprint | Zero edits to the cost-accounting module and the observer trait |
| Related | [I-12](#i-12), [phlo](glossary.md#phlo), [host work](glossary.md#host-work) |

**Statement.**

- Phlo comes from the existing RSpace accounting-observer protocol.
- `SpaceTxn` drives the protocol for every staged produce, consume, and COMM, exactly as the host RSpace does, in play and replay.
- At the baseline, only `Comm`, `RSpaceProduce`, and `RSpaceConsume` carry consensus cost (`accounting/mod.rs::consensus_cost_unit`).
- `MeteredMachine::reserve_primitive` is diagnostic only, with cost 0, so it cannot charge anything.
- Matching, attach, and rehydration work use the existing host-work (liveness) budget. The tools are the metered methods of the injected matcher (`get_metered`, `check_commit_metered`) and `reserve_host_work`.
- The node charges host work before the work, independent of cache state.
- The Reified host-work cost functions live in a new `reified/costs.rs`.
- In a legacy deploy, each staged operation also pays the [legacy charges](glossary.md#legacy-charge) of its equivalent host operation ([decision 45](#decision-45), Option P).

**User's words.** The section quotation above.

**Reason.** The observer protocol already charges every RSpace operation, so the campaign needs no edit of the cost-accounting module.

### <a id="m-4"></a>M-4: Replay consult through child modules

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force |
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
| Status | In force |
| Amends | Keeps `rrs-parser-grammar (5)` |
| Related | [Decision 23](#decision-23) |

**Statement.** The tree-sitter parser implements bare floats as the contract says, which is a small change. `preprocess_floats` retires at the final pin.

**Reason.** The design agent suggested a deferral. The plan overrides that suggestion to avoid a known workaround.

### <a id="i-8"></a>I-8: Params only on factories and handles

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force |
| Amends | `rrs-reducer (6)` |
| Related | [Decision 18](#decision-18), [inert](glossary.md#inert) |

**Statement.** Params are allowed on system channels only for factory and handle channels. The `PARAMS` tuple of [decision 18](#decision-18) carries them.

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
| Status | In force |
| Amends | `rrs-reducer (2)` |
| Related | [Space transaction](glossary.md#space-transaction) |

**Statement.** The scheduler footprint of a space operation is a per-space key.

**Reason.** A per-space key is safe with joins and prefixes. A later benchmark decides any refinement.

**Note of decision 47 (2026-10-07).** The no-deadlock argument of native replay rests on this key (design v2, section 4.5).
The key keeps the transactions of one space in one conflict group, in operation order.
So every predecessor of a space row is complete when the row begins.
`NativeReplay.tla` checks `NoBlockedReadiness` with this key alone, without the native key [2, 0].

### <a id="i-12"></a>I-12: Charging rules

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force, revised 2026-10-04 |
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
| Status | In force, revised 2026-10-04 by [decision 18](#decision-18) |
| Amends | None |
| Related | [Decision 6](#decision-6), [decision 15](#decision-15) |

**Statement.** `empty_state_hash_fixed` does not change ([decision 18](#decision-18)), and its baseline test is a regression gate at every milestone. Milestone commits exclude only `Cargo.toml` (the `[patch]`) and `Cargo.lock`.

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
| Status | In force |
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

**Reason.**

- A P2 finding: the evaluation and the substitution of the baseline keep the flag `connective_used` of a tuple, a list, or a map. They keep it also when the children of the container change.
- The evaluation keeps the flags at `rholang/src/rust/interpreter/reduce.rs:3119-3141`, `3143-3164`, and `3135-3155`.
- The substitution keeps them at `rholang/src/rust/interpreter/substitute.rs:1342-1363`, `1365-1384`, and `1405-1428`.
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

- One space transaction runs at most `FIRE_ROUND_CAP = 64` fire rounds (`rholang/src/rust/interpreter/reified/costs.rs:44`).
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
