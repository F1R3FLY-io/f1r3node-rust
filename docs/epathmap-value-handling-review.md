# EPathMap value-handling fix — upstream-review packet (D5)

Status: REVIEW PACKET for the user's consensus review, prepared 2026-07-20.
Nothing in this stack has been pushed toward origin; this document is the
checklist-bearing record to review BEFORE any upstreaming decision.

Scope: the nine-commit stack on the `f1r3node-rust-mettail` branch
`fix/epathmap-value-handling` @ `ead2f152`, merged back (fast-forward, no
merge commit) into `feature/mettail` on 2026-07-20 so both refs name the same
history. Base: `feature/mettail` @ `2c1d243a` (the per-reduction observation
seam). The first two commits (`31b354e6`, `84a0fbe4`) are the pre-EPM fixes
that the E-6a benchmark campaign surfaced; the remaining seven are the
EPathMap value-handling fix proper (phases P0–P4 of the reviewed plan and its
PM-1…PM-7 amendments).

```
feature/mettail base
2c1d243a  feat(stepper): per-reduction observation seam
    │
    ▼
31b354e6  fix(rholang): split-id width routing [129, 256]      ┐ pre-EPM fixes
84a0fbe4  perf: trie-cache memo + native prefix descent        ┘ (E-6a fallout)
    │
    ▼
602144bd  P0    parity harness — the gate suite (test-only)
c3d5b3f2  P1    content-addressed intern store (K2)
351e494d  P2    method-chain view fusion
4e422b6b  P3    hand-maintained EPathMap wrapper (cached bytes)
60aaa02e  P4.1  Arc-shaped hot-store payloads
6c0a90cb  P4.2  borrowed Matcher signature
ead2f152  P4.3  spliced event hashing (StableHashSerialize)
    ▲
    └── fix/epathmap-value-handling == feature/mettail (merge-back 2026-07-20)
```

Terminology used below:

| term | meaning |
|---|---|
| EPM / EPathMap | the `rhoapi.EPathMap` protobuf value — the PathMap-backed map/index Par |
| COMM | a produce/consume communication event on the tuplespace |
| inj | the benchmark's measured injection wall time (per-cell median, ms) |
| DNF | did-not-finish (a recorded refusal/timeout line, never a silent failure) |
| P0 goldens | the `602144bd` byte pins: prost bytes + `encoded_len`, bincode + JSON serde, produce/consume event hashes, Ord fixtures |
| charge traces | the `602144bd` ordered (reservation-KIND, operation, weight) cost snapshots |
| byte goldens | the P4.1 `serializer_byte_goldens.rs` literal cold-store bytes |
| replay equivalence | the P4.3 spec: log re-derivation (`check_replay_data`), identical play/replay cost, identical checkpoint roots |
| K2 | user decision D1 (2026-07-20): digest-bucket keying with a mandatory full-byte structural verify on every hit |
| L1.5 / L2 | wrapper stages: L1.5 = cached-bytes handle on the prost-field struct (landed); L2 = byte-array-backed model / reference-shaped `ps` (NOT landed — user decision D2) |

---

## 1. `31b354e6` — fix(rholang): route parallel widths 129..=256 to split_short

**What it does.** `DebruijnInterpreter::eval` splits its per-term
`Blake2b512Random` by the term's 0-based index. The old branch sent every
top-level parallel width ≤ 256 to `split_byte(i8)`, so widths in [129, 256]
reached term id 128 and panicked (`TryFromIntError(PosOverflow)`) — that whole
width range crashed unconditionally and never produced output. The fix moves
the branch boundary from `> 256` to `> 128`: widths ≤ 128 keep byte-identical
`split_byte` randomness (the consensus-critical defined range untouched);
[129, 256] joins the `split_short(i16)` path already used by every larger
width; widths > 256 are unchanged. The commit message carries the
no-collision argument: a `split_short` child appends TWO domain-separation
path bytes where `split_byte` appends one, so the rerouted range cannot
coincide with any defined `split_byte` output, and no new aliasing class is
introduced.

**Consensus-relevance.** EXPLICITLY FLAGGED by the commit: "the split id
feeds name derivation — review before upstreaming." This is the one commit in
the stack that changes interpreter-produced randomness for a (previously
crashing) input class, and unforgeable-name derivation consumes that
randomness.

**Scala-divergence flag.** THE sharpest item in the packet. Scala's
`id.toByte` WRAPS for ids 128..=255 (negative single path bytes), so the
reference implementation DEFINES [129, 256] differently from this fix; the
Rust port panicked there instead, so no Rust-produced history changes either
way — but a mixed Scala/Rust network evaluating a Par of top-level width in
[129, 256] would now derive DIFFERENT randomness per implementation. Aligning
(or explicitly dispositioning) this range against Scala is a consensus
decision that must be made before upstreaming.

**Gates.**
`eval_of_wide_par_should_not_panic_across_the_split_byte_width_boundary`
evaluates widths 128/129/256/257 and pins the exact per-term randomness stored
in the tuplespace (split_byte at 128, split_short above); reduce_spec 115/115.
Diffstat: 2 files, +79/−3.

## 2. `84a0fbe4` — perf(models/rholang): trie-cache memo + native prefix descent

**What it does.** Root-caused from mettail experiment E-6a (pgmcp 145): every
EPathMap method dispatch rebuilt the ENTIRE PathMap trie from the protobuf
`ps` list, and the prefix-shaped query methods additionally iterated the whole
map per call — O(index) uncharged host work per query. Fix 1: a bounded
process-wide memo (64-entry LRU) around the pure function
`create_pathmap_from_elements`, keyed by the FULL prost encoding of the
EPathMap — hits certified by full-encoded-bytes equality, no truncated digest
trusted; O(1) refcounted clone on hit; copy-on-write isolates callers. Fix 2:
native trie descent replaces the whole-map scans (`path_prefix_exists`,
`collect_subtrie_values`, `collect_child_segments` with terminator-first DFS),
order-preservation proven and test-pinned against verbatim transcriptions of
the retired scans; plus removal of a redundant whole-EPathMap deep clone per
`readZipperAt`.

**Consensus-relevance.** From the commit: "Cost charges UNCHANGED — this
removes UNCHARGED host work only … Consensus-relevance: none intended
(pure-function memoization + algorithmic equivalence, both value- and
order-preserving); review before upstreaming." Replay determinism unaffected:
results are value-identical whether a call hits, misses, or was evicted.

**Scala-divergence flag.** The Scala node has no counterpart layer
(Rust-only performance mechanism); observable semantics unchanged.

**Gates.** Result-identity suites vs the retired scans over element-built AND
raw adversarial tries (values + order pinned); memo hit/miss value-identity,
no-alias, CoW-isolation tests; full-runtime end-to-end assertions per rewired
method; the cost-invariance probe (identical `EvaluateResult.cost` memo-cold
vs memo-warm). Diffstat: 5 files, +936/−112.

## 3. `602144bd` — test(models,rholang): P0 parity harness (the gate suite)

**What it does.** TEST-ONLY. Captures the `84a0fbe4` derived truths that every
later phase must preserve byte-identically: hand-built E-6a-shaped fixtures;
golden prost bytes + pinned `encoded_len`; golden bincode + JSON serde
(pinning field order/names and the locally_free serialize-as-empty
normalization end-to-end through the produce hash); golden
`Produce::create`/`Consume::create` event hashes (including per-produce
`random_state` placement); Ord fixtures documenting the AlwaysEqual-`==` vs
derived-Ord inconsistency; the charge-trace utility with ordered
(reservation-KIND, operation, weight) snapshots for the four E-6a chain shapes
and the error-edge programs; and the fused-vs-unfused differential scaffold
P2 later plugs into (with the today-path proven byte-deterministic — the null
differential). Also records findings rather than normalizing them away: the
zipper-link constant weighs 3; `create_from_length` is nondeterministic (the
harness seeds via `create_from_bytes`); the exhaustion-boundary determinism
contract (errors + consumed deterministic; committed rows schedule-dependent
at racing budgets).

**Consensus-relevance.** No production change; this commit DEFINES the
consensus gates (event hashes, serde bytes, charge order) the rest of the
stack is pinned by. Only build-surface change: models gains a
`[dev-dependencies]` bincode 1.3.3.

**Scala-divergence flag.** None (test-only).

**Gates.** It IS the gate suite; baseline counts recorded green on unchanged
production code. Diffstat: 21 files, +3320/−0.

## 4. `c3d5b3f2` — perf(models): P1 content-addressed intern store (K2)

**What it does.** Evolves the `84a0fbe4` memo IN PLACE (one mechanism, one
capacity — the plan forbids a second cache) into the intern store:
`InternedEPathMap` (Arc'd) carrying the map, canonical prost bytes,
`encoded_len`, Blake2b-256 digest, `entry_count`, the `eval_stable`
classification, and an unpopulated `serde_bytes` OnceLock (P4.3's lazy
consumer). Keying is the digest of the canonical prost bytes, computed by
STREAMING `Message::encode_raw` through a `BufMut` adapter (zero heap
allocation on the lookup path); the miss path computes `encoded_len` once and
encodes into an exact-capacity Vec, building the trie outside the lock. The
`eval_stable` classifier (ground-only grammar, verified arm-by-arm against
reduce.rs, conservative-false) is the fusion gate P2 consumes. Public API:
`interned_epathmap(&EPathMap) -> Arc<InternedEPathMap>`;
`e_pathmap_to_rholang_pathmap` becomes a shim — all 56 call sites unchanged.

**Consensus-relevance.** From the commit: "NO reserve_* charge lives in this
layer … results are value-identical hit/miss/evicted/collision; replay
determinism untouched (pure content addressing). Consensus-relevance: none
intended — a Rust-only performance layer with the same standing as
84a0fbe4." USER DECISION D1 = K2 (2026-07-20): every digest hit is certified
by an allocation-free FULL-PROST-FIDELITY structural verify against the
stored canonical bytes — byte-exact including locally_free at every level; a
verify mismatch is treated as a miss and emits a once-per-process diagnostic
(a $`\approx 2^{-128}`$ event worth loud evidence). This PRESERVES the
`84a0fbe4` documented stance: no truncated digest is ever trusted — the
digest only selects the bucket; a hit is still certified by
full-encoded-bytes equality.

**Scala-divergence flag.** The Scala node has no counterpart (flagged in the
commit for divergence review before upstreaming).

**Gates.** P0 goldens + charge traces green UNCHANGED; the `84a0fbe4` memo
suites green under digest keying without adaptation; new intern-store suite
(44): streamed digest == one-shot Blake2b-256 (cross-checked against crypto's
independent impl), digest-vs-bytes key-equivalence proptest (the K2 verify
accepts exactly the byte-equal), forced-collision injection (collision list
disambiguates + diagnostic fires), LRU eviction at 64, `eval_stable`
per-category units. Diffstat: 3 files, +1403/−105.

## 5. `351e494d` — perf(rholang): P2 method-chain view fusion

**What it does.** The highest-value phase. One seam,
`DebruijnInterpreter::try_eval_fused_method_chain`
(`interpreter/fused_pathmap_chain.rs`), called FIRST in both EMethodBody
dispatch arms; `Ok(None)` means the existing per-link path runs UNCHANGED. An
O(1) method-name gate rejects before any spine walk; the recognizer accepts a
maximal spine of the 15 read-only PathMap/zipper links with exact arities
(write methods never fuse); the base resolves BY BORROW via the additive
`Env::get_ref`. Fusion is gated on `eval_stable == true` (so skipping today's
re-evaluation of entries is byte- and charge-invisible), and one intern per
chain replaces the per-link O(map) re-walk. The view evaluator pins link
semantics arm-for-arm to the landed implementations; CHARGE REPLAY drives the
same MeteredMachine entry points, same constants, same within-fork order —
mid-sequence budget exhaustion fails at the same charge index with the same
error and consumed total; error parity down to the exact `ReduceError`
payloads and arity-check-first ordering. The differential force-disable
toggle and fusion-hit counters compile ONLY under `cfg(test)` or the
`epathmap-fusion-differential` feature — production builds contain no
runtime-flippable fusion path.

**Consensus-relevance.** From the commit: "interpreter-internal evaluation
strategy only; observable semantics pinned by differentials."

**Scala-divergence flag.** From the commit: "Scala evaluates per-link —
divergence flag: evaluation-strategy only, observably equivalent."

**Gates.** P0 goldens 11/11 + charge traces 9/9 UNCHANGED — the pinned
`84a0fbe4` sequences now reproduce THROUGH the fused path; a 42-row
fused-vs-unfused differential matrix pinning FULL observation equality
(result Par bytes, `random_state`, persist, produce event hashes, error
variant+payload, consumed, canonical charge trace) plus fusion-hit
accounting and vacuousness guards; the control-neutrality falsifier
(byte-identical observations, fusion-hit count exactly 0 on a method-heavy
zero-PathMap program); the budget-exhaustion-at-index-k differential over the
deterministic projection. Diffstat: 6 files, +1934/−19.

## 6. `4e422b6b` — perf(models): P3 hand-maintained EPathMap wrapper (extern_path)

**What it does.** Full stage L1.5. The post-P2 profile's #1 residual was the
store rendezvous itself — every `interned_epathmap` call re-walked the map
through the streaming digest (`Par::encoded_len` alone 10.11% of wall). P3
pins the interned entry ON the instance: `models/build.rs` gains
`.extern_path(".rhoapi.EPathMap", "crate::rust::rhoapi_ext::EPathMap")` so
prost no longer generates the struct; the hand-maintained wrapper keeps the
proto fields (tags 1/3/4/5) plus a private shadow cell
`intern: OnceLock<Arc<InternedEPathMap>>`. The manual `prost::Message`
replicates the prost-derive 0.14.3 expansion with a cached fast path:
`encoded_len` returns the interned length and `encode_raw` memcpys the
canonical bytes (the digest-pipeline kill), with debug_asserts re-verifying
fields against the cached bytes on every cached use. Manual Clone PROPAGATES
the filled cell (first-touch cost per clone family); the `ps` deep-copy
remains — the stated L1.5 limitation (L2 = user decision D2). Serde is
derived with `#[serde(skip)]` on the cell — the same macro expansion the
generated struct had, including the locally_free serialize-as-empty /
deserialize-reads-real-bytes asymmetry. AlwaysEqual `==`/Hash moved verbatim;
Eq/Ord replicate the derived declaration-order compare INCLUDING locally_free
(the `==`-vs-Ord wart preserved and pinned). `#[repr(C)]` dropped after a
workspace sweep found zero transmute/FFI/raw-pointer use. The store consults
the shadow cell FIRST (filled ⇒ O(1) Arc clone — no digest walk, no lock, no
K2 verify); construction sites migrate to `EPathMap::new`.

**Consensus-relevance.** From the commit: "serialization-identical by
construction, gated by the P0 goldens (11/11 byte-identical vs the committed
84a0fbe4 bytes: prost + encoded_len, bincode + JSON, produce/consume event
hashes, Ord); block hashes/deploy signatures over Par bytes unchanged."

**Scala-divergence flag.** From the commit: "Scala models untouched — flag:
Rust-side representation only."

**Gates.** P0 goldens 11/11; charge traces 9/9; P1 store 44/44; P2
differentials feature-off/on; new wrapper-cell suite (14): cell propagation,
cached-vs-computed `encoded_len`/`encode_raw` property tests, the serde
derived-twin differential (bincode + JSON) + both halves of the locally_free
asymmetry, the `==`-vs-Ord wart through the wrapper, Debug parity, the
should_panic stale-cell police. Diffstat: 18 files, +1284/−218.

## 7. `60aaa02e` — perf(rspace,rholang): P4.1 Arc-shaped hot-store payloads

**What it does.** The PAYLOAD fields of the rspace internal records become
Arc-shaped while every public type name and generic signature stays put:
`Datum.a: Arc<A>`; `WaitingContinuation.{patterns: Arc<Vec<P>>, continuation:
Arc<K>}`; `ConsumeCandidate.removed_datum: Arc<A>`. The per-consume-attempt
`get_data` deep clone, per-produce-attempt `get_continuations` clone,
speculative-candidate and history-cache-fill copies, and `locked_produce`'s
double copy become refcount bumps; `produce_inner` passes by move.
Fail-closed serializer enumeration: Serialize/Deserialize are DROPPED from
Datum/WaitingContinuation (no serde "rc" feature), so every serialize site
was compile-enumerated and materializes explicitly — borrowed serialize
twins + owned deserialize twins replicate the pre-P4.1 field layout exactly
(bincode 1.3.3 is positional and ser(&T)==ser(T) ⇒ byte-identical by
construction); the `CandidateOrderingBytes` trait keeps the replay-visible
COMM candidate order byte-identical; value-shaped results materialize once
per FIRED COMM (cost-identical to the pre-P4.1 clones — the multiplicative
per-attempt copies are the ones removed). `RSpaceResult`/`ContResult`
deliberately stay value-shaped (stage L2, user decision D2, out of scope).

**Consensus-relevance.** From the commit: "Event log + history are the
consensus surfaces — both pinned byte-identical (P0 event-hash goldens;
serializer byte goldens; history checkpoint-root suites)." Cold-store leaves
are a consensus surface — hence the literal byte goldens.

**Scala-divergence flag.** From the commit: "Scala rspace stores by value —
flag: in-memory representation only, log/history bytes identical."

**Gates.** rspace++ full set (295: hot_store 44+135 incl. proptests, history
repository/checkpoint roots, export/import, replay, reporting, storage
actions, the new `serializer_byte_goldens.rs` with literal bytes captured
from the `4e422b6b` derives); models + rholang suites baseline-identical.
Diffstat: 16 files, +487/−98.

## 8. `6c0a90cb` — perf(rspace,rholang): P4.2 borrowed Matcher signature

**What it does.** THE one public API SIGNATURE change of the stack:
`Match::get` becomes `(&self, p: &P, a: &A) -> Option<A>` and `check_commit`
takes `matched: &[&A]`. The space no longer clones the pattern and datum
payload per candidate per match attempt; `extract_first_match` zips
channel/pattern references; the interior rework enters the Par pair walk
through `SpatialMatcherContext::spatial_match_par_ref` — the exact branch
structure of the owned matcher with ownership hoisted to the branch that
needs it: the `!connective_used` comparison runs copy-free end-to-end (the
discard-heavy failure path), while a binding/connective pattern clones the
pair ONCE into the owned spatial lattice (the consensus-critical bind
machinery deliberately byte-untouched); `free_check` reads
`Par.locally_free` directly. All implementors adapted: the production
matcher, the four rspace++ test doubles, and mettail's `CountingMatcher`
(its own mandated survival edit on the mettail branch).

**Consensus-relevance.** From the commit: "no serialization or hashing is
touched by this sub-commit; match RESULTS are value-identical, only ownership
of the inputs changed" — event log + history pinned byte-identical.

**Scala-divergence flag.** From the commit: "Scala rspace stores by value —
flag: in-memory representation only, log/history bytes identical."

**Gates.** rspace++ 295 across 12 suites (the replay/reporting/export-import/
storage-actions doubles exercise the borrowed trait); rholang matcher suites
baseline-identical; the mettail measurement path builds and passes (lib 48 +
e6a 5 + equivalence 5). Diffstat: 11 files, +141/−79.

## 9. `ead2f152` — perf(rspace,rholang): P4.3 spliced event hashing (StableHashSerialize)

**What it does.** The hashed unit is unchanged —
`bincode(vec![channel_hash_bytes, bincode(ListParWithRandom), bincode(persist)])`
— but the datum leg (and `hash_consume`'s per-pattern/continuation legs) now
flow through a serialization CONTRACT: `trait StableHashSerialize: Serialize`
whose default body IS the historical `bincode::serialize` path; rspace++
stays fully generic (the orphan rule places the rhoapi impls in models — no
dependency cycle, no Any-downcast); a `&T` forwarding impl keeps overrides
reachable through references. `models/src/rust/spliced_event_bytes.rs` is the
hand-rolled intern-aware emitter for exactly the container spine down to
EPathMap, honoring the FULL bincode-1.3.3 legacy rule set (u64-LE lengths,
u32-LE enum variant declaration indexes, 1-byte Option tags,
declaration-order struct fields, fixint-LE integers, locally_free emitted as
empty bytes at every traversed level). At a filled-cell EPathMap node the
emitter SPLICES `InternedEPathMap.serde_bytes`, populating the P1 OnceLock
lazily — sound to share across the intern family because the store is
content-addressed by canonical prost bytes, which determine the serde bytes.
Interned-map-free subtrees use black-box `bincode::serialize` (legacy
fixint-LE is COMPOSITIONAL — the red-team fact the splice depends on); the
intern-aware path activates ONLY when the value contains a filled cell
(`interned_handle()` is a read-only peek that never forces an intern). Event
structs unchanged (they store only hashes).

**Consensus-relevance.** From the commit: "Event log + history are the
consensus surfaces — both pinned byte-identical: the P0 event-hash goldens
hold UNCHANGED through the trait plumbing AND are re-asserted with FILLED
cells (the spliced path reproduces the exact 84a0fbe4 pins)." Also: models'
bincode moves dev→main dependency (the emitter composes with it).

**Scala-divergence flag.** From the commit: "Scala rspace stores by value —
flag: in-memory representation only, log/history bytes identical."

**Gates.** P0 event-hash goldens unchanged + filled-cell twins (canonical
fixtures 13 = 11 pins + 2 spliced twins); a spliced-vs-direct suite covering
a filled-cell map at EVERY container position of the spine plus a 96-case
proptest over arbitrary bounded trees mixing interned/uninterned maps on all
three legs (hand-emitted == `bincode::serialize` bytes); the NEW
replay-equivalence spec (EPathMap-heavy program: `check_replay_data` log
equality, identical play/replay cost, identical checkpoint roots); rspace++
295; all P0–P3 gates re-run baseline-identical. Diffstat: 18 files,
+1667/−50.

---

## 10. The full measured arc — four committed data records

All four records live in the mettail repository under
`docs/benchmarks/data/sa-vs-naive/` and re-run the SAME E-6a measured corpus
(same workloads, seeds, 33-rep/3-warmup protocol, `taskset -c 0-7`,
performance governor, AMD Threadripper PRO 5975WX; per-record environment
headers in each directory):

| record | f1r3node state | pgmcp experiment | mettail commit | verdict (one line) |
|---|---|---|---|---|
| `2026-07-19-e6a/` | pre-fix (`31b354e6` only) | 145 | `87faea85` | E-6a primary CONFIRMED (6.8–18.6× fewer spread+matching COMMs; `NestedEntryMultiSite` dissolved) but treatment inj wall 2.54×–39.70× SLOWER — the trie-rebuild artifact |
| `2026-07-19-e6a-postfix/` | + trie-cache (`84a0fbe4`) | 145 | `06e1d9f0` | counters byte-identical; wall does NOT flip (band → 2.44×–37.33×); residual root-caused to by-value EPathMap transport |
| `2026-07-20-e6d1/` | + P0–P2 (`351e494d`) | 148 | `c631c051` | counters byte-identical; swap16 4.31× / nested16 3.97× vs postfix (all completed cells 1.90×–4.31×); band → 1.30×–8.34×; new #1 cost = the P1 digest pipeline |
| `2026-07-20-e6d2/` | full stack (`ead2f152`) | 149 | `7b4d5663` | counters byte-identical to ALL THREE baselines; further 1.43× / 1.34× (all cells 1.14×–1.43×); cumulative 6.15× / 5.34×; band → 1.19×–6.35×; digest frames collapsed; residual = the ps-deep-copy floor |

Treatment inj medians (ms) across the arc, from the e6d2 record's four-point
table:

| workload | n | pre-fix | + trie-cache | + P0–P2 | + P3–P4 | further (e6d1→e6d2) | cumulative (postfix→e6d2) |
|---|---|---|---|---|---|---|---|
| swap_comb | 4 | 41.999 | 38.333 | 13.298 | 10.319 | 1.29× | 3.71× |
| swap_comb | 16 | 1660.510 | 1541.572 | 357.969 | 250.735 | **1.43×** | **6.15×** |
| swap_comb | 64 | DNF | DNF | DNF | DNF | — (machine trie-key cap, untouched) | — |
| multi_rule_shared | 402 | 75.254 | 65.246 | 21.944 | 16.295 | 1.35× | 4.00× |
| multi_rule_shared | 803 | 623.416 | 567.204 | 150.638 | 109.207 | 1.38× | 5.19× |
| nested_spine | 2 | 8.543 | 7.832 | 3.737 | 3.190 | 1.17× | 2.46× |
| nested_spine | 8 | 211.945 | 195.893 | 57.373 | 44.674 | 1.28× | 4.38× |
| nested_spine | 16 | 1382.703 | 1315.088 | 331.159 | 246.227 | **1.34×** | **5.34×** |
| lambda_chain | 4 | 56.797 | 53.389 | 28.057 | 24.614 | 1.14× | 2.17× |
| lambda_chain | 8 | 232.249 | 217.392 | 97.665 | 83.289 | 1.17× | 2.61× |

The treatment/control inj ratio band across the arc:
**2.54×–39.70× (pre-fix) → 2.44×–37.33× (trie-cache) → 1.30×–8.34× (P0–P2) →
1.19×–6.35× (full stack)**. Statistics per the frozen experiment criteria:
Welch one-sided with Benjamini–Hochberg across cells (e.g. swap16
q ≈ 1e-79 at e6d1, q ≈ 4e-37 at e6d2); every completed treatment cell
significant at both steps.

Control-neutrality honesty notes (both recorded in the run READMEs, gate
calls owned by the coordinator): e6d1 had ONE frozen-threshold violation
as-run (swap_comb 64 control +10.36%) root-caused as a machine-settling
transient (the settled diagnostic re-probe reproduced postfix +0.4%); e6d2
applied the frozen 300 s settle and saw flat within-cell trends, but controls
ran uniformly FASTER than e6d1 (−0.96%…−8.02%) — either a real shared-transport
win (P4 sits on the produce/consume path both arms exercise) or a
between-session machine offset; under the MOST CONSERVATIVE reading
(ratio-of-ratios) the primary still passes (swap16 1.31×, nested16 1.26×; all
nine cells 1.08×–1.37×).

## 11. Byte-identical counters — the semantic-invariance statement

At EVERY step of the arc, all 15 deterministic counter columns
({primary, matching_comms, consumed_cost_units, program_encoded_len,
attempts} × {median, min, max}) and the extended counters (spread_sends,
successes, observed_count, receiver_count, plus all 10 `comm.*` classes) were
byte-identical on every cell/arm to every prior baseline: postfix ≡ pre-fix;
e6d1 ≡ both; e6d2 ≡ all three. The swap_comb 64 treatment DNF (the machine
trie-key cap) is unchanged throughout. Combined with the in-repo gates (P0
goldens, charge traces, fused-vs-unfused differentials, serializer byte
goldens, replay equivalence), the stack's claim is: NO observable semantics
changed anywhere in P0–P4 — every measured speedup is uncharged host work
removed.

## 12. Known residual — the `ps` deep-copy floor (the L2 junction; USER decision)

The e6d2 profile (perf cpu-clock, swap_comb n=16 treatment, calibrated flat
self-time classifier) shows the clone-class as the new #1 cost class: 29.76%
of wall, ≈74.6 ms/inj. Within it, the prost boxed-oneof `to_vec` deep-copies
are absolutely FLAT across e6d1→e6d2 (≈44.6 → ≈44.8 ms/inj) while models
`Clone::clone` fell −31%: the handle economy (P3 shadow cell + P4 Arc
transport) removed the digest tax, NOT the boxed-oneof deep-copy floor of
≈45 ms/inj. That floor is the `ps: Vec<PathMapEntry>` (and general
boxed-`ExprInstance`) deep-copy that survives because the wrapper still
carries prost-shaped owned fields — eliminating it is stage L2 (a byte-array-
backed / reference-shaped model, `RSpaceResult`/`ContResult` dispatch shapes
included), which is USER DECISION D2 and is deliberately NOT part of this
stack. No work proceeds on L2 without that decision.

## 13. The held-local Cargo.toml overlay (NEVER committed)

The `f1r3node-rust-mettail` checkout permanently carries ` M Cargo.toml` — an
uncommitted `[patch."https://github.com/F1R3FLY-io/rholang-rs"]` overlay
pointing the parser crates (`rholang-parser`, `rholang-tree-sitter`,
`rholang-tree-sitter-proc-macro`) at the local cost-accounted-grammar
worktree, mirroring the base `feature/cost-accounted-rho` worktree's patch.
The published-rev pin (rev=c163755) is the only mergeable state — the parser
rev is part of normalized-Par byte-identity — and `Cargo.lock` churn is held
out of git via `git update-index --skip-worktree Cargo.lock`. The overlay is
NEVER committed, stashed, or reset; every commit in this stack was made with
only its intended files staged, and the review of any future push must
confirm the overlay is absent from the pushed history.

## 14. Review checklist — what to verify before upstreaming

1. **The split-byte range divergence (`31b354e6`).** Scala defines
   [129, 256] via wrapped `id.toByte`; this stack defines it via
   `split_short`. Decide the cross-implementation stance explicitly (align
   with Scala's wrap, change Scala, or fence the range) — the split id feeds
   unforgeable-name derivation, so a mixed network diverges on that width
   range.
2. **The K2 stance change vs `84a0fbe4`'s documented stance.** `84a0fbe4`
   documented "the memo key IS the full prost encoding … no truncated digest
   is trusted." P1 re-keys by Blake2b-256 digest BUCKETS (user decision
   D1 = K2) while still certifying every hit by an allocation-free
   full-byte structural verify. Confirm the reviewed stance is that the
   digest only SELECTS, never CERTIFIES; confirm the once-per-process
   collision diagnostic is acceptable operationally; re-run the
   digest-vs-bytes key-equivalence proptest and forced-collision suite.
3. **The serde-derive-with-skip layout identity (`4e422b6b`).** The hand
   wrapper's derived serde with `#[serde(skip)]` must expand to the exact
   layout the generated struct had (struct name, field order, locally_free
   serialize-as-empty vs deserialize-reads-real-bytes asymmetry). Pinned by
   the P0 bincode+JSON goldens and the derived-twin proptest — verify both
   against the upstream serde/prost versions at merge time (the replication
   targets prost-derive 0.14.3 and bincode 1.3.3 semantics).
4. **The spliced-hash trait plumbing (`ead2f152`).** Verify the
   `StableHashSerialize` bounds ripple is total (every hashed leg routes
   through the trait; the default body is byte-for-byte the historical
   path), the `&T` forwarding impl, the orphan-rule placement of the rhoapi
   impls in models, and the emitter's bincode-1.3.3 legacy fixint-LE rule
   set (the compositionality fact) via the spliced-vs-direct suite + the
   96-case proptest + the P0 event-hash goldens with filled cells + the
   replay-equivalence spec.
5. **The Arc storage shape (`60aaa02e`).** `Datum.a`,
   `WaitingContinuation.{patterns, continuation}`,
   `ConsumeCandidate.removed_datum` are Arc-shaped with Serialize/Deserialize
   DROPPED (fail-closed enumeration; no serde "rc" feature anywhere).
   Verify the borrowed-serialize/owned-deserialize twins against the literal
   byte goldens captured from the `4e422b6b` derives, the
   `CandidateOrderingBytes` replay-order path, and the history
   checkpoint-root suites (cold-store leaves are a consensus surface).
6. **The borrowed Matcher trait (`6c0a90cb`) — the ONE public API signature
   change.** `Match::get(&self, &P, &A) -> Option<A>` and
   `check_commit(&[&A])`. Every implementor must adapt (production matcher,
   four rspace++ test doubles, and mettail's `CountingMatcher` are done);
   enumerate any OTHER downstream implementors at upstream time before
   merging.
7. **Gate re-run.** Re-run the full gate set on the merge candidate: P0
   goldens + charge traces, P1 intern-store suite, P2 differentials
   (feature-on), P3 wrapper-cell suite, rspace++ 295 (byte goldens +
   checkpoint roots + replay), the P4.3 replay-equivalence spec, and the
   mettail package gate — all counts baseline-identical, zero admissions of
   deviation.
8. **The overlay hygiene (§13).** Confirm no pushed commit contains the
   held-local Cargo.toml `[patch]` overlay or a drifted `Cargo.lock`.

Cross-references: mettail run records
`docs/benchmarks/data/sa-vs-naive/{2026-07-19-e6a, 2026-07-19-e6a-postfix,
2026-07-20-e6d1, 2026-07-20-e6d2}/` @ mettail commits
`87faea85`/`06e1d9f0`/`c631c051`/`7b4d5663`; pgmcp experiments 145/148/149
(and 144 for the Track-B sa-vs-naive protocol these records extend); the
run-record index in the mettail
`docs/benchmarks/data/sa-vs-naive/README.md`.
