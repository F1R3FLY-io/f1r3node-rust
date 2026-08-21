# W1: Integrating the Cost-Accounted Rholang Surface Syntax into the Native Cost-Accounted Runtime

Status: superseded historical implementation plan. Branch:
`feature/cost-accounted-rho`. Consensus-critical.

> **Current representation:** signed terms and token stacks are not erased.
> `CostSignedTerm`, `CostStack`, datum/continuation `CostAuthority`, runtime-bound
> slot substitution, and certificate-bound physical draws persist through
> normalization, RSpace, replay, and settlement. Sections below that say the
> normalized `Par` is byte-identical to unsigned `P`, stacks lower to `Nil`, or
> per-region attribution is diagnostic describe the retired W1 staging design.
> See [End-to-End Authority Settlement](end-to-end-authority-settlement.md) and
> the [Executable Conformance Matrix](../cost-accounting-executable-conformance-matrix.md).

> **Normative correction:** references below to reducer-side send/receive billing describe the superseded W1 implementation. The current runtime charges one token only when RSpace commits a complete binary or join match. Unmatched I/O is free, trigger order is irrelevant, and an N-way join costs one. `delta_sigma` is a conservative structural introduction bound for the closed non-persistent fragment; exact production cost comes from state-bound execution evidence. The observer, replay, settlement, and formal models documented in [End-to-End Authority Settlement](end-to-end-authority-settlement.md) are authoritative.

> Provenance: produced 2026-06-15 by a fact-verified Plan pass that read `transpiler.md` in full, every file of the transpiler worktree's `cost_accounting/` module, and the native runtime's `supply.rs`, `resource_logic.rs`, `accounting/mod.rs` (`Sig`/`from_sig`/`lane_hash`/`envelope_sig*`), `delta_sigma.rs`, `metering.rs`, plus the normalizer-wiring diff `91b5c70a..0f6ee989`.

## 0. Executive summary

The transpiler worktree's `cost_accounting/` module is cleanly bisected by transpiler.md §7 into **Part A** (surface syntax recognition + signature-resolution algebra — a permanent asset) and **Part B** (the §8 source-to-source lowering to plain `Par` fuel-gates — retired). W1 ports Part A and drops Part B. The native substrate provides content-addressed supply pools, signature authority, state-bound demand evidence, atomic-COMM observation, and deterministic apportionment/settlement. Synthesizing Part-B fuel processes would add protocol-visible rendezvous that are not part of the user's process and would distort both state and cost.

The current load-bearing native change is the RSpace observer: it identifies a committed atomic COMM from stable consume/produce identities, reserves authority before state mutation, and attributes the event through the installed signer-channel mapping. Scheduler-local reducer paths do not enter consensus evidence.

Key verified facts driving the plan:
- This branch and the transpiler branch BOTH pin parser `rev = c163755` (in `rholang/Cargo.toml`). The new grammar is NOT at `c163755`; it is the single commit `51e28a6` ("cost-accounted Rholang surface syntax") in the sibling parser worktree `rholang-rs-cost-accounting-transpiler`, layered ON TOP of `c163755`. The transpiler builds it only via a DEV-ONLY workspace-root `[patch]`.
- This branch has NO `cost_accounting/` module and NO surface dispatch arms (`normalize.rs` is still the plain `Proc::ForComprehension → normalize_p_input`; there is no `Proc::SignedTerm`/`Proc::TokenStack` arm — they don't exist in the AST at `c163755`).
- Native `Sig` (`accounting/mod.rs`) = `{ Unit, Ground(Vec<u8>), Quote(Vec<u8>), And, Threshold, Plus, With, Bang, WhyNot, Lolly }`. There is **no `Bound` variant**, and `from_sig` puts **no domain separator** on the channel (DR-1: `Sig::Ground(b) | Sig::Quote(b)` are byte-identical arms). The transpiler's `ir::Sig` = `{ Ground, Bound, Quote, Compound }` WITH `DOMAIN_GROUND/QUOTE/BOUND/COMPOUND` separators. These two `Sig` algebras must be reconciled (§3).
- `rholang` crate does NOT depend on `rholang-lib`, so the parser-side resolver's pattern rejection does not run; the `pattern_guard` belt-and-suspenders is genuinely needed.

## 0.1 The optimal combination — normalization/rewriting + native cost-accounting (overview)

W1 is the *optimal combination* of Rholang normalization (+ desugaring/rewriting) and **native**
cost-accounting: the surface forms are **recognized** (decorate + validate), the inner process is
lowered through the **ordinary** normalizer, and metering happens **per atomic COMM at RSpace's match boundary** — never
through synthesized fuel-gate processes. The three surface forms and their recognition:

| Surface form | Recognized as | Lowered to |
|---|---|---|
| `{% P %}[s]` (signed term) | resolve + validate the signature `s` to a native `accounting::Sig`; apply the lollipop (`s₁ -o s₂`) and uniform-signing AST rewrites | `P` through the **ordinary** dispatch — byte-identical `Par` to unsigned `P` |
| `s :: S` (token stack) | resolve each layer's signature (validation only — no in-program mint; DR-13) | the empty process (it decorates, it does not emit) |
| `for(... {% y <- x %}[s] ...)` (signed join) | `strip_signed_binds` recovers the natural-arity plain join + collects clause signatures | the plain `for` through the ordinary dispatch — no fuel folded into the data join |

The key property is that recognition emits **no** `for(t <- Σ⟦s⟧){…}` fuel-gate node, so the
normalized `Par` of `{% P %}[s]` is byte-identical to `P`. RSpace meters one token per successful
atomic match, attributed to the authority lane. The Part-B gate translation would instead add
rendezvous and state that are not part of `P`. Dropping it preserves source semantics and leaves one
authoritative accounting boundary.

![W1 recognition pipeline. Surface forms enter the recognizer, signatures are validated, and the ordinary normalizer emits no fuel-gate process. The current runtime meters successful atomic RSpace matches; the historical diagram's reducer-side wording is superseded by the normative correction above.](../diagrams/w1-native-pipeline.svg)

(*Source: [`diagrams/w1-native-pipeline.puml`](../diagrams/w1-native-pipeline.puml) — render with `plantuml -tsvg docs/theory/diagrams/w1-native-pipeline.puml` (or `./render.sh w1-native-pipeline.puml`). Converted from the former Mermaid source — PlantUML is preferred over Mermaid where their capabilities overlap.*)

The signature `s` is resolved to the native `Sig` algebra, which is bisected by **F-A** into the
funding formers (`g | #P | s∘s` = `Unit`/`Ground`/`Quote`/`And`) — the only shapes that may key a
supply pool `Σ⟦s⟧` — and the capability / value type-logic connectives (`Threshold`, `⊕`, `&`, `!`,
`?`, `⊸`), which are never funding formers. Post-§D2.9 the funding key is literally the `Ground(pk)`
atom (the public key), so the funding path is the `g` ground atom of the grammar:

![The Sig algebra bisected — the one accounting::Sig enum split into the funding formers (Unit, Ground(g) = the public key (the §D2.9 funding key), Quote(#P), And(∘)) which key the supply pool Σ⟦s⟧, and the capability / value type-logic connectives (Threshold, Plus ⊕, With &, Bang !, WhyNot ?, Lolly ⊸) which are rejected at the funding chokepoint by is_funding_former() (F-A). funding_sig is built only from {Unit, Ground, Quote, And}.](../diagrams/sig-funding-vs-capability.svg)

(*Source: [`diagrams/sig-funding-vs-capability.puml`](../diagrams/sig-funding-vs-capability.puml) — render with `plantuml -tsvg docs/theory/diagrams/sig-funding-vs-capability.puml`.*)

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
1. **Publish-then-pin (production path).** Land `51e28a6` on `rholang-rs` upstream, bump the workspace `rholang-parser` `rev`, update `Cargo.lock`'s three crate entries, and pass `.github/scripts/check-rholang-parser-pin.sh`. This is the ONLY pushable state (transpiler.md §6: must not be pushed until the parser rev is published). Consensus code MUST NOT merge to a release branch on a local `[patch]`.
2. **Dev `[patch]` (development path).** Mirror the workspace-root `[patch."https://github.com/F1R3FLY-io/rholang-rs"]` block pointing the three crates at `../rholang-rs-cost-accounting-transpiler/`, + `git update-index --skip-worktree Cargo.lock`. Reverted before push.

**Invariant.** The parser rev is part of every normalized `Par`'s byte-identity (it determines the AST → the lowered `Par` → its hash). All validators MUST run the SAME parser rev; a `[patch]`-built node and a `rev`-built node could diverge if `parser.c` differs by one byte. Merge-gate: published rev pinned, `[patch]` absent, `Cargo.lock` restored, `cargo build -p rholang` green against the published rev.

**Build flag (carry verbatim).** `RUSTFLAGS="-C target-cpu=native"` is REQUIRED whenever the new parser is in the tree — it pulls `gxhash` (AES/SSE2 intrinsics).

## 3. The native wiring — how each surface construct maps to native metering

Governing principle: **surface forms decorate; they never re-emit metered operations.** Recognition resolves `s` to a native `Sig`; production funding comes from authenticated authority supply, and successful atomic matches are attributed to that authority. No user-level supply send or fuel gate is synthesized.

### 3.1 Reconciling `Σ⟦s⟧`: native `from_sig` wins
transpiler.md §2 flags that the transpiler domain-separates `DOMAIN_GROUND`/`DOMAIN_QUOTE` in the channel hash while native does NOT (DR-1). **Native `from_sig` wins:** it is consensus state — `supply_channel(sig) = SignatureChannel::from_sig(sig).par`, and `Sig::lane_hash` is the domain-separated Blake2b256 of that channel encoding; purse inventory, the WD-D2 gate, replay, and native authority evidence are all anchored to it (`supply_channel_matches_canonical_purse_identity`). A separate channel hash would fork the basis and break play/replay byte-identity. The transpiler's separation was benign only because Part B targets a non-metering reducer; on native, byte-parity with `from_sig` is mandatory. So W1's `sig.rs` keeps `canon_*` (content bytes), maps `ir::Sig → accounting::Sig` (§3.2), and derives the channel via native `from_sig`.

### 3.2 The two `Sig` algebras — bridge `ir::Sig → accounting::Sig`

| transpiler `ir::Sig` | native `accounting::Sig` | bridge |
|---|---|---|
| `Ground(content)` | `Ground(content)` | identity on bytes (`canon_ground(name)`) |
| `Quote(content)` | `Quote(content)` | identity on bytes (`canon_quote(P)`) |
| `Compound(vec)` | left-assoc `And(Box, Box)` fold | fold sorted atoms into `And` (matches `fold_compound_sig`; `from_sig`'s `And` arm is sort-matched ⇒ AC holds) |
| `Bound(content)` | **no native variant** | DECISION below |

**The `Bound` problem (load-bearing).** Native `Sig` has no ring-fenced variant. **Recommended (no enum change):** map `ir::Sig::Bound(span_bytes)` to `accounting::Sig::Ground(DOMAIN_BOUND ‖ span_bytes)` — fold the bound-domain separator + binder span INTO the ground content bytes. Then native `from_sig` produces a distinct `GPrivate` channel (bytes differ from any free sig's `canon_ground(name)`), so ring-fencing holds intrinsically via content-addressing, with ZERO consensus-surface change. Disjointness reduces to "`DOMAIN_BOUND`-prefixed span bytes never equal a `canon_ground` `Par` wire encoding" — provable by construction; assert in a test. **Rejected for W1:** adding `Sig::Bound` to the native enum (touches `to_proto`/`from_proto`/`from_sig`/`lane_hash`/wire + Rocq/TLA+ — too consensus-risky for surface syntax). The bridge lives in `cost_accounting/sig.rs` as `signature_to_native_sig`.

### 3.3 `{% P %}[s]`, cons-tokens, and the located-stack attribution
The historical native gap was that normalized `Par` carried no per-layer
signature and `metering.rs` stamped `sig_hash = budget.signature().lane_hash()`
once. W1 closed it; the current wire form retains `CostAuthority` regions.
1. **Recognition (normalizer).** `Proc::SignedTerm { proc: P, sig: s }` resolves `s` to a native `Sig` and retains the region in the normalized `Par`. RSpace combines the persisted datum and continuation authorities at each atomic COMM, and the resulting `AuthorityEvent` names the exact purse keys used by physical settlement. Per §D2.9 a signer purse is keyed by `funding_sig = Ground(pk)`, while a runtime-bound unforgeable slot retains its own key; the wire-signature `Quote(hash(wire_sig))` remains solely the `deploy_id` basis. `AuthorityByteEvent` carries the same region into quantitative-byte settlement. No synthetic fuel gate or second lane ledger exists.
2. **`delta_sigma::demand` extension (located stacks, P14).** The analyzer projects potential send/receive introductions per `SigKey`. It is a conservative structural proof for closed non-persistent terms, not an event-for-event runtime dual. Exact per-lane realized demand comes from state-bound atomic-COMM evidence.
3. **Cost = per COMM, ONE consumable (Greg's model).** No `Pay(τ)` second token (it is a type). The system token is the supply unit on `Σ⟦s⟧`; phlogiston is the degenerate single-lane case. User cons-tokens `S ::= () | S(x, s :: S)` are signed and desugar to system-token provisioning on `Σ⟦s⟧`, the signature tracking origin. Users decorate with `{% %}[s]`; they cannot mint system tokens (`Σ⟦s⟧` is unforgeable `GPrivate`, §5).

### 3.4 Compound `s1 (*) s2` and balanced multi-sig cost (P8)
A compound resolves to native `Sig::And(s1, s2)`; its channel is the sort-matched union (`from_sig` `And` arm), permutation-invariant (commutative ∘, P8). Per §D2.9 the component pools the `And`-fold draws from are keyed by `funding_sig = Ground(pkᵢ)` — the cosigners' genesis-seeded wallets `Σ⟦Ground(pkᵢ)⟧`, NOT the wire-sig envelope — so the compound funds from `effectiveΣ_compound = Σ⟦And(…)⟧(absent ⇒ 0) + min(Σ⟦left⟧, Σ⟦right⟧)` over those wallet pools. Cost is apportioned by `compute_settlement_debits` + `ApportionmentPolicy` (`resource_logic.rs`). **RESOLVED (task #12, Greg P8):** the committed `DefaultApportionment` ALREADY realizes Greg's "balanced cost per wallet" — it debits the component pair `(left, right)` the SAME `draw_pair` each, so every cosigner wallet pays an equal share, order-independently (commutative). No `BalancedApportionment` replacement is needed; the combined-pool-first step is the orthogonal joint-funds policy (`Σ⟦s₁∘s₂⟧` is co-owned). W1 reuses this verbatim. Split/Join interchange (combined-cell `a (*) b :: ()` vs separate stacks `a :: () | b :: ()`) uses native `effective_supply_with`/`split_join_decompositions`, NOT the transpiler `infra.rs` splitter. Phase 1: separate-stack funding (R2/R4) in scope; combined-cell (R3/R5) via the native Split/Join closure.

## 4. The join-sequential-fuel rule — where it is enforced
Greg's rule: a token-gated receive must acquire fuel via SEQUENTIAL nested single-channel receives, NEVER fold fuel tokens into a data join (RSpace join-matching is combinatorial in arity; an n-clause join → 2n-way is "extremely slow"). Valid because the tokens are independent (∘ commutative, no double-spend per token).

![Token-gated joins: sequential fuel (left, valid) versus folding fuel into the data join (right, rejected). On the left, a signed n-clause join acquires fuel through SEQUENTIAL nested single-channel fuel gates (each an O(1) match on Σ⟦sᵢ⟧) wrapping the natural-arity data join, then the continuation P (not re-signed) — linear in n, each clause attributed to its own lane. On the right, folding the n fuel tokens INTO the data join produces a 2n-way join; because RSpace join-matching is combinatorial in arity, the candidate cross-product explodes. The shared note records Greg's rule: never fold fuel into a data join; the sequential form is valid because ∘ is commutative and no-double-spend is per-token (compute_settlement_debits charges each pool independently).](../diagrams/token-gated-join-comparison.svg)

(*Source: [`diagrams/token-gated-join-comparison.d2`](../diagrams/token-gated-join-comparison.d2) — render with `d2 --layout elk docs/theory/diagrams/token-gated-join-comparison.d2 docs/theory/diagrams/token-gated-join-comparison.svg` (or `./render.sh token-gated-join-comparison.d2`).*)

Enforcement:
1. **Recognition never builds a fuel+data join.** W1's recognition emits NO extra `for`; fuel is provisioned on `Σ⟦s⟧` and the data `for` is metered per-COMM by lane, so a fuel token is structurally incapable of entering the data join's `ReceiveBind` set. ASSERT a debug invariant that the data `for`'s `binds` contains only the original linear clauses (mirroring `normalize_p_input`'s `debug_assert!(… no Bind::Signed …)`).
2. **Per-clause attribution is per-lane independent.** Each `Bind::Signed { sig: s_i }` clause attributes its rendezvous COMM to `s_i`'s OWN lane; `compute_settlement_debits` charges per-pool independently. The continuation `P` is NOT re-signed (one token per clause).
3. **Documentation anchor.** A code comment at the signed-join entry point in `normalize.rs` and `cost_accounting/signed_term.rs` MUST cite this rule (Greg 2026-06-15) so a future refactor doesn't "optimize" fuel into the join.

## 5. Security invariant — protocol `Σ` pools stay unforgeable
Invariant: an in-term ground signature `g` written by a user MUST NOT alias a protocol-controlled system pool `Σ⟦v⟧`. It holds on native because protocol/funding pools are keyed by `from_sig(funding_sig) = from_sig(Sig::Ground(pk))` → the genesis-seeded wallet channel `Σ⟦Ground(pk)⟧` (§D2.9, the funding-key correction; cross-ref `wd-d2-acceptance-gate.md` §D2.9 + `d2-9-funding-flow.md`). `Quote(Blake2b256(DEPLOY_SIGNATURE_DOMAIN ‖ wire_sig))` survives ONLY as the (unchanged) `deploy_id` basis — it no longer keys any pool. The ONLY writer of a supply-balance datum is the Rust `supply.rs` module on a `GSysAuthToken`-bearing system deploy (no bytes→`GPrivate` surface primitive, DR-13). A user's `g` resolves to `Sig::Ground(canon_ground("g"))` (a sort-matched `GString` wire form). **NEW no-alias obligation introduced by §D2.9:** because a signer's `funding_sig` is now itself a `Sig::Ground(pk)` (not a `Quote`), a user in-term ground sig CAN in principle collide with a signer's funding key iff `canon_ground("g")` bytes equal a signer `pk` bytes (both flow through the SAME `from_sig(Ground(·))` arm, DR-1 no separator). So the no-alias audit must ADDITIONALLY assert user-`g` bytes ≠ any signer `pk` bytes (a `canon_ground` wire-encoded `GString` never equals a raw 32/33-byte Ed25519/secp256k1 pubkey by leading-byte/length construction — assert it directly). **AUDIT TEST (deliverable):** for representative funding sigs `v` (single `Ground(pk)` + compound `And`-fold of `Ground(pkᵢ)`) and a fuzzed/adversarial set of user surface ground/`#P`/compound sigs, assert `from_sig(user_sig).par != from_sig(funding_sig(v)).par` and `user_sig.lane_hash() != funding_sig(v).lane_hash()` for all pairs, AND user-`g` bytes ≠ any signer `pk` bytes; plus assert the recognition path never emits a `produce_balance`-style write on a channel that decodes to a system / signer wallet pool.

## 6. Phasing + test strategy
- **Phase 0 — parser pin.** Dev `[patch]` (workspace-root `Cargo.toml`) → `../rholang-rs-cost-accounting-transpiler/{rholang-parser,rholang-tree-sitter,rholang-tree-sitter-proc-macro}`; `git update-index --skip-worktree Cargo.lock`. Verify `cargo build -p rholang` sees `Proc::SignedTerm`/`TokenStack`/`Bind::Signed`. Merge-gate: publish rev + bump pin (§2 path 1).
- **Phase 1 — surface recognition.** Port `ir.rs`/`sig.rs`/`desugar.rs`/`pattern_guard.rs`; add `cost_accounting/mod.rs` (recognition entry points, no `LowerToPar`); re-do the five wiring edits. End state: surface syntax PARSES + is RECOGNIZED + pattern-rejected; signatures resolve to native `Sig`; attribution may still collapse to the envelope (no behavior change yet). Tests: ported `signature_to_ir`/`canon_*`; pattern-rejection; `ir_to_native` AC/commutativity against `from_sig`.
- **Phase 2 — native sig-resolution + channel reconciliation.** `signature_to_native_sig`, the `Bound → Ground(DOMAIN_BOUND‖span)` fold, channel via `from_sig`. Tests: ring-fence trio (free vs `new`-bound → distinct channels; free shared sig → same channel); the §5 security audit test.
- **Phase 3 — demand attribution (located stacks, P14).** Project structural potential per `SigKey`, and attribute realized events at the atomic RSpace boundary. Tests distinguish the conservative structural bound from exact state-bound runtime evidence and check OSLF conformance per lane.
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
1. **Parser-rev skew (consensus).** A `[patch]`-built node vs a published-rev node could produce divergent normal forms. Merge-gate: published rev pinned, `[patch]` absent, lock restored, green build; the required Lint job executes the parser-pin invariant guard and its patch/skew/missing-package rejection regressions.
2. **Two `Sig` algebras drift.** A conformance test asserts `from_sig(to_native(ir_sig))` agrees with the intended channel for every constructor, and `Bound` channels are disjoint from all `Ground` channels by construction.
3. **Semantic distortion if any Part-B leaks in.** §1 drops `lower.rs`, `build_gates`, token send-chains, and `infra.rs`. Full normalized-`Par` equality proves recognition adds no process behavior; atomic-COMM tests independently pin runtime cost.
4. **Replay byte-identity for new supply writes.** Any new `Σ⟦s⟧` provisioning is consensus state; route through the existing replay-stable `random_state` family (anchored to the close-block deploy `initial_rand`, fresh disjoint `RNG_PATH`). Prefer NO new write paths in the normalizer (keep provisioning in the Workstream-C producer).
5. **`metering.rs` sig_hash generalization (hot path).** Keep the envelope as default `sig_hash`; only OVERRIDE for COMMs inside a recognized signed region / on a `Σ⟦s⟧` channel. Single-sig non-cost deploys take the unchanged path (lanes empty, byte-identical — `legacy_single_sig_byte_identical`). Add a regression.
6. **`delta_sigma` over-approximation interaction.** Carry the `unknown`/Thm-20 flag per lane; conformance `law_sound` two-regime runs per lane.

## 7b. Red-team findings (2026-06-15) + resolutions — READ BEFORE IMPLEMENTING

An adversarial red-team pass (skeptic posture, empirically probed) found one BLOCKER and four MAJOR issues that MUST be resolved in this design before any code. Recorded with their resolutions:

- **BLOCKER-1 — in-term `s` has no native funding/settlement backing.** Native consensus funding is keyed EXCLUSIVELY on the deploy signer(s) — post-§D2.9 by `funding_sig = Sig::Ground(pk)` (single) / the `And`-fold of `Ground(pkᵢ)` (multi-sig), the signer's genesis-seeded wallet (`acceptance.rs::admit_by_funding` groups/reads/debits pools by `accounting::funding_sig`; `delta_sigma.rs` says `Δ_s = 0` for any `s ≠ a signer`; `supply.rs` DR-13: `Σ⟦s⟧` is written ONLY by Rust on a `GSysAuthToken` system deploy, "unnameable from Rholang"). So an arbitrary in-term `{% P %}[s]` signature has NO pool the gate funds or the close-block debits, and §3.3.1's "head layer mints one unit on `Σ⟦s⟧`" assumed a user-deploy write path DR-13 forbids. **RESOLUTION (aligns with Greg + simplifies W1):** an in-term `s` MUST resolve to an **envelope signer** — the cons-token is *signed by the user* (Greg: "user-provided tokens are signed by that user so they can be tracked to their origin"), so its signature IS a deploy signer / cosigner. Located-stack attribution therefore routes a COMM to that **signer's** pool: the signer's wallet pool `Σ⟦Ground(pk)⟧` (single-sig — all in-term `s` collapse to it, §D2.9) or a **compound component pool** `Σ⟦Ground(pkᵢ)⟧` (multi-sig — which native ALREADY funds via the `And`-fold of `Ground(pkᵢ)` + `compute_settlement_debits` component draws). **No new in-term pool-write path; DR-13 preserved.** An in-term `s` NOT among the deploy's signers is a malformed/unfundable deploy → rejected (the `is_funding_former`/`pattern_guard` analogue). Located stacks (P14) ARE realized — as attribution to signer pools — not as fresh per-term pools.
- **Consequence — multi-party = multiple deploys (re-scopes Phase 5).** The transpiler demo simulates many parties (`eve`/`Fab1`/`diSig`/…) via arbitrary in-term signatures in ONE program; native binds a deploy to its signer(s), so a multi-party ecosystem is MULTIPLE deploys (each signed by its party), not one program. The demo's PARK invariants (`eve`/`Zed`/thief never run) come from the transpiler's in-term gate-receive-blocks-without-a-token mechanic, which native (recognize-only, envelope-funded) does NOT reproduce (MAJOR-4). **Phase 5 re-scope:** assert only funded-path numeric conservation under signer-keyed funding; the PARK invariants become per-deploy admission-rejection tests (an unfunded signer's deploy is gate-rejected), NOT in-program parking. Drop "the demo is a drop-in native integration test."
- **MAJOR-2 — superseded by the atomic-COMM correction.** The earlier red-team note accurately diagnosed the then-current reducer implementation but not the publication's normative unit. Billing now occurs only for a committed RSpace match. Part B remains excluded because it adds behavior, not because introductions are themselves billable.
- **MAJOR-3 — resolved at the RSpace boundary.** Attribution no longer moves billing around `eval_send`/`eval_receive`. The observer receives the complete COMM, derives its stable identity, reserves before mutation, and applies the signer-channel projection after a successful reservation.
- **MAJOR-5 — the join "semantic equivalence" claim, RESOLVED by TLA+ and the atomic RSpace observer.** `TokenGatedJoin.tla` is retained as a historical translation-threat comparison: its M1 track proves atomic authority settlement and its M2 track intentionally refutes nested sequential fuel gates. It is not the native runtime-cost model. `AtomicCommAccounting.tla` proves that a successful N-way RSpace join costs one regardless of arity, while `AtomicCommRejection.tla` proves that exhausted reservation precedes event-log and tuplespace mutation. DR-31 then proves finite state-bound evidence, exact settlement, root-chain continuity, and replay refinement for persistent ambient contracts. **Net:** W1 must not lower fuel into nested receives; production observes one atomic COMM and settles its authenticated evidence.
- **MINOR-6 — channel disjointness (b) is SOUND but document the construction.** Empirically: `canon_ground("g")` begins `0x2a` (the protobuf `Par.exprs` field-5 tag); `DOMAIN_BOUND` begins `0x66` (`'f'`). Since native `from_sig(Ground(b))` hashes `b` with NO separator (DR-1), disjointness rests on these content-byte prefixes — incidental, not enforced. State the leading-byte/length construction in §3.2, assert it directly in the audit test, and pin a regression that `canon_ground` always emits the `0x2a` prefix (a future `canon_*` refactor could otherwise let a user name literally `f1r3fly.cost.sig.bound.v1:…` collide). Consider folding a structurally-impossible-as-Par-prefix tag into the `Bound→Ground` bridge content to make it enforced rather than accidental.
- **MINOR-7 — the `from_sig`-wins choice (c) silently collapses the ground/quote axis.** Native `from_sig` makes `Ground(b)|Quote(b)` byte-identical (DR-1), whereas the transpiler domain-separates them (its `sigma_ground_and_quote_axes_are_disjoint` asserts the opposite). Adopting `from_sig` is correct for consensus byte-identity but is a BEHAVIORAL divergence for any surface program relying on `g ≠ #P` separation — not "ZERO consensus-surface change." Note it in §3.1; add a test that a ground/quote pair of equal canon bytes is the SAME native channel.

**SOUND (survived attack):** the parser-rev consensus-fork facts + merge-gate (§2, §7 risk 1); the no-alias security argument (e); the `from_sig`-wins choice (c, modulo MINOR-7); P8 balanced apportionment; and the one-consumable token model. **Net:** BLOCKER-1's signer-keyed resolution stands; MAJOR-5 is resolved by `AtomicCommAccounting`, `AtomicCommRejection`, state-bound admission, and the retained negative translation control. No runtime fuel gates are introduced.

## 8. Critical files for implementation
- `rholang/src/rust/interpreter/compiler/normalize.rs` — three surface dispatch arms; recognition entry point.
- `rholang/src/rust/interpreter/accounting/delta_sigma.rs` — extend `demand` from envelope-only (s₀) to per-`Σ⟦s⟧`-lane attribution (P14); keep `effective_supply`/Split-Join.
- `rholang/src/rust/interpreter/metering.rs` — generalize the single `sig_hash = budget.signature().lane_hash()` so a gated COMM attributes to its gate signature's lane.
- `casper/src/rust/util/rholang/supply.rs` — the canonical `Σ⟦s⟧` = `from_sig` basis (§3.1); host of the §5 audit test + any replay-stable supply-write derivation.
- `rholang/src/rust/interpreter/accounting/resource_logic.rs` — the authoritative `OslfResourceLogic`/`ResourceSignature`/`ApportionmentPolicy` to reuse; balanced multi-sig (P8).
- New ported Part-A sources under `rholang/src/rust/interpreter/compiler/normalizer/cost_accounting/`: `ir.rs`, `sig.rs`, `desugar.rs`, `pattern_guard.rs`, `mod.rs` (+ re-targeted `signed_term.rs`/`token.rs` recognition, `tests.rs`).
