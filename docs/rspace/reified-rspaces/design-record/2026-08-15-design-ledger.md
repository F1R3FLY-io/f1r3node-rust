# Reified RSpaces — Migration & Modernization Plan (f1r3node-rust + rholang-rs)

> STATUS: DRAFT — exploration in progress. This file is built incrementally; sections marked
> `[PENDING]` await agent reports or user decisions.

## Context

The Reified RSpaces feature (FIPS `2025-09-26-Reifying-RSpaces`) generalizes RSpace so different
data structures serve as tuple spaces (outer stores: HashMap, PathMap, Array, Vector; inner
collections: bag, set, cell, queue, stack, priority queue, bag+VectorDB), with channel qualifiers
(`default`/`temp`/`seq`), `use` blocks, send hyperparameters, and VectorDB similarity queries.
It was implemented against the OLD f1r3node repo (branch `feature/reified-rspaces-11-formal-verification`,
an 11-stage stacked series) and the OLD rholang-rs base (branch `feature/reified-rspaces-v2`),
but never merged before f1r3node was superseded by f1r3node-rust. The work must be:

1. **Migrated** into f1r3node-rust (current working branch `feature/cost-accounted-rho`, may land on
   dev/master first — code substantially frozen until this epic executes) and into rholang-rs
   (working branch `feature/cost-accounting-transpiler`, based on current master 5c4c9d2).
2. **Modernized**:
   - Replace the `~ sim("cos","0.85") ~ rank("topk",10) ~ "1.1,1.3,3.5"` chain + string-encoded
     vectors with an ergonomic hyperparameter syntax (user sketch:
     `for (@doc <- embeddings ~ q(query, sim="cos", threshold=0.85, rank="topk", k=10))`, plain
     `embeddings ~ [.5, .5, .1]`, `embeddings ~ query`), chosen for fluency/familiarity/consistency.
   - Remove ALL string-vector parsing now that floats exist in the language; support all sensible
     Rholang value types (floats, ints, rationals, possibly bools for binary vectors).
3. **Reviewed** end-to-end: design (modularity/extensibility), architecture, implementation
   (time/space complexity), formal verification, documentation (pgmcp documentation guidelines).
4. Tracked as a rigorous **pgmcp epic** whose steps cannot be misinterpreted.

**Constraints from user:**
- ONE PR per repo (no stacked PRs).
- No changes now — review + plan + epic creation only.
- Iterate the design with critical agents until they recommend no further improvements.
- Target branches won't change much before implementation (next priority after this plan).
- **FV is MANDATORY, not optional** (2026-08-14): f1r3node-rust now carries formal verification
  models in-tree (165 Coq + 136 TLA+ files already), so PR #338's "optional" framing is void.
  The reified-rspaces PR must include the ported+modernized Rocq proofs and TLA+ specs, adapted
  to this repo's conventions (docs/theory layout, axiom-free policy, bounded-memory TLC via
  scripts/lib/tlc-run.sh, local verification gates), and they must be green at merge. FV must
  also be UPDATED to match the modernized design (new syntax, native numeric vectors, determinism
  rules) — not merely transliterated.

## Established facts (verified this session)

### Branch topology
| Repo | Source branch | Checkout | Tip | Base (merge-base) | Scope |
|---|---|---|---|---|---|
| f1r3node (old) | `feature/reified-rspaces-11-formal-verification` | `../f1r3node-reified-rspaces` | ff479042 | 5c0363d9 = "Merge origin/rust/dev into new_parser" | 238 files, +85,000/−1,001 |
| rholang-rs | `feature/reified-rspaces-v2` | `../rholang-rs-master` (worktree) | 047647e | 1ff2110 (= `f1r3node_dependecies`, PR #82) | 126 files, +65,341/−51,588; base is 399 commits behind master |
| rholang-rs | target: `feature/cost-accounting-transpiler` | `../rholang-rs-cost-accounting-transpiler` | 02cef80 | based on master 5c4c9d2 (current) | — |
| f1r3node-rust | target: `feature/cost-accounted-rho` (this repo) | `.` | working tree with uncommitted cost-accounting work | — | — |

Stacked series in f1r3node: 01-foundation, 02-inner-collections, 03-outer-storage,
04-generic-rspace, 05-vectordb, 06-vectordb-inmemory, 07-system-runtime, 08-useblock-normalizer,
09-hyperparameters, 10-integration-docs, 11-formal-verification. Everything is contained in 11.

### PR #83 (rholang-rs) — syntax as implemented (v2)
- `use SPACE { ... }` block — space context for sends/news within.
- Send hyperparameters: `x!(42; 0, 100)`, `y!(msg; priority=1, ttl=3600)` — positional then named
  after `;`; names/arities validated against current space context.
- `channel!?(data; ttl=60)` persistent-send + receive combination.
- VectorDB similarity: `for (@doc <- embeddings ~ sim("cos", "0.85") ~ rank("topk", 10) ~ "1.1,1.3,3.5")`
  — `~`-chained override expressions ending in query vector; floats as strings (workaround);
  `[50, 50, 10]` integer vectors literal.
- MeTTaIL theory enforcement (stubbed): `free Nat()`, `HMB!?("default", free Int())`.
- Space-type annotations: `new x_1 : space_j, ... in { ... }`.
- System URIs: `rho:space:HashMapBagSpace`, `rho:space:PathMapSpace`, `rho:space:getSpaceAgent`,
  `rho:assert:assertEqual`.
- Author (user) explicitly not fixed on syntax; PR notes interesting bits are in the backend.

### FIPS spec content (read in full)
- RSpace = KV store; channels→(data bag, continuation bag); **no-match-remains invariant** after ops.
- Space agent behavioral type: `constructor(qualifier, theory, ...)`, `gensym()`,
  `produce(channel, process)`, `consume(listMatcherChannel, arrowType, body)` (arrows `<-`, `<=`, `<<-`).
- Outer stores: HashMap (current), PathMap (prefix-send semantics; quantale; MeTTa), Array
  (fixed: out-of-names abort; cyclic: reuse w/ interference), Vector (grow; OOM abort),
  HashSet-of-channels (sequential; empty-pattern receives).
- Inner collections: bags (default), sets (idempotent send), cells (≤1 item; error on 2nd send),
  queues (FIFO — nodes must agree on arrival order), stacks (FILO), priority queues
  (per-priority queues; patterns may specify priority), bag+VectorDB (consumes→bag, data→vecDB;
  patterns give similarity-distance bound + query vector; a `for` consumes ONE vector within bound).
- Qualifiers: default = persistent+concurrent; temp = concurrent, never disk; seq = temp +
  (never sent on channels, no concurrent use, ≤1 sequential process).
- `use` block: default space for untagged channels in `new` blocks and forged names.
- Joins permitted within one space, prohibited across spaces.
- `getSpaceAgent(space)` returns underlying agent.
- Proposed 6-layer refactor: DataCollection/ContinuationCollection traits → ChannelStore trait →
  SpaceAgent trait → CheckpointableSpace/ReplayableSpace → concrete impls → SpaceFactory +
  SpaceRegistry (`rho:space:*`). Migration path of 6 phases mirrors the stacked branches.
- **Spec holes** (filled ad hoc by the branches, to be settled properly now): similarity-query &
  hyperparameter surface syntax; theory documents ("specified in another design document";
  MeTTaIL not ready → stubs); unification semantics question in quantale section left open;
  tensor-logic & nonlinear vector ops flagged as "look into".

### PR stack #328–#338 (F1R3FLY-io/f1r3node) — all OPEN, ZERO review comments (no reviewer constraints)
Per-stage inventory from PR bodies (authoritative stage descriptions):
1. **#328 Foundation**: `DataCollection`/`ContinuationCollection` traits; `SemanticEq`/`SemanticHash`;
   `SimilarityCollection` trait; `TopKEntry`; collections module decomposition. Depends on rholang-rs PR #83.
2. **#329 Inner collections**: Bag, Queue, Stack, Set, Cell, PriorityQueue data collections;
   Bag/Queue/Stack/Set continuation collections; `VectorDBDataCollection`; `SmartDataStorage`/`SmartIterator`;
   `LazyResultProducer`.
3. **#330 Outer storage**: `ChannelStore` trait; HashMap/PathMap/Array/Vector/HashSet stores;
   VectorDB-aware store (similarity routing); `InnerCollectionType`/`OuterStorageType` specs; PathMap prefix semantics.
4. **#331 GenericRSpace**: produce/consume/install; `SpaceAgent` + `AsyncSpaceAgent`; **`ChargingAgent`
   (gas accounting)**; `SpaceQualifier`; similarity extraction helpers; **ISpace adapter (backward compat)**.
5. **#332 VectorDB base**: `VectorBackendDyn` trait; `EmbeddingType` + pack/unpack; backend registry;
   `SimilarityMetric` (Cosine, Euclidean, Manhattan, …).
6. **#333 In-memory VectorDB**: backend impl; similarity handlers; distance metrics; swap-and-pop
   defragmentation (ndarray lacks masked ops); compound metric handling.
7. **#334 System runtime**: `rho:space:*` URN system processes; `SpaceRegistry` + checkpoint mgmt;
   `SpaceFactory`; use-block evaluation in reducer; normalizer support for UseBlock/FunctionCall/TheoryCall
   AST; Send/SendSync normalizers with hyperparameters.
8. **#335 Normalizer cleanup**: AstKeyValuePair rename; p_if_normalizer simplification.
9. **#336 Docs**: docs/architecture/extending-spaces.md; docs/performance/*.md; benchmarking methodology.
10. **#337 Tests/benches/examples**: rholang/benches/spaces_benchmark.rs; rholang/examples/reified_rspaces/*.rho;
    rholang/tests/spaces/*.rs; scripts/benchmark-with-perf.sh.
11. **#338 FV (marked "optional")**: Rocq formal/rocq/reified_rspaces/ — GenericRSpace.v, Checkpoint.v,
    Phlogiston.v, PathMapQuantale.v, OuterStorage.v, Collections/*.v, Safety/Properties.v; TLA+ formal/tla/ —
    GenericRSpace.tla, CheckpointReplay.tla + cfg; docs/formal-verification/.

rholang-rs PR #83 also has ZERO review comments.

### rholang-rs master already has the numeric types
- d25f953 = PR #90 "feat: add extended numeric types (**Float, BigInt, BigRat, FixedPoint**)" (09de1a0) —
  and d25f953a is EXACTLY the rev f1r3node-rust pins. Native float/rational literals exist in the parser today.
- 988edd6 adds **`where` clauses to receipts and match cases** — a candidate host for similarity predicates.

## SCOPE ADDITION (2026-08-14): MeTTaTron Reified RSpace — DEEP-DIVE REQUIRED

User directive (high priority, "final piece to productionize MeTTaTron"):
- An additional Reified RSpace representing MeTTaTron's **Atom/MORK Space**
  (`../MeTTa-Compiler/`), layered atop the existing reified-rspaces architecture.
- **Direct process-to-process communication** between Rholang and MeTTaTron — the previous link
  was a CUSTOM SERIALIZATION SCHEME (user clarification 2026-08-14), and ALL of it must be
  replaced: zero serialization round-trips in the communication path; the reified space IS the
  communication medium (shared in-process Atom/MORK space; direct memory-level integration).
  Also eliminates the "clunky MeTTaTron<->Rholang support code" on both sides.
- Query the Atom/MORK space with BOTH **MeTTa and MM2 queries** in clean, elegant, expressive
  syntax; highly efficient implementation.
- Principled design; really deep-dive and work hard on this component.
- Prior art: old support branch `dylon/mettatron` in ../f1r3node/, worktree at
  `../f1r3node-mettatron/`.
- Convergence note: FIPS PathMap outer store is annotated "For MeTTa" (prefix-send quantale
  semantics) — the old branch's PathMapChannelStore faked it with linear-scan HashMaps; the
  MeTTaTron space should productionize this layer over the REAL PathMap/MORK machinery
  (PathMap at ../PathMap, MORK at ../MORK — both read-only dependencies, do not modify).
- Existing bridge to study/replace: `../MeTTa-Compiler/src/rholang_integration.rs` (2,194 lines)
  + `../MeTTa-Compiler/src/pathmap_par_integration.rs`.

**MeTTaTron-space design axes (brief seeds for the deep-dive designer; refined after exploration):**
1. Space semantics: produce = assert atom (Par→Atom mapping); consume w/ MeTTa or MM2 pattern =
   query/match (one result per receive? all-results iteration via lazy channel like VectorDB
   top-k? multiplicities?); persist flags ↔ atom permanence; prefix semantics per FIPS quantale.
2. Query-language embedding syntax: how MeTTa and MM2 queries appear in a `for` — via the chosen
   unified receive-hyperparameter mechanism (e.g. query text/AST + lang selector), dedicated
   pattern literal, or quoted-Par patterns compiled to MORK queries; must meet the 9-adjective
   rubric and compose with the similarity-query syntax design (same `~` family?).
3. Process model: SAME-PROCESS embedding of the MORK space (no serialization) — Rholang reified
   space and MeTTaTron evaluator sharing one Atom/MORK space; concurrency + evaluation-vs-mutation
   story; who owns the space lifecycle; how MeTTaTron evaluation is triggered from Rholang and
   vice versa (continuations firing on atom assertion?).
4. Atom↔Par mapping: canonical, total (or well-defined partial) bidirectional encoding; reuse
   pathmap_par_integration approach vs MORK's native S-expr byte encoding; numeric types incl.
   floats/rationals.
5. Determinism/consensus: trie iteration = canonical byte order (advantage over HashMap stores);
   canonical state root over PathMap serialization; deterministic query-result ordering + totality;
   replay semantics; whether MeTTaTron-space ops participate in block state (vs node-local space
   qualifier temp/default decision).
6. Efficiency: zero-copy or near-zero-copy produce/consume; structural-sharing snapshots for soft
   checkpoints (PathMap persistence); prefix queries via zipper descent NOT linear scans; batched
   query execution; complexity targets stated up front.
7. Layering: an additional Reified RSpace atop the existing architecture (new OuterStorageType +
   backend? or a dedicated SpaceAgent impl?) — must slot into ChannelStore/SpaceAgent traits,
   URN family (e.g. rho:space:mettatron or rho:space:atom:pathmap:*), factory + registry, gas
   accounting, and FV obligations like every other space.
8. Elimination target: enumerate exactly which MeTTa-Compiler bridge code + f1r3node support code
   dies; migration path for existing MeTTaTron users.

## Syntax design space [DRAFT — refined after agent reports + critique rounds]

Candidates for the similarity/hyperparameter receive syntax (send side already `x!(m; k=v)`):

- **A. Query-constructor:** `embeddings ~ q(query, sim="cos", threshold=0.85, rank="topk", k=10)`
- **B. Postfix parameterization:** `embeddings ~ query(sim="cos", threshold=0.85)`;
  `embeddings ~ [.5,.6,.7](sim="jaccard")`
- **C. Payload;kwargs dual of send:** `embeddings ~ (query; sim="cos", k=10)` — mirrors
  `x!(payload; kwargs)` exactly (receive-side dual; maximal internal consistency)
- **D. Status quo v2 chain:** `~ sim(...) ~ rank(...) ~ query` — rejected (user: less ergonomic)
- Simple forms in all candidates: `embeddings ~ [.5, .5, .1]` and `embeddings ~ query`.

Evaluation axes (user's words): **principled, fluent, ergonomic, efficient, professional,
user-friendly, familiar, consistent, and ELEGANT** (added 2026-08-14). Operationalized as:
familiarity (Python kwargs precedent), fluency (reads left-to-right as intent), consistency with
send-`;` hyperparams and the rest of Rholang's grammar, elegance (minimal orthogonal forms — no
redundant ways to say the same thing; simple cases look simple, complex cases grow smoothly from
simple ones), grammar conflict surface (application-on-expression is new; `;` inside for-receipts
collides with sequential receipt separator unless parenthesized), extensibility to space-specific
params (priority-queue receive params etc.), and consensus determinism of defaults.

Type modernization: native vector literals (List of Float/Long/Rational/Bool), hyperparam values as
native literals; **determinism requirements** (consensus!): canonical similarity arithmetic
(sequential reduction order; defined NaN handling; total tie-break for top-k — e.g. by insertion
order/Blake2b of datum), exact rational option where feasible.

## Agent report syntheses

### A. f1r3node reified implementation review — COMPLETE (full report in session; distilled here)

**Shape**: new `rholang/src/rust/interpreter/spaces/` subsystem (~22k LoC; types/, collections/,
channel_store/ [hashmap|pathmap|rholang_pathmap|array|vector|hashset|vectordb], generic_rspace.rs
(3.8k), registry.rs, matcher.rs, agent.rs+async_agent.rs+adapter.rs, vectordb/ [registry, in_memory
backend+handlers+db+metrics], factory/ [urn, config, theory], phlogiston.rs, charging_agent.rs,
history.rs, similarity_extraction.rs, errors.rs) + `tensor/operations.rs` (1.1k, rho:lang:vector);
reduce.rs +1.4k (space routing, use-block task-local stack, Seq guards, hyperparam eval/validation,
allocation modes); system_processes.rs +2.3k (`create_generic_rspace!`, create_space, 96 URN
factories `rho:space:{inner}:{outer}:{qualifier}`, vector_ops); rho_runtime.rs +171 (auto-gen 96
Definitions, shared DashMaps); 7 normalizers touched + 2 new (p_use_block, p_function);
RhoTypes.proto: Par.use_blocks(12), Send.hyperparams(8, oneof positional|named), ReceiveBind.
pattern_modifiers(5, EFunction), New.space_types(6), UseBlock, Expr.EFree(34)/EFunction(35);
sorter: EFREE=123/EFUNCTION=124 scores but par/send/receive sorters DO NOT score/sort new fields;
rspace++ interface: produce(+priority), consume_with_modifiers(Vec<Vec<u8>>), RSpaceResult.suffix_key;
empty_state_hash changed (consensus-visible). Parser = external rholang-parser pinned to
feature/reified-rspaces-v2 rev 047647ee (NOT embedded).

**Execution model**: live state in DebruijnInterpreter DashMaps (space_store: id→RhoISpace =
Arc<Mutex<Box<dyn ISpace>>>, channel_space_map, qualifier/config maps, seq guards, index counters);
`spaces/registry.rs` SpaceRegistry is a spec-shaped PARALLEL implementation used only by tests/TLA
mirror (dead in runtime). Channels stamped to creating space (not ambient use-block) — use-block
stack only for non-GPrivate fallback. Similarity: consume-side modifiers → EFunction bytes →
GenericRSpace: query via backend (brute force O(n·d), min-heap top-k), results LAZY via fresh
GPrivate result channel + LazyResultProducer; produce-side SimilarityQueryMatrix (ndarray, one
mat·vec vs all waiting queries — but cosine-only, ignores per-query metric; fallback path honors
metric = divergent dual semantics D10). Theories: EFree→BuiltinTheoryLoader (Nat/Int/String/Bool/
Any); MeTTaIL file/inline/URI → TheoryNotSupported. Gas: ChargingSpaceAgent + PhlogistonMeter
PARALLEL to interpreter accounting/costs.rs (unreconciled, D12).

**Defect register (D1–D13)** — all must be resolved in migration:
- D1 UUID space ids (system_processes.rs:1490) — replay-fatal; → Blake2b512Random-derived ids.
- D2 UUID lazy result channels + empty random_state (generic_rspace.rs:2422,:2455) — replay-fatal.
- D3 Produce::default() events on all 3 produce-match paths (:943,:1905,:1981) — event log
  meaningless; space_store never merkleized into casper block state (structural replay gap).
- D4 VectorDB clone: HashMap-order re-store + next_id not reset → id scrambling on soft checkpoint.
- D5 Unordered iteration: PathMap prefix candidate selection; checkpoint serialize→hash over
  HashMap iteration (same state ⇒ different roots across runs!); find_and_remove visit order;
  float max_by ties (NaN→Equal).
- D6 WildcardMatch in ALL factory spaces — patterns never matched in user spaces (for (@42 <- ch)
  receives anything); SpatialMatcherContext never invoked on GenericRSpace path.
- D7 use_blocks/hyperparams/pattern_modifiers cloned but unscored/unsorted in canonical sorters.
- D8 Seq guard TOCTOU (contains-then-insert); cyclic array wrap fetch_add/store race.
- D9 try_lock().unwrap() on space mutexes (reduce.rs:637,:659,:736) — panics under contention.
- D10 Matrix path fires "first" continuation not matched one ("refine later"); pattern-equality
  removal may remove wrong continuation; metric divergence matrix-vs-fallback.
- D11 Checkpoint drops persist flags ((a,false)) + installs + log.
- D12 Dead parallel SpaceRegistry (SystemTime::now inside!); two gas systems; vectordb feature flag
  always-on.
- D13 Stale comments/docs; ttl advertised unimplemented; rank("all")/sim("boost") doc-only;
  priorityqueue(N) URN param ignored (always 3 + dynamic growth); 4-byte LE modifier framing;
  TypeId pointer cast for theory validation; HNSW/SQ8 configs parsed but NEVER used in search.

**String-vector parse sites (§3, complete)**: similarity_extraction.rs:62–79 (query embeddings),
:556–587 (stored embeddings), :139–150 (threshold strings); system_processes.rs:2486–2503
(vector_ops in), :2595–2631 (out), 2018–2032 (config threshold); rholang_backend.rs:96–100;
raw_config.rs:199; + integer 0–100 scaling convention sites; + docs/examples/tests advertising
comma-format. EmbeddingType {Boolean, Integer(0–100), Float=comma-string} → replace with native.

**Hyperparams (§4)**: proto oneof; only PriorityQueue schema accepts anything (max_positional=1,
["priority"]); priority named-wins-over-positional; GInt≥0; system channels reject all; 0=highest;
sim params {metric ∈ cos|dot|euclidean|manhattan|hamming|jaccard, threshold 0–1 string or 0–100
int}; rank {topk only, k≥1 default 1}; unknown modifiers warn+skip.

**Perf (§5)**: brute force O(n·d) SIMD mat·vec; top-k heap O(n log k); swap-and-pop O(d) OK;
SimilarityQueryMatrix.swap_remove REALLOCATES O(q·d); VectorDB clone O(n·d) EVERY soft checkpoint;
PathMap stores = linear HashMap scans (PathMap crate UNUSED — misnomer!); consume modifiers do
encode→decode round-trip in-process; per-op dyn downcasts; whole-space Mutex serialization;
DashMap+task-local design is contention-conscious; honest perf ledger docs (2.3× phlogiston etc.).

**FV (§6)**: formal/rocq/reified_rspaces/ 19 files ~14.6k lines, 526 statements, 498 Qed,
0 Admitted, 2 documented Aborts, 9 Axioms/Parameters (Checkpoint.v hash assumptions;
PathMapQuantale.v path enumeration+ordering). TLA+: GenericRSpace.tla (NoPendingMatch), 
CheckpointReplay.tla (Rust OperationLog cites its line numbers), SpaceCoordination.tla (use-block
stack; liveness commented out). NOT in CI. Gaps: no mechanization of implemented similarity
pipeline/lazy channels/hyperparams/DashMap routing/floats/cross-space replay; proofs model the
idealized registry, not the shipped reducer; "replay_determinism" can't hold given D1–D5;
code↔proof line citations will rot.

**Docs (§7)**: extending-spaces.md API drift (add/remove/find_match vs real put/find_and_remove/
peek); examples README references 4 nonexistent files + stale `tasks!!priority(3,{})` syntax +
wrong priority polarity; no similarity-semantics design doc, no consensus/replay design note, no
FIPS conformance matrix, no security argument for index-based ids, no proper diagrams.

**Tests (§8)**: ~900 tests (18 spaces files ≈760 + vectordb ≈130 + similarity_spec 26 e2e + Seq
suite) + spaces_benchmark.rs (15 groups) + benchmark-with-perf.sh. Gaps: no replay-determinism
test, no cross-run determinism, no pattern-enforcement test (would expose D6), no casper-level
block replay with spaces, no sorter tests for new fields.

### B. rholang-rs grammar/parser + forward-port sizing — COMPLETE (full report in session; distilled)

**True size**: reified-v2 = 2 substantive commits; ~1,200 hand-written lines (grammar.js +129,
parsing.rs +527, ast.rs +149, ast_builder +36, traverse +19, rholang-lib ~40, corpus 301) — the
other ~92% of +65k is regenerated parser.c/grammar.json/node-types.json + insta golden snapshots +
corpus re-records (send_inputs reshapes the CST of EVERY send).

**Grammar rules (v2, verbatim inventory in session)**: use_block `use NAME BLOCK` (prec 2);
send_inputs = `( data? (';' hyperparam_list?)? )` with named_hyperparam `var = _proc` (prec 2) |
positional; applied to send, send_sync, send_receive_source; similarity_modifier on linear_bind =
`(~ sim(metric[, threshold[, extra]]))? (~ rank(fn[, params]))? ~ query`; free `free theory_call`
(prec 10), theory_call `var ( )`; function_call `var (proc_list)` (prec 11, added for
getSpaceAgent); name_decl gains `: space_type` before optional `(uri)`. sim/rank are contextual
strings; `use`/`free` RESERVED GLOBALLY (v2 mistake — against master's PR #95 contextual-keyword
philosophy; port should model on agent_block and keep them contextual). `rho:space:*` URIs have
ZERO grammar footprint (ordinary uri_literals).

**AST (v2)**: Hyperparam {Positional(AnnProc) | Named{key: Id, value}}; HyperparamList =
SmallVec<[_;2]>; Send/SendSync gain hyperparams: Option<HyperparamList> (None vs Some([]) distinct);
Source::SendReceive has the field but parsing.rs NEVER populates it (TODO — for-source `!?`
hyperparams dropped); Proc::{UseBlock, FunctionCall, TheoryCall}; PatternModifier{name, function,
params: SmallVec<[_;4]>}, PatternMatchSpec{modifiers: SmallVec<[_;2]>, query}; Bind::Linear gains
pattern_match: Option<PatternMatchSpec>; NameDecl gains space_type: Option<Name>. NO new
ParsingError variants — hyperparam ordering/keys and sim/rank arg types UNVALIDATED at parse level.
rholang-lib sem gap: hyperparams + pattern_match matched away with `..` — free vars in queries
never resolved/diagnosed. QA: the branch's own vectordb_similarity.rho corpus test still uses v1
`~>` syntax → golden snapshot RECORDS A PARSE FAILURE (must fix, not inherit).

**Master (5c4c9d2) state**: numeric literals native (signed/unsigned ints `42i32`/`42u8`, bigint
`-7n`, bigrat `3r`, float `1.5f64` w/ e-notation, fixed-point `3.14p10`) BUT float suffix is
MANDATORY — bare `0.85` does not lex (f1r3node-rust's compiler.rs preprocess_floats regex bridges
bare→f64 on its side). Receipt model REWRITTEN for where-clauses (Receipt = struct{binds, guard},
Case/Branch guards); parsing.rs 3,277 lines w/ agent-block + try/catch desugaring (hygienized) +
send_method `x!y(args)` → `x!?("y",args)` rewrite that PUSHES K::ConsumeSendSync (the continuation
v2 re-shapes); 8 new crates (bytecode, compiler, interpreter, process, rspace, vm, wasm×2);
rholang-compiler codegen has `_ => bail!` catch-alls (new variants fail at RUNTIME); rholang-lib
sem has EXHAUSTIVE Proc matches (new variants fail at compile time — good); parser is cdylib+rlib
with wasm stub to mirror. NO reified syntax on master (all greps 0).

**CA-transpiler branch (02cef80)**: +67/−2 grammar (signed_term `{% P %}[s]`, token_stack `s :: S`,
signed_bind in receipt choice, input proc → choice(block, signed_cont), signature sub-grammar
ground/hash/compound/transfer); AST Bind::Signed 4th variant + Proc::SignedTerm/TokenStack +
Signature enum; parsing.rs +465. Tokens DISJOINT from reified ({%,%},::,(*),-o vs ;,~,use,free,:)
→ semantically composable; but BOTH touch receipt/bind/input rules (3-way textual merge, hand-
resolvable) and send_inputs reshape forces re-recording CA's fresh cost snapshots + parser.c regen.

**Conflict analysis**: `~` = prefix negation on master (prec 15) — reified `~` is postfix-of-source
in linear_bind, coexistence PROVEN at base; new hazards: compose with where-guard tail
(`for (x <- ch ~ q where g)` — helped by `where` reserved) and CA signed_bind; `~ sim(...)`
ambiguity (sim_modifier vs function_call query) resolved by lookahead at base — must re-validate
via tree-sitter generate against master's larger automaton (no conflicts: section anywhere; new
ambiguity fails generation LOUDLY). `;` inside send parens is additive (no `;` there today), but
`x!?(a; k=v); cont` has two `;` roles (parens disambiguate); MUST decide send_method hyperparam
policy (`x!m(args; ttl=1)`?). `: space_type` in name_decl: cleanest port (rule byte-identical to
base; map `k:v` colon is a different context; CA chose `::` for cons partly to avoid this).
Generated parser.c/grammar.json/node-types.json: NEVER merge — regenerate (tree-sitter CLI 0.25.6,
LANGUAGE_VERSION 15); proc-macros kind!/kw!/field! resolve at COMPILE TIME from compiled grammar.

**Test infra (master)**: tree-sitter corpus 68 files (feature .txt + whole-program .rho); parser
golden.rs = rstest glob over tests/corpus/*.rho (95) → insta debug snapshots; desugar_equivalence.rs
= the pattern for sugar; numeric_types_tests.rs, where_clauses_tests.rs precedents. All snapshots
re-record on send Debug change. CI: only npm publish workflow; Rust tests via Makefile/scripts.

**Publishing**: rholang-parser v0.1.0 consumed by git-rev pin (f1r3node-rust dev/master pin
527af8a = PR #95 merge; feature/cost-accounted-rho pins 02cef80 = CA tip; CLAUDE.md's d25f953a is
STALE). Port lands via new upstream commit + Cargo.toml/lock re-pin (established W1 gate pattern).

### C. f1r3node-rust integration surface — COMPLETE (full report in session; distilled here)

**Parser consumption**: workspace Cargo.toml:81 pins `rholang-parser = { git=…rholang-rs, rev="02cef80" }`
= "Merge master into feature/cost-accounting-transpiler" — the pinned parser IS the cost-accounting
line (W1 syntax incl. SignedTerm/TokenStack + numeric literals). NO [patch] section. Pipeline:
compiler.rs source_to_adt (preprocess_floats regex) → RholangParser::parse → normalize.rs arm
dispatch → p_*.rs normalizers → rhoapi protobuf → par_sort_matcher. Cost-accounting arms
(Proc::SignedTerm :461, Proc::TokenStack :465 → normalizer/cost_accounting/*) are the EXACT template
for reified syntax. **Parser AST merge is a prerequisite external deliverable**: reified-v2 AST
(UseBlock/FunctionCall/TheoryCall/Hyperparam/receipt modifiers) × cost-accounting AST must merge in
rholang-rs, new rev pinned (mirrors W1 "upstream parser publish" gate).

**Proto field renumbering (REQUIRED — old wire format cannot port)**:
Par.use_blocks 12→≥15 (12=conditionals, 13/14=cost); ReceiveBind.pattern_modifiers 5→≥6
(5=cost_signature); Expr.e_free 34→≥38, Expr.e_function 35→≥38 (34–37 = g_double/g_big_int/
g_big_rat/g_fixed_point); Send.hyperparams=8 OK; New.space_types=6 OK. models/build.rs strips
PartialEq/Hash derives → extend manual impls in models/src/lib.rs (+167 precedent). Consensus:
bump empty_state_hash_fixed() (runtime_manager.rs:1807; repo precedent exists incl. a WIP
2026-08-13 bump) — activates atomically on unreleased branch.

**rspace++ deltas are small BUT trait shape changed**: ISpace here is **async fn + &self** (old
branch patched sync &mut self); add RSpaceResult.suffix_key (serde default), produce(+priority:
Option<usize>), default-method consume_with_modifiers; mirror in Tuplespace alias + replay/
reporting. Spaces layer (~70 new files) is self-contained → ports near-verbatim EXCEPT async
adaptation + determinism fixes; ~35 modified files need true 3-way merges (this repo since gained:
async ISpace, where-clauses + If + rho-pure-eval in receive path, numeric types, cost accounting).

**ChargingRSpace is GONE here** — old branch's charging_rspace.rs diff has no target; phlogiston/
ChargingSpaceAgent must be re-based onto accounting/{costs,cost_accounting,…} `self.cost.charge()`
idiom (old branch's use_block_eval_cost arm in accounting/costs.rs ports as-is). Resolve D12
two-gas-systems here: unify on interpreter Cost accounting.

**System processes**: Definition{urn, fixed channel, arity, remainder, body_ref, handler};
FixedChannels bytes 0..=37 OCCUPIED (incl. ollama 28–30, deploy_data 31, chroma 32–36,
registry_lookup 37) → **space-factory byte base must rebase to ≥200** (old branch used 25–120,
collides). Injection seams: std_system_processes() or casper's additional_system_processes
(established seam via create_runtime_from_kv_store:1579). chromadb_service.rs + CHROMA_QUERY in
non_deterministic_ops is the service-level VectorDB precedent (record/replay). **Reified similarity
CANNOT use NonDeterministicCall record/replay (it's inside consume) — must be intrinsically
deterministic**: total-order tie-break (secondary key = stable datum hash), total f64 ordering +
NaN policy, canonical iteration for stores; gate modules: spaces/matcher.rs, collections/
similarity.rs, vectordb handlers, GenericRSpace event emission vs legacy Log.

**Determinism/merge gates**: trace/event.rs (Event Ord; COMM identity =(channel_hash,hash,
persistent)), stable_hash_provider (hash_produce/hash_consume), replay_rspace + reduce.rs
produce_inner :316–353 replay consult; casper interpreter_util/runtime_manager/replay_failure/
event_converter; merging dag_merger/conflict_set_merger/block_index/deploy_chain_index +
rspace++ merger/* + mergeable_tags — new space COMMs must be merge-understood or declared
conflicting. **Open design question the epic MUST answer**: do non-default spaces participate in
the tuplespace state hash/replay check (old branch: in-memory spaces/history.rs OUTSIDE the LMDB
Merkle history) or are they per-block ephemeral?

**Numeric types: NO models change needed for floats** — Expr.g_double=34 stores RAW IEEE-754 bits
(fixed64; bit-exact, −0.0/NaN payloads preserved), g_big_int=35 (two's-complement bytes),
g_big_rat=36, g_fixed_point=37; parser AST + reducer arithmetic (18 GDouble refs) + sorter support
all present; num-bigint/num-rational in workspace. Old branch's ndarray/tensor ops port on top.
→ Thresholds/embeddings upgrade to native g_double (+ optionally BigRat for exact metrics).

**FV/docs layout**: formal/rocq/reified_rspaces/ drops in at same path (repo pattern:
cost_accounted_rho, fork_choice, …); TLA relocates to formal/tlaplus/reified_rspaces/; TLC via
scripts/lib/tlc-run.sh; gates scripts/check-reified-rspaces-{ALL,proofs,tla-invariants,…}.sh;
CI mirrors dev exactly (FV local-gated only). Docs → docs/theory/reified-rspaces/* (+ diagrams
via docs/theory/diagrams render.sh + palette; PlantUML-preferred), language reference extends
docs/rholang/ numbered chapters (01–07 exist; PathMaps/Zippers already ch.07). Examples →
rholang/examples/reified_rspaces/*.rho; tests → rholang/tests/spaces/* etc.

**Sequencing**: 343 uncommitted WIP files (+29.6k/−14.6k) overlap the port's hottest files
(RhoTypes.proto, models/src/lib.rs, sorters, normalize.rs, p_input_normalizer, accounting/*,
casper runtime files) → land/stabilize cost-accounting WIP FIRST, then port. Old-branch base is
1,214 commits behind old rust/dev tip; module layout identical here (same paths/names).

**Top risks (1-line each)**: parser AST merge prerequisite; proto renumbering + state-hash bump;
sync→async ISpace adaptation; phlogiston redesign onto accounting/; byte-name rebase ≥200;
intrinsic determinism for similarity; sequencing under WIP overlap.

## Phase-2 design outputs

### FV/Testing/Docs plan — COMPLETE (full text in session; key decisions distilled)

**Two CORRECTIONS to earlier synthesis (evidence-verified)**:
1. NO Parameters/Axioms may remain (check-cost-accounted-rho-proofs.sh:42–50 hard-fails on them);
   house mechanism = Section `Hypothesis` discharged at section close (Translation.v:98–135
   precedent) + every headline theorem prints "Closed under the global context". All 9 source
   Axioms/Parameters eliminated: Checkpoint.v hash pair → Section Hypotheses; apply_operation →
   concrete Fixpoint (makes replay_determinism HONEST); PathMapQuantale.v enumeration+4 axioms →
   concrete sorted enumeration + 4 proved lemmas (doubles as the canonical-ordering modernization).
2. CheckpointReplay.tla is VACUOUS as shipped (root=counter :74–77; Consume removes nothing
   :128–138; ResetToCheckpoint=UNCHANGED :182–187; ReplayDeterminism/CheckpointsImmutable = TRUE
   :262–266/:246–248) → REWRITE around the reducer DashMap architecture with two-copy
   self-composition (ReplayPrefixEqual, SameRootIffSameState), deterministic id counters, and a
   `SpacesInBlockHash` CONSTANT so OQ-1's answer pins a cfg, not a spec rewrite.
Also test-count precision: 505 spaces + 123 vectordb + 25 similarity #[test] attrs (~900 counts
proptest cases + legacy suites).

**Rocq dispositions (19 modules)**: port-as-is {Prelude, VectorStore~, Substitution~}; update
{Match (+SpatialMatch interface section; WildcardMatcher retained as documented degenerate
instance), DataCollection (canonical visit order; 2 Aborts → proved negative results),
ContinuationCollection (+stable-key removal identity — spec-kill of D10), PriorityQueue
(hyperparam domain + FIFO-within-level), HashMapStore (canonical-iteration lemmas),
PathMapStore (lex-ordered prefix enumeration), GenericRSpace (+consume_fires_only_matching
capstone — the theorem D6 falsified), OuterStorage (cyclic wrap = atomic step, model for D8),
Routing, Safety, SpaceFactory (+factory determinism)}; rewrite {Checkpoint core, VectorDB
(id stability under clone, swap-and-pop multiset preservation)}; retarget {Registry/Invariants →
reducer DashMap state (registry object deleted per D12), Phlogiston → slim space-cost model
bridging unified accounting, meter-duplicate laws retired-with-annotation}.
**NEW modules**: Similarity.v (metrics over ordered field + exact-Q instance; cosine bounds via
Cauchy-Schwarz; topk_deterministic = permutation-invariant function of candidate multiset with
(distance, stable-key) total tie-break) + EventCorrelation.v (one-op-one-event, op-list→canonical
state→root composition; honest D3 statement).
**Float story (decided)**: DO NOT model IEEE-754 in Rocq; ordered-field abstraction + Q instance;
bridge = Rust property tests (f64::total_cmp satisfies total-order laws on accepted domain) +
NaN-ingress-rejection theorem+test + Z3/Sage witnesses. Consensus needs determinism/totality,
not accuracy.

**TLA**: formal/tlaplus/reified_rspaces/ w/ MC* wrappers + tlc-run.sh envelope; GenericRSpace.tla
update (CONSTANT Matches; +GenericRSpaceWildcardUnsafe.cfg expected-refutation of
PatternEnforcement = D6 as permanent counterexample); CheckpointReplay rewrite (above; +UuidUnsafe
cfg refuting ReplayPrefixEqual = D1/D2); SpaceCoordination update (SeqExclusive; +SeqGuardToctouUnsafe
cfg = D8; drop gated temporal liveness per house precedent, use-block balance as safety).
Exhaustive bounds stated in cfg headers, <10 min/run. Apalache BASE+STEP IndInv fail-soft leg.
Top-k tie-break totality deliberately NOT TLA (Rocq+Z3/Sage territory; fork-choice made same split).

**Cross-provers (proportionate — mirror fork_choice, NOT the full arsenal)**: Z3
formal/z3/reified_rspaces/{metric_algebra.py, topk_tiebreak_total_order.py}; Sage
formal/sage/reified_rspaces/similarity_metrics.sage (exact-rational cross-check + float-divergence
probe + NaN catalog); loom {loom_seq_guard.rs, loom_space_routing.rs} (loom =0.7.2 already in
rholang Cargo.toml). EXCLUDE Why3/Isabelle/Lean/mcrl2/Tamarin/ProVerif/Verus/Iris/Wolfram (no
property needs them — anything more is gold-plating).

**Gates**: check-reified-rspaces-{proofs,tla-invariants,ALL}.sh pattern-copied from named scripts
(cost-accounted-rho proofs template; fork-choice ALL template w/ 8 legs incl. PlantUML render
check, Rust verification-test leg, citation audits). Add formal/rocq/reified_rspaces/theories to
scripts/audit/check-rocq-line-refs.sh THEORY_DIRS; source cites NAME-FIRST (file+lemma name, no
line numbers in code); TLA line-number citations BANNED (no auditor exists — cite action/invariant
names). CI: ZERO changes (.github/workflows untouched — local-only FV policy).

**Tests**: port map (505+123+25 attrs; registry proptests retarget onto reducer maps; charging
tests retarget onto unified accounting; string-vector cases → native vectors) + NEW N1–N10:
N1 replay-determinism (D1/D2/D3), N2 cross-run determinism proptest (D5), N3 pattern-enforcement
matrix (D6), N4 casper block-replay w/ spaces (casper/tests/spaces_replay/, blocked on OQ-1),
N5 sorter coverage for all new fields (D7), N6 proto round-trip + golden bytes + field-number pin
assertions, N7 loom (D8/D9), N8 metric properties incl. matrix≡fallback equivalence w/ metric
honored (D10), N9 shared determinism harness, N10 doctests (nextest skips them — cargo test --doc).

**Docs** (26 pgmcp guidelines fetched; fork-choice templates): docs/theory/reified-rspaces/
{specification.md (normative + Determinism section + FIPS conformance matrix + spec-hole
resolution table — wording must avoid gate-banned incompletion tokens), verification.md (dossier:
D1–D13→resolution map, invariant→artifact matrix; absorbs old 13-file docs/formal-verification
tree — NOT ported as parallel dir), design.md (replaces drifted extending-spaces.md with real
API), security.md (NEW: id unforgeability, isolation, Seq, resource exhaustion, consensus attack
surface, adversarial floats), glossary.md + docs/Glossary.md anchored terms, performance.md
(re-benchmarked on this host), diagrams/ 01–07 PlantUML w/ shared palette}; language reference:
NEW docs/rholang/20-reified-spaces.md + edits to 02/06/08/10/README; examples: port 17 .rho to
final syntax + honest README. Unimplemented-feature claims (ttl, rank("all"), sim("boost"),
HNSW/SQ8) removed-or-implemented per architect.

**Acceptance criteria**: measurable per workstream (gate exit 0 w/ zero SKIP on dev host; ≥25
headline theorems closed; TLC exhaustive within stated bounds <15 min wall; 3 Unsafe cfgs refute
exactly their named invariant; ported counts recorded; N1–N10 green; diagrams render populated
SVG w/ empty stderr; conformance matrix 100% of normative clauses; workflows diff empty).

**FV open questions**: OQ-1 spaces-in-block-hash (architect; blocks N4 + cfg pin); OQ-2 Phlogiston
retarget depth; OQ-3 priorityqueue(N) honored-or-removed; OQ-4 final syntax (fixtures only).

### Syntax design — COMPLETE (full text in session; distilled)

**RECOMMENDATION: Candidate C generalized — unified "hyper-inputs" `( payload ; params )`**
(scored 41/45 vs A q(...) 32, B postfix-apply 26, E where-clause 21, D v2-chain 17). ONE mechanism
for send AND receive AND constructors:
- Receive: `for (@doc <- embeddings ~ (query; metric="cos", threshold=0.85, k=10))`; bare sugar
  `embeddings ~ query`, `embeddings ~ [.5, .5, .1]`; PQ receive `tasks ~ (; priority=0)`;
  composes w/ where-guards + joins; grammar STRICTLY SIMPLER than v2 (no sim/rank lookahead).
- Send: `x!(msg; k=v)` uniformly across !, !!, !?, for-source-!? (v2 drop FIXED), send_method
  `x!m(args; ttl=1)` desugars through existing rewrite.
- Constructors: `VDBF!?(free Any(); qualifier="temp", dimensions=4, metric="cosine",
  threshold=0.5, embedding_type="float")`.
- `use` block + `new x : space` + `free Nat()` port w/ CONTEXTUAL keywords (reverses v2
  reservation; agent/try precedent; `use!(x)` at proc-start fails = documented limitation pair).
- **Bare floats native**: extend float_literal so `0.85`/`.5`/e-notation lex as f64 (additive —
  previously parse errors; trailing-dot `5.` stays suffix-only protecting `5.toByteArray()`;
  keep f1r3node preprocessor during transition, then retire).
- Vectors = plain lists of native numerics incl. `[true,false,…]` binary for hamming/jaccard.
- **function_call/EFunction DELETED** (converges w/ architect D-1): getSpaceAgent = ordinary
  system process `getSpaceAgent!?(*mySpace)`; theory_call reachable ONLY behind `free`.
- **URN reduction**: structure-only `rho:space:{inner}:{outer}` (~24 factories), qualifier +
  size/cyclic/priorities/dimensions/metric/threshold/embedding_type ALL constructor kwargs;
  parametric-URN mini-parser + [theory=…] + legacy CamelCase aliases die. [DIVERGES from
  architect's keep-96-one-channel-per-URN AND from FIPS spellings (`rho:space:HashMapBagSpace`,
  positional qualifier `HMB!?("default", free Th())`) — RECONCILE IN CRITIQUE + user sign-off.]
- **Lazy result channels KILLED → atomic top-k**: `k=N` binds ONE List datum (desc similarity,
  total tie-break) in ONE COMM; `for (@[best ...rest] <- db ~ (q; k=3))` recovers ergonomics.
  [DIVERGES from architect's keep-lazy-channels-with-deterministic-ids (D2 fix) — RECONCILE:
  syntax agent argues one-COMM is deterministic/replayable/mergeable BY CONSTRUCTION.]
- AST: Hyperparam/HyperparamList survive v2; NEW QuerySpec{payload: ProcList, params:
  Option<HyperparamList>} on Bind::Linear (+SelectPattern); Proc::FreeTheory folds free+
  TheoryCall; PatternModifier/PatternMatchSpec/ModifierParams/FunctionCall DIE. Proto:
  Par.use_blocks=15; Send.hyperparams=8 REUSED by ReceiveBind (query_payload=6, query_params=7,
  has_query=8); New.space_types=6; Expr.e_free=38; NO e_function. [NB architect proposed ONE
  message SimilarityQuery{query, params} at ReceiveBind=6 — field-shape reconcile in critique.]
- Validation 3 layers: P parser (positional-after-named, dup keys — NEW ParsingError variants);
  S sem (free vars resolved in queries/params — v2 gap closed; queries are EVALUATED context not
  pattern; named-only receive params; literal range checks w/ message sketches); R runtime
  (unknown key = ERROR w/ did-you-mean; schema per space type; defaults ONLY from on-chain
  config). Desugar rules 1–5 (bare ~e canonicalizes; embedding default = payload-is-vector).
- Syntax-agent user questions: (1) atomic top-k List vs streamed channel [pairs w/ architect
  divergence]; (2) `metric=` vs `sim=` (recommend metric=); (3) URN reduction sign-off.

### Migration architecture — COMPLETE (full text in session; distilled)

**8 CORRECTIONS (evidence-verified)**: (1) WIP now 526 uncommitted entries; (2) charging idiom =
MeteredMachine/RuntimeBudget — `self.metering.reserve_primitive(cost())?` (reduce.rs:993) +
reserve_canonical/commit_canonical_batch — NOT self.cost.charge; (3) byte base ≥200 IMPOSSIBLE
(200+96−1=295>255; bytes 101–108 now occupied by test_framework_contracts) → **two-byte GPrivate
namespace** `GPrivate{id: vec![0xF3, idx]}` (Decision D-2; collides with nothing; 256 slots);
(4) docs/rholang = 12 numbered chapters (new chapter = 13) [NB: FV agent said 19 — RESOLVE IN
REVIEW]; (5) ISpace also has set_comm_observer, remove_data_at_recorded, take_event_log — all
must be implemented; (6) old branch's ndarray rayon+matrixmultiply-threading = order-
nondeterministic float accumulation → consensus path uses UNTHREADED sequential kernel;
(7) **Expr.EFree/EFunction avoidable (Decision D-1)**: ReceiveBind.similarity_query becomes
first-class `message SimilarityQuery {Par query; repeated ParKeyValue params}` (kills EFunction
blobs + 4-byte-LE framing + encode/decode round-trip), theories become config-map entries
("theory":"Nat") — NO Expr renumbering needed if syntax contract doesn't require general calls;
(8) CA parser = parser/{parsing (3,671 ln),ast_builder,errors,mod}.rs; corpus 98 golden + 69 ts.

**Sequencing**: P0 preconditions (WIP landed+green, surface-syntax contract FROZEN as AST/proto
contract doc, baseline benchmarks archived as pgmcp experiment, epic created) → R1–R4 rholang-rs
(grammar → AST/parsing+new ParsingError validation → sem resolution+wasm+compiler named-bails →
corpus/goldens/fix-broken-v1-test/version 0.2.0) → F1–F10 f1r3node-rust. rholang-rs PR MERGES
FIRST (no consumer on master; safe). Lockstep: UNCOMMITTED workspace [patch] to local parser
during dev (grep-gate that it's never committed) → final pin = new rev + cargo update -p
rholang-parser + remove override (W1 gate pattern).

**F-phases**: F1 models (proto: Par.use_blocks=15, ReceiveBind.similarity_query=6,
Send.hyperparams=8 {repeated positional, repeated ParKeyValue named sorted-by-key},
New.space_types=6; build.rs strip-filters + manual PartialEq/Hash; sorters score+sort ALL new
fields — Par-level USE_BLOCK=318; sorter tests) → F2 rspace++ (RSpaceResult.suffix_key serde-
default; produce(+priority); consume_with_modifiers TYPED (Vec<Option<SimilarityQuery>>,
id_seed: Option<Vec<u8>> for D2) default-delegates) → F3 spaces/ port (async &self ISpace w/
interior tokio::Mutex, release-before-dispatch, NO try_lock; SpatialMatcherContext wired = D6
fix w/ per-collection candidate order [bag/set insertion-index, queue oldest-matching, stack
newest, cell 0/1, pq ascending-priority-FIFO-within, vdb similarity-or-bag]; canonical iteration
per-site [BTreeMap for pathmap stores w/ range() prefix scans; sort-before-hash checkpoints;
f64::total_cmp + Blake2b256(datum) tie-break; NaN REJECTED at ingestion]; checkpoint fidelity
[persist flags+installs+log, COW VectorDB snapshots, next_id checkpointed]; similarity core
[single canonical kernel, matrix groups by metric + carries continuation IDENTITY, matrix≡
fallback property test]; DELETE spaces/registry.rs; DO NOT port charging_agent/phlogiston;
remove placebo vectordb feature; deps: dashmap/im/ndarray-NO-threading/smallvec, uuid GONE)
→ F4 string-vector elimination (EmbeddingVector {F64/I64/BigRat/Bool-bitset}; extract from
homogeneous EList of GDouble/GInt/GBigRat/GBool, int-in-float widens iff |i|≤2^53; thresholds
GDouble|GBigRat in [0,1], 0–100 scaling RETIRED w/ named error; enum dispatch kills TypeId/Any
downcasts; grep-gate: zero split(',') sites) → F5 normalizers (SignedTerm template;
p_use_block_normalizer; dup-key=error; !? source hyperparams populated; query+guard interaction
tests; sorted-output assertions) → F6 system processes (two-byte channels; BodyRefs base 200
i64; create_space determinism via NEW ContractCall::unapply_with_rand → Blake2b512Random
split_byte(TAG) 32-byte ids; config-map theory/params w/ unknown-key=ERROR not warn+skip;
rho:lang:vector minimal op set {add,scale,dot,norm,distance} (D-8); registration in rho_runtime
core (spaces are language-level); exhaustive uniqueness test) → F7 reducer (DashMaps keyed-
lookup-only + collect-and-sort rule; task-local USE_BLOCK_STACK; Seq guard atomic entry-claim +
RAII release; cyclic wrap fetch_update CAS; lazy channels = Blake2b256(id_seed‖k) from consume-
side rand split; hyperparam eval charged via substitute_and_charge, unknown modifier=ERROR;
costs.rs: space_create/use_block_eval/space_produce/space_consume/similarity_query(n,d,k)
PRE-charged/vector_op costs through reserve_primitive BEFORE the op; with_metering_child test)
→ F8 consensus (real Produce/Consume via stable_hash_provider on ALL paths; replay consult
extended; **Decision D-3 (USER SIGN-OFF): recommend (B) block-scoped ephemeral user spaces in
PR-1** — deploys in a block share them, replay re-derives from empty, space unforgeable ids
registered as conflict tags in mergeable_tags so same-space deploys conflict (conservative);
(A) merkleized persistence = tracked follow-up epic (PathMap merkleization.rs candidate);
FIPS default-persistence deviation documented; empty_state_hash bump = single atomic activation,
LAST; new consensus test class: play/replay equality, cross-run identical postStateHash, casper
propose+validate, merge conflict/disjoint tests) → F9 performance (port benches +
benchmark-with-perf.sh; protocol: taskset + performance governor + perf lbr + hyperfine, archived
pgmcp experiments; hotspots: matrix swap_remove arena fix, COW checkpoint win, BTreeMap-vs-real-
PathMap-crate evaluated on benchmarks (D-6 data-driven), verify round-trip/downcast elimination
via profile) → F10 docs/examples/FV/D13/final-pin (chapter 13; conformance matrix; D13: ttl
reject+track, rank("all") IMPLEMENT via lazy channel, priorityqueue(N) drop param, HNSW/SQ8
schema-removed+tracked; 18 examples run as gate; grep-gate no [patch]).

**Determinism table D1–D13 → fix+phase** (in session; D1 = unapply_with_rand + split_byte;
D2 = id_seed threading; D3 = real events + D-3 decision; D4/D5/D10/D11 = F3; D6 = F3.2;
D7 = F1.3; D8/D9 = F7/F3; D12 = F3.6/F3.8/F7.7; D13 = F1/F4/F6/F10).

**User decisions**: D-3 (block-scoped vs merkleized — consensus-critical, needs sign-off);
D-1 drop EFree/EFunction (contingent on syntax contract); D-2 two-byte namespace; D-5 exact
BigRat mode covers ALL SIX metrics via squared-form comparisons (supersedes "restrict exact
mode?" — no sqrt needed); D-8 vector-op scope; confirm unreleased-branch atomic activation.
**Deferred+tracked**: HNSW/ANN+SQ8, external backends, tensor-logic beyond minimal ops, MeTTaIL
real theories, quantale unification, PathMap-crate adoption (data-driven), design-A persistence,
ttl, AsyncSpaceAgent (folded).

**Risk register**: (in session) WIP churn (P0 gate), syntax drift (freeze doc), ts ambiguity
(generation gate), lock-across-await (discipline + loom), float determinism (unthreaded kernels
+ cross-process suites), byte collisions (2-byte + exhaustive test), merge conservatism
(acceptable PR-1, measured F9), test-migration effort (budgeted), fixture bump (single atomic
F8), FV-green-at-merge (FV engaged from P0 w/ frozen contract).

### D. MeTTaTron compiler + bridge exploration — COMPLETE (full report in session; distilled)

**HEADLINE: prior design doc EXISTS** — `MeTTa-Compiler/docs/design/reified-mettatron-rspace.md`
(477 lines, "Reified RSpaces Integration Plan: Expose MeTTaTron as ISpace"): MorkSpaceAgent
implementing ISpace<C,P,A,K> with MettaValue as Channel/Pattern/Data; separate
ContinuationRegistry (MORK stores data, not continuations); PathMap prefix semantics w/
suffix-key wrapping; checkpoints delegate to Environment O(1) CoW fork; Phase 4 explicitly
deprecates the Par serialization bridge. Use as the design baseline.

**Architecture**: mettatron v0.2.0; parse (LL(1) → MettaValue hot path) → compile →
trampoline/CESK eval (interp→bytecode VM→Cranelift JIT tiers; slab alloc + concurrent GC +
work-stealing WorkPool). AtomSpace (atom_space.rs:41–178) = btm: RwLock<PathMap<Multiplicity>>
(ground atoms as MORK byte keys; rules as `(= lhs rhs)` keys) + wide_btm (arity ≥64) +
type/subtype/inferred_type tries + variable_atoms Vec (SEPARATE — trie search is directional) +
shared_mapping (mork-interning) + bloom filters + LSM ACT base w/ tombstones. O(1) CoW fork
(:219–252). MettaValue = Copy tagged word (slab ptr | NaN-boxed Long48/Bool/Empty/Unit);
queries via mork::space::Space::query_multi + ProductZipper (n−1)-factor left-deep trie join;
bidirectional matching fallback (stored-side vars can bind too).

**MM2 DEFINITIVE**: MM2 = MORK's surface language — single directive `(exec <loc> <patterns>
<templates>)`, patterns `(, …)`/`I`, templates `(, …)`/`O` (+/− add/remove + reduction sinks
count/sum/fsum/fmin/fmax/fprod/and/hash), locus-ordered by trie-prefix lex order; normative spec
at ../mm2-specification/. MeTTa query = `!(match &self pat tmpl)` (HE-compatible). BOTH compile
to De Bruijn MORK byte patterns → query_multi over the SAME trie: "a reified space that exposes
the trie exposes both." MeTTaTron implements exec in eval/mork_forms.rs (17/17 ancestor.mm2);
MORK's own Space::metta_calculus scheduler NOT used by MeTTaTron. ACT ops: (save-space!/
load-space!/query-act/attach-act-base!/compact-space!).

**Bridge (to delete)**: NO gRPC — in-process Par construction, feature rholang w/ models =
{path="../f1r3node-rust/models"} (type-identity hazard). Every `.run()`: decode 2 EPathMap
states → MTTS blob parse → symbol table restored VIA TEMP FILE → re-insert every path into fresh
PathMap → rebuild bloom filters → eval → re-encode whole trie + symbol table VIA TEMP FILE again.
EPathMap misused as envelope for opaque GByteArray blobs (trie structure NOT exploited as paths).
Node side: reduce.rs run_method :2511–2630 (EPathMap.run(pm) w/ flat method_call_cost for
arbitrary eval!); system_processes metta_compile :974–1030 (rho:metta:compile, channel byte 200,
PRETTY-PRINTS the Par to recover source text then strips quotes); pretty_printer.rs:28–319 blob
sniffing. Deletion inventory: ALL 2,875-line pathmap_par_integration.rs; most of
rholang_integration.rs scaffolding (compile_safe error-as-sexpr, JSON debug, scatter-gather GC
rooting); run_state stays for CLI. E4 continuation shipping (postcard SerializedContinuationSlice,
resume_shipped) = precedent for shipping COMPUTATIONS not state.

**Atom↔Par map audit (current, broken)**: Atom↔GString OK; String quote-wrapped-in-band (sentinel
hazard); Long/Bool OK; **Float(f)→GString(to_string)→decodes as Atom — LOSSY**; Error/Type tagged
ELists — decode branches UNREACHABLE (dead tag gate: requires quoted tag, encoder writes
unquoted); Quoted/Conjunction/Empty/NotReducible not restored; Space/State/Memo by-reference
(dangling outside process); reverse: no ETuple/EMap/ESet/GUri/GByteArray/unforgeables/sends —
Rholang processes + unforgeable names CANNOT enter atom space; MeTTa grounded fns/spaces can't
leave. Variable identity: Par path keeps names, MORK path renames to `$a%epoch` De Bruijn —
canonical mapping must pick. Multiplicity only via MTTS side-channel — should map onto RSpace
multiset semantics.

**Concurrency/eval model**: shared Arc<AtomSpace>, lock-then-clone readers, make_owned + write
locks; run_state sync; run_state_async batches consecutive `!` on WorkPool; SUB_PROFILE
"MT-Partitioned" (spawn!/await!/snapshot!/partition-space/das-barrier!; spawn! currently lazy).
No global transactions; epochs invalidate memo caches; blooms monotonic. **match with no result
returns EMPTY — not parking**: no reactivity to space changes mid-run — exactly what the ISpace
consume-side ContinuationRegistry adds (the reified space MAKES MeTTa reactive).
External-produce constraints: (a) GC rooting via global_factory for values w/ vars (ground atoms
safe — btm stores bytes only), (b) update variable_fact_count + blooms + epoch exactly as
add_to_space does, (c) completion semantics via continuation registry.

### E. MORK + PathMap API exploration — COMPLETE (full report in session; distilled)

**MORK** (fork feature/arbitrary-space-value-types @16acf5d, nightly, edition 2024):
`Space<V> = {btm: PathMap<V>, sm: SharedMappingHandle, mmaps}` — whole space is ONE PathMap;
atoms = preorder byte encodings (tags: Arity(≤63) 0x00|a, VarRef 0x80|i, NewVar 0xC0,
SymbolSize 0xC0|s; **0x40–0x7F RESERVED = 64 free code points**). Space<V> generic on this fork
(value slot for payloads/multiplicity) BUT MM2 I/O layer hardcoded Space<()> (un-hardcode needed
for V-slot use). Query: query_multi (ProductZipper n-way joins) + coreferential_transition
(trie-guided DFS w/ ByteMask class jumps) + Robinson unify (occurs check, MAX_UNIFY_ITER=1000,
namespaced (u8,u8) vars, BTreeMap bindings). metta_calculus: byte-lex-first exec selection,
atomic rule consume, clone-snapshot query + ZipperHead exclusive writes, batch RemoveSink via
subtract_into. Sinks: +/−/COUNT/SUM/AND/HASH/HEAD/ACT/pure(C-ABI)/wasm. dump_sexpr = one-shot
MeTTa match (pattern → template per binding). NO servers in-tree (upstream `server` branch is
where REST lives) — in-process rlib embedding is the path.

**PathMap** (fork feature/mettatron-fixes v0.3.0): radix-256 COW trie, Arc<dyn TrieNode>,
**Clone = O(1) structural sharing** (THE soft-checkpoint primitive; MORK leans on it);
ZipperHead partitioned concurrency (per-path one-writer-XOR-many-readers, Send zippers,
worked multithreading example); algebraic ops join/meet/subtract/restrict + graft (=path-set
CONCATENATION) + insert_prefix/remove_prefix/join_k_path — **FIPS quantale ops are native**
(crate even has pub(crate) Quantale trait); prefix semantics native (read_zipper_at_path +
to_next_val; rz.path() IS the residual suffix FIPS wants; byte-lex-first = deterministic
descendant choice); morphisms: Catamorphism + into_cata_cached (cached over shared subtries) +
merkleize(); serialization .paths (zlib, values dropped) + ACT mmap format (ascending-order
children) + experimental hash-shared.

**Determinism assessment**: iteration = ascending-byte DFS EVERYWHERE (trailing_zeros chain
verified) → canonical total order on paths; bindings BTreeMaps; NO floats in core; NO iterated
HashMaps. BUT hashing NOT consensus-grade: PathMap hash/merkleize = XOR-shim/xxh3; MORK
Expr::hash = gxhash (ISA-DEPENDENT!) → **implement Blake2b-256 Merkle catamorphism via
into_cata_cached** (right extension point; doubles as checkpoint root). HAZARDS: silent 63-byte
symbol truncation (prefix collision!) → pre-hash/intern long tokens; interning handle order is
thread/insertion-order-dependent → single-threaded deterministic insertion + backup_symbols in
checkpoints; metta_calculus priorities sensitive to symbol bytes; stats = relaxed atomics
(diagnostics only).

**Embedding costs**: nightly (compatible w/ repo); **jemalloc feature = global-allocator
takeover** (trim features or accept); 4 GiB VIRTUAL scratch buffers (Linux overcommit fine);
ACT_PATH hardcoded /dev/shm; **setjmp/longjmp over Rust frames** on query early-exit (callback
must hold no droppables when returning false); interpret() PANICS on malformed exec →
pre-validate; unsafe surface large but behind space.rs. Bag semantics: paths are a SET →
multiplicity in V-slot or deterministic path disambiguator (datum hash / seq no). Par hosting
options: (a) chunked ≤63-byte symbols under Arity spine, (b) interned handle, (c) V-slot payload
w/ structure in path [needs I/O un-hardcoding].
**Wrapper sketch validated**: produce = encode+insert; consume = read_zipper_at_path +
query_multi → fire continuation → remove_val_at (persist K = skip removal); soft ckpt = clone();
hard root = Blake2b catamorphism; replay = byte-lex event order; cold = .paths + backup_symbols.

### F. Old dylon/mettatron branch — COMPLETE (full report in session; distilled)

**Scope**: 12 files, ~560 real lines (rest Cargo.lock); same merge-base 5c0363d9 as reified
branch (3-way merges tractable). NO protobuf/gRPC/docker/tests/docs. In-process statically-linked:
rholang crate depends on `mettatron = { path = "../../MeTTa-Compiler" }` UNPINNED sibling path
dep; MUTUAL coupling (MeTTa-Compiler → f1r3node models path dep); branch NO LONGER COMPILES
against current MeTTa-Compiler (API + wire-shape drift).

**Two touchpoints, both Rholang-initiated request/response** (MeTTa never calls back):
(1) `rho:metta:compile` system process, byte_name(200), BodyRefs::METTA_COMPILE=200, arity 2 —
extracts source by PRETTY-PRINTING the Par + quote-strip hack; (2) `.run` method on EPathMap
exprs in reduce.rs RunMethod — double deserialize + block_in_place/block_on + re-serialize.

**Fatal flaws catalogued**: registered ONLY in create_runtime_with_test_framework (production
create_runtime + create_replay_runtime get NONE → blocks w/ compile can't propose or validate);
METTA_COMPILE not in non_deterministic_ops → DeterministicCall replay against an UNPINNED path
dep; flat 10-phlo method_call_cost for unbounded JIT-parallel eval (DoS); temp-file symbol
backup/restore inside reducer w/ `let _ =` swallowed errors (silent divergence); interned
symbol-table numbering can depend on parallel interleaving → on-chain GByteArray instability;
-C target-cpu=native + gxhash per-host binaries (reproducibility-hostile); pretty-printer
magic-sniffing half dead-code + approximated display; error asymmetry (compile errors = typed
state requiring pattern-probe; run errors = uncatchable ReduceError); ZERO tests/examples/docs;
handler doc example sends on @"rho:metta:compile" string channel which NEVER resolves (would park
forever); space rides chain as opaque magic-tagged blob (unmatchable/unmergeable — THE deficiency
the reified space removes); two pathmap versions in one lockfile.

**Collisions w/ reified branch**: byte/body-ref 200 collides with reified's space-factory range
(200–255 reserved, is_system_channel_id hardcodes 200–210; reified's VECTOR_OPS=150 chosen to
dodge it) → renumber or (better) fold MeTTa into the URN factory scheme as rho:space:metta:*;
same-file conflicts in Cargo.toml/system_processes/reduce/pretty_printer; parser pins diverge
(f1r3node_dependecies vs reified-v2) — one grammar absorbs the other (moot: both re-derived on
the CA-transpiler line now).

**Reusable**: metta_contracts() Definition shape; tagged-list value conventions (fix Float +
sentinel); MettaState {source, environment, output} + run_state_async determinism contract
(sequential rule defs, parallel independent `!` evals, index-preserved outputs); `{||}` ≡ empty
state convention; `!?` return-channel-first arity-2 convention; post-branch
run_state_async_resumable + SerializedContinuationSlice (shipped continuations). Discard: all
sniffing, temp files, quote-strip, block_in_place bridge, byte-blob env encoding, flat cost,
unpinned mutual deps, test-only registration.

### MeTTaTron space design ("MettaSpace") — COMPLETE (full text in session; distilled)

**Architecture (D-M1/D-M2)**: MettaSpaceAgent wraps a full MettaEnvironment (NOT bare PathMap/
Space — all mutation invariants live on env's add_to_space funnel: blooms, epochs, RuleIndex,
type maps) + **canonical twin trie** in mm2-spec §04 NORMATIVE encoding (raw label bytes; twin
repairs interning-order nondeterminism since engine trie bytes depend on thread interning order)
+ persist_set trie + ContinuationRegistry + event log + LiveEnvHandle GC registration. Twin
extensions BigSym 0x40 / BigArity 0x41 (reserved range 0x40–0x7F = provably injective escapes).
INV-TWIN: multiset(canonical) = π(multiset(btm ⊎ wide_btm)); maintained via new
**AtomSpace::set_mutation_observer** hook (MeTTa-Compiler `reified` feature: observer + canonical
encoder + eval_with_fuel + raw entry points; replaces deleted `rholang` feature; DEPENDENCY
INVERTED: f1r3node depends on mettatron; NO MORK/PathMap changes needed). Node owns the space;
on-chain eval BORROWS agent env under lock; CLI/REPL keep own env + run_state.

**Semantics**: single atom plane (space ref = only channel; patterns ARE addresses — pattern
prefix = trie descent = FIPS prefix semantics, better; `new x in use mettaSpace` = typed error
PR-1; joins = conjunction ProductZipper query; dedicated SpaceAgent NOT a ChannelStore).
produce = assert (Par→bytes zero-alloc fast path for ground atoms; typed rejection); persist =
consume-immune not remove-immune (persist_set); **multiplicity = V-slot counter** (MeTTaTron's
own production semantics; bag algebra native; NOT path disambiguator). consume = canonical
De Bruijn query via query_multi over twin; no match → registry parks (MAKES METTA REACTIVE —
verified today match-no-result returns empty forever); arrows: <- linear, <<- peek, <= standing
query. **MM2 mapping**: exec directives = produces (atoms); scheduler = `mettaStep!(*sp, n,
*ret)` metered system op (canonical-lex dequeue = SCH-LEX-ORDER spec-conformant BECAUSE twin;
step budget + per-step fuel = double DoS bound); standing rules = persistent consumes (reactive
exec w/ Rholang consequent, strictly more expressive); patterns=consume-side templates=produce-
side, join-consume + batch-produce fused under lock. `mettaEval!(*sp, src, *ret)` = fuel-metered
sequential-contract eval. **Multi-result: atomic List one-COMM** (canonical asc = spec Dump
order; sides w/ syntax agent vs architect's lazy channels; repeat-consume = iteration idiom;
streaming deferred-tracked). ContinuationRegistry: GC-FREE (Pars + precompiled canonical bytes
only), BTreeMaps + seq order, (head,arity) goal index + wildcard set, semi-naive delta wake
charged to producing deploy, real Consume identities, release-before-dispatch.

**Query syntax: STRUCTURAL (recommendation)** — Rholang patterns ARE MeTTa patterns via
canonical mapping (tuples=s-exprs; Rholang free vars = NewVar/VarRef w/ repeated-var coreference
FREE); zero new grammar (rides R-phase QuerySpec); kwargs: `sp ~ pattern`, `sp ~ (pattern;
results="all")`, `(pattern; k=10)`, `(pattern; tmpl=…)`. String queries (b) REJECTED for
receipts (reopens v2 validation gap; two pattern languages; injection); metta-literal (c)
rejected PR-1 (tooling macro later). mettaEval keeps textual door for PROGRAMS.

**Atom↔Par (fixes every audit defect)**: GDouble native (Float lossiness dead; ryu shortest;
NaN ingress-rejected); escaped string sentinels; head-position-string=symbol rule w/ ~sym/~str
escapes; ETuple=SExpr; EList=("~list",…); Quoted/Conjunction/Error RESTORED (dead gates fixed);
Type erased-documented; stored vars literal `$x` symbols (no $a%epoch leakage; De Bruijn only
query-side); unforgeables REJECTED PR-1 (capability-laundering risk — queryable store would
exfiltrate names); BigInt/BigRat/FixedPoint typed-rejection PR-1 (tracked); EMap/ESet/GByteArray/
sends/bundles rejected at ingestion w/ named errors.

**Determinism/consensus**: root = Blake2b256(tag ‖ cata(canonical) ‖ cata(persist_set) ‖
registry_digest) via into_cata_cached (NOT PathMap merkleize/gxhash — ISA-dependent); T1–T6
theorem-shaped claims (canonical total order; root = f(logical state) invariant under interning/
schedule; query canonicity; INV-TWIN inductive; SCH conformance; mapping round-trip). On-chain
eval IS consensus when default-qualified: interpreter tier only, WorkPool DISABLED (sequential
contract), canonical result ordering; temp = node-local full-parallel. Qualifiers: default =
block-scoped per D-3(B) + conflict tags; temp = node-local; seq deferred. Metta space nominated
as design-(A) merkleized-persistence PILOT (follow-up). Metering table: C_ASSERT/C_BYTE/C_QUERY
(charged inside enumeration, deterministic abort)/C_STEP·fuel/C_EXEC per SCH step — kills flat-
10-phlo hole. Checkpoints: soft = O(1) CoW (twin+persist+fork engine+registry clone; D11-class
closed); hard = serialize_paths_WITH_AUXDATA (multiplicity+persist preserved) — **SYMBOL-TABLE-
FREE by design** (canonical bytes label-complete; no temp files); restore = canonical-order
single-thread re-intern (deterministic normalization) + rebuild derived.

**7 corrections to plan file (evidence)**: Space<()> un-hardcoding NOT needed (query_multi
generic; I/O layer unused); 63-byte truncation only in NON-interning builds (MeTTa-Compiler
interns — real hazard was interning IDs, fixed by twin); .paths _with_auxdata PRESERVES values;
backup_symbols INVERTED (consensus checkpoints symbol-free; CLI-only); prior design doc's
dependency direction reversed + MettaValue-as-C/P/A replaced by Par plane + suffix-key dropped
for this space; metta space is SIBLING of generic pathmap store (does NOT replace it); atomic-
top-k adopted + D-3(B) endorsed.

**Phasing**: M0 (MeTTa-Compiler `reified` feature; parallel to R1–R4; MeTTa-Compiler PR merges
first, rev-pinned like parser) → M1 mapping (dep F1) → M2 agent core (F2/F3, M0, M1) → M3
registry+reactivity → M4 kwargs/factory/mettaEval+mettaStep (F6/F7; ZERO grammar delta) →
M5 consensus (F8, D-3) → M6 FV/TLA/benchmarks/docs/conformance. Deferred-tracked: sink parity,
ACT tier, type ENFORCEMENT (loading ships; enforcement = MeTTaIL successor w/ named hook),
named subspaces, streaming, parallel on-chain eval, design-(A), non-f64 numerics, unforgeables.
FV: MettaCanonical.v/MettaSpace.v/MettaSchedule.v/MettaRoot.v + registry lemmas;
MettaReactivity.tla + observer-absent Unsafe cfg; tests N-M1–N-M9 (incl. conformance vectors,
GC stress loom+ASan, symbol-free-checkpoint grep). Headline bench: old-bridge round-trip vs
direct ops, expected ≥100×.

**MeTTaTron-design user questions**: Q1 ship mettaEval+mettaStep PR-1 (rec: yes); Q2 jemalloc
strip-then-benchmark (rec: strip); Q3 single-next default + explicit results="all" (rec: yes);
Q4 head-position symbol rule w/ escapes (rec: yes); Q5 twin-fallback benchmark pre-approval
(rec: yes, decide on data).

## Critique-round ledger (iterate until no recommendations)

**Pre-round verifications by lead (facts settled before Round 1):**
- docs/rholang chapter count: **19 numbered chapters verified by ls** (01–19; architect's "12"
  correction was WRONG; FV agent right) → new chapter = `20-reified-spaces.md`.
- Known inter-agent divergences to reconcile in Round 1:
  (a) lazy result channels w/ deterministic ids (architect) vs atomic top-k List one-COMM
      (syntax agent) — also interacts w/ MeTTaTron multi-binding queries;
  (b) URN scheme: 96 qualifier-crossed one-channel-per-URN (architect, FIPS-literal) vs ~24
      structure-only + qualifier-as-kwarg (syntax agent) — NOTE FIPS behavioral type declares
      `constructor(qualifier, theory, ...)` with qualifier POSITIONAL FIRST (FIPS line ~121 +
      example `HMB!?("default", free Th_1())`); memory feedback_always_align_to_gregs_papers
      applies — deviation needs strong justification or user sign-off;
  (c) ReceiveBind proto shape: architect's single `SimilarityQuery{query, params}` message at
      field 6 vs syntax agent's flattened query_payload=6/query_params=7/has_query=8;
  (d) rank= dropped (syntax) vs rank("all") implement (architect D13) — reconcile semantics
      (k=N + absent-k=all-within-threshold? or best-1 default per FIPS?). FIPS says a `for`
      consumes ONE vector within distance — bare form = single best is FIPS-aligned.

### ROUND 1 — Critic-2 (coherence/fidelity): 12 recommendations, 1 blocker — ALL ACCEPTED

1. **[BLOCKER→FIXED] Divergence (a) RESOLVED: atomic top-k List for BOTH spaces; lazy channels
   die COMPLETELY.** FIPS line 257 binds the VECTOR not a channel; user's sketch binds doc
   directly; lazy leaks residue + multiplies events + is the reason D2 exists. SWEEP: delete
   id_seed from F2 consume_with_modifiers; delete F7 lazy-channel bullet; F10 rank("all") →
   `results="all"` atomic List; D2 → "moot — mechanism deleted"; streaming deferred-tracked;
   per-candidate charge before materialization in both spaces.
2. **Divergence (b) RESOLVED: qualifier POSITIONAL FIRST per FIPS (spec, not hole; delete
   qualifier= kwarg); URNs structure-only OUTER-FIRST `rho:space:<outer>:<inner>`** (hashmap:bag
   decomposes FIPS HashMapBag; architect's "96 = FIPS-literal" was FALSE — FIPS URNs carry no
   qualifier; SpaceRegistry.create_space(space_type, qualifier, theory, args) FIPS :801–812);
   aliases for EXACTLY the two FIPS spellings (HashMapBagSpace→hashmap:bag, PathMapSpace→
   pathmap:bag; alias precedent = old SeqSpace); constructor = `Factory!?([qualifier,] [theory];
   config-kwargs)` — FIPS example runs verbatim. → conformance rows + user sign-off item.
3. **Divergence (c) RESOLVED: single proto message `QuerySpec { repeated Par payload; repeated
   ParKeyValue params; }` at ReceiveBind.query_spec=6** (renamed from SimilarityQuery — serves
   metta+PQ; presence = message presence, NO has_query; empty payload legal for `(; priority=0)`;
   evolution inside message). Send.params at 8 + `reserved 4;` for the historical hole.
4. **Divergence (d) RESOLVED: `rank=` retired; unified kwarg table** {metric, threshold, k,
   results, tmpl, priority}; k=Int≥1 and results="all" MUTUALLY EXCLUSIVE (validation error);
   presence of either = list mode (ONE List datum: vectordb sim-desc / metta canonical order);
   absence = FIPS single-datum mode; same-key-same-meaning across spaces. rank= retirement →
   user sign-off list.
5. **FIPS modified/unmodified space-view clause (:318–326) was silently dropped** → PR-1 ships
   reference semantics; deviation documented + FIPS-author question + tracked follow-up.
6. **Seq + qualifier-support gating**: F7 names all THREE Seq enforcement points (never-sent =
   produce-time payload scan; not-concurrent + ≤1-process = atomic guard); F6 create_space gains
   per-space-type qualifier-support matrix w/ `QualifierUnsupported` abort; **HashSet store =
   Seq-only per FIPS :269** (recommended); quantale-pathmap seq-requirement stated; FV port of
   seq_cannot_be_sent noted.
7. **8 added conformance rows**: HashSet empty-pattern receives; getSpaceAgent return-value
   equality semantics + `!?` rendering of FIPS call form; rho:assert:assertEqual DECIDE
   (implement trivially or rewrite example); unforgeable-wrapped indices = optional-not-impl;
   nonlinear vector ops NAMED in deferred tensor item; metta gensym error = lawful "may abort";
   cross-space join enforcement point named; theory enforcement = produce-time Validatable path;
   queue arrival-order (FIPS :245) satisfied by D-3(B) conflict tags.
8. **Grammar ambiguity FIXED: parens after `~` are a query-spec ONLY when they contain `;`**;
   otherwise ordinary proc — so `sp ~ ("age", x, y)` = 3-tuple query. Corpus tests for tuple
   query / `(q; k=3)` / `(; priority=0)`. `(; …)` empty-payload form CONFIRMED (send dual).
9. **tmpl= scoping defined**: template term compiled in PATTERN-BINDER scope (the single
   exception to outer-evaluated params; same machinery as body/where-guard); sem rule
   vars(tmpl) ⊆ pattern free vars ∪ outer scope; normalizer test. KEEP (MeTTa parity).
10. **Metric vocabulary canonical**: full words {"cosine","dot","euclidean","manhattan",
    "hamming","jaccard"} in constructor AND receive; short codes rejected w/ did-you-mean.
11. **Naming glossary pinned**: "params" everywhere (hyperparameters retired to one historical
    alias sentence; AST params, proto Send.params/QuerySpec.params); QuerySpec = AST+proto+prose;
    `MettaSpaceAgent`/"the MeTTa space"/URN **`rho:space:metta`** (single, NOT metta:*);
    system ops **`rho:metta:eval`**, **`rho:metta:step`**; modules spaces/metta/{agent,par_atom,
    canonical,continuations,schedule}.rs; single SpaceError enum w/ named variants incl.
    QueryNotSupported (runtime — spaces dynamic), UnknownParam{suggestion}, QualifierUnsupported,
    SeqSent, SeqConcurrent, CellOccupied, OutOfNames, NaNRejected.
12. **User sign-off list EXPANDED**: (i) headline — candidate C `(payload; params)` replaced
    user's own `q(...)` sketch (scored 41 v 32; ASK, don't surprise); (ii) rank= retirement;
    (iii) metta-space BigInt/BigRat/FixedPoint typed-rejection PR-1 narrows "all sensible types"
    (tracked follow-up; ASK); (iv) state once: MeTTa-Compiler gets its own (third) PR = M0.

### ROUND 1 — Critic-1 (consensus/security): 10 recommendations, 3 blockers — ALL ACCEPTED

1. **[BLOCKER→FIXED] mergeable_tags misuse DELETED from F8.** mergeable_tags EXEMPTS channels
   from conflict detection (merging_logic.rs:288–330 subtracts mergeable ops from conflict
   sets) — the OPPOSITE of forcing conflicts; and under D-3(B) same-space cross-branch contact
   is unconstructible (handles can't cross DAG branches). REPLACEMENT: space ops emit real
   Produce/Consume events with channel identity = space id → standard channel-intersection
   conflict detection covers any future contact; F8 note: space-conflict semantics vacuous under
   D-3(B), revisit (NOT via mergeable_tags) for design-(A). Merge test: space events in
   EventLogIndex; distinct-space sibling deploys disjoint.
2. **[BLOCKER→FIXED] On-chain eval effect ALLOWLIST.** MeTTaTron grounded registry dispatches
   thread_rng (random.rs:533) + file-* I/O unconditionally — M0 eval_with_fuel takes an effect
   policy; consensus profile = deterministic allowlist (arith/cmp/logic/string + agent-routed
   space ops); typed rejection of random/fileio/json-I/O/ACT ops/spawn!/await!/anything mutating
   env outside the observer funnel (INV-TWIN premise). N-M test: op present → deterministic
   named error; FV side-condition: on-chain op set effect-free.
3. **[BLOCKER→FIXED] Fuel determinism.** thread-local SubgoalTable memo (tabling.rs:584) +
   global tier cache make step counts history/scheduler-dependent → divergent abort points.
   M0: on-chain eval runs FRESH per-evaluation tabling/hint state + tier pinned via existing
   TierSelection::Treewalker (tier_forced.rs — makes "interpreter tier only" concretely
   enforceable); fuel counts CESK transitions invariant under acceleration state. Warm-vs-cold +
   cross-process determinism test w/ tight budget asserting identical abort step.
4. **[MAJOR] Play-side lifecycle protocol added to F8**: reset/rig_and_reset clears ALL reified
   in-memory state (space_store, channel maps, metta envs, registry, seq guards, counters) —
   play AND replay start each block empty; per-deploy soft checkpoint (runtime.rs:1335/:1432
   pair) aggregates whole space-map snapshot (failing deploy: created spaces removed, mutated
   restored); exploratory deploys share NO reified state. Test: two consecutive blocks + failed
   deploy → replay equality.
5. **[MAJOR] Self-join multiset validity**: for every distinct atom a, conjuncts-bound-to-a ≤
   multiplicity(a) — post-filter in query, registry residual joins, mettaStep exec matching;
   multiplicity INCREMENT of existing atom IS a wake-delta; predicate added to T-claims
   (premise of no-lost-wakeup/no-double-fire) + conformance vectors.
6. **[MAJOR] k-result COMM replay spec**: one COMM records consume identity + ALL k produce
   identities (delivered order); replay-consult matches by consume + verifies produce multiset;
   leftover accounting in check_replay_data analogue; peek-with-k semantics defined; N1-class
   test with k>1 + registry wake parked by earlier deploy of the block.
7. **[MAJOR] ~sym/~str sentinel hazard closed**: symbol-ness decided ONLY by structural position
   (head rule); encoder ALWAYS escapes plain strings colliding with the marker namespace
   (leading `~` doubled) → Par→Atom injective over FULL Par domain, no value can alter its own
   type; non-head symbols via dedicated structural tuple shape, not string prefix; T6 restated
   over full domain; adversarial fuzz corpus in N-M tests.
8. **[MAJOR] C_WAKE(delta, parked) added to metering table** (charged inside wake enumeration,
   deterministic abort like C_QUERY); security.md metta section states the deliberate model:
   handle = the ONLY capability = whole-space read+write+rule-assert+standing-query rights; a
   shared space is a single mutual-trust channel; wrap-the-handle restricting-contract pattern
   documented for adversarial settings.
9. **[MAJOR] Dangling-handle semantics**: deterministic typed error ("space not found: <id>",
   fixed charge, identical play/replay) for produce/consume/use/getSpaceAgent on dead space id;
   tests: handle stored in default space used next block; parked continuation firing into dead
   space; lifetime rule in language reference.
10. **[MINOR] Temp-space quiescence invariant**: "no detached space work survives the deploy's
    evaluation" (WorkPool jobs joined before eval returns); temp-space revert-on-failure via
    aggregate snapshot OR documented non-transactional; loom test beside GC-stress suite.

**Key confirmations (verified sound, no change)**: unapply_with_rand per-branch/per-produce
randomness replay-stable for P|Q factory calls; cross-deploy COMM pairing handled by rig
new_stuff filter (replay_rspace.rs:395–404) — registry wakes fit; deploys sequential per block
(runtime.rs:1075); canonical sorter PRESERVES EList element order (top-k List order survives);
tier_forced per-eval pinning exists; 4 GiB reserve per-query_multi serialized under agent lock.
### ROUND 1 — Critic-3 (implementability/epic): 9 recommendations, 3 blockers — ALL ACCEPTED

1. **[BLOCKER→FIXED] QuerySpec-vs-metta-pattern composition ADJUDICATED: metta patterns occupy
   the bind LHS** (ordinary Rholang pattern position; binder rules + `_` wildcards apply; S1
   evaluated-context rule untouched — it governs the `~` payload, which metta does not use for
   patterns). Space-dependent validation impossible (channel's space unknowable at
   normalization); "repeated-var coreference FREE" WITHDRAWN (Rholang enforces pattern linearity
   — errors.rs:273 "Free variable used twice"); PR-1 coreference = where-guards post-match;
   trie-level VarRef coreference = tracked follow-up w/ explicit linearity-relaxation decision.
   **Consequences**: metta receives PR-1 = plain receives on the space channel (NO `~` needed —
   every receive on a MeTTa space IS a query; BindPattern{patterns, freeCount} → canonical
   De Bruijn; FreeVar indices ↦ NewVar 1:1 under linearity); `results=`/`k=`/`tmpl=` DROPPED
   from metta receipts PR-1 (no pattern home when LHS binds a List) — multi-result/templating
   via mettaEval textual `!(match &self pat tmpl)`; structural all-results = tracked follow-up.
   NEW M1/M2 acceptance criterion: **round-trip soundness — canonical-map(BindPattern)
   trie-matches atom bytes IFF SpatialMatcher matches pattern against decoded atom Par**.
   M4 "zero grammar delta" now genuinely true.
   → CROSS-CRITIC RECONCILIATION: amends Critic-2 #4 kwarg table (k/results = vectordb receipts
   only PR-1; tmpl deferred — overrides Critic-2 #9 keep-tmpl) and Critic-2 #8 parens-`;` rule
   (stands for vectordb `~`; moot for metta receives).
2. **[BLOCKER→FIXED] Per-instance datum metadata replaces bare multiplicity**: V-slot value =
   canonical multiset of instance records {randomState, source: Produce, persist,
   cost_authority, cost_stack} (Datum{a,persist,source} internal.rs:16–20 +
   ListParWithRandom{pars,randomState,cost_authority,cost_stack} incl. NEW cost-WIP fields);
   deterministic pop rule (ascending canonical serialization of record); included in state root;
   **randomState rule for ALL synthesized multi-datum Lists (vectordb top-k AND metta) =
   Blake2b512Random::merge of constituent randoms in delivery order** — added to M2/F3 bodies.
3. **[BLOCKER→FIXED] mettatron dependency surgery (M0 sub-items)**: convert MORK×4 + pathmap
   path deps → git+rev pins on the F1R3FLY forks; DELETE `rholang` feature + models path dep
   (breaks mutual coupling; keeps PR order acyclic); f1r3node-rust workspace root gains its OWN
   [patch] entries unifying all pathmap instances (MORK git, liblevenshtein crates-io) onto the
   pinned fork ([patch] only binds at CONSUMING WORKSPACE ROOT — "rev-pinned like parser" was
   impossible as written); drop pathmap `jemalloc` feature (Q2); delete vestigial gxhash dep.
   CONFIRMED-DEFUSED: node .cargo/config.toml already carries +aes/+sse2/target-cpu=native
   (transitive gxhash builds fine); nightly-2026-02-09 expected edition-2024-compatible but M0
   acceptance gate = "cargo check mettatron under the node's pinned toolchain + rustflags".
4. **[MAJOR] Proto precision**: `ParKeyValue` DOES NOT EXIST — reuse `KeyValuePair{Par key; Par
   value}` (RhoTypes.proto:405) w/ key = GString Par, canonical order = par-sorter score order
   (STATED), or define `{string key; Par value}` — F1 task body decides explicitly. All other
   field numbers verified available.
5. **[MAJOR] state-hash bump procedure spelled**: TWO fixtures — runtime_manager.rs:1807
   constant AND runtime_spec.rs:18/:42 second hardcoded hash; procedure: run both tests, take
   computed hashes from assertion failures, update BOTH, single atomic commit, LAST in PR.
6. **[MAJOR] Test/gate hosts assigned**: [patch] grep gate → leg in check-reified-rspaces-ALL.sh
   + acceptance criteria on normalizer task (introduced) + final-pin task (gone);
   cross-process determinism harness = NEW dedicated test bin spawning a fresh child process on
   an identical deploy set comparing postStateHash (no such harness exists; same-process reruns
   CANNOT catch RandomState/ASLR ordering bugs); ASan leg = scripts/check-metta-space-asan.sh
   (nightly -Zsanitizer=address, jemalloc OFF, gxhash off-or-suppressed, local-only); N-M9
   conformance names T03-byte-encoding/T04-spaces/T05-rewrite/T06-scheduler + profile (VERIFIED
   mm2-specification/conformance/ exists with these).
7. **[MINOR] Phase edges**: add M3→F7 (wake charging needs metering idiom); metta `new`-in-`use`
   typed error pinned to reduce.rs new-allocation path under USE_BLOCK_STACK (F7) w/ named
   variant + test, referenced from M4.
8. **[MINOR] Baseline-drift guards**: epic bodies cite SYMBOL NAMES not line numbers
   (empty_state_hash_fixed, reserve_primitive); P0 re-verify checklist enumerated (parser rev,
   byte occupancy re-derivation, WIP entry count, next chapter number = max+1, MeTTa-Compiler
   base rev pinned at epic creation + observer insertion points re-verified).
9. **[MINOR] Decision recording**: `rrs-decision-record` task holds ALL user sign-offs
   (frozen into epic root body after resolution; every design-contingent task depends_on it);
   bridge deletion SPLIT per repo (mc-bridge-deletion / fn-bridge-deletion); FV = one
   gate-owning task + per-task proof obligations named in each feature task's body (ca2p style).

**Draft epic skeleton ACCEPTED as the decomposition baseline: 25 tasks** (rrs-p0-baseline-verify,
rrs-decision-record, rrs-syntax-contract-freeze, rrs-parser-{grammar,ast-validation,
sem-corpus-publish}, rrs-models-proto, rrs-rspace-interface, rrs-spaces-{port-core,determinism},
rrs-normalizers, rrs-system-processes, rrs-reducer, rrs-consensus, mc-m0-reified-feature,
mc-bridge-deletion, rrs-metta-{mapping,agent,reactivity,surface,consensus}, fn-bridge-deletion,
rrs-fv-rocq-tla, rrs-tests-verification, rrs-perf-docs-final) with depends_on edges as drafted
(amendment from #1: task 8 rrs-rspace-interface loses id_seed per Critic-2 #1 sweep).

### ROUND 1 SUMMARY: 31 findings (7 blockers) — all adjudicated and folded in.
### ROUND 2 — Critic-5 (ledger/epic): 8 recommendations, 1 blocker — ALL ACCEPTED

1. **[BLOCKER→ADJUDICATED] Theory representation**: `free Theory()` SURVIVES as contextual-
   keyword expression — keep `Proc::FreeTheory` + **`Expr.e_free = 38`**; **EFunction stays
   deleted** (both agents agreed); theory = optional SECOND POSITIONAL constructor argument
   (NOT config-map entry); F1 proto list amended to include e_free=38; decision record entry:
   "D-1 = EFunction dropped; EFree kept at 38" (internal adjudication, NOT a user question).
   [Awaiting Critic-4 corroboration on wire-level detail; if it conflicts, reconcile then.]
2. **FV D2-scrub + k-COMM mechanization**: TLA UuidUnsafe re-scoped D1(+D3); "deterministic id
   counters" and N1 references likewise; EventCorrelation.v + TLA event alphabet amended for
   multi-produce COMM shape + randomState-merge rule; k>1 + registry-wake case in N1 — all in
   rrs-fv-rocq-tla body.
3. **Conformance-row fix**: queue arrival-order rationale rewritten — D-3(B) block-scoping +
   sequential per-block deploys + standard event-intersection conflict detection (conflict-tags
   mechanism is dead; do not cite it).
4. **fn-bridge-deletion RE-SCOPED**: f1r3node-rust has ZERO mettatron/rho:metta tokens (old
   bridge never merged here; byte 200 FREE; `.run` on EPathmapBody = no-op stub) → task =
   (i) do-not-port declaration + grep gate zero legacy tokens; (ii) `.run` stub fate (remove or
   typed error pointing at rho:metta:eval); (iii) real deletion mass lives in mc-bridge-deletion.
5. **25-task table MATERIALIZED into ledger** (repo | scope | depends_on — Critic-5's table is
   authoritative; supersedes the in-session sketch): scope splits pinned (port-core = F3
   structural D4/D6/D11/D12; determinism = D5/D10 + F4 + top-k synthesis + randomState merge);
   rrs-consensus owns fixture bump as PR's FINAL commit.
6. **Examples count = 18** verified (FV/docs "17" corrected); P0 checklist entry.
7. **Micro-decisions pinned in rrs-decision-record (NOT user questions)**: (a) rho:assert:
   assertEqual = implement trivially; (b) qualifier-support matrix cells: all stores
   default+temp; seq w/ F7 enforcement; HashSet seq-only; metta default+temp; (c) method name
   `consume_with_query_specs` (stale consume_with_modifiers renamed); (d) Send field 7 stays
   unallocated — stated in F1 body; (e) preprocess_floats retirement → rrs-perf-docs-final
   w/ grep gate.
8. **Deferred register**: consolidated in epic root body; rrs-decision-record acceptance
   criterion "every deferred item has a pgmcp follow-up work-item id" (~17 items enumerated).

**Deliverables recorded (full text in session)**: 29-entry override map (originals → superseding
authority; UNMARKED entries now marked); 31-finding → 25-task traceability (complete, no
orphans); FINAL USER-DECISION LIST = 12 questions w/ recommendations (1 syntax headline,
2 rank= retirement, 3 URN/constructor FIPS fidelity, 4 D-3(B), 5 state-hash activation,
6 FIPS space-view deviation, 7 metta atom-domain narrowing incl. unforgeables-rejection,
8 metta receive semantics + ship eval/step, 9 symbol-rule, 10 twin fallback pre-approval,
11 third PR confirm, 12 deferred-register block-approve) + MUST-NOT-ASK list (10 items decided
in-ledger); 5 drafted task bodies (models-proto, rspace-interface, metta-mapping,
system-processes, consensus) — writable from resolved ledger except the now-adjudicated gaps.
Scope-integrity: NO dropped user threads.

### ROUND 2 — Critic-4 (technical): 9 recommendations, 1 blocker — ALL ACCEPTED

1. **[BLOCKER→FIXED] Evaluator-originated instance records**: before eval_with_fuel the agent
   installs an eval-mutation context {rand = invoking system-op's rand.split_byte(TAG),
   counter: u64}; each observer-created record gets randomState = ctx.rand.split_short(counter++)
   (deterministic under sequential contract); source = Produce::create(space-channel,
   ListParWithRandom{[atom], randomState,…}, persist) — same constructor as Rholang-path records;
   cost attribution = invoking deploy; SAME rule for factory-constructor-seeded atoms (theory
   loading). N-M test: standing query woken by mettaStep-created atom → play/replay equality.
   (Root/T2 now defined over eval-touched states; k-COMM spec + wake-deltas completed.)
2. **[MAJOR] PR-1 metta-queryable pattern subset DEFINED**: one pattern per bind; no remainders;
   single-expression Par of ETuple/ground literals/FreeVar/wildcard; everything else (logical
   connectives `/\`, type connectives, VarRef `=x`, par-composition, list remainders, bind-level
   remainders, multi-pattern binds) → deterministic typed error `UnsupportedMettaPattern{reason}`
   (never silent mismatch); round-trip IFF criterion SCOPED to the subset + completeness
   criterion ("outside subset always errors"); QuerySpec-bearing receives on metta space also
   reject PR-1; binding extraction maps trie slots → FreeVar levels via encoder's recorded walk;
   `/\`-as-intersection = tracked follow-up.
3. **[MAJOR] Where-guard semantics on the agent path**: agent evaluates the guard (rho-pure-eval,
   bound pars in bind order) per candidate tuple in canonical enumeration order, first-pass-wins;
   wake path RE-CHECKS the guard — fail → stays parked, no decrement; guard evals charged inside
   enumeration (C_GUARD or folded into C_QUERY) w/ deterministic abort; conformance test both
   paths (this is the PR-1 coreference mechanism — now specified).
4. **[MAJOR] Theory wire form ADJUDICATED (corroborates Critic-5 #1)**: keep `Expr.e_free = 38`
   (EFree{name, repeated Par args}); D-1 narrowed to "drop EFunction only"; "theories become
   config-map entries" sentence DELETED; disambiguation rule: GString→qualifier, EFree→theory,
   else typed error. F1/sorter/R2 inherit.
5. **[MAJOR] randomState merge rule amended** (Blake2b512Random::merge asserts ≥2 —
   crypto/blake2b512_random.rs:177): n=1 → constituent verbatim; n≥2 → merge in delivery order;
   n=0 → NO datum synthesized, list-mode consume PARKS (stated explicitly). k=1/one-candidate
   cases added to N8 + N-M tests.
6. **[MINOR] Effect-policy seam corrected**: dispatch is static execute_grounded_op (6 call
   sites), NOT the per-env registry — policy rides MettaState/trampoline ctx; policy-aware
   wrapper at all 6 sites + grep-gate (no direct calls outside wrapper); thread-local cache
   clears (THREAD_TABLE, EVAL_MEMO, MATCH_RESULT_CACHE, hint gens) at eval ENTRY on the eval's
   own thread (inside any spawn_blocking closure).
7. **[MINOR] INV-TWIN restated**: ∀a: |records_twin(a)| = multiplicity_engine(a) (π-projected);
   instance records AUTHORITATIVE for persistence; persist_set demoted to derived index with
   stated consistency invariant OR dropped from the root formula (records already in root) —
   pick in M2 body (recommend: drop from root, keep as derived index for O(1) persist lookups).
8. **[MINOR] cost_stack parity**: cost_stack-bearing produces REJECTED at metta ingestion with
   named error (parity with Matcher::get exclusion match.rs:23–25); conformance row.
9. **[MINOR] [patch] gate re-scoped**: forbid rholang-parser/path-based [patch] entries
   specifically; committed patch table must EQUAL the approved pin allowlist (pathmap/MORK
   unification entries from Critic-3 #3 are permanent and allowed).

**Confirmations**: full BindPattern reaches spaces (reduce.rs:373–441); channel-routing
discrimination + cross-space-join enforcement sound under LHS-as-pattern; <<-/<= flow correctly;
proto arithmetic all verified; randomState deploy-deterministic → records-in-root preserves
replay-from-empty; Treewalker pins T0 bypassing tier promotion; thread-local clears cheap;
**exploratory deploys already run on their OWN spawned runtime** (runtime_manager.rs:1275–1287)
— C1#4 isolation holds structurally (one clarifying clause: reified maps/metta envs =
per-runtime-instance fields, never static); exec (O ±fact) flows through env funnels (observer
placement right; deterministic pop needs no extra callback info); PQ `(; priority=0)` survives
(dropped forms were metta-only); QuerySpec presence semantics + parens-`;` rule coherent.

### ROUND 2 SUMMARY: 17 findings (2 blockers, one jointly adjudicated) — all folded in.
### ROUND 3 — Critic-7 (completeness): 8 findings (3 major, 5 minor) — ALL ACCEPTED
**Verdict: "design core is complete and converged"; findings are peripheral work products +
epic mechanics, not design defects.**

1. **[MAJOR] Supply-chain work products**: deny.toml [sources].allow-git currently rholang-rs
   ONLY → extend with 3 URLs (MORK/PathMap/mettatron pins); MORK workspace has NO license field
   → [[licenses.clarify]] entries (house precedent deny.toml:89–103) or upstream license fields
   in the fork; **"F1R3FLY forks" DO NOT EXIST as org repos** (only github.com/dylon personal
   forks) → P0 checklist: fork org placement + push + visibility decision + CI/Docker
   fetchability of all pins (rholang-rs itself is private — credential reach question);
   `cargo deny check` green = mc-m0 acceptance criterion on the f1r3node-rust PR.
2. **[MAJOR] MeTTa-Compiler CI would be RED on M0 PR**: integration-tests.yml:30 clones dead
   `dylon/mettatron` f1r3node branch + runs the bridge test M0 deletes; nightly-tests.yml same.
   → mc-bridge-deletion body: rewrite both workflows (drop f1r3node clone + rholang_integration;
   add `--features reified` build+test leg under the node's pinned nightly; keep mtt/petta
   conformance); publish/release workflows unaffected (state no-change). **PR-B (MeTTa-Compiler)
   = mc-m0 + mc-bridge-deletion + CI rewrite in ONE PR** (now stated explicitly).
3. **[MAJOR] Epic mechanics materialized into ledger** (were session-only): epic-root outline
   ACCEPTED (root kind epic, public_id `reified-rspaces-mettatron-migration`, priority 100,
   **project f1r3node-rust on root AND all 25 tasks — NEVER null** (campaign-root filing-defect
   lesson); repo carried by slug prefix; relates_to → cost-accounted-rho-two-paper epic (P0 gate
   = that scope) + cost-accounting-transpiler root; priorities 95 gate-chain / 85 implementation
   / 75 FV+tests+docs; root body = scope + PR map [PR-B first, PR-A second, PR-C last w/
   state-hash bump final commit] + plan-file path + override map + decision list + 25-task table
   + deferred register + P0 checklist [now ALSO: deny.toml state, fork hosting, MC CI inventory,
   js-bindings status, npm version] + conventions [conventional commits (git-cliff), symbol-name
   citations, [patch] allowlist, rustdoc+doctests on new public APIs]).
4. **[MINOR] Editor/LSP consumers**: deferred row rrs-fu-editor-grammar-refresh (npm major bump;
   LSP re-pin off f1r3node_dependecies; nvim/vscode/emacs regeneration); rrs-parser-grammar
   preserves RHOLANG_NAMED_COMMENTS generate hook; rrs-parser-sem-corpus-publish: js-bindings
   green + npm version decision (CST reshape = breaking → 2.0.0 or defer-publish with reason).
5. **[MINOR] Observability decision**: PR-1 adds `f1r3fly.rspace.spaces` metrics source
   (produce/consume/query timers, spaces-count gauge, metta eval-fuel counter) mirroring
   metrics_constants.rs pattern — RECOMMENDED (cheap, house-consistent) — else defer row;
   one sentence in performance.md ops section. [Lead adjudication: ADD in PR-1 — decide-and-
   state placed in rrs-spaces-port-core.]
6. **[MINOR] Operator docs**: performance.md "operational requirements" subsection (vm.overcommit
   for 4 GiB virtual reserves, memory sizing, /dev/shm N/A while ACT deferred) cross-linked from
   docs/node/README.md; 0xF3 namespace documented in design.md + FixedChannels comment block;
   explicit sentence: integration pipeline + docker compose unchanged, space smoke deploy
   deferred to editor/ops follow-up.
7. **[MINOR] Provenance/licensing**: canonical.rs + vendored conformance vectors carry
   provenance headers (source repo + rev); P0 line: attach license to mm2-specification (same
   owner); conventional-commit discipline note in epic root (CHANGELOG auto via git-cliff).
8. **[MINOR] Deferred register WELL-FORMED: 21 rows drafted (scope | why | trigger)** —
   rrs-fu-{hnsw-ann, external-backends, tensor-ops, mettail-theories, quantale-unification,
   pathmap-generic-store, design-a-persistence, send-ttl, streaming-results, mm2-sink-parity,
   act-tier, named-subspaces, parallel-onchain-eval, metta-numerics, unforgeables-in-atoms,
   varref-coreference, metta-multiresult, conjunction-intersection, fips-space-view, seq-metta,
   editor-grammar-refresh} — full table in session, embed in epic root.

**Verified clean**: genesis untouched (standard_deploys = classic blessed contracts only; one
attestation line in rrs-system-processes); toolchain compatible; no Justfile/Makefile gap;
CHANGELOG auto; ALL session constraints delivered; cross-ISA float risk anchored by sequential
scalar kernel + N8 bitwise-equality wording.

### ROUND 3 — Critic-6 (technical): 6 findings (1 blocker, 1 major, 4 minor) — ALL ACCEPTED

1. **[BLOCKER→FIXED] Parallelism keys on EXECUTION CONTEXT, never qualifier**: every eval
   reached during block evaluation (play, replay, ANY qualifier incl. temp) runs the full
   sequential consensus contract (Treewalker tier, WorkPool disabled, fresh caches, canonical
   ordering, deterministic fuel); "full-parallel" lawful ONLY in CLI/REPL/off-node embedding.
   Design sentence restated; C1#10 amended (on-chain there are no WorkPool jobs to join;
   quiescence clause applies to off-chain embedding); NEW N-M test: play/replay equality of
   eval on a TEMP metta space with tight budget. Homes: mc-m0 (eval entry contract),
   rrs-metta-agent, rrs-metta-consensus.
2. **[MAJOR] Cache clear-list replaced by CLASSIFICATION-TABLE criterion**: M0 acceptance =
   exhaustive table of EVERY thread_local! + process-global cache under src/backend/
   {eval,environment,models,grounded} (~20 sites incl. tabling's THREE groups, process-GLOBAL
   shared_memo DashMap + CURRENT_DERIVATION_ID, BINDING_CAPTURE_STACK [leaks on fuel abort —
   entry-clearing also heals this], STATIC_ENV, FRESH_NAME_CACHE, dispatch_hints ×4), each
   classified cleared-at-eval-entry / proven-result-AND-fuel-neutral / removed; table pinned
   against `rg -c "thread_local!"` count so new statics fail the gate.
3. **[MINOR] split_short is i16**: eval-mutation context uses the evaluation_random precedent —
   deterministic typed abort when one eval exceeds i16::MAX observer records (no wrapping cast;
   0/32768 randomState collision = capability-forgery edge closed). Home: rrs-metta-agent.
4. **[MINOR] Ingestible ground domain enumerated POSITIVELY**: GBool/GInt/GDouble/GString-under-
   symbol-rule + Nil/empty-Par (pinned); everything else — incl. GUri — rejected via exhaustive
   non-wildcard ExprInstance match (future variants FAIL CLOSED); pattern-subset "ground
   literals" = same domain, others → UnsupportedMettaPattern. (ETuple-remainder unconstructible;
   EList outside subset — verified.)
5. **[MINOR] execute_grounded_op = 5 non-test sites** (not 6); count re-verified at M0 pin;
   grep-gate is the source of truth, not the count.
6. **[MINOR] Round-2/3 traceability lines added**: C5#1–8, C4#1–9, C6#1–6, C7#1–8 map onto the
   25 tasks (C4#1 → mc-m0 + rrs-metta-agent; C4#3 → rrs-metta-agent + rrs-metta-reactivity;
   ALL fit existing tasks — no new task needed).

**Verifications PASSED**: split_byte/split_short/merge signatures; rho-pure-eval sync fn (no
lock-across-await for per-candidate guards); existing where-guard machinery mirrored faithfully;
BindPattern plumbing reaches spaces; all proto arithmetic re-verified; Produce::create exists;
exec funnels + deterministic order; **MM2 sinks ALL consensus-safe** (hash = FNV-1a over sorted
group, ISA-independent; f64 reductions in deterministic enumeration order — sink-parity deferral
safe); tier_forced + thread_rng confirmed; FIPS citations verified; zero mettatron tokens in
f1r3node-rust; **re-entrancy impossible by construction** (env borrowed under held agent lock,
wake-deltas collected under lock, release-before-dispatch, no Rholang-callback op in allowlist);
**12-question user list correct and minimal** (bare-float rides Q1; D-8 rides Q12).

### ROUND 3 SUMMARY: 14 findings — all folded. Trajectory 31 → 17 → 6 (+8 peripheral).
### ROUND 4 (final convergence) — **NO FURTHER RECOMMENDATIONS. CONVERGED.**
Verified: all six C6 + eight C7 adjudications internally consistent and homed; cross-round
composition sweep clean (atomic-List×n=0-parks×k-COMM×registry-park; record-persist×root;
[patch]-allowlist×dev-override; PR-B→PR-A→PR-C valid linearization; execution-context pin×Q1);
workspace spot-checks re-confirmed (deny.toml allow-git, no committed [patch], Blake2b512Random
API); only decision-shaped residues are the two explicitly-delegated decide-and-state items
(KeyValuePair-vs-new-message in F1 body; persist_set drop-vs-derived in M2 body) — both fully
specified either way; 12-question user list + 10 must-not-ask FROZEN.
**Trajectory: 31 → 17 → 14 → 0. 62 findings adjudicated. Ready for user sign-off + epic assembly.**
## USER DECISION RECORD (2026-08-15 sign-off — FROZEN)

1. **Query syntax = unified `( payload ; params )` (candidate C)** — APPROVED as recommended.
2. **rank= retired** (k=N XOR results="all"; absence = FIPS single-datum) — APPROVED.
3. **URNs/constructors = structure-only outer-first + FIPS positional qualifier + 2 aliases**
   — APPROVED as recommended.
4. **D-3 = (A) MERKLEIZED PERSISTENCE IN PR-1** — USER OVERRIDE of recommendation (B).
   **RE-CONFIRMED 2026-08-15 after full trade-off review** (production benefit: durable
   validator-verified spaces + replay verification + free LFS sync + merge correctness;
   costs: +25–35% effort in rspace++/casper, write-amplification bound, merge rules, EventCoverage
   discipline; portability: Blake2b256+canonical bytes = ISA-independent by design [gxhash/xxh3
   explicitly rejected]; protocol-normative like the existing tuplespace trie — no NEW lock-in).
   Design-(A) machinery moves INTO PR-1 scope: space state in the tuplespace root, cross-block
   usage, live merge semantics, rehydratable in-memory indices. Deferred row
   rrs-fu-design-a-persistence REMOVED (now in scope). → DELTA DESIGN PASS REQUIRED (below).
5. **MeTTa receives = LHS patterns + rho:metta:eval + rho:metta:step in PR-1** — APPROVED.
6. **MeTTa numerics: BigInt/BigRat/FixedPoint atoms REQUIRED IN PR-1** — USER OVERRIDE of
   recommendation (typed-rejection + follow-up). Canonical numeric byte encodings must be
   designed; deferred row rrs-fu-metta-numerics REMOVED (now in scope). → DELTA DESIGN PASS.
   (Unforgeables/EMap/ESet/GByteArray/sends/bundles rejection stands — not contested.)
7. **Symbol head-position rule w/ always-escaped sentinels** — APPROVED.
8. **Bundle approved**: state-hash single atomic final commit; FIPS space-view deviation
   documented + author question; twin-fallback benchmark pre-approval (~2× threshold);
   THREE PRs (MeTTa-Compiler first, rholang-rs second, f1r3node-rust last); deferred register
   as drafted MINUS the two rows moved into scope (now 19 rows).

## DELTA DESIGN PASS (scope expansions from decisions 4 + 6) — COMPLETE (full text in session)

**DELTA A (merkleized persistence)**: Option (i) — namespaced channels inside the ONE merkleized
RSpace. Channel id = `0xF3 ‖ KIND{D data|C config|I path-index|A allocator|M metta} ‖ space_id(32)
[‖ canonical inner-channel bytes]`. Four verified substrate facts: trie keys = hash(channel)
(zero history changes); leaves canonically SORTED (⇒ insertion order does NOT survive checkpoint
⇒ NEW `ord: Option<u64>` field on Datum/WaitingContinuation, assigned max+1 under channel lock;
None for bag/set/cell/metta); hot store = native lazy per-channel rehydrator (read-through);
merged state writes through the same trie (TrieInsert actions bit-compatible with leaf encoding).
**EventCoverage invariant** (discovered: StateChange diffs ONLY event-referenced channels —
silent merge-loss otherwise): every touched channel must carry a same-channel event; Rocq
statement + debug-assert + merge test. C1#1 amended: event channel identity = NAMESPACED STORAGE
channel (metta: space-level channel — granularity preserved). Integration seam =
`SpaceStoreHandle` (accessors over existing internals + striped locks; GenericRSpace keeps its
architecture, storage substrate swaps; remove_data_at_recorded precedent). Meta-channels: config
(one immutable datum = space existence; temp/seq get NONE), PathMap path-index markers
(commuting adds; vacuum deferred), Array/Vector allocator counter (races = correct conflicts).
VectorDB: records authoritative, matrix/ids/next_id DERIVED (D4 moot-by-supersedure). Set rule:
store-all/view-dedupe-by-pars/consume-removes-all-equal (k-COMM multi-produce covers it).
**MettaSpace: single logical channel** — atom records = datums, parked queries = host
WaitingContinuations at [('M',id)]; **cata root DEMOTED to derived audit digest** (leaf multiset
IS the consensus state; no special root composition); twin+env+registry+persist_set ALL derived;
serialize_paths_with_auxdata RETIRED from consensus path (CLI export only); node-local twin cache
keyed by leaf hash (O(1) attach; **cost charged by fixed record-count formula REGARDLESS of cache
state**); whole-leaf re-encode on mutation = accepted PR-1 bound (bench-gated; NEW deferred row
rrs-fu-metta-shard-storage). **Merge (now live)**: standard checks #1–#3 suffice EXCEPT
discipline-violating write-write races → **NEW check #4**: `write_exclusive: bool` on Produce
(identity-NEUTRAL like is_deterministic — excluded from Eq/Hash/hash_produce; ProduceEventProto
field 8; over produces_created to catch enqueue-then-dequeue-own); flag set for cell/queue/stack/
pq data channels (per-discipline constant). Per-collection merge table: bag/set/vectordb/metta-
asserts/path-markers COMMUTE; cell/queue/stack/pq = #4 conflict (FIPS :245 arrival-order cannot
be derived cross-branch — correct); allocator = #1 conflict (correct); config unconstructible.
mergeable_tags stays OUT. **Rehydration**: lazy handle-triggered (config-channel read on attach;
absent ⇒ dangling-handle error — now scoped to temp/seq handles ONLY, default handles WORK
cross-block); canonical-order rebuilds; C_SPACE_ATTACH + C_REHYDRATE(records) charge-regardless;
cross-process restart-rehydrate harness scenario. **Lifecycle**: host SoftCheckpoint already
snapshots persisted spaces (aggregate protocol collapses into it + metta CoW + epoch-invalidated
derived caches); reset clears derived state which REHYDRATES from root; replay from parent
post-state root (from-empty rule DELETED; D1/C4#1 randomness block-context-free — verified);
temp spaces: in-memory, NO events emitted (would poison EventCoverage), no config datum.
**Tasks 25 → 28**: NEW rrs-persistence-substrate (ord + write_exclusive + proto field 8 +
event_converter ×4 + EventLogIndex + check #4 + SpaceStoreHandle + ChannelNetting lemmas);
NEW rrs-spaces-rehydration; rrs-consensus SPLIT → rrs-consensus-replay (rig consult in
GenericRSpace ops, k-COMM verify, fixture bump final commit) + rrs-consensus-merge (rules table,
merge tests, N4 unblocked); amended: spaces-port-core (substrate swap), metta-consensus (A1.5),
metta-agent/reactivity (registry = derived index over host continuations), fv-rocq-tla
(SpacesInBlockHash=TRUE cfg pin; 2-block self-composition; EventCoverage; check-#4 separation +
per-collection commutation lemmas in merge_algebra; D3 gap DISCHARGED), tests, perf-docs
(default-persistence row CONFORMANT; deviation row removed; queue row rewritten; write-
amplification table + spaces_persistence bench group; security trie-growth note).
**Scope: +25–35% effort, +2–3 weeks critical path, concentrated in rspace++/casper** — sub-epic
treatment, dedicated substrate task w/ own gate. 7 new risks w/ mitigations recorded.
Decision-record additions (vetoable, non-user): queue-conflict rule; set rule; metta single-
channel; write_exclusive field 8; ord rule.

**DELTA B (MeTTa numerics)**: marker-prefixed canonical text symbols in the approved ~-namespace:
`~bigint:<decimal>` (shortest, no +, no leading zeros); `~bigrat:<num>/<den>` (gcd=1, den>0, sign
on num; 7/1 ≠ ~bigint:7 ≠ GInt 7 — TYPE-PRESERVING, forced by round-trip IFF + verified reducer
semantics: cross-type compare = ReduceError, fixp cross-scale = error); `~fixp:<unscaled>p<scale>`
(NO scale normalization — scale is part of the type). Non-canonical forms (non-minimal bytes,
unreduced, den≤0) = typed error NonCanonicalNumeric (REJECTION not canonicalization — a
canonicalizing encoder would break the IFF criterion for patterns; honest values never trip it).
Malformed markers from MeTTa source decode as plain symbols (TOTAL; trie-distinct from canonical
forms); marked atoms ARE writable from MeTTa (canonical spelling of typed values); GString
collision impossible (C1#7 ~-doubling). **ZERO MeTTa-Compiler engine changes** (literal_classifier
verified: `~` prefix → Atom immediately; LongOverflow → Atom; "2/3" → Atom; grounded arithmetic
takes existing non-numeric path). Node-side: 3 arms flip in par_atom's exhaustive match; pattern
subset "ground literals" extended. Small scope: fits existing tasks (+days). Deferred row
rrs-fu-metta-numerics DELETED → replaced by narrow rrs-fu-metta-bignum-arith (evaluator
arithmetic only). Conformance row: "extended numerics stored/matched/round-tripped; arithmetic
deferred" (honest partial). Risk: user conflation of 7/7n/~bigrat:7/1 → four-way distinctness
table in language reference.

**Deferred register final: 22 rows** (19 after two moved into scope, + rrs-fu-metta-shard-storage
+ rrs-fu-pathmap-index-vacuum + rrs-fu-metta-bignum-arith [net-new: evaluator arithmetic only;
metta-numerics row was already removed by decision 6]). **No new user questions.**

### DELTA CRITIQUE (Critic-9): 5 findings (2 major, 3 minor) — ALL ACCEPTED & FOLDED
1. **[MAJOR] write_exclusive replay-verified**: the flag is a recomputable merge-decision input
   riding proposer-serialized logs (identity-neutral ⇒ invisible to replay pairing; tampered log
   could defeat check #4 → two same-ord queue records, an unserializable state). FIX: when the
   rig pairs a replayed op with a logged event, assert logged.write_exclusive == recomputed
   (discipline known: attach precedes produce in play AND replay — verified no ordering hazard);
   mismatch = deterministic replay error; tampered-log rejection test. Homes:
   rrs-persistence-substrate (field+semantics) + rrs-consensus-replay (verification+test) +
   merge_algebra FV soundness premise (flag faithful ⇔ verified).
2. **[MAJOR] Malformed-marker decode target = structural ~sym escape form** (single-`~`
   non-canonical marker symbols decode to ~sym form, byte-preserving on re-encode, OUTSIDE the
   ground-literal pattern subset ⇒ round-trip IFF holds vacuously for non-image atoms; totality
   kept; `~~`-doubled user strings unambiguous — canonical markers never start `~~`). Adversarial
   rows (~bigint:09, ~bigint:+9, ~bigrat:2/4, ~bigrat:1/-2, ~fixp:1p, ~x, ~~bigint:9) added to
   fuzz corpus + Delta-B conformance vectors. Home: rrs-metta-mapping.
3. **[MINOR] C_REHYDRATE count-before-work**: leaf record count = O(1) read from the bincode
   1.3.3 fixint u64-LE Vec-length prefix (verified) → charge C_SPACE_ATTACH, fetch leaf, read
   count, charge C_REHYDRATE(n) BEFORE decoding any record, deterministic abort. Home:
   rrs-spaces-rehydration + metering table.
4. **[MINOR] Deferred register = 22 rows** (arithmetic corrected above).
5. **[MINOR] Cata-root demotion propagated**: MettaRoot.v re-scoped (subject = leaf-multiset
   determinism + derived-digest integrity, not a composed consensus root); T2 wording → "leaf
   multiset = f(logical state)" as the consensus claim; symbol-free grep re-scoped to CLI export
   path. Homes: rrs-fv-rocq-tla + rrs-tests-verification.
**Critic-9's full verification list (sound, no change): substrate claims all confirmed at cited
code; ord semantics across blocks/revert/reuse sound; set rule composes with k-COMM + rand-merge;
write_exclusive no compute-before-config hazard; meta-channel events create no spurious
conflicts; metta merge row correct against actual check definitions (one throughput note: a
parked base query serializes sibling asserts — declared conservatism); temp-space no-events
consistent with check_replay_data + metering-based charging; twin cache consensus-neutral
(rebuild mints no randomStates; all consensus-observable orders route through the twin);
Delta-B facts verified (classifier, reducer, normalizer minimal two's-complement incl. -0n,
BigRational auto-reduce, distinctness matrix); composition clean (dangling rescope, from-root
replay, 28-task acyclicity, fixture-bump placement, SpacesInBlockHash pin, persist_set,
mergeable_tags-out).**

## FINAL EPIC STRUCTURE FOR PGMCP (convergence declared — Critic-10: NO FURTHER RECOMMENDATIONS)

**Loop complete: 31 → 17 → 14 → 0 (core) → 5 (delta) → 0. 67 adjudicated findings; 8 frozen
user decisions + informed re-confirmation of (A).**

### Epic root
- kind `epic`, public_id **`reified-rspaces-mettatron-migration`**, title "Reified RSpaces +
  MeTTaTron space — migration, modernization, and merkleized persistence (3-PR epic)",
  priority 100, **project f1r3node-rust** (root AND every task — never null).
- Links: relates_to → cost-accounted-rho-two-paper-native-completion-epic; relates_to →
  the cost-accounting-transpiler root; depends_on edges per the 28-task table below.
- Root body (per C7#3 outline + deltas): scope paragraph + PR map (**PR-B MeTTa-Compiler
  first** [mc-m0 + mc-bridge-deletion + CI rewrite], **PR-A rholang-rs second**, **PR-C
  f1r3node-rust last**, state-hash bump = final commit of PR-C); plan-file path
  ([HOME]/.claude/plans/help-me-plan-the-swift-adleman.md = the resolved ledger, sole
  design authority); the 29-entry override map note + DELTA-era supersessions; the frozen
  8-answer decision record + vetoable micro-decisions; the 28-task table; the 22-row deferred
  register (each row gets its own pgmcp work item, id recorded back); P0 re-verify checklist
  (C3#8 + C7 additions: deny.toml, fork hosting/visibility, MC CI inventory, js-bindings,
  npm version, mm2-spec license, byte occupancy, parser rev, chapter=20, example count=18,
  observer insertion points, MC base rev); conventions (conventional commits, symbol-name
  citations, [patch] allowlist gate, rustdoc+doctests).

### 28 tasks (public_id | repo | one-line scope | depends_on | priority)
| # | public_id | repo | scope | deps | pri |
|---|---|---|---|---|---|
| 1 | rrs-p0-baseline-verify | all | P0 re-verify checklist + WIP landed+green + baseline benchmarks archived | — | 95 |
| 2 | rrs-decision-record | cross | Frozen 8 answers + all vetoable micro-decisions + deferred-register ids | 1 | 95 |
| 3 | rrs-syntax-contract-freeze | doc | AST/proto/kwarg contract (C2#1–4, C3#1, C5#1/C4#4 EFree=38, kwarg table, parens-`;` rule) | 1,2 | 95 |
| 4 | rrs-parser-grammar | RR | R1 grammar (hyper-inputs, `~` query-spec, contextual use/free, `:` space types, bare floats, RHOLANG_NAMED_COMMENTS preserved) | 3 | 85 |
| 5 | rrs-parser-ast-validation | RR | R2 AST (QuerySpec, params, FreeTheory) + ParsingError variants + !?-source params | 4 | 85 |
| 6 | rrs-parser-sem-corpus-publish | RR | R3+R4 sem resolution, wasm/compiler bails, corpus+goldens, fix v1 test, js-bindings green + npm decision, publish 0.2.0 | 5 | 85 |
| 7 | rrs-models-proto | FN | F1 proto (use_blocks=15, QuerySpec=6, params=8+reserved 4, space_types=6, e_free=38; KeyValuePair decision; sorters + tests) | 3 | 85 |
| 8 | rrs-rspace-interface | FN | F2 (suffix_key, priority, consume_with_query_specs typed, k-COMM event shape, List-rand merge rule n=0/1/≥2) | 7 | 85 |
| 9 | rrs-persistence-substrate | FN | ord field + write_exclusive (proto 8, identity-neutral, replay-verified) + check #4 + SpaceStoreHandle + EventLogIndex + ChannelNetting lemmas | 8 | 85 |
| 10 | rrs-spaces-port-core | FN | F3 structural port on SpaceStoreHandle substrate (D6 matcher, D11/D12, HashSet seq-only, meta-channels, EventCoverage assert, metrics source) | 9 | 85 |
| 11 | rrs-spaces-determinism | FN | D5/D10 + F4 native vectors + kernel + top-k List + NaN + grep-gates | 10 | 85 |
| 12 | rrs-spaces-rehydration | FN | A2 attach/config/dangling-unified + count-prefix charging + composite soft-ckpt + restart harness scenario | 10 | 85 |
| 13 | rrs-normalizers | FN | F5 arms + dup-key/k-XOR-results validation + [patch]-introduced gate | 6,7 | 85 |
| 14 | rrs-system-processes | FN | F6 (0xF3 channels, outer-first URNs + 2 aliases, FIPS positional constructor, rand ids, qualifier matrix, assertEqual, vector ops, genesis attestation) | 7,10,2 | 85 |
| 15 | rrs-reducer | FN | F7 (routing, use-block stack, 3 Seq points, costs incl. C_WAKE/C_GUARD, new-in-use + dangling errors) | 13,14,11 | 85 |
| 16 | rrs-consensus-replay | FN | Real events @ storage channels, rig consult in ops, k-COMM verify, write_exclusive verify + tamper test, temp no-events, fixture bump FINAL COMMIT | 15,11,12,2 | 85 |
| 17 | rrs-consensus-merge | FN | Per-collection merge rules table, exclusive-flag table, merge tests, EventCoverage test, N4 | 16 | 85 |
| 18 | mc-m0-reified-feature | MC | Observer, canonical encoder, eval_with_fuel + effect allowlist + cache classification table + Treewalker pin, dep surgery, deny.toml, toolchain gate | 1,2 | 85 |
| 19 | mc-bridge-deletion | MC | Delete pathmap_par_integration + scaffolding + rholang feature; CI workflows rewrite; grep gates | 18 | 85 |
| 20 | rrs-metta-mapping | FN | M1 Atom↔Par map (audit fixes, symbol rule, numerics ~bigint/~bigrat/~fixp, ~sym malformed decode, fuzz corpus, round-trip IFF on subset, [patch] unification) | 18,7,2 | 85 |
| 21 | rrs-metta-agent | FN | M2 agent + twin + instance records + i16 abort + guard-on-agent + self-join validity + quiescence + complexity table | 20,8,10 | 85 |
| 22 | rrs-metta-reactivity | FN | M3 registry (derived index over host continuations) + delta wake + C_WAKE | 21,15 | 85 |
| 23 | rrs-metta-surface | FN | M4 rho:metta:eval + rho:metta:step + factory + pattern-subset errors; zero grammar delta | 22,14,15 | 85 |
| 24 | fn-bridge-deletion | FN | Do-not-port declaration + grep gate + EPathmapBody .run stub fate | 23 | 85 |
| 25 | rrs-metta-consensus | FN | M5 single-channel storage, cata-root demotion, twin cache + charge-regardless, execution-context parallelism pin, lifecycle | 23,16,17,2 | 85 |
| 26 | rrs-fv-rocq-tla | FN | Gate-owning FV (dispositions, new modules, TLA rewrites + Unsafe cfgs + SpacesInBlockHash=TRUE, EventCoverage, check-#4 lemmas, MettaRoot re-scope, gates, citation audits) | 3 start; 11,16,17,25 close | 75 |
| 27 | rrs-tests-verification | FN | N1–N10 + N-M1–N-M9 + cross-process/restart harness + ASan script + all delta tests | 16,17,25,26 | 75 |
| 28 | rrs-perf-docs-final | FN | F9 benches (+spaces_persistence) + docs/theory tree + ch.20 + conformance matrix (default-persistence CONFORMANT) + D13 + operator docs + provenance + 18+ examples + final pins + [patch]-allowlist gate + preprocess_floats retirement | 27,24,19,6 | 75 |

RR = rholang-rs PR-A; MC = MeTTa-Compiler PR-B; FN = f1r3node-rust PR-C.
Task bodies: composed at epic creation from the resolved ledger sections mapped by the
traceability records (C5 deliverable 2 + C6#6 + delta homes); the 5 drafted bodies
(models-proto, rspace-interface, metta-mapping, system-processes, consensus→split) are the
style templates; every body carries its acceptance criteria inline (ca2p house style).

## ADDENDUM (2026-08-15, post-epic consultation): native PathMap<Multiplicity(u64)> as Rholang's Bag/MultiSet

User question: would native `PathMap<Multiplicity(u64)>` Bag/MultiSet support in Rholang benefit
MeTTaTron integration or the reified rspaces? ASSESSMENT (verified against current tree —
EPathMap here = repeated-Par proto list, NO pathmap dep, NO Multiplicity type; the trie-native
EPathMap architecture [PathMap<()> set / PathMap<Par> map] lives on the feature/mettail–EPM line):
**YES, both — as a VALUE-PLANE enabler**: (1) representation identity with MeTTaTron's atom plane
(RwLock<PathMap<Multiplicity>>, additive/min/saturating algebra) → space snapshots/introspection
as ONE O(1)-CoW first-class value; (2) zero-copy BULK handoff both directions (extends the
zero-serialization property beyond per-atom ops); (3) FIPS bag-literal fidelity ({|"hi"|} examples
run verbatim; prefix aggregation representable); (4) bag algebra via PathMap join/meet/subtract;
(5) reified side: unifies set dedup views + metta twin/derived indices (planned BTreeMaps) +
clone-based soft-ckpt handling + strengthens D-6. **HARD BOUNDARY: cannot replace the Delta-A
merkleized leaf storage** (instance records carry randomState/source/persist/cost/ord — bare u64
can't; the Critic-3 #2 blocker stands). COSTS: new Expr variant (canonical serialization — trie
iteration already canonically byte-lex; sorter scoring; proto surface; state-hash implication —
cheapest riding the epic's single atomic bump); `{| |}` literal grammar (PR-A surface, `{|`
unused); spatial-matcher bag-equality semantics + u64 overflow policy need the epic's standard
determinism treatment; mettail/EPM-line overlap must be reconciled (land-in-baseline-first or
explicit composition). Post-mc-m0 the pathmap crate is transitively present anyway → marginal
dependency cost ≈ 0. Sequencing options: (a) new epic task landing with PR-A/PR-C (same bump);
(b) follow-up register row post-epic. **DISPOSITION (user, 2026-08-15): NEW EPIC TASK — approved.**

### rrs-native-bag task spec (29th task; to be created in pgmcp + wired on approval)
- **Title**: "Native Bag/MultiSet — PathMap<Multiplicity(u64)>-backed bag values, {| |} literals,
  bag algebra, MeTTa handoff". Repo: cross (PR-A sliver + PR-C mass). Priority 85.
- **Body (normative content)**: (1) PR-A deliverables: `{| p1, …, pn |}` bag-literal grammar
  (token `{|`/`|}` — unused, conflict-checked by generation), AST BagLiteral node, corpus +
  golden coverage, sem rules (elements = evaluated context). (2) PR-C: new Expr variant at
  **field 39** (free after e_free=38; e_function retired) — canonical proto encoding = repeated
  (path: canonical element bytes, count: u64) entries SORTED byte-lex (delegated final shape to
  the syntax-contract amendment, KeyValuePair-precedent); models build.rs strip + manual
  PartialEq/Hash; sorter scoring (expr score after EFREE) with reordered-entry equality tests;
  reducer: bag methods {add, remove, count, size, union, meet, subtract, toList, toSet} with
  PathMap<Multiplicity(u64)> backing (pathmap crate — transitively present post-mc-m0; direct
  dep declared + [patch]-unified), **saturating u64 arithmetic matching MeTTaTron's Multiplicity
  semantics (decision)**; spatial-matcher bag semantics (bag pattern matches by multiset
  inclusion/equality — exact rule frozen in the contract amendment) with the same determinism
  treatment as other collections; canonical iteration = trie byte-lex (native). (3) MeTTa
  handoff APIs: space-snapshot → bag value (O(1) CoW from the canonical twin, ground-atom plane,
  π-projected multiplicities) and bag value → bulk assert (metered, funnel-routed, eval-context
  randomState rule) — extends zero-serialization to bulk exchange; lands in the metta surface
  after rrs-metta-mapping. (4) FIPS bag-literal examples run verbatim (conformance rows).
  (5) HARD BOUNDARY restated: value-plane only — the Delta-A record-multiset leaf storage is
  UNCHANGED. (6) State-hash implication rides the epic's single atomic final commit (no second
  bump). (7) Tests: literal round-trip, algebra laws (comm/assoc/idempotence-where-applicable,
  saturation edges), matcher matrix, sorter stability, handoff e2e + determinism, doctests.
  (8) Docs: contract amendment + chapter section + conformance rows.
- **Wiring**: depends_on {rrs-syntax-contract-freeze, rrs-parser-ast-validation,
  rrs-models-proto, rrs-metta-mapping, rrs-decision-record}; rrs-perf-docs-final gains
  depends_on rrs-native-bag (epic cannot close without it).
- **Amendments to existing items (same approval)**: rrs-syntax-contract-freeze body + "includes
  the native-bag surface ({| |} literal, method set, matcher semantics, proto shape)";
  rrs-parser-grammar body + "{| |} bag literal (consumed by rrs-native-bag)";
  rrs-p0-baseline-verify checklist + "mettail/EPM-line reconciliation: the trie-native EPathMap
  value work on feature/mettail (PathMap<()> set / PathMap<Par> map) overlaps — decide
  land-in-baseline-first vs explicit composition, recorded in the decision record";
  epic root body updated (29 tasks; addendum disposition; P0 row).

### Execution steps upon plan approval (no code changes — epic creation only)
> **AMENDED 2026-08-15 (2nd approval)**: 29th task rrs-native-bag (id 5634) created + 6 edges
> (5 deps + perf-docs-final→native-bag) + 4 body amendments (contract-freeze, parser-grammar,
> p0-baseline, epic root) — all tracker-only, no code changes, per the user's explicit framing.
> **EXECUTED 2026-08-15**: epic root id 5582 + 28 tasks (5583–5610) + 46 depends_on edges +
> 2 relates_to (cost-accounted-rho-two-paper epic, cost-accounting-transpiler root) + 22
> rrs-fu-* register items (5611–5632, each relates_to the epic) all created in pgmcp, project
> f1r3node-rust; tree verified (29 rows, zero null project_ids, priorities 100/95/85/75);
> session memory saved (project_reified_rspaces_mettatron_epic).
1. Create epic root (work_item_create kind=epic, project f1r3node-rust, public_id above,
   priority 100) with the full root body.
2. Create the 28 tasks (kind=task, project-scoped, priorities per table) with dense bodies
   composed from the resolved ledger; work_item_link depends_on edges per table.
3. Create the 22 deferred-register work items (kind=nice_to_have or task per row nature,
   prefixed rrs-fu-*), each with scope/why/trigger; record their ids in the root body.
4. work_item_link relates_to → the two existing epics.
5. Report all created public_ids to the user.
6. Save session memory updates (reified-rspaces epic landed; key facts for future sessions).
