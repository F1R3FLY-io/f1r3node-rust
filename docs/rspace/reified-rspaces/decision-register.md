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
Decisions 19 to 39 override the earlier plan sections where they conflict.
The minimal-footprint revision (M-1 to M-5) overrides the target architecture of the plan where they conflict.

## Terms used in this register

- **Plan:** the realignment and completion plan [[PLAN](#ref-plan)].
- **Ledger:** the design ledger of 2026-08-15 in the [design record](design-record/2026-08-15-design-ledger.md). It is history, not a specification.
- **Baseline:** the commit `e748426` before the campaign changes. The plan calls it HEAD.
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
| [Decision 9](#decision-9) | Approved design proposals | 2026-10-03 | In force |
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
| [Decision 29](#decision-29) | Joins into ordered collections in P5 | 2026-10-04 | In force |
| [Decision 30](#decision-30) | Peek fires re-offer the sent value | 2026-10-04 | In force |
| [Decision 31](#decision-31) | Every operation on an ordered collection bumps its counter | 2026-10-04 | In force |
| [Decision 32](#decision-32) | P4 plan-text changes | 2026-10-04 | In force |
| [Decision 33](#decision-33) | assertEqual has arity 3 | 2026-10-04 | In force |
| [Decision 34](#decision-34) | P4 detail items | 2026-10-04 | In force |
| [Decision 35](#decision-35) | P1 close-out scope: lean, verify at P4 | 2026-10-04 | In force |
| [Decision 36](#decision-36) | Space replay reproduces play when a deploy runs out of phlo | 2026-10-05 | In force |
| [Decision 37](#decision-37) | Receive selection follows the F1r3lang rule from the F1r3lang merge on | 2026-10-05 | In force. Implemented in P12 |
| [Decision 38](#decision-38) | Priority-queue waiters wait in one queue per level | 2026-10-05 | In force |
| [Decision 39](#decision-39) | P5 design choices F1 to F17 and F20, without F21 | 2026-10-05 | In force, vetoable value of F9 |
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
| [I-11](#i-11) | Per-space scheduler footprint | 2026-10-04 | In force |
| [I-12](#i-12) | Charging rules | 2026-10-04 | In force, revised 2026-10-04 |
| [I-13](#i-13) | Native replay fails closed for spaces | 2026-10-04 | In force |
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
| Amended by | [Decision 30](#decision-30), which supersedes the storing of the incoming datum of a produce-side peek fire in the P1 design. [Decision 36](#decision-36), which moves the failure of an unpaired replay fire from staging to commit, so that D6 also covers `ReplayDivergence` |
| Related | [M-4](#m-4), [space transaction](glossary.md#space-transaction), [sub-order](glossary.md#sub-order), [sweep](glossary.md#sweep), [substrate design](design.md#substrate) |

**Statement.**

- D1: The volatile sweep runs at the end of `ReducerCore::inj_with_observation` and `ReducerCore::inj` (`reduce.rs`), in P7. Every play, replay, reporting, and native-replay path passes there, under the evaluation guard.
- D2: `SpaceStoreHandle` is a struct over `Arc<dyn SpaceTxnHost<C, P, A, K>>`. `SpaceTxn` takes whole operations with an explicit outcome (`ProduceOutcome::{Store, Fire}`, `ConsumeOutcome::{Park, Fire}`) and `commit`.
  `SpaceTxn` has no raw `put`, `remove`, or `log` methods and no `replay()`. The traits are generic over `C, P, A, K`, because rspace++ cannot name `Par`.
- D3: Each operation of a transaction runs under its own sub-order $`\mathit{parent} \mathbin{+\!\!+} [(i, 2)]`$, where 2 is `SUB_OPERATION_TAG`. Native phlo recording rejects two operations under one order (`accounting/native_runtime/operations.rs:212`).
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

## User decisions 28 to 39

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
| Status | In force |
| Supersedes | None |
| Related | [Decision 11](#decision-11), [head-only matching](glossary.md#head-only-matching), [counter meta-channel](glossary.md#counter-meta-channel) |

**Statement.**

- This is item A3 of the P4 arbitration.
- Queue, stack, and priority-queue spaces support joins across channels of one space (FIPS line 309).
- The design uses a continuation envelope with several `ord` values and a cascade that ends: it lifts a waiter and offers its data again.
- P5 implements these joins after its own design pass and its proofs of invariant preservation, cascade termination, and order independence.
- The P4 single-channel slice rejects such joins with `OrderedJoinUnsupported`, but only until P5 lands.

**User's words.** "Implement in P5 (Recommended)"

**Reason.** The FIPS allows these joins.
A join into an ordered collection needs an envelope with several `ord` values and a cascade, and both need their own design and proofs.

### <a id="decision-30"></a>Decision 30: Peek fires re-offer the sent value

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force |
| Supersedes | The storing of the incoming datum of a produce-side peek fire in the P1 design (decision 19, P1 design §6) |
| Related | [Decision 19](#decision-19), [retention](glossary.md#retention), [re-offer](glossary.md#re-offer), [substrate design](design.md#s58-peek-re-offer-fr-10) |

**Statement.**

- This is item A1 of the P4 arbitration. Fix rule FR-10 implements it.
- When a send fires a peek waiter, the substrate does not store the incoming datum.
- The agent keeps the datum in flight, and it offers the datum to the next waiter as the next sub-operation.
- The re-offer has the produce identity (`Produce::create`), the observer sequence, the counter step, and the authority of the re-produce of the baseline.
- If no waiter matches, the re-offer stores the datum.
- A consume-side peek still leaves the stored record in place.
- The fire receipt does not mark the incoming datum as retained.

**User's words.** "Offer to next waiter (Recommended)"

**Reason.** With a peek waiter at the head of a queue, a stored copy of the datum can match the next waiter.
That pending match breaks the no-pending-match invariant of the space.
The re-offer equals the re-produce of the baseline reducer (`reduce.rs:977-1012`), so the events and the observer calls stay equal to those of the host.

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
| Status | In force |
| Supersedes | The forwarder argument `GByteArray(sid)` of [I-2](#i-2), and the claim of [I-19](#i-19) that `SpaceAlreadyExists` stops the `Factory!!(…)` re-produce loop |
| Related | [Decision 18](#decision-18), [decision 22](#decision-22), [internal operation](glossary.md#internal-operation), [handle](glossary.md#handle) |

**Statement.** The user approved four changes of the plan text, items A5, A6, A7, and A21 of the P4 design.

- A5: `canonical_inner_channel_bytes` is the bincode of the sorted Par, which is the preimage of the store identity. Protocol Buffers encodes `locally_free`, so prost bytes would split one store channel into two keys.
- A6: `HANDLE_OP` is bound to the URN `rho:reified:internal:handleOp`. The baseline binds every system definition to a URN (`rho_runtime.rs:1574-1580`), and `registry_lookup` serves any key.
- A6, continued: the operation accepts a call only with the unforgeable `this`. It must be a `GPrivate` of exactly 34 bytes with KIND H or h, and its config record `C(sid)` must exist.
- A6, continued: this replaces the `GByteArray(sid)` forwarder of I-2, which would leak the capability through `config` and `toByteArray`.
- A7: P2 implements the send hook as the same-signature swap of `self.produce(` for `self.reified_produce(`. The function `reified_produce` splits the trailing params first, and it fails closed as decision 22 says.
- A7, continued: the H2 arm is one line without a guard, of at most 100 columns, for example `eval_marker`. The reason is that H1 admits only marker tuples into process position.
- A21: a persistent send (`!!`) to a two-byte `0xE0` operation channel, or to an H or h handle, aborts the deploy. The two-byte channels are the factories, `getSpaceAgent`, `assertEqual`, `vector`, and the internal operations.
- A21, continued: the baseline never cancels the re-produce sibling of a persistent send (`reduce.rs:713-779`, `1014-1062`). So the claim of I-19 that `SpaceAlreadyExists` stops the loop was wrong.

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
- A19: the code names are `RuntimeUnavailable` and `SpacesUnsupportedInNativeReplay`.
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
| Status | In force |
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
- **Until then**, space receives use the current guard rule of this tree, as the host does. A consume takes the first spatial match of each bind and checks the guard once, and a refusal parks the waiter (`ops_consume.rs:111-153`). On the produce side, a guard refusal moves to the next waiter (`space_matcher.rs:178-185`). Head-only disciplines test only the head, so both rules agree for the queue, the stack, and the priority queue.
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
- F11: a `new` inside `use` of an Array or Vector space runs one allocation job. The hot line `reduce.rs:1664` gains `.await`.
- F12: the tracker leaf `rrs-spaces-determinism-collision-ties` completes in P6.
- F13: the legacy Rocq modules get the dispositions that the P5 design report lists, with the changes of its readiness review. The port drops the axiom `FunctionalExtensionality`.
- F14: two set waiters are equal only when they are byte-identical.
- F15: a cell holds one record and one waiter.
- F16: PathMap bag and set peeks do not bump the token, which is the space counter that orders a PathMap space.
- F17: the `temp` and `seq` rows of the pattern-enforcement matrix complete in P7.
- F20: the `[D;n]` consume never parks, so [I-16](#i-16) holds.
- F21 is dropped: no `MixedArrowJoin` error code, because a program cannot reach it.

**User's words.** "Approve all (Recommended)", after the items were elaborated.

**Reason.** The P5 design pass found these choices where the plan and the FIPS leave details open, or where the plan does not fit the code.
The readiness review corrected the design, and it recommended each approved choice, some of them with changes.
A receive has one peek flag for all its binds, so no program can make mixed peeks, and F21 is not needed (readiness review S5).

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
| Status | In force, revised 2026-10-04 |
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

**Reason.** The rule needs no edit of the cost-accounting module. `reserve_primitive` is diagnostic only at the baseline, so it is not a charging path.

### <a id="i-13"></a>I-13: Native replay fails closed for spaces

| Field | Value |
|---|---|
| Date | 2026-10-04 |
| Status | In force |
| Amends | None |
| Related | [Decision 19](#decision-19) |

**Statement.** `create_native_replay_env` fails closed for spaces (`SpacesUnsupportedInNativeReplay`).

**Reason.** Only tests use that path, as the plan verified. Casper production replay uses `ReplayRSpace`.

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
- The evaluation keeps the flags at `rholang/src/rust/interpreter/reduce.rs:3065-3087`, `3089-3110`, and `3135-3155`.
- The substitution keeps them at `rholang/src/rust/interpreter/substitute.rs:1340-1361`, `1363-1382`, and `1405-1428`.
- Ordinary data can use connectives, as in `x!(Int)`. Without the refresh, a legal `use` body could fail closed as `NonCanonical`.
- Only the top level of a marker tuple can hold a stale flag, so the refresh costs time linear in the width of the tuple.
- To ignore the flags instead would weaken the contract of the decoder.

### <a id="i-21"></a>I-21: Fire-round cap of a space transaction

| Field | Value |
|---|---|
| Date | 2026-10-05 |
| Status | In force. The value is vetoable: the user may choose another cap. |
| Amends | Item A10 of the P4 design ("a bounded chain of fires"), arbitration item H8, and [I-12](#i-12) (work bounds for each operation) |
| Related | [Decision 34](#decision-34), [I-12](#i-12), [re-issue](glossary.md#re-issue), [space intent](glossary.md#space-intent) |

**Statement.**

- One space transaction runs at most `FIRE_ROUND_CAP = 64` fire rounds (`rholang/src/rust/interpreter/reified/costs.rs:28`).
- At the cap, the agent commits the staged prefix and re-issues the rest of the operation as a new `Intent::Space`. This is the re-issue path of A10.

**Reason.**

- A10 and H8 require a bound on the fires of one operation, but they give no value.
- The bound is deterministic, so play and replay agree.
- The bound changes the output only for a chain of more than 64 fires. There it sets the transaction boundaries and the counter bumps.

## References

<a id="ref-fips-agents"></a>[FIPS-AGENTS] M. Stay, "Agents," F1r3fly Improvement Proposal, 2025-08-20. [FIPS repository, approved/2025-08-20-Agents/Agents.md](https://github.com/F1R3FLY-io/FIPS/blob/main/approved/2025-08-20-Agents/Agents.md)

<a id="ref-fips-rrs"></a>[FIPS-RRS] M. Stay and L. G. Meredith, "Reifying RSpaces," F1r3fly Improvement Proposal, 2025-09-26. [FIPS repository, approved/2025-09-26-Reifying-RSpaces](https://github.com/F1R3FLY-io/FIPS/blob/main/approved/2025-09-26-Reifying-RSpaces/Reifying%20RSpaces.md)

<a id="ref-plan"></a>[PLAN] "Reified RSpaces: realignment and completion plan (takeover from codex)," approved by the user on 2026-10-04. Session plan file `review-pgmcp-epic-rrs-realignment-twinkly-wind.md` in `[HOME]/.claude/plans/`, outside the repository. The epic `reified-rspaces-mettatron-migration` (pgmcp id 5582) cites it in each leaf.
