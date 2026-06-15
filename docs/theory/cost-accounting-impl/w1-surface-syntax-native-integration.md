# W1: Integrating the Cost-Accounted Rholang Surface Syntax into the Native Cost-Accounted Runtime

Status: design doc (no code edits). Branch: `feature/cost-accounted-rho`. Consensus-critical.

> Provenance: produced 2026-06-15 by a fact-verified Plan pass that read `transpiler.md` in full, every file of the transpiler worktree's `cost_accounting/` module, and the native runtime's `supply.rs`, `resource_logic.rs`, `accounting/mod.rs` (`Sig`/`from_sig`/`lane_hash`/`envelope_sig*`), `delta_sigma.rs`, `metering.rs`, plus the normalizer-wiring diff `91b5c70a..0f6ee989`.

## 0. Executive summary

The transpiler worktree's `cost_accounting/` module is cleanly bisected by transpiler.md §7 into **Part A** (surface syntax recognition + signature-resolution algebra — a permanent asset) and **Part B** (the §8 source-to-source lowering to plain `Par` fuel-gates — to be retired). W1 ports Part A, re-targets it at this branch's native `Σ`/lane metering, and drops Part B. The native substrate already has everything Part B was emulating: content-addressed supply pools `Σ⟦s⟧` (`supply.rs::supply_channel` = `SignatureChannel::from_sig`), per-signature lanes (`RuntimeBudget.lanes: DashMap<[u8;32], Lane>` keyed by `Sig::lane_hash`), per-COMM `BillableTokenEvent` metering (`metering.rs`), the demand analyzer (`delta_sigma::demand`), and the apportionment/settlement machinery (`resource_logic.rs`, `acceptance.rs::compute_settlement_debits`). Running Part B on top of that would emit literal `for(t <- Σ⟦s⟧){…}` gates that the native reducer would then meter a SECOND time per-COMM — double-metering. So the lowering is dropped and replaced by a thin **recognition + native-attribution** path.

The single most important native change is in `metering.rs`: today `let sig_hash = budget.signature().lane_hash()` stamps EVERY COMM with the deploy's one envelope signature (the s₀ collapse, Remark 11). W1 makes a COMM that fires on a gate channel `Σ⟦s⟧` attribute to **`s`'s** lane, realizing the located stacks (P14). All other native machinery already consumes lane-keyed events correctly.

Key verified facts driving the plan:
- This branch and the transpiler branch BOTH pin parser `rev = c163755` (in `rholang/Cargo.toml`). The new grammar is NOT at `c163755`; it is the single commit `51e28a6` ("cost-accounted Rholang surface syntax") in the sibling parser worktree `rholang-rs-cost-accounting-transpiler`, layered ON TOP of `c163755`. The transpiler builds it only via a DEV-ONLY workspace-root `[patch]`.
- This branch has NO `cost_accounting/` module and NO surface dispatch arms (`normalize.rs` is still the plain `Proc::ForComprehension → normalize_p_input`; there is no `Proc::SignedTerm`/`Proc::TokenStack` arm — they don't exist in the AST at `c163755`).
- Native `Sig` (`accounting/mod.rs`) = `{ Unit, Ground(Vec<u8>), Quote(Vec<u8>), And, Threshold, Plus, With, Bang, WhyNot, Lolly }`. There is **no `Bound` variant**, and `from_sig` puts **no domain separator** on the channel (DR-1: `Sig::Ground(b) | Sig::Quote(b)` are byte-identical arms). The transpiler's `ir::Sig` = `{ Ground, Bound, Quote, Compound }` WITH `DOMAIN_GROUND/QUOTE/BOUND/COMPOUND` separators. These two `Sig` algebras must be reconciled (§3).
- `rholang` crate does NOT depend on `rholang-lib`, so the parser-side resolver's pattern rejection does not run; the `pattern_guard` belt-and-suspenders is genuinely needed.

## 1. File-by-file disposition of the transpiler `cost_accounting/` module

Source: `…-cost-accounting-transpiler/rholang/src/rust/interpreter/compiler/normalizer/cost_accounting/`. Disposition classes: **PORT** (carry essentially verbatim), **ADAPT** (re-target to native), **DROP** (Part B; native metering supersedes), **REWRITE** (re-do against this branch's diverged versions).

| File | LOC | Class | Disposition / rationale |
|---|---|---|---|
| `mod.rs` | 120 | ADAPT | Keep the module + submodule wiring. **Drop** the `CostLoweringStrategy` port, `strategy()`, `lower::LowerToPar`, and the `InteractionCut`/`RhoInteractionCut` Part-B scaffolding. Replace the two port methods with direct native-attribution entry points (`recognize_signed_term`, `recognize_token_stack` — §3). Rewrite the module-doc to "native attribution," not "internalise functor / Strangler-Fig lowering." |
| `ir.rs` | 171 | PORT (with reconciliation) | The `Sig` IR + `ResourceSignature` trait + `Sig::compound` (flatten + key-sort) + `Sig::atoms()` are the heart of Part A. **Reconciliation** (§3.2): keep `ir::Sig` as a normalizer-local front-end type (it has `Bound`, which native lacks) plus `fn to_native(&self) -> accounting::Sig`. `DOMAIN_*` constants stay for `key()`/ring-fence identity, but the **channel** comes from native `from_sig` (§3.1), not the transpiler's own `supply_channel`. |
| `sig.rs` | 171 | ADAPT | `signature_to_ir` + `canon_bound`/`canon_ground`/`canon_quote` PORT verbatim — the signature-resolution + `new`-bound ring-fencing. **Drop/replace** `supply_channel`/`signature_channel`/`atom_channel` (the transpiler's domain-separated content-addressing); route channel derivation through native `from_sig` (§3.1). Keep `canon_*` (they define the BYTES that become `Sig::Ground/Quote/Bound`). |
| `token.rs` | 71 | DROP→REWRITE | Part B: `lower_token_stack` emits `Σ⟦s⟧!(K⟦S⟧)` send chains. **Rewrite** as recognition: a `s :: S` surface stack is recognized and its layer signatures resolved to fund the corresponding gate lanes (§3.3). No separate Part-B `for(t<-Σ⟦s⟧)` gate. `build_splitter` goes away in Phase 1. |
| `signed_term.rs` | 195 | DROP→REWRITE | Part B: `build_gates` hand-builds nested `Receive` fuel gates — the double-metering hazard. **Rewrite** so `{% P %}[s]` recognizes the signature, lowers `P` normally, and stamps `P`'s COMMs with `s`'s lane (§3). **Preserve the join-sequential-fuel discipline** (`lower_signed_join`: strip `Bind::Signed` to linear, collect clause sigs, one fuel unit per atom, do NOT re-sign the continuation) as per-clause attribution (§4) — attributing to native lanes rather than emitting nested gates. Lollipop + `uniform_sign` kept as AST rewrites (Part A). |
| `desugar.rs` | 132 | PORT | `uniform_sign`, `lollipop`, `strip_signed_binds`, `rebuild_for` — pure Part A AST→AST rewrites. PORT verbatim. |
| `pattern_guard.rs` | 53 | PORT | `reject_cost_syntax_in_pattern` / `…_in_name_pattern` — needed verbatim (no `rholang-lib` resolver). |
| `infra.rs` | 61 | DROP | `build_splitter` is the Phase-2 combined-cell splitter, pure Part B. Native handles compound funding via `effective_supply`/`split_join_decompositions` (§3.4). |
| `oslf.rs` | 172 | DROP | A parallel mirror of the funding judgment. This branch has the authoritative `OslfResourceLogic`/`GsltPresentation`/`ResourceSignature` in `accounting/resource_logic.rs` with Rocq-anchored conformance laws — use it directly. |
| `lower.rs` | 50 | DROP | The `LowerToPar` Strangler-Fig seam. Pure Part B. |
| `tests.rs` | 513 | ADAPT | Lowering-SHAPE assertions become moot. **Re-target** the SEMANTIC ones to native attribution (ring-fencing free vs `new`-bound disjoint lanes; determinism; signed-join park/fire; `Σ`-AC against native `from_sig`). Port `signature_to_ir`/`canon_*` tests verbatim. |

Wiring files (REWRITE against THIS branch — 332 commits diverged but the seams are small):

| File | Disposition |
|---|---|
| `compiler/normalize.rs` | Add three dispatch arms: (1) `Proc::ForComprehension` gains a `Bind::Signed` check → signed-join attribution; (2) `Proc::SignedTerm { proc, sig }`; (3) `Proc::TokenStack { stack }`. Same surrounding structure as the transpiler pre-edit version → mechanical. |
| `compiler/normalizer/mod.rs` | `pub mod cost_accounting;`. |
| `processes/p_input_normalizer.rs` | Re-apply: the `debug_assert!` that no `Bind::Signed` reaches here; the `pattern_guard` scan over every bind's `lhs.names`; the `Bind::Linear | Bind::Signed` merge arms. |
| `processes/p_contr_normalizer.rs` | Re-apply: `formals` lifetime bump + `reject_cost_syntax_in_name_pattern` in the formals loop. |
| `processes/p_match_normalizer.rs` | Re-apply: `reject_cost_syntax_in_pattern` at the top of the case loop. |

## 2. The parser-rev dependency

**What the new grammar adds** (parser `c163755..51e28a6`, `ast.rs +52`):
- `Proc::SignedTerm { proc: &AnnProc, sig: Signature }` — `{% P %}[ s ]`.
- `Proc::TokenStack { stack: TokenStack }` — bare stack `s :: … :: ()` (no `purse(...)`).
- `enum Signature { Ground(Name), Hash(AnnProc), Compound(Box<Signature>, Box<Signature>), Transfer(Box<Signature>, Box<Signature>) }` — `g`, `#P`, `s1 (*) s2`, `s1 -o s2`.
- `struct TokenStack { layers: … }`.
- `Bind::Signed { lhs, rhs, sig }` — per-clause signed bind `{% y <- x %}[ s ]` (Axis-C).
- Tree-sitter regeneration (LANGUAGE_VERSION 15, STATE 1364→1457), `traverse.rs +44` (DFS into signed-term bodies/signatures, needed by `pattern_guard`), `ast_builder.rs +16` (`alloc_signed_term`, `alloc_for_with_guards`), `rholang-lib` resolver passes (NOT used by f1r3node).

**Published vs local.** The grammar is NOT at `c163755`; it is only `51e28a6` in the sibling worktree branch. This branch cannot build the surface syntax against its current pin without one of:
1. **Publish-then-pin (production path).** Land `51e28a6` on `rholang-rs` upstream, bump `rholang/Cargo.toml` `rev`, update `Cargo.lock`'s three crate entries. This is the ONLY pushable state (transpiler.md §6: must not be pushed until the parser rev is published). Consensus code MUST NOT merge to a release branch on a local `[patch]`.
2. **Dev `[patch]` (development path).** Mirror the workspace-root `[patch."https://github.com/F1R3FLY-io/rholang-rs"]` block pointing the three crates at `../rholang-rs-cost-accounting-transpiler/`, + `git update-index --skip-worktree Cargo.lock`. Reverted before push.

**Invariant.** The parser rev is part of every normalized `Par`'s byte-identity (it determines the AST → the lowered `Par` → its hash). All validators MUST run the SAME parser rev; a `[patch]`-built node and a `rev`-built node could diverge if `parser.c` differs by one byte. Merge-gate: published rev pinned, `[patch]` absent, `Cargo.lock` restored, `cargo build -p rholang` green against the published rev.

**Build flag (carry verbatim).** `RUSTFLAGS="-C target-cpu=native"` is REQUIRED whenever the new parser is in the tree — it pulls `gxhash` (AES/SSE2 intrinsics).

## 3. The native wiring — how each surface construct maps to native metering

Governing principle: **surface forms decorate; they never re-emit metered operations.** Recognition resolves `s` to a native `Sig`; the only runtime effect is (a) provisioning fuel on `Σ⟦s⟧` (a real supply send) and (b) attributing the gated COMMs to `s`'s lane. Exactly one COMM per gate firing (native per-COMM), never the Part-B doubled "explicit gate COMM + native COMM."

### 3.1 Reconciling `Σ⟦s⟧`: native `from_sig` wins
transpiler.md §2 flags that the transpiler domain-separates `DOMAIN_GROUND`/`DOMAIN_QUOTE` in the channel hash while native does NOT (DR-1). **Native `from_sig` wins:** it is consensus state — `supply_channel(sig) = SignatureChannel::from_sig(sig).par`, and `Sig::lane_hash` is the domain-separated Blake2b256 of that channel encoding; the supply producer, the WD-D2 gate, replay, and the lane pool are all anchored to it (`supply_channel_equals_lane_pool_channel`). A separate channel hash would fork the basis and break play/replay byte-identity. The transpiler's separation was benign only because Part B targets a non-metering reducer; on native, byte-parity with `from_sig` is mandatory. So W1's `sig.rs` keeps `canon_*` (content bytes), maps `ir::Sig → accounting::Sig` (§3.2), and derives the channel via native `from_sig`.

### 3.2 The two `Sig` algebras — bridge `ir::Sig → accounting::Sig`

| transpiler `ir::Sig` | native `accounting::Sig` | bridge |
|---|---|---|
| `Ground(content)` | `Ground(content)` | identity on bytes (`canon_ground(name)`) |
| `Quote(content)` | `Quote(content)` | identity on bytes (`canon_quote(P)`) |
| `Compound(vec)` | left-assoc `And(Box, Box)` fold | fold sorted atoms into `And` (matches `fold_compound_sig`; `from_sig`'s `And` arm is sort-matched ⇒ AC holds) |
| `Bound(content)` | **no native variant** | DECISION below |

**The `Bound` problem (load-bearing).** Native `Sig` has no ring-fenced variant. **Recommended (no enum change):** map `ir::Sig::Bound(span_bytes)` to `accounting::Sig::Ground(DOMAIN_BOUND ‖ span_bytes)` — fold the bound-domain separator + binder span INTO the ground content bytes. Then native `from_sig` produces a distinct `GPrivate` channel (bytes differ from any free sig's `canon_ground(name)`), so ring-fencing holds intrinsically via content-addressing, with ZERO consensus-surface change. Disjointness reduces to "`DOMAIN_BOUND`-prefixed span bytes never equal a `canon_ground` `Par` wire encoding" — provable by construction; assert in a test. **Rejected for W1:** adding `Sig::Bound` to the native enum (touches `to_proto`/`from_proto`/`from_sig`/`lane_hash`/wire + Rocq/TLA+ — too consensus-risky for surface syntax). The bridge lives in `cost_accounting/sig.rs` as `signature_to_native_sig`.

### 3.3 `{% P %}[s]`, cons-tokens, and the located-stack attribution
The native gap: `delta_sigma.rs` docs say the normalized `Par` carries NO per-layer signature (s₀ collapse); `metering.rs` stamps `sig_hash = budget.signature().lane_hash()` once. W1 closes it:
1. **Recognition (normalizer).** `Proc::SignedTerm { proc: P, sig: s }` resolves `s` to a native `Sig` and normalizes `P` ordinarily, threading the resolved `Sig` so COMMs inside `P` are tagged with `s`. Preferred mechanism: a "signature context" on the lowered `Par` region so the reducer, firing a COMM whose channel is `Σ⟦s⟧` / whose enclosing signed region is `s`, reserves the billable event with `sig_hash = s.lane_hash()` instead of the envelope. The native gate already keys lanes by `lane_hash` and `attempt_in_lane` routes events to lanes — the only change is WHICH `sig_hash` a COMM carries. The token stack `s :: S` becomes a real supply provisioning (head layer mints one unit on `Σ⟦s⟧` — a supply write, the analogue of `produce_balance`, NOT a re-metered user send); the consuming gate's COMM is the metered op, attributed to `s`. One COMM per op, no Part-B double gate.
2. **`delta_sigma::demand` extension (located stacks, P14).** Today `demand` ignores `deploy_sig` and counts all COMMs to the envelope. Extend it to attribute per-`Σ⟦s⟧`-channel COMMs to `s` (a per-`SigKey` `Δ_s` map) feeding `compute_settlement_debits` per pool. This is the static dual of the runtime attribution in (1); the two MUST agree COMM-for-COMM (extend `delta_sigma_spec.rs` from single-lane to multi-lane).
3. **Cost = per COMM, ONE consumable (Greg's model).** No `Pay(τ)` second token (it is a type). The system token is the supply unit on `Σ⟦s⟧`; phlogiston is the degenerate single-lane case. User cons-tokens `S ::= () | S(x, s :: S)` are signed and desugar to system-token provisioning on `Σ⟦s⟧`, the signature tracking origin. Users decorate with `{% %}[s]`; they cannot mint system tokens (`Σ⟦s⟧` is unforgeable `GPrivate`, §5).

### 3.4 Compound `s1 (*) s2` and balanced multi-sig cost (P8)
A compound resolves to native `Sig::And(s1, s2)`; its channel is the sort-matched union (`from_sig` `And` arm), permutation-invariant (commutative ∘, P8). Cost is apportioned by `compute_settlement_debits` + `ApportionmentPolicy` (`resource_logic.rs`). **RESOLVED (task #12, Greg P8):** the committed `DefaultApportionment` ALREADY realizes Greg's "balanced cost per wallet" — it debits the component pair `(left, right)` the SAME `draw_pair` each, so every cosigner wallet pays an equal share, order-independently (commutative). No `BalancedApportionment` replacement is needed; the combined-pool-first step is the orthogonal joint-funds policy (`Σ⟦s₁∘s₂⟧` is co-owned). W1 reuses this verbatim. Split/Join interchange (combined-cell `a (*) b :: ()` vs separate stacks `a :: () | b :: ()`) uses native `effective_supply_with`/`split_join_decompositions`, NOT the transpiler `infra.rs` splitter. Phase 1: separate-stack funding (R2/R4) in scope; combined-cell (R3/R5) via the native Split/Join closure.

## 4. The join-sequential-fuel rule — where it is enforced
Greg's rule: a token-gated receive must acquire fuel via SEQUENTIAL nested single-channel receives, NEVER fold fuel tokens into a data join (RSpace join-matching is combinatorial in arity; an n-clause join → 2n-way is "extremely slow"). Valid because the tokens are independent (∘ commutative, no double-spend per token). Enforcement:
1. **Recognition never builds a fuel+data join.** W1's recognition emits NO extra `for`; fuel is provisioned on `Σ⟦s⟧` and the data `for` is metered per-COMM by lane, so a fuel token is structurally incapable of entering the data join's `ReceiveBind` set. ASSERT a debug invariant that the data `for`'s `binds` contains only the original linear clauses (mirroring `normalize_p_input`'s `debug_assert!(… no Bind::Signed …)`).
2. **Per-clause attribution is per-lane independent.** Each `Bind::Signed { sig: s_i }` clause attributes its rendezvous COMM to `s_i`'s OWN lane; `compute_settlement_debits` charges per-pool independently. The continuation `P` is NOT re-signed (one token per clause).
3. **Documentation anchor.** A code comment at the signed-join entry point in `normalize.rs` and `cost_accounting/signed_term.rs` MUST cite this rule (Greg 2026-06-15) so a future refactor doesn't "optimize" fuel into the join.

## 5. Security invariant — protocol `Σ` pools stay unforgeable
Invariant: an in-term ground signature `g` written by a user MUST NOT alias a protocol-controlled system pool `Σ⟦v⟧`. It holds on native because protocol pools are keyed by `from_sig(envelope_sig_single(sig_bytes)) = Sig::Quote(Blake2b256(DEPLOY_SIGNATURE_DOMAIN ‖ wire_sig))` → `GPrivate`, and the ONLY writer of a supply-balance datum is the Rust `supply.rs` module on a `GSysAuthToken`-bearing system deploy (no bytes→`GPrivate` surface primitive, DR-13). A user's `g` resolves to `Sig::Ground(canon_ground("g"))` (a sort-matched `GString` wire form); aliasing would require a Blake2b256 second-preimage between that and a `DEPLOY_SIGNATURE_DOMAIN`-prefixed signature hash. **AUDIT TEST (deliverable):** for representative envelope sigs `v` (single + compound) and a fuzzed/adversarial set of user surface ground/`#P`/compound sigs, assert `from_sig(user_sig).par != from_sig(envelope_sig(v)).par` and `user_sig.lane_hash() != envelope_sig(v).lane_hash()` for all pairs; plus assert the recognition path never emits a `produce_balance`-style write on a channel that decodes to a system envelope.

## 6. Phasing + test strategy
- **Phase 0 — parser pin.** Dev `[patch]` (workspace-root `Cargo.toml`) → `../rholang-rs-cost-accounting-transpiler/{rholang-parser,rholang-tree-sitter,rholang-tree-sitter-proc-macro}`; `git update-index --skip-worktree Cargo.lock`. Verify `cargo build -p rholang` sees `Proc::SignedTerm`/`TokenStack`/`Bind::Signed`. Merge-gate: publish rev + bump pin (§2 path 1).
- **Phase 1 — surface recognition.** Port `ir.rs`/`sig.rs`/`desugar.rs`/`pattern_guard.rs`; add `cost_accounting/mod.rs` (recognition entry points, no `LowerToPar`); re-do the five wiring edits. End state: surface syntax PARSES + is RECOGNIZED + pattern-rejected; signatures resolve to native `Sig`; attribution may still collapse to the envelope (no behavior change yet). Tests: ported `signature_to_ir`/`canon_*`; pattern-rejection; `ir_to_native` AC/commutativity against `from_sig`.
- **Phase 2 — native sig-resolution + channel reconciliation.** `signature_to_native_sig`, the `Bound → Ground(DOMAIN_BOUND‖span)` fold, channel via `from_sig`. Tests: ring-fence trio (free vs `new`-bound → distinct channels; free shared sig → same channel); the §5 security audit test.
- **Phase 3 — demand attribution (located stacks, P14).** Extend `delta_sigma::demand` to per-`SigKey` attribution; generalize `metering.rs`'s single `sig_hash` capture to per-region/channel. Tests: extend `delta_sigma_spec.rs` to multi-lane static-vs-runtime COMM equality; OSLF conformance per lane.
- **Phase 4 — join-sequential-fuel + per-clause + multi-sig.** Signed-join recognition with per-lane independent attribution; assert no fuel-in-data-join (§4). Reuse `compute_settlement_debits` + apportionment for balanced multi-sig (P8). Tests: signed-join funded/unfunded/mixed; compound balanced-debit conservation.
- **Phase 5 — demo as native integration test.** Port `examples/cost_accounting_demo.rho` and run it through THIS branch's native reducer; assert its audit invariants (MONEY 410, WIDGET 83, no negative cell, one flash-sale winner, unfunded desk + free-`diSig` thief PARK). Headline acceptance test.

**Verification commands:**
```
RUSTFLAGS="-C target-cpu=native" cargo fmt --check
RUSTFLAGS="-C target-cpu=native" cargo clippy -p rholang -p casper --all-targets -- -D warnings
RUSTFLAGS="-C target-cpu=native" cargo build  -p rholang -p casper
RUSTFLAGS="-C target-cpu=native" cargo test   -p rholang -p casper
```
`target-cpu=native` is mandatory while the new parser (`gxhash`) is in the tree.

## 7. Risks
1. **Parser-rev skew (consensus).** A `[patch]`-built node vs a published-rev node could produce divergent normal forms. Merge-gate: published rev pinned, `[patch]` absent, lock restored, green build; CI asserts no `[patch]`/lock churn.
2. **Two `Sig` algebras drift.** A conformance test asserts `from_sig(to_native(ir_sig))` agrees with the intended channel for every constructor, and `Bound` channels are disjoint from all `Ground` channels by construction.
3. **Double-metering if any Part-B leaks in.** §1 DROPs `lower.rs`/`build_gates`/`token.rs` send-chains/`infra.rs`. Test: the normalized `Par` of `{% P %}[s]` has the SAME COMM-count as `P` alone (not +1 per gate); `delta_sigma_spec` runtime-vs-static equality catches inflation.
4. **Replay byte-identity for new supply writes.** Any new `Σ⟦s⟧` provisioning is consensus state; route through the existing replay-stable `random_state` family (anchored to the close-block deploy `initial_rand`, fresh disjoint `RNG_PATH`). Prefer NO new write paths in the normalizer (keep provisioning in the Workstream-C producer).
5. **`metering.rs` sig_hash generalization (hot path).** Keep the envelope as default `sig_hash`; only OVERRIDE for COMMs inside a recognized signed region / on a `Σ⟦s⟧` channel. Single-sig non-cost deploys take the unchanged path (lanes empty, byte-identical — `legacy_single_sig_byte_identical`). Add a regression.
6. **`delta_sigma` over-approximation interaction.** Carry the `unknown`/Thm-20 flag per lane; conformance `law_sound` two-regime runs per lane.

## 8. Critical files for implementation
- `rholang/src/rust/interpreter/compiler/normalize.rs` — three surface dispatch arms; recognition entry point.
- `rholang/src/rust/interpreter/accounting/delta_sigma.rs` — extend `demand` from envelope-only (s₀) to per-`Σ⟦s⟧`-lane attribution (P14); keep `effective_supply`/Split-Join.
- `rholang/src/rust/interpreter/metering.rs` — generalize the single `sig_hash = budget.signature().lane_hash()` so a gated COMM attributes to its gate signature's lane.
- `casper/src/rust/util/rholang/supply.rs` — the canonical `Σ⟦s⟧` = `from_sig` basis (§3.1); host of the §5 audit test + any replay-stable supply-write derivation.
- `rholang/src/rust/interpreter/accounting/resource_logic.rs` — the authoritative `OslfResourceLogic`/`ResourceSignature`/`ApportionmentPolicy` to reuse; balanced multi-sig (P8).
- New ported Part-A sources under `rholang/src/rust/interpreter/compiler/normalizer/cost_accounting/`: `ir.rs`, `sig.rs`, `desugar.rs`, `pattern_guard.rs`, `mod.rs` (+ re-targeted `signed_term.rs`/`token.rs` recognition, `tests.rs`).
