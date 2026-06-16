# Cost-Accounted Rho Calculus — Decision Records

**Status:** Implementation-aligned design record
**Date:** 2026-05-29
**Governing authority:** the specification `publications/cost-accounting/cost-accounted-rho.tex`
("Cost-Accounted Rho Calculus: A Spectral Decomposition of Phlogiston," May 2026) is the **law of the
implementation**. No deviation from its design is admitted unless a bug in the spec is *proven* to exist
and the correction *proven correct*. Two adversarial red-team rounds against the spec found **zero spec
bugs**; the implementation therefore conforms to the spec everywhere it dictates. Extensions *beyond*
what the spec dictates are permitted only when they (1) do not conflict with the spec, (2) introduce no
performance bottleneck, and (3) introduce no security vulnerability.

This document records the load-bearing decisions taken while realizing the spec, each with its rationale,
spec basis, and the alternatives considered (recorded for future reference). It complements
[cost-accounting-migration.md](cost-accounting-migration.md),
[cost-accounting-linear-logic.md](cost-accounting-linear-logic.md), and the
[verification companion](cost-accounted-rho-verification.md).

---

## DR-1 — Ground signature `g` vs cryptographic quote `#P` (split the conflated atom)

**Decision.** The core signature grammar realizes exactly `s(G) ::= g | #P | s∘s` (spec Def 3.3). The Rust
`Sig::Hash(bytes)` atom — which conflated a *ground* signature `g` with a *cryptographic quote* `#P` — is
split into `Ground(g)` and `Quote(#P)`; the wire `SigAtom` gains an `AtomKind{Ground=0,Quote=1}`
discriminant (default `0` for back-compatibility). The Rocq `sig` gains `SGround` (the `g` axis) and `SQuote`
(the `#P` axis), with linear-logic atom images `ASGround`/`ASQuote`.

**Spec basis.** Def 3.3, §4.2 (cryptographic quoting), Remark 2.6 ("two axes of reflection").

**Rationale.** The spec makes `g` (recoverable identity) and `#P` (one-way process hash) distinct sorts;
a single byte-bag cannot express the recoverable-vs-non-recoverable distinction.

**Consensus-safety note.** The `AtomKind` discriminant MUST be excluded from the hash preimage when
`kind=Ground`, so every pre-split deploy hashes byte-identically; guarded by a golden-vector test.

**Alternatives considered.** (a) Leave `Sig::Hash` conflated — rejected: fidelity gap vs Def 3.3.

---

## DR-2 — Signatures parametric over the cryptographic backend `G`

**Decision.** Parameterize the signature layer over a backend `G` exposing a decidable-equality predicate
and a hash function; add an OQS post-quantum backend (ML-DSA-65, FALCON-512, SLH-DSA) as a feature-gated,
off-by-default instantiation, plus hybrid classical+post-quantum multi-sig via compound signatures.

**Spec basis.** §4.5 (genericity over `G`; OQS named explicitly).

**Rationale.** The existing `SignaturesAlg` trait + factory + per-atom algorithm tag already *is* the
runtime encoding of "parametric over `G`"; the cost-accounting semantics are agnostic to `G` (the Rocq
atom is opaque), so a post-quantum migration requires no change to the cost semantics.

**Alternatives considered.** (a) A `trait Backend<G>` genericization of `SignaturesAlg` — rejected:
destabilizing churn for zero semantic gain; the dynamic-dispatch trait already suffices.

**Superseded in part by DR-16.** The `G`-parametricity decision stands (realized by the `SignaturesAlg` trait
+ factory). The specific OQS post-quantum *instantiation* named above was removed (DR-16): its upstream
`oqs-sys` dependency does not compile on the pinned toolchain. §4.5's requirement is the parametricity, not
the OQS instantiation, so the trait satisfies it; a post-quantum backend re-enters as a drop-in
`SignaturesAlg` impl (pure-Rust `ml-dsa`/`slh-dsa`, or `oqs` once a fixed `oqs-sys` ships).

---

## DR-3 — Two-effect slashing with redemption

**Decision.** Slashing has two effects: (i) remove all remaining phlogiston and halt further minting (the
validator's wallet bootstrap blocks → effectively offline); (ii) move stake to a private unforgeable
channel pending adjudication. A slashed validator may be redeemed (minting resumes next epoch); the
quarantined stake is then returned, partially redistributed, or burned.

**Spec basis.** Appendix B ("Slashing"; "the adjudication contract is itself a Rholang program").

**Rationale.** Realizes the spec's validator-economics model; supersedes the prior "bond→0 + immediate
Coop-vault transfer." The bug-fix safety theorems are independent of the prior framing and are preserved.

**Stage-B/C halt-interface refinement (Cost-Accounted Rho).** Effect (i) "remove all remaining phlogiston
and halt further minting" is realized as THREE supply-side writes: (a) drain `@W_v` (consume the resident
MakeMint purse ⇒ `VB` re-blocks ⇒ the DR-3 liveness halt); (b) insert the validator into the
`"mintingHalted"` PoS state key (the cross-epoch mint halt — the Stage-B `closeBlock` fold and the Rust
`CloseBlockDeploy::post_eval` recompute both skip `v ∈ mintingHalted`); and (c) **zero `Σ⟦v⟧`** via the
slash deploy's Rust `post_eval` calling `supply::produce_balance(from_sig(Ground(pk)), 0)` — the
spec-complete realization of "all remaining phlogiston is removed" (tex 3030-3033), idempotent, eliminating
the residual-funding edge case. Redemption (`redeemSlashed`, DR-7) writes NEITHER `Σ⟦v⟧` NOR `@W_v` directly:
it clears `mintingHalted` + removes stale `mintedEpochs (v, e≥current)` and lets the normal next-epoch mint
re-fund (all phlogiston creation stays on the single authorized path). Proved by `MintingHalt.v`
(`halted_validator_supply_not_increased`, `halted_validator_not_minted`) + `SlashFlow.tla` `Inv_HaltedNotMinted`.
Stage B EXPOSES the `mintingHalted` key + `supply::produce_balance`; Stage C consumes them. See
[cost-accounting-impl/stageb-minting-halt-interface.md](cost-accounting-impl/stageb-minting-halt-interface.md)
Decision 4.

**Alternatives considered.** (a) Keep the immediate Coop-vault transfer — rejected: the spec mandates a
private adjudication channel. (b) VB-block + `mintingHalted` only, no `Σ⟦v⟧` zero — sufficient for consensus
safety, rejected as the interface default (the explicit zero is spec-complete and edge-case-free).

---

## DR-4 — Fee conversion via a conserving `Exchange(c,v)` contract

**Decision.** Fees are converted through a Rholang `Exchange(c,v)` market-making contract: a **conserving
1:1 swap** that consumes one `c`-token and one `v`-token and re-emits one of each with swapped remainders
(extensible to variable rates / AMMs). The fee token is a client-signature token; converting it to
validator fuel is a market operation, not a mint. An empty-wallet validator is bootstrapped by **epoch
minting**, not by `Exchange`.

**Spec basis.** Appendix B ("Fee conversion": `Exchange(c,v) = for(t_c←n_c){for(t_v←n_v){ n_c!(*t_v) |
n_v!(*t_c) }}`, 1:1 peg).

**Rationale.** Conserves per-channel token count (provable); fees replenish phlogiston without minting.

**Alternatives considered.** (a) Direct fee→stake bond increase — rejected: not what the spec's `Exchange`
does (it requires both inputs); would also raise consensus weight over time (concentration risk).

**Realization (Stage D).** Implemented as three layers (design `staged-fee-exchange.md`):
1. The blessed **`Exchange.rhox`** (registered at `rho:lang:exchange`) is the spec's conserving 1:1 swap as
   a persistent JOIN over ordinary **carrier** channels (`exchange_conserves_per_channel` /
   `exchange_total_conserved` / `exchange_requires_both_inputs` in `Exchange.v`). It is genesis-wired exactly
   like `capabilities_registry` and is the acquisition mechanism #13 clients use.
2. The validator economic loop's fee→v conversion does NOT route through the blessed `Exchange` contract at
   runtime; it is the **Rust `supply::produce_balance` mirror** (`CloseBlockDeploy::post_eval`): the
   collected fee pool `F_v` is credited 1:1 into the gate pool `Σ⟦v⟧` (`Σ⟦v⟧ += f`, `F_v := 0`). Rationale:
   `Σ⟦v⟧`/`F_v` are unnameable from Rholang (DR-13), so the credit is a Rust write — the same dual-write
   discipline as the StageB mint. The 1:1 peg makes the Rust credit and the Exchange swap semantically
   identical; the Rocq `fee_convert_credit_is_backed` proves the `Σ⟦v⟧` credit is BACKED by (equal to) the
   drained fees, never a mint (DR-4: empty `F_v` ⇒ no credit).
3. **fee ≠ cost:** the `F_v` carve (the spec's flat `FeeExtract`, one client token per admitted deploy,
   realized under F-C/F-D as a supply-CONSERVING carve from the client's own `Σ⟦c⟧` into `F_v` — NOT a mint)
   is SEPARATE from the WD-D2 settlement debit (the burned COST). PoS owns only the conversion ELIGIBILITY
   (`active ∧ ¬mintingHalted ∧
   ¬convertedEpochs`) + `convertedEpochs` idempotency, publishing the eligible list on
   `sys:casper:feeConvertList`. **Settled (DR-14, user-ratified): the OD-4 `@W_v` mirror is unnecessary; `Σ⟦v⟧`-only is the permanent,
   spec-complete fee realization** — the convert credits the consensus pool `Σ⟦v⟧` only (the released form of
   the single spec phlo location); the `@W_v` purse *amount* is operationally inert under the s₀-collapse, so
   no `@W_v` fee-credit — and no Rust→PoS seam to perform it — is built. See DR-14.)

---

## DR-5 — Remove precharge/refund; deploys draw from the wallet

**Decision.** Remove the per-deploy precharge/refund machinery (`PreChargeDeploy`/`RefundDeploy`, PoS
`chargeDeploy`/`refundDeploy`, the runtime fan-out). Deploys draw phlogiston from the per-validator wallet;
the acceptance gate commits tokens linearly at acceptance.

**Spec basis.** §7.6 (acceptance commits resources; "no tokens are consumed" on rejection).

**Rationale.** The acceptance-by-linear-proof model makes escrow precharge/refund unnecessary.

**Alternatives considered.** (a) Keep escrow alongside the wallet — rejected: redundant; not the spec's model.

---

## DR-6 — Deployment: fresh-genesis / new shards only

**Decision.** The new model is deployed on **fresh-genesis / new shards only**. Existing chains keep the
legacy model; wire-format and `ProofOfStake` genesis-format changes are therefore unconstrained.

**Spec basis.** (Deployment is outside the spec's scope; this is an operational decision.)

**Rationale.** Cleanest path — no dual-model historical replay, no migration of historical state; matches
the magnitude of the architecture change.

**Alternatives considered.** (a) Hard-fork at an activation height (retain dual code paths for historical
replay); (b) versioned dual-model maintained indefinitely — both rejected as heavier with no benefit for
the intended greenfield deployment.

---

## DR-7 — Slashing adjudication / redemption authority = PoS multisig

**Decision.** Adjudication of quarantined stake and triggering of redemption are authorized by the
**existing PoS multisig governance** (`posMultiSigPublicKeys` + `posMultiSigQuorum`), via a
multisig-authorized system deploy.

**Spec basis.** Appendix B ("arrangement with the shard"; adjudication is a Rholang contract) — the spec
leaves the *authority model* unspecified, so this is a permitted in-scope decision.

**Rationale.** Reuses audited governance with the least new attack surface.

**Alternatives considered (recorded per the standing request to enumerate options).**
- **(b) Stake-weighted validator vote** — the active set votes (weighted by stake) on adjudication
  outcomes, as a Rholang contract (the spec mentions stake-weighted voting is expressible). More
  decentralized; larger attack surface and complexity.
- **(c) Dedicated governance / DAO contract** — a standalone proposals/voting/timelock contract owns
  adjudication. Most flexible and future-proof; largest new design + verification surface.

These remain available as future refinements; the implementation begins with (a).

---

## DR-8 — Remove the Rust↔Scala bisimilarity theorems

**Decision.** Remove the Rust↔Scala bisimilarity development: `formal/rocq/slashing/theories/Bisimulation.v`,
the T-13/14/15 components in `MainTheorem.v`, the corresponding Rust property tests, the build-manifest
entries, and the bisimilarity sections in the slashing docs. The headline `main_slashing_algorithm_correct`
and all T-1..T-12 / T-9.x bug-fix safety theorems are preserved (they are independent of the bisimilarity).
The `cost_accounted_rho/Bisimulation.v` (the §5 s₀-limit conservative-extension result) and
`MergeableChannelAccounting.v` are KEPT.

**Spec basis.** (The bisimilarity related Rust to a Scala implementation; not a spec concept.)

**Rationale.** The migration to the cost-accounted architecture makes the Rust and Scala slashing
implementations no longer comparable; the bisimilarity's bug-finding purpose is complete. Git preserves
the history.

**Alternatives considered.** (a) Re-scope the bisimilarity to the spec's model — rejected by the user:
the architectures are no longer comparable, so a Rust↔Scala bisimilarity is vacuous.

---

## DR-9 — Cost model: enforce token-per-COMM; per-operation gas is diagnostic only

**Decision.** The spec **replaces phlogiston with tokens** (the §4.6 spectral decomposition), so the
implementation replaces the singular-phlo gas model with signature-indexed token consumption enforced
**token-per-COMM** (Rules 1–5; §3.6). The acceptance gate (§7) is the sole enforcing cost authority.
`DeployData.phlo_limit`/`phlo_price` (singular escrow) are removed in favor of signature-indexed token
supply; the per-operation gas table (`costs.rs`) is **retained only as diagnostic telemetry**
(non-consensus), extending the TM-CA-151 direction. "Phlogiston" persists as the *name* of the renewable
validator resource, now realized as tokens.

**Spec basis.** §3.6 (token-gated rules), §4.6 (spectrum / "phlogiston as a limit case"), §7.2 (rendezvous
= one token, matching = a second).

**Rationale.** The spec meters at COMM/matching granularity, not per-operation; keeping the per-op gas as
the *enforcing* model would create a currency mismatch (an accepted deploy could exhaust its op-budget
mid-execution, re-introducing the partial-funding §7 eliminates). Demoting per-op gas to diagnostic
resolves the mismatch while preserving the verified per-op machinery as telemetry.

**Alternatives considered.**
- **(b) Keep per-op gas enforcing + prove a bridging lemma** that gate-acceptance implies sufficient
  op-budget — rejected: requires bounding per-COMM op-cost, which the spec does not model.
- **(c) Two independent resources** (token gate + separate op-budget) — rejected: the spec has a single
  resource; this would be a deviation by addition.

---

## DR-10 — Out-of-spec ILLE signature connectives: kept wired as a documented extension

**Decision.** The repo's 9-connective ILLE signature algebra (Threshold/Plus/With/Bang/WhyNot/Lolly beyond
the spec's `g|#P|s∘s`) is **kept wired** (proto + Rocq) and **documented as an out-of-spec extension**. The
spec **core** realizes exactly `g|#P|s∘s`; `⊸` is **sugar** (§3.8), which coexists with the `Sig::Lolly`
extension connective.

**Spec basis.** §3.3 (core grammar), §3.8 (`⊸` is sugar). Extensions are permitted under the standing
three-guard rule.

**Extension obligations (must be discharged).** (1) **No spec conflict** — a Rocq lemma that core
`g|#P|s∘s` terms reduce/cost identically whether or not the extension is present. (2) **No performance
bottleneck** — the extra connectives never appear on the per-COMM hot path for core deploys (the N=1
scalar fast-path is untouched; confirmed by benchmark). (3) **No security vulnerability** — the extension
cannot enable unauthorized capability amplification or bypass `sysAuthToken`/the acceptance gate (threat-model
rows + Sage adversarial search).

**Alternatives considered.** (a) Segregate the connectives behind a feature gate, removed from the core
wire type; (b) delete them entirely — both rejected by the user in favor of keeping them wired as a
documented extension (the proven work is preserved; the core remains spec-faithful).

---

## DR-11 — Concurrent acceptance: per-signature static linear-proof gate at block assembly

**Decision.** Replace run-to-completion with concurrent acceptance gated by a static linear-resource proof.
A new static analyzer computes per-signature token demand `Δ_s` (over the **fully-desugared** AST, counting
`{·}_σ` layers by whole-signature value per Def 7.4, with Split/Join closure for split-vs-combined
granularity) and supply `Σ_s` (token messages resident on the signature channel `Σ⟦s⟧`). Admission is a
**per-signature-group batch fold at block assembly** (`prepare_user_deploys`) — no global lock, no global
barrier (§7.6 "no interleaving" is per-signature). Un-analyzable (higher-order/`*x`) demand is **rejected**
unless `effectiveΣ_s ≥ knownLowerBound_s + margin`, with the margin + resolution algorithm pinned as
shard-genesis constants and recomputed in replay. Execution-on-receipt is **speculative**, discarded
unless the deploy survives the block gate (I/O sinks gated on "committed"). The per-signature token pool is
a `DashMap<Sig, AtomicI64>` so disjoint signatures have zero cross-signature contention; the scalar
fast-path is retained for the common single-signature deploy.

**Spec basis.** §2 (concurrent acceptance), §7.4 (desugar-then-count), §7.5 (decidability + over-approximation
+ safety margin), §7.6 (acceptance protocol), §7.7 (deployment boundaries; simultaneous-arrival =
parallel-composition `Δ`).

**Rationale.** Realizes the spec's "is this deployment funded?" budgeting model; eliminates the
run-to-completion lock and most merge analysis (only channel-based shared-data-channel reconciliation
remains, per §2.3).

**Alternatives considered.** (a) Accept-then-runtime-backstop (admit un-analyzable deploys and rely on a
runtime counter) — rejected: §7.5/§7.6/§7.7 mandate rejection at the gate before execution; admitting
un-analyzable deploys re-introduces the partial-funding the spec eliminates.

---

## DR-12 — Validator lifted into Rholang with a multi-prover behavioral contract

**Decision.** Validator *decisions* (accept-gate, slash decision, epoch minting, voting/redemption) are
lifted into Rholang so customers can supply custom validators; the Rust node shell retains P2P/TLS, LMDB
storage, the reducer/RSpace engine, equivocation detection, the slash-authorization predicate, the
finalization oracle, and replay. Custom validators satisfy a formally-specified behavioral contract whose
**spec obligations** are exactly §6.3 (block well-typed in the cost-accounted grammar + token stacks
present for every signed communication) + §7.6 (accept iff `Σ_s ≥ Δ_s`) + §7.7 (linear no-double-spend) +
the §7.1 transaction mapping; **platform obligations** (slash-authorization correctness, finalization
safety, determinism/replay) are labeled out-of-spec. The contract is **multi-prover**: a TLA+ model plus a
proof-obligation set with Rocq **and** Lean backends; a custom validator ships TLA+ + Rocq or Lean; the
built-in validator is proven in all three. Lean is scoped to the validator obligation set (not the whole
corpus) and staged behind Rocq.

**Spec basis.** §6.3 (syntactic block-validity), §6.4 (validators/slashing/minting/redemption/voting as
Rholang contracts; Lean anticipated), §7 (acceptance/atomicity).

**Rationale.** Unifies the consensus and execution layers under one formal semantics; lets customers
implement custom validators without loss of performance, behind verifiable obligations.

**Alternatives considered.** (a) A minimal (informal) entry-point contract — rejected by the user in favor
of a richer, formally-specified, multi-prover contract; (b) default-validator-first, framework-later —
folded in as the staging order (the built-in is proven first as the reference).

---

## DR-13 — Per-signature supply is a balance datum on `Σ⟦s⟧ = from_sig(s)`, committed at acceptance

**Decision.** Token supply for a signature `s` is a **single balance-carrying datum** `(TOKEN_TAG, n)` on the
unforgeable channel `SignatureChannel::from_sig(s)` (`Σ⟦s⟧`); `supply(s) = n` (0 if absent). It is written
**only** by the Rust `sysAuthToken`-gated mint/settlement path (`produce_balance`), never from Rholang. The
acceptance gate (DR-11) reads `Σ_s` in O(1) from the merged pre-state and commits `Δ_s` by decrementing an
in-pass residual at block assembly; settlement writes `post = pre − ΣΔ_admitted`. The per-COMM token unit
(DR-9) is diagnostic and yields `reconcile().consumed = Δ_s`. The validator's *draw* channel `@W_v` (spec
Appendix B) is DISTINCT from the *supply pool* `Σ⟦v⟧`.

**Spec basis.** §4.6 (per-`s` pool), Appendix A (`Σ⟦·⟧`, `K⟦s:S⟧ = send(Σ⟦s⟧, K⟦S⟧)`), Def 17 (`Σ_s` is a
layer COUNT), §7.6 (compute `Σ` then accept), tex 1677-1729 (tokens *committed* at acceptance; "no
interleaving of acceptance and execution"), Remark 11 (the s₀-collapse lifts the per-COMM gate to static
analysis).

**Rationale.** The balance is the spec's `Σ_s` count expressed in the runtime's existing normal form
(`Token::Count{sig,remaining}`, accounting/mod.rs:1156-1164). A literal-message representation is O(n) per
gate read and bottlenecks block assembly (extension-guard #2). `from_sig`'s unnameability in Rholang (no
bytes→GPrivate surface primitive) makes supply unforgeable (extension-guard #3). Full design + formal
obligations: [cost-accounting-impl/supply-realization-c-d-handoff.md](cost-accounting-impl/supply-realization-c-d-handoff.md).

**Producer-seam note (LANDED, Stage B).** The supply PRODUCER is `CloseBlockDeploy::post_eval` (a default-no-op
`SystemDeployTrait::post_eval` hook invoked symmetrically in `RuntimeOps::play_system_deploy` and
`ReplayRuntimeOps::replay_block_system_deploy`), with the helpers in
`casper/src/rust/util/rholang/supply.rs` (`TOKEN_TAG="phlo"`, `supply_channel`, `decode_balance_datum`,
`read_balance`, `produce_balance`). `produce_balance` is consume-existing-then-produce-new (single datum;
`checked_add` overflow → `.expect("phlogiston supply overflow")`). The mint set is recomputed identically on
play and replay because both re-run the same `closeBlock` fold, which publishes the `[(pk, amount)]` mint list
onto a Rust-known, user-unforgeable env channel (`sys:casper:mintList`) that `post_eval` reads (the grounding
adaptation, since Rust cannot name the pre-`closeBlock` PoS `stateCh`). Replay adds the `ReplaySupplyMismatch`
write-readback guard. The consensus-critical play/replay symmetry is exercised by
`close_block_supply_mint_is_play_replay_deterministic`. Full design:
[cost-accounting-impl/stageb-minting-halt-interface.md](cost-accounting-impl/stageb-minting-halt-interface.md).

**Fee-seam note (LANDED, Stage D).** The Stage-D FEE writes ride the SAME authorized `post_eval` write seam
as the StageB mint, with a THIRD per-validator content-addressed pool: `F_v =
supply::fee_collection_channel(pk)` (a `(TOKEN_TAG, n)` balance keyed by `Blake2b256(FEE_COLLECTION_DOMAIN ‖
pk)` — domain-separated from `Σ⟦v⟧` and from `@W_v`, all three DISTINCT). Like `Σ⟦v⟧`, `F_v` is
reducer-unwritable and written ONLY by Rust `produce_balance`. `CloseBlockDeploy::post_eval`/`post_eval_replay`
gain two phases after the mint + settlement: (3) CONSERVING CARVE (F-C/F-D) — for each admitted client pool,
CARVE the flat `FeeExtract` (one token per admitted deploy) from the client's OWN post-cost `Σ⟦c⟧`
(`Σ⟦c⟧ -= fee`) and credit the carved total to `F_v(proposer) += Σ fee` (a supply-conserving transfer, NOT a
mint — clients debited == `F_v` credited, Rocq `fee_collect_conserves`). The per-client carve is threaded
play-side via `fee_carve` (`AdmissionOutcome.fee_debits`) and recomputed replay-side from the block by
`recompute_settlement_debits(block.body.deploys, …).fee` — the SAME recompute-from-block discipline +
`compute_settlement_debits` apportionment as the cost settlement debit; (3b) CONVERSION — read the eligible
`[(v, epochIdx)]` list PoS published on `sys:casper:feeConvertList`, and
for each eligible `v` credit `Σ⟦v⟧ += f` and zero `F_v` (`f = read F_v(v)`; `f ≤ 0 ⇒ skip`, DR-4). Disjoint
replay-stable `random_state` paths (`fee_carve_random_state` `-0x30`, `fee_collect_random_state` `-0x2e`,
`fee_convert_random_state` `-0x2d`, disjoint from mint `lo≥0` / debit `-0x2b` / slash `-0x2c` / mint-list
`0x2a`) + the `ReplaySupplyMismatch` readback guard on every fee write. The cost ≠ fee separation holds: the
fee is a transferred token (`Σ⟦c⟧ → F_v`, conserving), the cost is the burned settlement debit on `Σ⟦s⟧`.
Play/replay symmetry exercised by
`fee_collection_and_convert_is_play_replay_deterministic` + `fee_convert_converted_epochs_idempotent_deterministic`.

**Alternatives considered.** (a) literal nested-send messages, one per token — rejected (O(n) gate-read
bottleneck); (b) a Rust-injected supply name `@sigSupplyCh` bound into `VB`'s continuation — rejected
(re-exposes `Σ⟦v⟧` to the Rholang layer, enlarging the trusted surface); (c) a `sysAuthToken`-gated
`sigChannelOps` system process resolving sig→channel — recorded as a future refinement for in-Rholang
minting contracts (ERC-20-style), unnecessary while the only authorized writer is Rust.

---

## DR-14 — `Σ⟦v⟧`-only fee realization is permanent and spec-complete (the `@W_v` fee-mirror is unnecessary)

**Decision.** Stage D's fee→phlogiston conversion credits the per-signature supply pool `Σ⟦v⟧` ONLY (the
load-bearing, gate-read pool). It does **not** credit the validator's `@W_v` draw wallet with the converted
fee amount (the "OD-4 `@W_v` mirror"), and the project will **not** build the proposed `rho:casper:feeCount`
Rust→PoS pre-eval data seam to do so. `Σ⟦v⟧`-only is the permanent, spec-complete realization of the spec's
fee feedback loop. (User-ratified after an independent second-opinion Plan-agent review.)

**Spec basis.** The spec has a SINGLE phlogiston location — the wallet `\quot{W_v}` holding a token stack
(tex:2389-2392) — and `Σ⟦v⟧` (the spec's `n_v`) is the *released form* of that stack ("a token stack becomes
a chain of sends … on the signature channel", tex:1906; released by `\drop{t}`, tex:1965). So "fees can be
converted to replenish the phlogiston supply" (tex:3097-3098) is satisfied the moment `Σ⟦v⟧` is credited —
`Σ⟦v⟧` *is* the supply. Under the adopted s₀-collapse (Remark 11, tex:1063-1071; §5; §6.4 block-validity is a
*presence* predicate) the static acceptance gate (DR-11) against `Σ⟦v⟧` is the operative funding check.

**Rationale.** (1) *No-op:* the `@W_v` purse *amount* is read by nothing — every consumer reads presence, not
quantity (VB `for(phlo<=@W_v){*phlo}` drops it with VH=nil; slash `for(_<-@W_v){Nil}` discards it; no
`getBalance`/arithmetic on a `@W_v` purse exists). Crediting `@W_v` with the fee amount changes no
consensus-observable state. (2) *Safety:* a Rust→PoS pre-eval seam to feed the fee count `f` into the Rholang
`closeBlock` would re-introduce the DR-13 alternative-(b)-rejected Rholang-exposure of a Rust economic
quantity plus a standing replay-rig fragility (the seed `produce` double-counts the rigged play event log →
`ConsumeFailed`), on the most consensus-critical path — all to perform a no-op. (3) *Performance:* `Σ⟦v⟧`-only
is the landed code (zero new work, no new `RwLock`/system-process read, no contention). `@W_v` presence (the
DR-3 halt anchor) continues to be maintained by the epoch mint.

**When it would matter (and why it does not now).** The `@W_v` amount-mirror would be load-bearing ONLY under
the spec's *literal* per-COMM measured-`VB`-draw model (where `@W_v`'s amount gates each draw). DR-11 rejected
that model on O(n)-gate-read performance grounds in favor of the s₀-collapse. So the mirror is contingent on
reverting a committed, spec-sanctioned decision — not a current obligation.

**Alternatives considered.** (a) *`Σ⟦v⟧`-only, permanent* — CHOSEN. (b′) a Rust-side fixed *presence* top-up of
`@W_v` (no `f`, riding the existing `post_eval` seam) — viable if a literal "wallet replenished" artifact is
ever demanded, but still a consensus no-op; subsumed by (a). (c) a PoS-state fee accumulator — rejected
(duplicate `f` ledger, two sources of truth, merge-drift risk). (d) the `rho:casper:feeCount` Rust→PoS
pre-eval seam — rejected (over-engineers a no-op; re-introduces a rejected coupling + replay fragility).

---

## DR-15 — Run-to-completion was already eliminated in the Rust port; D4.3 reinterpreted (the multi-parent merge dispatcher is retained)

**Status.** Settled (Workstream D, D4.2/D4.3). **Spec law:** `cost-accounted-rho.tex` §2.1–§2.3 (tex:196–320).

**Context.** The master plan's Wave-2 listed three parallel removals — precharge/refund (D4.1), "RtC-driven
speculative-merge orchestration" (D4.2), and "run-to-completion callers" (D4.3). D4.1 landed (see DR-5).
D4.2/D4.3 were specified against an inaccurate model of the current merge code. Grounding them in the Rust
source (verified file:line + spec-line evidence) shows the removals they name **do not exist as such** in
`f1r3node-rust`: the port is already the spec's §2.3 channel-based model.

**Decision.**
1. **Run-to-completion (spec §2.1, tex:196–227)** — the legacy RChain/Scala "execute one deployment to
   termination, commit, then accept the next" serialization — **was never ported.** The reducer already runs
   intra-deploy with per-channel locks (`rspace.rs` `phase_a/b_locks`), and the multi-parent merge
   (`dag_merger::merge`) operates entirely on pre-computed event-log diffs (`DeployChainIndex` /
   `NumberChannelsDiff`); it **never re-executes deploys.** The §2.3 replacement — acceptance by linear proof
   — is live (`block_creator.rs` `acceptance::admit_by_funding` + the pure `delta_sigma.rs` gate; DR-9/DR-11).
2. **`compute_parents_post_state`'s `parents.len()` dispatch is RETAINED (not re-gated).** It is the
   multi-parent **block-merge dispatcher** (0 ⇒ genesis empty-trie hash; 1 ⇒ the parent's stored post-state;
   2+ ⇒ descendant fast-paths → the channel-based DAG merge) — i.e. the entry point of the §2.3 path the spec
   **preserves** (tex:305–308: "The only case requiring attention is deployments that interact via shared data
   channels"). The plan's literal D4.3 — "gate on writes-a-shared-data-channel instead of parent count;
   disjoint path early-returns empty" — is a **misread that would fork**: the 0/1-parent cases have no
   shared-channel pair to test, and an "empty" return for disjoint 2+ parents emits a wrong post-state (the
   merged state is the deterministic number-channel fold of both parents' diffs over the LFB base, never
   empty). For disjoint parents the existing merge already yields that fold via an empty-conflict set, so no
   re-gate is needed or correct. **No production change to `compute_parents_post_state`.**
3. **`conflict_set_merger::merge` (the convenience wrapper) is REMOVED.** It had zero production callers
   (`dag_merger::merge` calls `resolve_conflicts`/`compute_merged_state` directly); its only consumers were
   two tests, re-pointed to those same two primitives (identical coverage; no test disabled). It was generic
   plumbing, not an RtC artifact.
4. **Determinism pin added.** `compute_parents_post_state_regression_spec.rs` now asserts the disjoint
   sibling-parent merge is byte-identical under reversed input order — the §2.3 order-determinism guarantee.

**Outcome — wholly (not partially) satisfied.** D4.2/D4.3's spec intent (§2.3: "merge reduces to the
shared-data-channel residual, deterministically ordered") is fully realized and now regression-pinned; the
only code residue (a dead wrapper) is removed; the fork-risk literal mechanism is correctly declined with
proof. Workstream D's removal obligations (D4.1 precharge/refund + D4.2/D4.3 merge/RtC) are completely
discharged. **No consensus-state change** — the wrapper was dead; the dispatcher and merge are untouched.

**Cross-refs.** DR-5 (precharge/refund removal), DR-9 (token-per-COMM cost), DR-11 (acceptance gate),
DR-13 (Σ⟦s⟧ supply). KEEP-LIST: `MergeableChannelAccounting.v`/`.tla` (the merge path's formal anchor).

---

## DR-16 — OQS post-quantum backend removed; §4.5 G-parametricity realized by the SignaturesAlg trait

**Status.** Settled (Workstream F). **Spec law:** `cost-accounted-rho.tex` §4.5 "Genericity over the
Cryptographic Backend" (tex:978–1010).

**What was attempted.** Workstream F added an OQS (Open Quantum Safe / liboqs) post-quantum signature backend
— `crypto/src/rust/signatures/oqs_pq.rs` providing ML-DSA-65 (FIPS 204), FALCON-512, and SLH-DSA-SHA2-128s
(FIPS 205) — as the §4.5 demonstration of the calculus's genericity over the ground signature scheme G
(tex:995–1001 names `G = OQS` as an instantiation, characterised there as ongoing work). It was off-by-default
behind the `oqs_pq_experimental` feature, with all five registry touch-points (factory, Deserialize,
`signed.rs`, `validate.rs`, `web_api.rs`) feature-gated, a startup-availability assertion, and a
domain-separated, FIPS-parameter-pinned test suite.

**Why it failed (upstream, unresolvable in-repo).** The `oqs` 0.11 crate (the latest published version) pulls
`oqs-sys 0.11.0+liboqs-0.13.0`, whose bindgen-generated bindings render `OQS_SIG` opaque (1 byte) while
emitting a layout-test asserting `size_of::<OQS_SIG>() == 88`. On the pinned Rust nightly (2026-02-09) the
strict const-eval computes `1 - 88` and rejects it with `error[E0080]`, so the experimental feature does not
compile. liboqs / cmake / ninja are all present (not the blocker); the break is purely the upstream Rust FFI
binding. There is no newer `oqs` release to bump to, and no clean in-repo layout-test toggle.

**Decision.** Per the project owner, removed the `oqs` dependency, `oqs_pq.rs`, and all five feature-gated
touch-points, **keeping the `SignaturesAlg` trait and the classical backends** (Ed25519, secp256k1,
secp256k1-eth, Schnorr, FROST). This is **spec-faithful**: §4.5's load-bearing requirement is the
*parametricity over G*, which the `SignaturesAlg` trait realizes (it abstracts the ground signature scheme);
`G = OQS` is a *named example instantiation*, not a load-bearing requirement, so its removal preserves §4.5
fidelity. The change is pure deletion (385 lines removed, 0 added — no new dependency); the default build and
consensus were never affected (the feature was off-by-default and the default build always compiled clean).

**Resolution path if a PQ instantiation is wanted.** Because the trait abstracts G, a post-quantum backend
drops in without touching the calculus: a pure-Rust implementation of the same NIST schemes — RustCrypto's
`ml-dsa` (FIPS 204) and `slh-dsa` (FIPS 205) — realizes the identical §4.5 instantiation and compiles cleanly
with no C-FFI; or re-add `oqs` once a fixed `oqs-sys` ships. Either is a drop-in `SignaturesAlg` impl.

**Cross-refs.** Spec §4.5 (G-genericity); the g/#P signature split (DR-1/DR-2) and the `SignaturesAlg` trait
are the realized parametric surface.

---

## DR-17 — §3.8 syntactic sugar and the `system`/`proc` representation choice

**Status.** Settled (Workstream H). **Spec law:** `cost-accounted-rho.tex` §3.8 (syntactic sugar, tex:793–825),
§3.2/§3.3/§3.5 (identities + free names, tex:592–619), §1 ("signed terms pervade the syntax", tex:162).

**Context — the representation.** The Rocq syntax layers a Rho-calculus `proc` (`RhoSyntax.v`: `PInput`/`POutput`
carry `proc` bodies/payloads) under a thin cost-accounted `system` (`CostAccountedSyntax.v:137`,
`SSigned : proc -> sig -> system`). The signed thing is therefore a **bare `proc`**, and the spec's §3
four-sort mutual grammar — where `for(y<-x){T}` carries a *signed-term* continuation `T` and `send(x,U)` a
signed-term payload `U` (tex:439–471) — is **not natively representable** at the `system` level: a
`system`-level equation cannot place a `{P}_s` continuation inside a `for` body, because that body is a `proc`,
not a `system`. The §1 slogan "signed terms pervade the syntax" (tex:162) is the property this layering does
not realize natively. (Self-documented at `SyntacticSugar.v:14–20`.)

**Decision — Option A is the adopted, spec-faithful discharge.** The spec's §3.8 *defining equations* and the
§3.2/§3.3/§3.5 *identities* are all discharged at the source/translation level and are proof-gated (axiom-free,
in `scripts/check-cost-accounted-rho-proofs.sh`):

| Spec obligation | tex | Rocq theorem (file:line) |
|---|---|---|
| §3.8 uniform signing `{·}_s` | 793–803 | `uniform_sugar_translation_equiv` (`SyntacticSugar.v:111`) |
| §3.8 linear transfer `⊸` (desugars to nested plain-signature gates; coexists with the DR-10 ILLE extension) | 815–825 | `lollipop_sugar_translation_equiv` (`SyntacticSugar.v:148`) + the `lollipop_image_inner_gate_is_plain_*` witnesses |
| §3.2 `T ∥ () ≡ T` (signed-term ∥-unit) | 615–619 | `sse_par_unit` (`SystemStructEquiv.v:94`) |
| App. A `s:S ≡ (s:())∥S` (token-stack peel) | — | `token_decomp` (`SystemStructEquiv.v:124`) |
| §3.5 `FN_s(#P)=FN(P)` (also `FN_s(g)=∅`, `FN_s(s₁∘s₂)=∪`) | 592–595 | `sig_free_names_quote`/`_ground`/`_and` (`SystemStructEquiv.v:457,465,472`) |
| DR-10 core/extension demand invariance | — | `core_demand_invariant_under_extension` (`LinearLogicResources.v:492`) |

Because every equation and identity the four-sort native grammar would let one *state* is already *proven* at
the source/translation level, **the implementation conforms to §3.8 and §3.2/§3.3/§3.5**; the non-native
expressibility of "signed terms pervade the syntax" is a **representation choice, not a spec-fidelity gap**.

**Recorded representation migration (Option B), for a later faithful-native pass.** A representation change
would make signed terms pervade the syntax natively: refactor `RhoSyntax.v` + `CostAccountedSyntax.v` into the
spec's four mutually-inductive sorts `proc / name / signed-term / token-stack` (tex:433–471), re-type
`PInput`/`POutput` to carry signed-term continuations/payloads, move `SSigned` to `… -> signed_term`,
re-derive the locally-nameless binding/substitution machinery across the now-4-way mutual recursion, and
re-mechanize the downstream stack (`CostAccountedReduction`, `Translation`, `TranslationFaithfulness`,
`Bisimulation`, `TokenConservation`, `StrongNormalization`, `Confluence`, `StepDeterminism`) against the new
carrier. The §3.8 sugars then become native `signed_term` equalities rather than translation-level `≡`. This
is a multi-module re-mechanization that proves **no new theorem** (Option A already discharges every spec
obligation); it is recorded here as the faithful-native representation it would take, available as a subsequent
migration, and is intentionally not performed under the spec-minimal reconciliation.

**Cross-refs.** DR-1 (the `g`/`#P` axes the sugar signs over), DR-10 (the ILLE extension; `⊸` coexists with it
as sugar). Spec §3.8/§3.2/§3.3/§3.5/§1.

## DR-18 — Slashing Rocq tree axiom-gated (and the funext it caught); redemption un-halt invariant; Burned is terminal, which is spec-faithful

**Status.** Settled (StageC formal hardening, task #14). **Spec law:** `cost-accounted-rho.tex` paragraph
*Slashing* (tex:3027–3059) and *Stake vs.\ phlogiston* (tex:2359–2387): slashing's two effects (all phlogiston
removed + no further minting; stake moved to a private channel pending adjudication), "Upon redemption,
phlogiston minting resumes at the next epoch boundary", the stake outcomes (Returned / Partially redistributed
/ Burned), and "minting … contingent on … good behaviour" (tex:2368–2369, 3108–3109).

**(a) The slashing Rocq tree is now axiom-gated.** `scripts/check-cost-accounted-rho-proofs.sh` already
compiled `formal/rocq/slashing/` (the validator-contract dependency, DR-12) but did NOT subject it to the
axiom/hygiene gate the cost_accounted_rho + validator trees get. It now does, two ways: (i) the sanitized
`Admitted`/`admit`/`Axiom`/`Conjecture`/`Parameter` + incompletion-marker scan covers `slashing__*` sources;
(ii) a `Print Assumptions` block over the 73 headline theorems — the `MainTheorem.v` composition
(`main_T1…main_T12`, the `main_T9_*` bug-fix family, the top-level `main_slashing_algorithm_correct`), the
`ValidatorRedemption.v` redemption set (`redeem_vindicated_restores`, `redeem_guilty_redistributes`,
`redeem_burned_conserves`, **`redeem_burned_stays_halted`**, `slash_then_redeem_conserves_total`, …), and the
un-composed `BugFixAtomicBufferDagTransition.v` `t_9_20_*` — appended to the same assumptions file so the
existing closed-count invariant requires every one to report "Closed under the global context". This catches
both in-tree axioms (the regex scan) and IMPORTED (library) axioms (which only `Print Assumptions` reveals).

**The axiom the gate caught on its first run.** `BugFixAtomicBufferDagTransition.v` (Bug Fix #17, T-9.20)
declared itself axiom-free (its §1 note) yet `t_9_20_recon` and `t_9_20_step_idempotent_on_projection` pulled
in `FunctionalExtensionality.functional_extensionality_dep`: a `HashSet` is modelled as a function
`BlockHash -> bool`, and the two idempotence lemmas proved Leibniz function-equality `f = g`, which funext
axiomatises. **Resolution:** restate the two lemmas (`set_insert_idempotent`, `step_idempotent_dag`) and the
two T-9.20 theorems with POINTWISE equality (`forall x, f x = g x`) — the observational meaning of "same
slashing projection", provable without funext — and drop the `FunctionalExtensionality` import. All four are
leaf results (used only within their own file; not composed by `MainTheorem`), so the change is contained and
proves the same observational property. The slashing tree is now wholly axiom-free (all 73 headline theorems
Closed; proof gate green).

**(b) Redemption un-halt TLA+ invariant.** `formal/tlaplus/slashing/SlashFlow.tla` gains
`Inv_RedeemedValidatorUnhalted == \A v \in activeValidators : v \notin mintingHalted` (wired into
`MC_SlashFlow.cfg`): the TLA image of "Upon redemption, phlogiston minting resumes." It FAILS if a
Vindicated/Guilty `Redeem` re-activates an offender (`activeValidators \cup {o}`) but omits the un-halt
`mintingHalted' = mintingHalted \ {o}`. Soundness rests on the model's `active => bond > 0` (Init bonds all
positive; `ExecuteSlash` zeros-the-bond-and-deactivates atomically; `Redeem` restores a positive bond), so the
`bond = 0` idempotent-slash branch never halts an ACTIVE validator.

This safety invariant is established **DEDUCTIVELY by TLAPS**, not by model-checking: the full `MC_SlashFlow`
reachable-state space is far too large to enumerate exhaustively (a memory-bounded TLC run passes tens of
millions of distinct states without converging — which is exactly what made a naive full-enumeration attempt
impractical and motivated the deductive route). The inductive invariant
`IndInv == TypeOK /\ Inv_ActiveImpliesBonded /\ Inv_RedeemedValidatorUnhalted` is proved in
`SlashFlowProofs.tla` (`Init => IndInv`; `IndInv /\ [Next]_vars => IndInv'` split across all seven actions + the
stutter step; `THEOREM Spec => []Inv_RedeemedValidatorUnhalted`) — **199 obligations, NO state search (cannot
OOM), proved for ALL parameter values**. The auxiliary `Inv_ActiveImpliesBonded == \A v \in activeValidators :
bonds[v] > 0` is precisely what discharges the idempotent-slash case (it forces `o \notin activeValidators`
when `bonds[o] = 0`, so adding `o` to `mintingHalted` cannot halt an active validator). The proof lives in a
SEPARATE module `SlashFlowProofs.tla` because it must `EXTENDS TLAPS` (absent from the standalone TLC jar) and
because `tlapm 1.5.0` aborts on `RECURSIVE` operators — so the TLC-only `RECURSIVE` conservation operators +
`Inv_StakeConservation` were relocated verbatim to `SlashFlowConservation.tla`, with `MC_SlashFlow` re-pointed
at it (TLC coverage unchanged; all four modules SANY-parse clean). Two constant-typing `ASSUME`s were added
(`InitialBonds \in [Validators -> Nat]`, `MaxSeqNum \in Nat`) — the declared types of otherwise-untyped TLA+
constants, matching the pre-existing `MintAmount` ASSUME and satisfied by every instantiation; they are model
parameter well-formedness, not property-altering axioms. A tiny 2-validator / `MaxSeqNum=1` instance
`MC_SlashFlowRedeem` (completes in < 1 s, 9480 distinct states, no error) is the bounded TLC cross-check. Both
are wired into `scripts/ci/check-tla-invariants.sh` (`tlapm SlashFlowProofs.tla`, mirroring the cost-accounting
gate's `tlapm Validator.tla`, plus the `MC_SlashFlowRedeem` TLC run). Verification is deductive ⇒ it cannot
OOM the host regardless of model size — directly addressing the incident that motivated this work.

**Burned is a TERMINAL state — and that is spec-faithful, not a deviation.** The spec lists Returned /
Partially redistributed / Burned as dispositions of the *stake*; the StageC model keeps a Burned offender
halted (`SlashFlow.tla` Burned branch; Rocq `redeem_burned_stays_halted`; `PoS.rhox` `redeemSlashed` Burned
branch). A deep-dive of how a Burned validator is used downstream settles the apparent tension with "Upon
redemption, minting resumes": a Burned validator **cannot mint** (epoch-mint eligibility is
`active ∧ ¬halted ∧ ¬minted`; Burned fails both `active` and `¬halted` — `PoS.rhox:590–592`), **cannot
re-bond** (`bond` rejects a pk already in `allBonds`, which slash left at 0 — `PoS.rhox:428`), and **cannot be
re-redeemed** (redemption requires a quarantine record, which Burned clears — `PoS.rhox:934/1039`): it is
permanently dead. So the spec's "Upon redemption, minting resumes" is realized by the **restorative**
redemptions (Vindicated = proven right; Guilty = an arrangement, restored with a positive bond); Burned is the
non-restorative case, and a burned validator's permanent halt is exactly "minting … contingent on … good
behaviour" (tex:2368–2369, 3108–3109). The un-halt invariant therefore scopes to active (restored) validators
and correctly excludes Burned (never active).

**Cross-refs.** DR-3 (two-effect slashing + redemption), DR-7 (redemption authority = PoS multisig), DR-12
(validator multi-prover contract). Spec paragraphs *Slashing* / *Stake vs.\ phlogiston*.

---

## DR-19 — Speculative execution-on-receipt (D2-perf, task #11) is NOT implemented: a data-driven, spec-minimal decision

**Context.** Task #11 ("D2-perf: speculative execution-on-receipt + committed I/O gate") proposed pre-executing
gate-passing deploys at ingress into a discardable soft-checkpoint, with a `committed` flag gating I/O sinks
(stdout, peer sends), to hide deploy-execution latency before a proposer assembles a block.

**Decision.** Do **not** implement it. This is a *decision to close*, not a deferral of required work.

**Rationale (verified, not assumed).**
1. **Not spec-required.** The spec mandates accept-then-execute (tex 1726–1729), which the acceptance gate
   already provides: no admitted deploy executes before the funding decision, and rejected deploys never
   execute. The spec does **not** mandate speculative execution-on-receipt — DR-11's "gate-before-speculate"
   is a *constraint on any speculation* (it must not feed acceptance/commit), **not** a requirement to
   speculate (`docs/theory/cost-accounting-impl/wd-d2-acceptance-gate.md` §D2.6: "Nothing consensus-critical
   is deferred").
2. **No measured bottleneck.** The data-driven mandate is "profile before optimizing" — an optimization needs
   a measured target. There is currently **no** execution-on-receipt at all (deploys sit in storage until a
   proposer picks them), and no production workload against which to measure whether the receipt→assembly
   window is latency-bound. Building a large architectural change (a new ingress execution trigger + a
   speculative soft-checkpoint lifecycle + a `committed` I/O gate, touching ingress/runtime/I/O) **absent a
   measured bottleneck is textbook premature optimization**, which the project's engineering principles
   explicitly forbid.
3. **Spec-minimalism.** Adding a non-spec-required subsystem on consensus-adjacent paths introduces
   complexity and risk for zero measured benefit.

**Revisit trigger (a concrete condition, not a standing deferral).** Reopen *only* if profiling under a
representative production workload shows the receipt→assembly window is a measured throughput/latency
bottleneck. The enabling machinery (`create_soft_checkpoint` / `revert_to_soft_checkpoint`, ~33 call sites)
already exists, so the option stays cheap to take up later. The acceptance gate's correctness is independent
of this decision (a pure O(AST) static analysis that needs no speculative results).

**Cross-refs.** DR-11 (acceptance gate; gate-before-speculate), `wd-d2-acceptance-gate.md` §D2.6. Spec §7.6
accept-then-execute (tex 1726–1729).

---

## DR-20 — The Rule-4/5 continuation re-seal (GAP-2) is proved cost-benign; the native-model migration (GAP-1) trigger sharpened; spec-delegated parameters (GAP-3) enumerated

**Status.** Settled (spec-ambiguity refresh, tasks #17/#20–22). **Spec law:** `cost-accounted-rho.tex` §3.6
Rules 4–5 (tex:714–742), §3.8 uniform signing (tex:793–803), §3.1/§1 ("signed terms pervade the syntax",
tex:162), §4.2 crypto-quoting (hash), §3.4 (name equality), §4.6/§4.7 (per-signature supply).

**Context — the refresh.** A re-examination of the 38-entry spec-ambiguity catalog against the spec itself
(behavioral induction: does the spec address each, explicitly or by how the construct is USED?) found the spec
DETERMINES 28/38 and 6 are non-calculus; only **three** are genuine, and the only entry where impl↔spec
faithfulness was not already locked was #7 (the Rule-4/5 continuation signing). This DR records the resolution
of that residual and the precise remaining representation gaps.

**(a) GAP-2 — the Rule-4/5 re-seal is proved COST-BENIGN (#7, closed).** The paper's Rule 4/5 RHS
(`T{@U/y} ∥ S ∥ S'`, tex:714–742) seals the continuation under the RECEIVER's signature `s₁` (uniform signing,
§3.8). The Rocq model (`ca_rule4`/`ca_rule5`, `CostAccountedReduction.v`) re-seals the bare-`proc` continuation
under the COMPOUND `SAnd s₁ s₂` — a direct consequence of the proc-under-system representation (DR-17: a
continuation is a bare `proc` with no seal of its own, so the rule supplies the consuming signatures). New
`theories/Rule45ContinuationAdequacy.v` proves this re-seal cannot change the consensus-metered cost: a seal
carries no fuel (`system_token_count (SSigned _ _) = 0`, `CostAccountedSyntax.v:208`), so

- `signed_process_holds_no_fuel : system_token_count (SSigned P s) = 0`
- `continuation_seal_is_cost_irrelevant : system_token_count (SSigned P s₁) = system_token_count (SSigned P s₂)`
- `rule45_result_cost_independent_of_seal : count ((P)^seal ∥ t) = count ((P)^seal' ∥ t)`

— the result has the same token count (hence the same `Δ_s`, a COMM count, and the same value under every cost
theorem) under the compound `s₁∘s₂` as under the spec's receiver `s₁`. With `ca_cost_deterministic` (terminal
cost of a fixed system is path-independent, `Confluence.v`) and the §5 s₀-limit bisimulation (at s₀ every
signature is equal, so the distinction vanishes), the re-seal has NO consensus-observable effect. Both
headlines are axiom-free ("Closed under the global context") and in the proof-hygiene gate. #7 is therefore
**resolved in place** — the discrepancy is real at the calculus-model level but proved benign — without the
Option-B refactor.

**(b) GAP-1 — the native four-sort grammar migration trigger (sharpened).** GAP-2 is the operational face of
the representation choice DR-17 records: `SSigned : proc → sig → system` carries a bare `proc`, so "signed
terms pervade the syntax" (§1) is not native, and the §3.2/§3.5/§3.8 signed-term identities are discharged at
the source/translation level (Option A, axiom-free; DR-17's obligation table). The faithful alternative — the
native four-sort mutually-inductive grammar in which `for`/`send` bodies are signed terms and a continuation
retains its own seal, dissolving the GAP-2 re-seal outright — remains the recorded Option-B migration (DR-17).
This DR sharpens its trigger: **undertake Option B when, and only when, a required result must reason NATIVELY
about a multi-signature continuation's own seal** (not merely its cost — Option A plus the (a) adequacy theorem
already settle the cost). Option B proves no new cost theorem; until the trigger is met, Option A + (a) are the
spec-faithful, spec-minimal discharge.

**(c) GAP-3 — intentional spec delegations (enumerated, not bugs).** Three constructs the paper uses but
explicitly leaves to the implementation: (i) **the hash function** for `#P` (§4.2, "a configurable hash
function (SHA-256, Blake2b, …)") — mechanized as the `hash_process` parameter with the three
structural/cryptographic hypotheses on it (§11.1/§12.1; the G-parametric realization is DR-16); (ii) **name
equality `≡_N`** (§3.4) — used in the communication rules, never defined at its use site, realized as
structural equality of the normalized quoted process (the runtime `normalize_preserves_struct_equiv`
correspondence, verification §12.3); (iii) **the per-signature supply-pool runtime representation** (§4.6/§4.7)
— behavior + injectivity fixed (the `Σ⟦s⟧` balance datum, DR-13; `lane_pool_disjoint`), the concrete container
(`DashMap<Sig, AtomicI64>`) an unconstrained implementation choice. Each is intentional in the paper; the
impl's choice is consistent with every behavioral law the paper fixes. Recorded in the verification doc §12.3
("Implementation-delegated parameters").

**Cross-refs.** DR-17 (the representation choice + Option A/B), DR-13 (per-signature balance datum), DR-16
(G-parametric hash), DR-1 (g/#P axes). Spec §3.6/§3.8/§3.1/§4.2/§3.4/§4.6/§4.7.
`Rule45ContinuationAdequacy.v`; verification §12.3.

---

## DR-21 — Option B EXECUTED: the native four-sort grammar; GAP-2 dissolved; native SN is conditional on the linearly-funded fragment

**Status.** In progress (the `continued-gslt-cost-v2` alignment). The DR-17/DR-20 Option-B native-grammar
migration — previously recorded-but-not-performed — is now being **executed**, triggered by the sibling paper
`publications/cost-accounting-as-monad/continued-gslt-cost-v2.tex` ("Continued Interactive GSLTs and the Cost
Endofunctor"), whose central revision **"wrapping by construction"** (continuation slots sorted as wrapped
terms 𝕋; no-leak a sorting invariant) IS the native four-sort grammar. The user directed full alignment with
both papers, with full multi-prover rigor (Rocq + TLA+ + Sage + Lean). **Spec law:** cost-accounted-rho.tex
§3.1 (four-sort grammar), §3.6 (Rules 1-5); continued-gslt-cost-v2.tex (the categorical construction).

**(a) Carrier split (the migration's load-bearing design).** The pure rho calculus `proc`/`name` of
`RhoSyntax.v` is kept UNCHANGED as the translation TARGET; the cost-accounted SOURCE is introduced as three
new mutually-inductive sorts in `CASyntax.v` — `caproc` / `caname` / `signed_term` — reusing `sig` and the
`token` stack (`() | s:S`) from `CostAccountedSyntax.v`. `for`/`send` (`CPInput`/`CPOutput`) carry
`signed_term` continuations/payloads, so "signed terms pervade the syntax" is native and "every redex lies
inside a wrapper" is a SORTING invariant. The wrapper is `STSigned` (the old `system` `SSigned` coexists
during the incremental migration). This split is what keeps the erasure target signature-free and lets the
proof gate stay green at every stage. The §3.8 sugars become native `signed_term` equalities.

**(b) GAP-2 dissolves syntactically.** Because the native continuation `T` is a `signed_term` carrying its own
seal, the COMM rules (`CAReduction.v`) yield `T{@U/y} = subst_st T 0 (CQuote U)` — the continuation keeps its
own signature, with NO `SAnd s1 s2` re-seal in the split-process rules (old `ca_rule4`/`ca_rule5`). The
re-seal GAP-2 (DR-20a) is simply absent; `gap2_split_{combined,split}_keeps_own_seal` witness it.
`Rule45ContinuationAdequacy` (which proved the OLD re-seal cost-benign) remains valid for the old model and is
retired when the old model is removed (a later stage).

**(c) Native strong normalization is CONDITIONAL — a genuine finding.** The old `token_strictly_decreases`
(every step strictly drops `system_token_count`) is **false** for the native model: a `for`-continuation that
is a located purse (`STStack`) RELEASES spine fuel, and a non-linear continuation (a received quote
dereferenced ≥2 times) DUPLICATES a token-bearing payload — so `st_total_fuel` can strictly INCREASE
(`st_total_fuel_can_increase_off_funded` exhibits a concrete witness, 3→4). Native `ca_step` therefore does
NOT strongly normalize unconditionally. SN holds on the **linearly-funded fragment** (`funded_linear`: every
continuation forces its bound variable at most once — the term-level image of `LinearLogicResources`'
no-contraction — and no continuation is a self-replenishing purse). There, every COMM strictly drops
`st_total_fuel` by the consumed gate (`funded_step_decreases`), and `ca_SN_funded` follows by
well-foundedness of `<`. **This conditioning is not a weakening: it MATCHES the operational acceptance gate** —
only funded deploys are admitted (`strict_reject_when_underfunded`), so cost-determinism on the funded
fragment is exactly the consensus-relevant statement. It is also faithful to the paper's "multiplicity is
carried by the stack (linear token consumption), not by key distinctness" (continued-gslt-cost-v2.tex,
"Duplication needs no fresh signatures"). Design decision (locked): the `funded_linear` clause for a bare
`STStack` continuation is the restrictive, provably-sound one (terminal purses only); relax later only if a
use-case requires it.

**(d) Module inventory (committed, axiom-free, proof-gate green).** `CASyntax`, `CABinding`, `CAStructEquiv`
(native grammar + locally-nameless metatheory + 3-way structural congruence); `CAReduction`,
`WrappingSubjectReduction` (the five gated COMM rules + subject reduction / no-leak); `CATokenConservation`
(`st_total_fuel`, spine-invariance, `funded_linear`); `CAStrongNormalization` (the bridge lemma, conditional
SN, divergence witness); plus the categorical `SignatureMonoid` (the two monoids the Cost monad descends from)
and the Sage witness `cost_monad_laws.sage`. Remaining stages (confluence/cost-determinism re-base on
`ca_SN_funded`; translation/faithfulness/bisimulation re-mechanization; the categorical endofunctor/monad
layer; TLA+ and Lean legs; the §12.3 verification-doc update) build on this foundation.

**Cross-refs.** DR-17 (the Option A/B representation choice), DR-20 (GAP-1/GAP-2/GAP-3 + the Option-B
trigger), DR-9 (per-COMM cost), DR-13 (per-signature balance). Spec §3.1/§3.6/§3.8; continued-gslt-cost-v2
("wrapping by construction", "duplication needs no fresh signatures", "stack consumption is the modulus").

---

## DR-22 — N-ary Join landed natively (spec §4.8, Tier-2) + the abstract category-theory layer (§6–§9)

**Status.** Done (gate-green, axiom-free). Closes the one genuine spec-vs-formalization gap the
spec-alignment audit surfaced (`cost-accounting-spec-alignment-audit.md` §6) and mechanizes the
abstract ciGSLT category-theory of `continued-gslt-cost-v2` §6–§9.

**Finding.** The audit's adversarial sweep found the **N-ary Join schema** (`tex §4.8`: Def 4.6's
`for(y₁←x₁ & … & yₙ←xₙ){P}` with the two named firing cases J1/J2, and Prop 4.7's conservation of
authority across token-presentation partitions) was the only spec construct present in the law but
absent from the formalization: the reduction relation had only the five **binary** gated COMM rules,
no join former, no join desugaring, and Prop 4.7 was discharged by no theorem. (Label correction: the
verification doc had numbered joins "§4.5"; the tex is **§4.8** — Def 4.6 `tex:1099`, Prop 4.7
`tex:1131`, J1 `tex:995`, J2 `tex:1021`.)

**Decision — Tier-2 native realization.** A first-class `CPJoin : list caname → signed_term → caproc`
grammar former (CASyntax), with the full ~40-module metatheory swept to cover it, plus **two** native
reduction rules — the two cases the spec names:
- `ca_join1` (J1, eq:join-J1) — whole-join under one funding seal `s`, single `s`-token; the N-ary
  analogue of Rule 1. N=1 is exactly Rule 1.
- `ca_join2` (J2, eq:join-J2) — separately-signed receiver + N separately-signed senders, ONE combined
  token keyed `s₁ ∘ t₁ ∘ … ∘ tₙ` (`join_token_key`), fired atomically; the N-ary analogue of Rule 3. A
  J2 configuration is *stuck* without this rule (Prop 4.7 concerns authority multisets, not reduction),
  so J2 is required for operational faithfulness, not gold-plating.

The general partition schema (axes C/B/A, `tex:1073`) is deliberately NOT mechanized as a constructor
(it wrecks the determinism/confluence inversions); its content — that grouping never changes the
consumed authority — is captured as the multiset theorem **Prop 4.7** (`CAJoinConservation`:
`join_authority_conserved`, `reverse_curry_iso` §4.8.4, `join_demand_partition_invariant`,
`join_no_weakening` §4.8.5), all up to `Permutation`/`≡` since `SAnd` is a free constructor.

Metatheory landed for both rules (axiom-free, behind the LOCAL-ONLY gate): subject reduction / no-leak
(WrappingSubjectReduction), token conservation + funded strong normalization
(`funded_step_decreases`, the keystone `linear_subst_many_fuel_le`), local confluence + per-rule
determinism (`ca_step_join1_det`, `ca_step_join2_det`, `signed_sends_injective`,
`ca_local_confluence`, `ca_step_deterministic`), the graded transition relabelling (`g_join1`,
`g_join2`) and its image-finiteness enumeration (`graded_succ`/`graded_succ_all` + sound/complete), and
translation progress (`ca_translation_progresses`). Newman/cost-determinism/modulus re-close unchanged
(they consume the feeders generically).

**Design crux — closed payloads.** `subst_st_many T Us` is iterated binary `subst_st … 0` with NO
inter-step lifting, so for N≥2 an open payload's free de Bruijn 0 would be captured by the next
substitution (visible in the `subst_st_many_two` example). Closed payloads (`Forall closed_st Us`,
a premise on `ca_join1`/`ca_join2`) are therefore the precondition that makes the N-simultaneous
substitution *capture-correct* — transmitted values are closed — and are exactly what the SN fuel
bound needs (a closed payload injects no dereferences — `closed_deref_zero_ca`,
`deref_subst_closed_ca` — so linearity propagates through the fold). The premise is local to the join
rules; `ca_step_join{1,2}_det`'s interface (length + step) stays stable, so Confluence/Determinism
needed no change, and the binary rules' funded proofs are untouched. The graded successor enumeration
decides the premise with the boolean reflection `closed_st_dec`/`closed_stb_spec`.

**Decision — abstract category-theory layer (§6–§9).** A bespoke axiom-free **hom-setoid** scaffold
(`CategoryInterface`: `Category`/`Functor`/`NaturalTransformation`/`Monad`/`Adjunction` with a
Prop-valued `heq` carrying its own refl/sym/trans + composition congruence; no field is a function
equality, so no funext). On it: the concrete ciGSLT category `CACategory.CICat` (carrier `signed_term`,
transition `graded_step`, reachable signature fragment, hom-equality pointwise `graded_bisim`;
`graded_bisim_trans` newly added); Thm 7.1 (`CACostFunctor`, Cost as a genuine `Functor` record + the
law conjunction; `CACostFunctorCI.CostCI` as the concrete `CICat` lift preserving graded transition,
graded bisimulation, and quote-faithfulness); Prop 6.2 closure;
Prop 6.1 (`CAProperSubcategory`: faithful + not-full via a key-collapse `discriminate` + not-eso
bounded by `no_leak_stack_inert`); Prop 9.1 (`CACostMonadCat`); Prop 9.2 (`CAAdjunctionI`); Prop 9.3
(`CAAdjunctionII`) via the **counit-dissolution** — `Imp_G` is modelled as the *intra-carrier*
gate-firing `graded_step` (not the cross-sort `st_tr` into `proc`), so the counit `η_G ∘ Imp_G ⇒ id`
is typeable where the cross-sort version was not, and the force-obstruction
(`CAForceSeparation.ca_force_overgating_separation`, a property of `st_tr` at force points) is absent
intra-carrier; the capstone `CAAbstractCapstone.continued_gslt_cost_abstract_capstone` conjoins all.

**Honest bound (stated, banned-word-free).** The Rocq deliverable for the simulation 2-cells
(`CASimulationBicat`) and Adjunction II is the **2-truncation**: Prop-valued 2-cells (`weak_match`) with
reflexive/transitive vertical composition. The full setoid-bicategory coherence (interchange +
associator/unitor pentagon-and-triangle stated as *equalities of 2-cells*) compares `weak_match`
witnesses in a Σ-type over Props, which needs UIP/funext on the 2-cell layer — outside the axiom-free,
no-funext fragment the Rocq mandate permits. That coherence is therefore the truncation ceiling here
and is routed to **Lean/Mathlib + Isabelle/AFP** (the foundations that permit it); the *result* is not
bounded, only the *Rocq* realization is. Likewise Prop 6.1 not-eso is witnessed against ONE precisely
reified clause (a stack-headed transition `graded_step` cannot exhibit), per the spec's "enumerating
cases is hopeless".

**Cross-refs.** DR-20 (the §4.8/GAP nomenclature), DR-21 (the native four-sort grammar this builds on),
the spec-alignment audit (§6 the gap, §3.3 the abstract-layer bound). Spec `cost-accounted-rho.tex`
§4.8 (Def 4.6, Prop 4.7, J1/J2, §4.8.4/§4.8.5); `continued-gslt-cost-v2.tex` §6 (Prop 6.1/6.2), §7
(Thm 7.1), §9 (Prop 9.1/9.2/9.3).

---

## DR-23 — Multi-prover completion + a deep cross-validation review (findings + remediation)

**Status.** The DR-22 "honest bounds" are closed and the framing/bridge/scope remediations are done
(gate-green, axiom-free). The C `CICat` lift is closed by DR-24; A/E remain separately tracked.

**Context.** Two passes. (1) The DR-22 honest bounds were closed once the host toolchains became
available. (2) A fresh, careful re-read of BOTH papers plus two independent arbitration passes over the
Rocq sources cross-validated papers↔papers↔formal↔impl and surfaced previously-undocumented
misalignments — several in the DR-22-era / this-session additions themselves.

**(1) Bounds closed** (commits 4ddf9d50, 7b00eab4, 88eaac49, cf88bbe8, 94a04c2e).
- Multi-prover arsenal now **8/8 VERIFY** on this host (was 6 verify + 2 skip): Tamarin loads (the
  `libHSfgl` GHC lib was installed) and the gate auto-sets `MAUDE_LIB=/usr/share/maude` → 3 lemmas
  verified; Verus's pinned rust `1.95.0` installed → "3 verified, 0 errors" (the gate runs verus in a
  `mktemp` scratch dir so no build artifact lands in the repo).
- **Iris**: `Reconcile.v` now proves the LOGICALLY-ATOMIC triple `debit_atomic_spec` (linearizability
  under concurrent interference — `iLöb` + atomic-update + CAS-fail retry), not just the contention-free
  `debit_spec`. The earlier syntax-error trap was importing `iris.bi.lib.atomic` instead of
  `iris.program_logic.atomic`.
- The DR-22 **2-truncation** of `CASimulationBicat`/`CAAdjunctionII` is **DISSOLVED in core Lean**
  (`formal/lean/CostAccountedRho/SimulationBicategory.lean`): the full bicategory coherence
  (interchange/pentagon/triangle) holds by Lean's DEFINITIONAL `Prop` proof-irrelevance — axiom-free
  (`#print axioms` = none) — NOT via Mathlib/AFP (both still absent). That proof-irrelevance is precisely
  the principle the axiom-free Rocq fragment lacks.
- **Prop 9.1/9.2 records instantiated** (`CACostMonadInstances`): `cost_monad_instance : Monad GCat` and
  `cost_kleisli_adjunction : Adjunction Free Forget` (the genuine Cost-generating resolution), on the
  setoid category `GCat` — the grade-aware base the unit/assoc laws need (R-C).

**(2) Cross-validation findings** (severity ▲ substantive / ◆ doc-framing / ○ cosmetic).
- ◆ **Papers (REPORT-ONLY — read-only law).** Rule-count A↔B (Paper A's 5 concrete Rule 1–5 vs Paper B's
  3 abstract R1–R3 over `K`) with no spelled-out bridge; the Adjunction-I prose ("Forget erases on the
  nose / runs without friction") reads against its own Prop ("induced monad Forget∘Free is Cost"), though
  the paper is internally consistent (cross-category composite); §4.8's join substitution is simultaneous
  and unrestricted (no closed-payload condition).
- ▲ **(D) Adjunction-I formal mislabel.** `CAAdjunctionI`/`CAAdjunctions` prove the install/strip
  SECTION-RETRACTION `Forget∘Free = id` (the §9 "detachable layer" remark, `tex:1174-1184`), NOT the
  Cost-generating resolution (`Forget∘Free = Cost`); the latter is the separately-added
  `cost_kleisli_adjunction`. The banner overclaim is FIXED and cross-referenced (Phase 1).
- ▲ **(C) Functor/monad on the wrong base.** `cost_is_endofunctor` (TypeCatL) and `cost_monad_instance`
  (GCat setoids) are the writer-monad skeleton, NOT the concrete ciGSLT `CICat`. Closed by DR-24:
  `CACostFunctorCI.CostCI` lifts Cost to `CICat`, `CIMor` carries signature maps and quote-faithfulness
  over reachable signatures, and `cost_ci_preserves_step`/`cost_ci_preserves_bisim`/
  `cost_ci_preserves_quote_faithful` discharge the paper's load-bearing `Cost(f)` obligation
  (`tex:769-777`).
- ▲ **(E) Adjunction-II Turing-completeness conditioning.** Prop adj2 is gated on `G ∈ ciGSLTtc`. RESOLVED:
  `Internalisable` + `internalisation_retraction_param` make that hypothesis explicit (retraction for any
  internalisable base), with `rho_internalisable` the witness. The **⟹ direction (Turing-completeness ⟹
  internalisability) is REALIZED at rho**, NOT an open residual: `rho_internalises_by_interpreter` bundles
  the decidable guards (`sig_eq_dec`) and the section (`eta_is_section_2cell`), and the computable
  INTERPRETER `Imp_G = st_tr` realises `Imp_G ∘ η_G ≈ id_G` up to weak bisimulation
  (`CAInternalisation.ca_internalisation_retraction`, each gated step one rho COMM) — exactly Prop adj2's
  "computable encoding + decidable guards … standard interpreter construction", mechanized for the
  calculus's actual (universal) base. The ONLY unmechanized piece is the fully-abstract "EVERY ciGSLTtc G"
  quantified over NON-rho bases (an arbitrary universal calculus building its own interpreter) — the monad
  paper's general theory, the SAME CCS/λ/ambient-foils-level scope boundary the development draws elsewhere
  (only rho is the impl target), sketched in the paper, out of scope for this rho formalization.
- ▲ **(A) Join closed-payload restriction.** `ca_join1`/`ca_join2` carry `Forall closed_st Us` (the
  binary rules do NOT) — strictly narrower than Def 4.6/J1/J2, which allow open payloads — because
  `subst_st_many` is iterated, not simultaneous, substitution. → Phase 2 (genuine simultaneous subst).
- ◆ **(B) General schema only at conservation level.** Only the J1/J2 corners are reduction rules; the
  general Def 4.6 partition schema is captured at the multiset level (Prop 4.7) only. Scope note ADDED
  (Phase 1).
- ▲ **(F) not-eso overclaim.** `U_not_eso` uses IMAGE-FINITENESS, which is NOT one of Paper B's three
  not-eso reasons (unfactorable rewrites / undecidable ≡ / non-wrappable), and ciGSLT objects are not
  image-finite in general — so `ci_realizable`'s `image_finite` is a sufficient-not-necessary proxy. The
  comments are REFRAMED (Phase 1) to state it as a model-specific witness (W1 stack-inertness + W2
  image-finiteness), NOT "the fully general Prop 6.1 not-eso". `U_not_full`/`U_faithful` DO match.
- ◆ **(J-1) Operational join key bridged.** `ca_join2` gates on `join_token_key` (left fold) but the
  conservation theorems were stated about `combined_key` (right-nested); the bridge `join_key_atoms_perm`
  + `join_authority_conserved_operational` are ADDED (Phase 1).
- ◆ **Impl (C-impl, Phase 3 — equivalence argument).** Paper §8 specifies overcharge-and-refund:
  escrow the conservative demand Δ_c^max at acceptance, refund Δ_c^max − κ after the run (κ = tokens
  actually forced) → **net charge κ**. The Rust (DR-5) instead does **gate-on-conservative + exact-
  charge**: the acceptance gate (`delta_sigma::is_funded` / `effective_supply`) requires per-signature
  supply ≥ Δ_c^max *before* executing; settlement then debits the EXACT κ (no escrow, no refund step) →
  **net charge κ**. The two schemes are therefore **economically identical** (same net κ); they differ
  only mechanistically (escrow-then-refund vs gate-then-exact-debit).
  - *Net-charge equivalence* — formally witnessed: the refund scheme's conservation `charged + refund =
    escrow` with `charged = κ` is `Settlement.charged_plus_refund_eq_escrow` (+ `refund_le_escrow`,
    `refund_zero_when_exhausted`); the gate scheme's debit IS κ by construction. Both net to `charged = κ`.
  - *No-stranding* (the property the paper's escrow exists to guarantee) — preserved WITHOUT escrow: the
    acceptance gate establishes supply ≥ Δ_c^max ≥ κ *before any step fires*, so every statically-
    reachable branch is fully funded and no accepted deploy can stall mid-execution. Under the block-
    assembly gate (DR-11) the per-signature supply is committed at acceptance, so concurrent deploys in a
    block are all gated against the committed supply — no TOCTOU window. The escrow's availability
    guarantee is thus discharged by the acceptance-time commit + the static linear-resource proof, not by
    holding funds in escrow. So the §8 divergence is sound and DR-5/DR-11-documented, not a defect.

**Remediation status.** Phase 1 (D banner, F comments, J-1 bridge, B scope note, this DR + audit
refresh): done, gate-green. Phase 2 C (`CICat` lift) is done by DR-24. Phase 2 A (simultaneous
substitution) and E (`internalisable` Adjunction II) remain separately tracked; Phase 3 impl equivalence
note is documented by DR-5/DR-11 and the audit.

**Cross-refs.** DR-22 (the bounds this supersedes), DR-5 (refund lifecycle), DR-11 (block-assembly
acceptance gate), the spec-alignment audit. Arbitration evidence: `CAAdjunctions.v:33`,
`CACostMonadInstances.v:106/156/167`, `CACostFunctor.v:47`, `CACategory.v:78`, `CAInternalisation.v:79`,
`CAProperSubcategory.v` (`U_not_eso`), `CAReduction.v:178/197/200`. Spec: `cost-accounted-rho.tex` §4.8;
`continued-gslt-cost-v2.tex` §6/§9 (Prop 6.1, prop:adj1/adj2, thm:functor).

---

## DR-24 — Generic GSLT/OSLF boundary; MeTTaIL is an adapter, not a dependency

**Decision.** The node exposes the cost-accounting resource checker through the specification-level
GSLT/OSLF boundary, not through `mettail-rust` or any other concrete theorem-engine runtime. Rust now names
that boundary in `rholang/src/rust/interpreter/accounting/resource_logic.rs`:
`GsltPresentation`, `ResourceSignature`, and `OslfResourceLogic<G>`. The current native implementation is
the specialization `RhoGslt + DefaultResourceLogic`; existing `ResourceLogic` remains a compatibility alias
for that specialization.

**Implementation consequence.** The Casper D2 admission and replay-settlement paths have injected variants
(`admit_by_funding_with_logic`, `recompute_settlement_debits_with_logic`) that consume an
`OslfResourceLogic<RhoGslt>` and otherwise preserve the public default entry points. Candidate construction
uses `RhoGslt.canonicalize_for_funding`, `ResourceSignature::key`, and
`ResourceSignature::split_join_decompositions`; the channel encoding remains the native Rholang supply
realization (`Σ⟦s⟧ = from_sig(s)`). This keeps per-signature accounting local, associative, and mergeable:
independent signature pools still settle through `BTreeMap`/`DashMap` keyed by the same canonical lane basis,
with no global MeTTaIL coordination point.

**Formal consequence.** The concrete ciGSLT category now reifies reachable signatures and quote-faithful
signature maps in `CIMor`; `CACostFunctorCI.CostCI` maps both state and accumulated signature, and
`cost_ci_preserves_quote_faithful` is checked with the existing `CostCI` transition and bisimulation
preservation obligations. This closes the DR-23 C finding for Thm 7.1 on the concrete `CICat`, while leaving
the writer skeleton (`CACostFunctor.cost_is_endofunctor`) as the algebraic presentation.

**MeTTaIL boundary.** When MeTTaIL is ready, the integration should be a crate or module that implements the
generic traits for its presentation and proof checker, then opts into the injected admission/replay entry
points. It should not become a required dependency of `rholang` or `casper`, and `mettail-rust` is guarded
against accidental workspace Cargo dependency introduction. This keeps MeTTaIL folded into the design as a
supported use case rather than coupling cost-accounted Rholang to one implementation.

---

## DR-25 — Untyped-λ "R1-only" instance mechanized (the cost-endofunctor genericity witness)

**Status.** Done (gate-green, axiom-free). 2026-06-08.

**Decision.** Mechanize the untyped-λ instance of the generic cost transform as a standalone, axiom-free Rocq
witness. `theories/CAUntypedLambda.v` defines a host λ-calculus (`lterm`, de Bruijn) and a fuel wrapper
(`lsys`, reusing `sig`/`token` only), with ONE β-R1 contact rule `lca_beta_r1` — the exact analogue of
`CAReduction.ca_rule1` — and proves: R1-only (`lca_only_beta_r1`, `lca_contact_requires_token`,
`lca_stack_inert`, `lca_funded_nonredex_stuck`), the funded run-bound (`lca_step_decreases`,
`lca_funded_run_bounded`), unconditional funded strong normalization (`lca_SN_funded`) including the Ω
divergence/halting seam (`omega_pure_diverges`, `lca_omega_funded_one_step`), and erasure to pure β
(`lca_beta_r1_erasure`). `theories/CAUntypedLambdaCI.v` exhibits the metered λ calculus as a second object
`Lambda_ciGSLT` of the ciGSLT category under the cost endofunctor `CostCI` (`Lambda_ciGSLT_nonvacuous`),
beside `Rho_ciGSLT`.

**Spec basis.** The cost-endofunctor paper (`continued-gslt-cost-v2.tex` §8) gives λ-calculus, CCS, and
ambient foils as worked examples of cost-accounting over a rigid (free) contact; `cost-accounted-rho.tex`
§3.6 is the R1 shape. The companion `mettail-rust` `cost-decoration` prototype emits one metered rule
(`Beta_R1`) for its untyped-λ reification (rigid `App`, in no equation, not a comm-collection) versus all five
for its communication calculus (`Par` is a comm-collection ⇒ AC). This DR supplies the formal λ instance of
that genericity, which DR-24 framed at the GSLT/OSLF boundary level.

**Rationale.** "Rigid K ⇒ R1 only" is forced, not stipulated: the λ host has no associative-commutative
operator (so signatures never conjoin across independently-signed participants ⇒ no Rules 2/3) and no
independent environment-introduction/output sort (so there is no second signed process to bring into contact
⇒ no Rules 4/5). The instance is also where the cost discipline's force is sharpest: untyped λ is
Turing-complete and Ω is non-normalizing in pure λ, yet a finitely-funded configuration is unconditionally
strongly normalizing — λ wrappers carry no fuel-bearing subterm, so the fuel measure can never rise, in
honest contrast to the rho fragment, where native SN is conditional on the linearly-funded fragment (DR-21,
witnessed by `CAStrongNormalization.st_total_fuel_can_increase_off_funded`).

**Artifacts.** `theories/CAUntypedLambda.v`, `theories/CAUntypedLambdaCI.v` (registered in `_CoqProject`); 16
headline theorems gated by `scripts/check-cost-accounted-rho-proofs.sh` (each `Closed under the global
context`); the correspondence row and verification §4.2.1; this DR.

**Cross-refs.** DR-21 (the native four-sort grammar + the conditional-SN finding this contrasts with), DR-22
(the abstract §6–§9 category-theory layer; `Lambda_ciGSLT` is a second object under `CostCI`), DR-24 (the
generic GSLT/OSLF boundary and the MeTTaIL adapter — DR-25 is the formal λ instance of that genericity).

---

## DR-26 — Behavioral alignment via compile-time shapes; external-proof certificates are optional assurance (supersedes DR-12 enforcement)

**Status.** Done (prover gates relaxed). 2026-06-15.

**Decision.** Per the project lead's directive: validator behavioral alignment is supplied by the **compile-time
type discipline (shapes)** — the OSLF spatial+modal usage types of the cost-accounted calculus — so external-proof
**CERTIFICATES** (Rocq / Lean / TLA+ / Sage) are **NOT a required gate** on the Rust implementation. The four
prover gates `scripts/check-cost-accounted-rho-{proofs,lean,sage,tla-invariants}.sh` are now **ADVISORY by
default** (report posture, exit 0) with **`CA_ENFORCE_PROOFS=1`** opt-in strict (the full compile + `rocqchk` +
axiom-free `Print Assumptions` path is preserved verbatim). This **supersedes the *enforcement* posture of DR-12**
(the validator behavioral contract proven in three provers + gated by the script): the proofs remain valuable as
**optional assurance**, but they are not certificates the implementation must carry to be correct.

**Spec basis.** The OSLF type discipline IS the checkable behavioral alignment
(`continued-gslt-cost-v2.tex` §1400–1471: *"the type system is not new machinery; it is the logic OSLF already
generates"*; opt-in, per-term). Shapes-at-compile-time deliver the alignment DR-12 pursued via external certificates.

**Consequence.** TM-CA-161 and UC-CA-159 (the `validator_contract_*` rows) are reframed: those proofs are optional
assurance, not the enforced gate; the gate scripts are advisory. Formal verification stays welcome and LOCAL-ONLY
(never CI — standing policy); run it with `CA_ENFORCE_PROOFS=1`. DR-12's *content* (the contract obligations
S1–S4 / P1–P3 and their proofs) is retained and unchanged; only its *enforcement-as-certificate* role is dropped.

**Cross-refs.** DR-12 (enforcement superseded; content retained), DR-18 (axiom-gating), DR-25, the W2 typed-token
compile-time discipline in the plan; `feedback_formal_verification_is_local_only_not_ci`.

---

## DR-27 — Token-source model realigned to the papers (spectral phlogiston + typed value); implementation divergences + mortality

**Status.** Original clean-slate findings 2026-06-15. **Token-source verdict SUPERSEDED the same day by Greg's
authoritative answers** (REV is a NAME for the one system token, `wallets.txt` IS the genesis root — see the
CORRECTED paragraph below). **F-A / F-B / F-C-F-D + the F-1 red-team finding are LANDED + adversarially red-teamed**
(commits e329aed3, c94e980f, a5a26f5c, e55769dd, e011e0e7, 59c59b1e, 87c012f2). pgmcp #481.

**Decision (token sources).** A clean-slate re-reading of the `publications/*.tex` establishes the intended model:
**ONE species — phlogiston as signature-indexed first-class token stacks `s:S`** (the old homogeneous phlo is the
degenerate `s₀`-collapse), **plus a typed-value `Pay(τ)`** layer (`TypedCurrency/typed_value.tex`) metering
*transfer* (rivalrous, contraction-rejected) while phlogiston meters *computation*. **Stake** is a distinct
locked-token *role* (consensus weight, slashable; *"phlogiston is not drawn from stake — a separate resource"*,
cost-accounted-rho.tex:3024). **CORRECTED by Greg's 2026-06-15 authoritative answers** (which OVERTURNED the pre-answer "off-model" verdict on
REV / `wallets.txt`): **REV is NOT a separate species and NOT off-model** — it is one of several inconsistent NAMES
(*token / Phlogiston / REV / Rock / F1r3caps*; avoid `F1r3caps`, collides with F1R3FLY.io *Capabilities*; canonical
= **phlogiston**) for the ONE system token. **`wallets.txt` IS the genesis trust-root (Greg P12)** and the impl's
**`client_fuel_allocations` IS its seeding mechanism** — both ON-model. The REV ledger UNIFIES with the `Σ` supply
(one consumable, Greg P9); the legacy `SystemVault`/`MakeMint` value-transfer layer is a SEPARATE concern migrating
to the one-token + `Pay(τ)`-TYPE model (Greg P13, deferred — blocked on OSLF). User-provided cons-notation tokens
(signed) **desugar to system tokens**, the signature tracking origin. Minting drops the `sysAuthToken` MONOPOLY (the
genuinely off-model part) → capability + (forward) type-gated; the unforgeability mechanism (DR-13) stays, only the
system-monopoly half is superseded; protocol minting uniform. Genesis supply = `wallets.txt`.

**Remediation direction (docs now; code = W3 in the plan).** Retire the REV-as-distinct-token-ledger framing in the
docs (keep STAKE as a role; value = purse + `Pay(τ)`); replace the `sysAuthToken` monopoly with capability+type-gated
first-class minting that keeps protocol pools **unforgeable** (the unforgeability mechanism, not the monopoly, is
the safety lever — DR-13's unforgeability half stays; its system-monopoly half is superseded here).

**Implementation divergences vs the calculus papers (cross-check 2026-06-15; tracked in pgmcp #481), with decided remediations:**
- **F-A (CONSENSUS, EXTRA):** 6 LL signature connectives (`Threshold/Plus/With/Bang/WhyNot/Lolly`) ride the
  consensus wire (`CasperMessage.proto` `sig_algebra` field 17; `accounting/mod.rs`) but are in NEITHER calculus
  paper (signature grammar = `g | #P | s∘s`; ⊕/&/!/?/⊸ are the VALUE type-logic in `typed_value.tex`, not
  funding-signature formers; Rocq verifies only `And`). **RESOLVED + LANDED (e55769dd; user-ratified):** the
  funding/capability SEPARATION shipped — `reject_capability_connectives` rejects ⊕/&/!/?/⊸ at gRPC ingress
  (`from_proto_cosigned_with_sig_algebra`), `Sig::is_funding_former()` gates the funding envelope, `debug_assert`
  guards `supply_channel`. **Threshold = (A) admission-boundary quorum** (kept, lowered to a flat `Cosigned`; zero
  semantic change, no Rocq change). The funding `Sig` stays `g|#P|s∘s`; the 6 connectives are capability/type-layer
  only. NO wire-format change, NO hard-fork — a red-team verified the funding path was ALREADY paper-faithful
  (`envelope_sig` total to Quote/And; `from_proto` dead on funding) and the dormant decode carried no fund-theft
  (sig-verified, sig-keyed channels); (c) closes the latent confused-deputy gap.
- **F-B (CONSENSUS, INCORRECT):** the acceptance gate folds `min_phlo_price` margin into the correctness inequality
  `Σ_s ≥ Δ_s` for EVERY deploy (`delta_sigma.rs:475`; `block_creator.rs:896`); Def 19 has no margin (the Thm-20
  margin is ONLY the data-dependent `unknown` branch). **RESOLVED + LANDED (e329aed3):** the margin is restricted
  to the `unknown==true` branch; Def-19 `Σ_s ≥ Δ_s` is now exact for resolvable demand (Greg P3/P11 confirmed: the
  over-estimate is needed ONLY for data-dependent interactions; for resolvable demand the bare inequality holds).
- **F-C / F-D (CONSENSUS — RESOLVED, conserving carve landed):** the per-block `FeeExtract` previously credited
  an additive 1-token/deploy mint (and the formal model "collected" `F_v += f` from outside the ledger).
  **Remediation (LANDED, aligned to the paper):** the fee is now a supply-CONSERVING CARVE from the client's OWN
  `Σ⟦c⟧` into `F_v` (`Σ⟦c⟧ -= fee`, `F_v += Σ fee`; clients debited == `F_v` credited, no mint) — the full flow
  `Σ⟦c⟧ → F_v → Σ⟦v⟧` is conserving (Rocq `fee_collect_conserves` / `fee_collect_then_convert_conserves`). The
  per-epoch fee→`Σ⟦v⟧` convert is KEPT (UNCHANGED) but is now BACKED by the carve rather than being an
  unbacked mint (the F-C fix was to BACK the convert, not remove it; `MintingInjection.fee_collect_is_client_backed`).
  Play-side `AdmissionOutcome.fee_debits` (`fee_carve`), replay-side `recompute_settlement_debits(...).fee` — the
  same recompute-from-block discipline as the cost debit. (The removed `FeeCredit`/`recompute_fee_credits` API and
  the additive 1-token/deploy mint no longer exist.)

**Sanctioned `s₀` simplifications (no change):** located stacks `S(I,·)` / `near(I,J)` and first-class token TERMS
are not realized (Remark 11). **Cosmetic:** Rocq `ca_rule4`/`ca_rule5` token-shape labels are transposed vs the
paper's Rule-4/5 (rule SET faithful).

**Mortality lifetime split.** Ratified: compute-funding is fixed at acceptance (*"over-charge, never under-fund"*);
storage solvency drifts over time (rent + the refund-flag lifetime declaration). "Unfunded residual never fires"
is sanctioned (germ/soma/Weismann), but mission-critical residuals must be **germ-line-pinned** (unevictable
on-chain backing) — the pin MECHANISM is unspecified (W4). The rent paper's `phloLimit×phloPrice` precharge is
legacy (DR-9 removed it); rebase rent funding to a located-`Σ`-purse debit (headline numbers survive).

**Cross-refs.** DR-9 (per-COMM cost; escrow removed), DR-13 (system-only minting — monopoly half superseded, the
unforgeability half retained), DR-24 (generic GSLT/OSLF boundary), DR-25; the plan's W3 (token-source + minting)
and W4 (rent/economics) workstreams; pgmcp work item #481.
