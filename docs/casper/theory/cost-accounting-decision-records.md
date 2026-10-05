# Cost-Accounted Rho Calculus — Decision Records

**Status:** Implementation-aligned design record
**Date:** 2026-05-29

Implementation status does not establish upstream Casper ratification.
The [ratification status ledger](../design/cost-accounting-ratification-status.md)
separates branch history, current evidence, and required upstream approval.

**Governing authority:** the specification `publications/cost-accounting/cost-accounted-rho.tex`
("Cost-Accounted Rho Calculus: A Spectral Decomposition of Phlogiston," May 2026) is the **law of the
implementation**. No deviation from its design is admitted unless a bug in the spec is *proven* to exist
and the correction *proven correct*. Two adversarial red-team rounds against the spec found **zero spec
bugs**; the implementation therefore conforms to the spec everywhere it dictates. Extensions *beyond*
what the spec dictates are permitted only when they (1) do not conflict with the spec, (2) introduce no
performance bottleneck, and (3) introduce no security vulnerability.

The two governing papers assume familiarity with the existing F1R3node
architecture. Later records therefore supersede early staging designs when a
paper-level wallet, purse, supply channel, mint, or fee channel is refined by an
existing native service. Historical decisions remain in this file to preserve
the reasoning trail; the newest explicit superseding record is authoritative.

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
the residual-funding edge case. Redemption (`redeemSlashed`, DR-7) writes NEITHER `Σ⟦v⟧` NOR `@W_v` directly.
It clears `mintingHalted` and preserves the collective mint frontier. Only the
normal next-epoch close can re-fund the validator. Proved by `MintingHalt.v`
(`halted_validator_supply_not_increased`, `halted_validator_not_minted`) + `SlashFlow.tla` `Inv_HaltedNotMinted`.
Stage B EXPOSES the `mintingHalted` key + `supply::produce_balance`; Stage C consumes them. See
[cost-accounting-impl/stageb-minting-halt-interface.md](cost-accounting-impl/stageb-minting-halt-interface.md)
Decision 4.

**Alternatives considered.** (a) Keep the immediate Coop-vault transfer — rejected: the spec mandates a
private adjudication channel. (b) VB-block + `mintingHalted` only, no `Σ⟦v⟧` zero — sufficient for consensus
safety, rejected as the interface default (the explicit zero is spec-complete and edge-case-free).

---

## DR-4 — Fee conversion via a conserving `Exchange(c,v)` contract

**Status.** The conservation requirement and blessed two-sided Exchange remain
normative. The former `F_v`, `Σ⟦v⟧`, `@W_v`, and close-block mirror realization
described below is retired and superseded by DR-36 and DR-38. Native fees are an
atomic, conserving SystemVault payer-to-proposer transfer; Exchange remains
available for swaps of already-existing assets or authority carriers.

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

**Historical realization (removed).** Stage D originally used three layers
(design `staged-fee-exchange.md`):
1. The blessed **`Exchange.rhox`** (registered at `rho:lang:exchange`) is the spec's conserving 1:1 swap as
   a persistent JOIN over ordinary **carrier** channels (`exchange_conserves_per_channel` /
   `exchange_total_conserved` / `exchange_requires_both_inputs` in `Exchange.v`). It is genesis-wired exactly
   like `capabilities_registry`; it cannot name or mutate native `Σ`, so client acquisition uses conserving
   first-class stack transfer instead.
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
   `sys:casper:feeConvertList`. DR-14 selected a `Σ⟦v⟧`-only mirror within that
   now-retired architecture. DR-36 subsequently removed both the mirror and the
   duplicated ledgers in favor of canonical SystemVault custody.

---

## DR-5 — Remove precharge/refund; deploys draw from the wallet

**Decision.** Remove the per-deploy precharge/refund machinery (`PreChargeDeploy`/`RefundDeploy`, PoS
`chargeDeploy`/`refundDeploy`, the runtime fan-out). Deploys draw phlogiston from the per-validator wallet;
the acceptance gate commits tokens linearly at acceptance. (For USER deploys the wallet is the signer's
`Σ⟦Ground(pk)⟧`, keyed by the signer's public key so that `Σ⟦signer⟧ == Σ⟦wallet⟧` — the now-literal sense
of "draw from the wallet"; §D2.9 and the DR-13 §D2.9-refinement below.)

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

**Status.** Retired storage realization, superseded by DR-36 through DR-38.
Stable canonical signature identity, point-wise sufficiency, atomic rejection,
and replay determinism remain normative. The separate `Σ⟦s⟧` balance datum,
`produce_balance`, fee pool, and close-block settlement paths do not. Production
supply is canonical SystemVault custody plus authenticated located `CostStack`
cells, and settlement follows each retained native authority region.

**Decision.** Token supply for a signature `s` is a **single balance-carrying datum** `(TOKEN_TAG, n)` on the
unforgeable channel `SignatureChannel::from_sig(s)` (`Σ⟦s⟧`); `supply(s) = n` (0 if absent). It is written
**only** by the Rust `sysAuthToken`-gated mint/settlement path (`produce_balance`), never from Rholang. The
acceptance gate (DR-11) reads `Σ_s` in O(1) from the merged pre-state and commits `Δ_s` by decrementing an
in-pass residual at block assembly; settlement writes `post = pre − ΣΔ_admitted`. The per-COMM token unit
(DR-9) is diagnostic and yields `reconcile().consumed = Δ_s`. The validator's *draw* channel `@W_v` (spec
Appendix B) is DISTINCT from the *supply pool* `Σ⟦v⟧`.

**Spec basis.** §4.6 (per-`s` pool), Appendix A (`Σ⟦·⟧`, `K⟦s:S⟧ = send(Σ⟦s⟧, K⟦S⟧)`), Def 17 (`Σ_s` is a
layer COUNT), §7.6 (compute `Σ` then accept), tex 1677-1729 (tokens *committed* at acceptance; "no
interleaving of acceptance and execution"). Remark 11 defines the mathematical
single-signature limit; DR-41 records why that limit is not the production
settlement topology.

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

**§D2.9 refinement (the user-deploy funding key).** This DR fixes that supply is a balance datum on
`Σ⟦s⟧ = from_sig(s)`; it does not, by itself, fix what `s` IS for a user deploy. The validator/system pools
were always keyed by a GROUND public key (`Σ⟦v⟧ = from_sig(Sig::Ground(pk))`, the mint/fee target — and the
Rocq model `WalletNaming.v` keys the wallet `@W_v := @(*walletTag, validatorPk)` by `pk`/`SGround`, proved
injective). The USER-deploy gate/settlement key, however, was originally derived from the per-deploy WIRE
signature (`envelope_sig = Sig::Quote(Blake2b256(DEPLOY_SIGNATURE_DOMAIN ‖ wire_sig))`), so a deploy's pool
was a fresh, genesis-absent channel and the wallet was never debited. §D2.9 (`3a4e03eb`) re-points the user
funding key to `funding_sig = Sig::Ground(pk)` (single) / the left-associated `Sig::And`-fold of
`Sig::Ground(pkᵢ)` (multi), so `Σ⟦signer⟧ == Σ⟦Ground(pk)⟧ == Σ⟦wallet⟧`. This UNIFIES the user and validator
pools under one keying (`from_sig(Ground(pk))`) and is what makes `client_fuel_allocations` (already
pubkey-keyed at genesis) the binding seed for configured clients. The `deploy_id` continues to derive from
`envelope_sig` (the wire signature) — on-chain identity, NOT a funding key — so it is byte-identical
pre/post-§D2.9 (the decoupling). The unforgeability / balance-datum / disjointness decisions of this DR are
otherwise unchanged. Threshold envelopes EXCLUDE empty-`sig` placeholder cosigners from `funding_sig` (only
signers who actually signed fund the deploy). See `wd-d2-acceptance-gate.md` §D2.9 and
`cost-accounting-impl/d2-9-funding-flow.md`.

**Alternatives considered.** (a) literal nested-send messages, one per token — rejected (O(n) gate-read
bottleneck); (b) a Rust-injected supply name `@sigSupplyCh` bound into `VB`'s continuation — rejected
(re-exposes `Σ⟦v⟧` to the Rholang layer, enlarging the trusted surface); (c) a `sysAuthToken`-gated
`sigChannelOps` system process resolving sig→channel — recorded as a future refinement for in-Rholang
minting contracts (ERC-20-style), unnecessary while the only authorized writer is Rust.

---

## DR-14 — `Σ⟦v⟧`-only fee realization is permanent and spec-complete (the `@W_v` fee-mirror is unnecessary)

**Status.** Retired with the duplicated-ledger architecture it compared. DR-36
supersedes this record's claim of permanence: neither `Σ⟦v⟧` nor `@W_v` is the
native production fee ledger. Its still-valid conclusion is narrower—fee
conversion must conserve value and must not create an unauthenticated mint.

**Decision.** Stage D's fee→phlogiston conversion credits the per-signature supply pool `Σ⟦v⟧` ONLY (the
load-bearing, gate-read pool). It does **not** credit the validator's `@W_v` draw wallet with the converted
fee amount (the "OD-4 `@W_v` mirror"), and the project will **not** build the proposed `rho:casper:feeCount`
Rust→PoS pre-eval data seam to do so. `Σ⟦v⟧`-only is the permanent, spec-complete realization of the spec's
fee feedback loop. (User-ratified after an independent second-opinion Plan-agent review.)

**Spec basis.** The spec has a SINGLE phlogiston location — the wallet `\quot{W_v}` holding a token stack
(tex:2389-2392) — and `Σ⟦v⟧` (the spec's `n_v`) is the *released form* of that stack ("a token stack becomes
a chain of sends … on the signature channel", tex:1906; released by `\drop{t}`, tex:1965). So "fees can be
converted to replenish the phlogiston supply" (tex:3097-3098) is satisfied the moment `Σ⟦v⟧` is credited —
`Σ⟦v⟧` was treated as the supply. The former design interpreted Remark 11's
mathematical single-signature limit as a production storage simplification; that
interpretation is superseded by DR-36 and explicitly corrected by DR-41.

**Rationale.** (1) *No-op:* the `@W_v` purse *amount* is read by nothing — every consumer reads presence, not
quantity (VB `for(phlo<=@W_v){*phlo}` drops it with VH=nil; slash `for(_<-@W_v){Nil}` discards it; no
`getBalance`/arithmetic on a `@W_v` purse exists). Crediting `@W_v` with the fee amount changes no
consensus-observable state. (2) *Safety:* a Rust→PoS pre-eval seam to feed the fee count `f` into the Rholang
`closeBlock` would re-introduce the DR-13 alternative-(b)-rejected Rholang-exposure of a Rust economic
quantity plus a standing replay-rig fragility (the seed `produce` double-counts the rigged play event log →
`ConsumeFailed`), on the most consensus-critical path — all to perform a no-op. (3) *Performance:* `Σ⟦v⟧`-only
is the landed code (zero new work, no new `RwLock`/system-process read, no contention). `@W_v` presence (the
DR-3 halt anchor) continues to be maintained by the epoch mint.

**Historical scope.** The `@W_v` amount-mirror question applied only to the
removed dual-ledger design. Native settlement now draws exact authority from
SystemVault custody and located stack cells without either mirror.

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
4. **Determinism and cache-identity pins added.** `compute_parents_post_state_regression_spec.rs` now asserts that
   disjoint sibling composition is byte-identical under permutations of secondary parents while retaining the selected
   GHOST main parent at index zero. Changing index zero is a different Casper transition because it changes the merge
   base. The parents-post-state cache binds that main-parent identity and sorts only secondary parents, preventing two
   distinct transitions from aliasing while preserving §2.3 order-determinism within the secondary set.

**Outcome — wholly (not partially) satisfied.** D4.2/D4.3's spec intent (§2.3: "merge reduces to the
shared-data-channel residual, deterministically ordered") is fully realized and now regression-pinned; the
only code residue (a dead wrapper) is removed; the fork-risk literal mechanism is correctly declined with
proof. Workstream D's removal obligations (D4.1 precharge/refund + D4.2/D4.3 merge/RtC) are completely
discharged. The dispatcher and merge semantics are unchanged; the cache repair prevents reuse across distinct
main-parent transitions.

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
secp256k1:eth, Schnorr, FROST). This is **spec-faithful**: §4.5's load-bearing requirement is the
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
carry `proc` bodies/payloads) under a thin cost-accounted `system` (`CostAccountedSyntax.v:136`,
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
`active ∧ ¬halted ∧ ¬minted`; Burned fails both `active` and `¬halted` — `PoS.rhox:460-464`), **cannot
re-bond** (`bond` rejects a key in `burnedValidators` — `PoS.rhox:345-361`), and **cannot be
re-redeemed** (the Burned resolution clears quarantine and records the terminal key — `PoS.rhox:788-793`): it is
permanently dead. So the spec's "Upon redemption, minting resumes" is realized by the **restorative**
redemptions (Vindicated = proven right; Guilty = an arrangement, restored with a positive bond); Burned is the
non-restorative case, and a burned validator's permanent halt is exactly "minting … contingent on … good
behaviour" (tex:2368–2369, 3108–3109). The un-halt invariant therefore scopes to active (restored) validators
and correctly excludes Burned (never active).

**Cross-refs.** DR-3 (two-effect slashing + redemption), DR-7 (redemption authority = PoS multisig), DR-12
(validator multi-prover contract). Spec paragraphs *Slashing* / *Stake vs.\ phlogiston*.

---

## DR-19 — Speculative execution-on-receipt (D2-perf, task #11) is NOT implemented: a data-driven, spec-minimal decision

**DR-31 clarification.** The ingress-caching decision remains closed. Production
now performs bounded dependent proof evaluation during block assembly; this is
not execution-on-receipt, does not mutate committed state, and is consumed only
after an opaque context-bound certificate has been produced.

**Context.** Task #11 ("D2-perf: speculative execution-on-receipt + committed I/O gate") proposed pre-executing
gate-passing deploys at ingress into a discardable soft-checkpoint, with a `committed` flag gating I/O sinks
(stdout, peer sends), to hide deploy-execution latency before a proposer assembles a block.

**Decision.** Do **not** implement it. This is a *decision to close*, not a deferral of required work.

**Rationale (verified, not assumed).**
1. **Not spec-required.** The spec mandates accept-then-execute (tex 1726–1729), which the acceptance gate
   already provides: no admitted deploy executes before the funding decision, and rejected deploys never
   execute. The spec does **not** mandate speculative execution-on-receipt — DR-11's "gate-before-speculate"
   is a *constraint on any speculation* (it must not feed acceptance/commit), **not** a requirement to
   speculate (`docs/casper/theory/cost-accounting-impl/wd-d2-acceptance-gate.md` §D2.6: "Nothing consensus-critical
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

**Status.** EXECUTED + landed natively (the `continued-gslt-cost-v2` alignment; see DR-22). The DR-17/DR-20 Option-B native-grammar
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
- ◆ **Impl (C-impl, Phase 3 — protocol-4 realization).** Paper §8 specifies conservative
  reservation and residual refund: reserve Δ_c^max before evaluation, debit the realized κ after
  evaluation, and return Δ_c^max − κ. Protocol 4 realizes that lifecycle directly in RevVault custody.
  Admission atomically fixes the physical-authority bound, canonical-byte bound, and fee allocation for
  every located purse. Settlement burns the realized physical and byte components, transfers the fixed
  fee, and refunds only the unused reservation. The removed legacy `phloLimit × phloPrice` precharge is
  not part of this mechanism.
  - *Exact conservation* — `Settlement.debit_plus_refund_eq_reservation`,
    `refund_le_reservation`, and `refund_zero_when_components_exhausted` prove that exact debit plus
    residual refund equals the fixed reservation. `VaultBackedByteAccounting` refines the debit into
    physical authority, byte transfer, and fee components.
  - *No-stranding* — the acceptance transaction moves the full conservative bound into reserved RevVault
    custody before execution. Evaluation can consume only that immutable reservation; a concurrent top-up
    changes unreserved balance but cannot expand an in-flight bound. Under the block-assembly gate (DR-11),
    every accepted deploy is therefore fully funded without a TOCTOU window.

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
(commits e329aed3, c94e980f, a5a26f5c, e55769dd, e011e0e7, 59c59b1e, 87c012f2).

**Decision (token sources).** A clean-slate re-reading of the `publications/*.tex` establishes the intended model:
**ONE species — phlogiston as signature-indexed first-class token stacks `s:S`** (the old homogeneous phlo is the
degenerate `s₀`-collapse), **plus a typed-value `Pay(τ)`** layer (`TypedCurrency/typed_value.tex`) metering
*transfer* (rivalrous, contraction-rejected) while phlogiston meters *computation*. **Stake** is a distinct
locked-token *role* (consensus weight, slashable; *"phlogiston is not drawn from stake — a separate resource"*,
cost-accounted-rho.tex:3024). **CORRECTED by Greg's 2026-06-15 authoritative answers** (which OVERTURNED the pre-answer "off-model" verdict on
REV / `wallets.txt`): **REV is NOT a separate species and NOT off-model** — it is one of several inconsistent names
(*token / Phlogiston / REV / Rock / F1r3caps*; avoid `F1r3caps`, which collides with F1R3FLY.io *Capabilities*;
canonical = **phlogiston**) for the one system-token denomination. **`wallets.txt` is the platform genesis
value-allocation trust root (Greg P12)**. The native cost ledger cannot consume it directly: `wallets.txt` is keyed
by REV address while `Σ` is keyed by canonical public key. The implementation therefore commits an explicit,
canonical `client_fuel_allocations: [(PublicKey, amount)]` payload. Test builders can derive that payload because
they possess the test keypairs; a production node cannot invert an address to recover the key. The one-denomination
decision requires an explicit conserving reallocation between `SystemVault` custody and `Σ`; it forbids treating
both independently stored representations as spendable copies of the same allocation. User-provided cons-notation
stacks retain their signature provenance. The `SystemVault`/`MakeMint` and `Pay(τ)` realization is a separate
platform layer not specified by the two cost-calculus papers, while native cost-purse unforgeability remains the
DR-13 safety boundary.

**Realization boundary.** Retire the REV-as-a-different-denomination framing while retaining separate custody and
cost-purse representations until a paired, replay-authenticated reallocation joins them. Keep stake as a role and
native cost purses unforgeable. A transfer between SystemVault and `Σ` must debit one representation exactly when
it credits the other; neither genesis configuration nor lollipop syntax is such a transfer.

**Implementation divergences from the calculus papers (cross-check 2026-06-15), with selected corrections:**
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
  (sig-verified, sig-keyed channels); (c) closes the latent confused-deputy gap. **§D2.9 update:** the funding
key was subsequently re-pointed from `envelope_sig` (the wire-sig `Quote` atom — now `deploy_id`-only) to
`funding_sig = Sig::Ground(pk)` / the `And`-fold of `Sig::Ground(pkᵢ)` (the signer's pubkey wallet,
`Σ⟦signer⟧ == Σ⟦wallet⟧`; DR-13 §D2.9-refinement). F-A is UNAFFECTED: `is_funding_former()` now gates the
`funding_sig` envelope, and `Ground`/`And` are both funding formers — indeed the funding key is now literally
the `g` ground atom (the public key), making the funding path MORE paper-faithful (`g` = "an Ed25519 public
key / secp256k1 key hash"). The connective-separation argument holds identically for the `Ground`-keyed
funding envelope.
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
and W4 (rent/economics) workstreams.

## DR-28 — Cross-group cumulative-demand bound (live residual ledger) + the §D2.9-R2 no-weakening closure correction

**Context.** Two red-team passes on the §D2.9 funding gate found a soundness hole and a latent inconsistency:

1. **TM-CA-165 (cross-group over-admission).** The acceptance gate admitted each per-signature group against a
   STATIC per-group effective supply (`effective_supply_with(raw, …)` computed once, each group re-reading its
   own slice). Two DISTINCT cosigner sets sharing a component wallet — `{A,s}` and `{B,s}` both drawing
   `Σ⟦Ground(s)⟧` — were each admitted against `s`'s full balance, so their COMBINED demand could exceed it.
   The deploys run unmetered-for-liveness; `compute_settlement_debits` caps the post-state (no conservation
   break, play↔replay agree), so `ΣΔ − Σ⟦Ground(s)⟧` units of un-funded compute executed undetected —
   honest-proposer-reachable. Linearity admits no contraction: a shared token funds at most one consumer.
2. **TM-CA-166 / §D2.9-R2 (single-component no-weakening over-credit).** `effective_supply_with` credited a
   single component with the compound pool (`effective[s₁] = Σ_{s₁} + Σ_{s₁∘s₂}`), but a single-sig group
   settles only on its own pool — so once a compound pool is provisioned, a single-sig deploy could be admitted
   against a capacity the settlement cannot honor (underflow). This is **weakening** (discharging a single-`s₁`
   demand by consuming a compound token discards the `s₂` authority).

**Decision.**

- **(R2) Correct the closure, not the settlement.** Drop the single-component over-credit from
  `effective_supply_with`; a single component passes through at its raw balance. Rocq
  `CAJoinConservation.join_no_weakening` and the paper's "Weakening Is Forbidden" already forbid the operation;
  the over-credit was a code-only outlier matching no proof/model/spec. Funding a single component from a
  compound requires the explicit, observable `Split` reduction crediting `Σ⟦s₂⟧` (the runtime Splitter), never
  a static admission credit. *(R1 — extend the settlement to split-with-byproduct — was REJECTED: it would mint
  `s₂`-authority no demand requested and require re-proving the settlement conservation; R2 keeps the verified
  core untouched and the code merely matches the model.)*
- **(TM-CA-165) Bound admission with a LIVE cross-group residual ledger.** The gate's admission DECISION and the
  replay re-verification each run one `remaining` ledger (seeded `raw.clone()`) over the `SigKey`-ordered
  groups, drawing each shared component down as groups are admitted; a group is admitted only if its folded
  `cost + fee` fits its effective supply read from the drawn-down ledger. This is the linear-resource discipline
  (admit greedily in canonical order, draw the shared stack down) — **not** reject-both-across-groups, which
  would destroy fundable demand (both could be partially funded) and violate maximality. Single-sig caps at its
  own-pool live residual (consistent with R2).
- **§7.7 admit-in-canonical-`SigKey`-order**, not reject-both-across-groups. The order is a pure function of the
  cosigners' public keys (deterministic, identical play↔replay), so there is no determinism hazard; option (a)
  also admits strictly more than (b), maximizing liveness.
- **Margin-free replay.** The replay re-verification checks the hard `folded demand ≤ live capacity` bound
  WITHOUT the margin (the margin is a play-side admission tightening that only REMOVES deploys; a removed deploy
  is not in the block, so replay never needs it — matching the pre-existing margin-free recompute).
- **Settlement untouched.** `compute_settlement_debits` was already cross-group-correct and byte-identical
  play↔replay; the gate, replay, and settlement now share the `group_shape_from` / `group_capacity` /
  `index_decompositions` helpers (single source of truth, no drift). The folded-`cost+fee` admission ledger
  DOMINATES the two-pass cost-then-fee settlement on every pool (the flat fee draws a single component, a subset
  of the cost pair-draw), so `admission-fundable ⟹ settlement-safe` — capturing the admission draws as debits
  (the rejected INVASIVE variant) would have forked play vs the two-pass replay settlement.

**No-execution-serialization.** The fix is pure bookkeeping over the finite shared stacks, run BEFORE execution;
admitted deploys still execute concurrently and unmetered-for-liveness. Linearizing the *admission accounting*
over a finite resource is the required linear-logic semantics (no contraction), not an artificial serialization
of compute (cf. the no-serialization mandate).

**DR-31 supersession.** The live residual ledger and no-weakening decision remain
binding. The unmetered-execution statement does not: the retained bounded play
and certificate-constrained replay use finite authority-derived capacity, and
exhaustion cannot authorize a deployment.

**Migration.** Funding is consensus-mandatory. Missing pools are zero and reject any
positive cost-plus-fee reservation. Validators bootstrap through
`initial_phlogiston`; explicitly configured clients bootstrap through
`client_fuel_allocations`. R2 remains a no-op on post-states where
`Σ_compound = 0`, but the shared-component residual rule applies uniformly as
soon as compound authority exists.

**Verification.** Rust: 31 acceptance lib tests (6 new cross-group) + `gate_decision_replay_determinism`,
`multi_sig_funds_balanced`, `redeem_outcomes_and_multisig_gate`; `compound_debit_play_replay_byte_identical`
confirms the settlement refactor is byte-identical. Rocq (axiom-free): `cross_group_draw_le_supply`,
`cross_group_admission_sound`, `competing_funding_shared_component`. TLA+ (TLC PASS): `AdmitGate` threads the
shared residual; `Inv_CrossGroupAdmissionBounded`, `Inv_SecondGroupDrawMatchesDemand`. Sage: 12,605-trace
cross-group admission sweep, 0 sound + 0 necessity violations.

**Cross-refs.** §D2.9 (the funding-key correction this extends), DR-9 and DR-31 (per-COMM cost with a finite authority-derived runtime backstop),
DR-11 (block-assembly gate), TM-CA-153 / TM-CA-164 (the over-admission lineage this completes), TM-CA-165 /
TM-CA-166. Commits `eec6e323` (R2), `8575e7c0` (cross-group gate), `4d848faf` (formal verification).

## DR-29 — Overflow → deterministic rejection; the i64-bounded ledger (over/underflow modeled); diagnostic-refinement scoping

**Context.** A formal-verification faithfulness sweep of the supply ledger found three gaps between the Rust
runtime and the Rocq model:

1. **Overflow halt (item 2494 / TM-CA-167).** `close_block_deploy::dual_write_supply` credits supply pools with
   `old.checked_add(amount).expect(..)` at four close-block sites (the `Σ⟦v⟧` epoch mint, the fee-carve running total,
   the `F_v` collection credit, and the fee→`Σ⟦v⟧` convert). Near `i64::MAX` the `expect`
   PANICS — non-deterministic across nodes (a validating node crashes on the block instead of rejecting it),
   halting the network. The UNDERFLOW side is already deterministic (`recompute_and_verify_admission` raises
   `ReplayAdmissionMismatch`); the overflow side was asymmetric.
2. **`nat`-vacuous conservation (item 2505).** `MintingInjection.v` and the settlement conservation theorems
   model balances as `nat`. `nat` is unbounded, so overflow is UNREPRESENTABLE and the over/underflow branches the
   `checked_*` code actually guards are domain-excluded — the conservation guarantees say nothing about the
   adversarial arithmetic boundary.
3. **Over-broad replay-auth naming (item 2509 / TM-CA-151).** `RuntimeBudgetRefinement.v`'s
   `RbCostAccountedReplay` mode is digest-INCLUSIVE, broader than deployed consensus (TM-CA-151 dropped the
   per-op digest from consensus; it is diagnostic/telemetry only). The name implied a consensus obligation.

**Decision.**

- **(item 2494) Symmetric deterministic rejection, not a business cap.** Every credit site routes through a
  `checked_supply_credit` helper that returns the deterministic `ReplayFailure::ReplaySupplyOverflow` (mapped to
  an INVALID block in `interpreter_util`) instead of panicking. The overflow is a pure function of the block +
  pre-state, so play and replay reject the same block identically. There is deliberately **no** economic
  `SUPPLY_MAX` constant — the `i64` bound IS the cap (a technical machine bound, not a business parameter; a
  `SUPPLY_MAX` was rejected per *prefer-technical-over-business*). The invariant: *Σ operations use checked
  arithmetic that deterministically errors on overflow (never wraps, never panics).*
- **(item 2505) A bounded-`ℤ` ledger that MODELS both branches (`BoundedLedger.v`).** The pool quantity is
  re-modeled in `ℤ` bounded to the `i64` non-negative range, with `checked_add_i64` / `checked_sub_nonneg` that
  return `Some` (conservation) OR `None` (deterministic rejection). The credit dichotomy
  (`checked_add_i64_conserved_or_rejected`, the fold `supply_credit_conserved_or_rejected`), the non-wrap
  guarantee (`checked_add_i64_never_wraps`), and the debit dichotomy
  (`checked_sub_nonneg_conserved_or_rejected`) make the adversarial branches PROVEN, not excluded. Crucially the
  existing `nat` laws are NOT weakened: `checked_add_i64_matches_nat` proves the `nat` model is exactly the
  in-range restriction of the bounded model, so the bounded layer strictly ADDS the overflow branch. *(Re-typing
  the existing `nat` theorems in place was REJECTED — it would churn the whole conservation corpus and the
  headline-assumptions gate; a companion module that carries the guard premise the code enforces, and bridges to
  the `nat` happy path, keeps every existing Qed-closed theorem intact.)*
- **(item 2509) Diagnostic-refinement scoping.** `RbCostAccountedReplay → RbDiagnosticRefinement` (obligations
  `rb_diagnostic_refinement_requires_commitment` / `rb_diagnostic_refinement_rejects_absent_commitment`), with a
  disclaimer ported from `replay_auth_model.sage`. `rb_full_replay_payload_equiv` is SPLIT into a consensus-core
  equivalence (`rb_full_replay_payload_consensus_equiv` — RSpace events + `consumed_units` + status) vs a
  diagnostic-only digest equivalence (`rb_full_replay_payload_diagnostic_equiv`), proven equal to their
  conjunction (`rb_full_replay_payload_equiv_split`); `rb_full_replay_payload_consensus_coarser_than_full`
  witnesses the digest is diagnostic, not consensus. Theorem CONTENT is preserved; only scope naming is clarified.

**Verification.** Rust: `close_block_deploy::tests::supply_credit_overflow_is_deterministic_error_not_panic` (+
in-range/exact-sum and max+max variants) — a mint/convert at `i64::MAX` yields the deterministic error, not a
panic; `casper` compiles with the new `ReplayFailure::ReplaySupplyOverflow` variant and its `interpreter_util`
invalid-block arm. Rocq (axiom-free, `CA_ENFORCE_PROOFS=1` gate PASS — every headline `Closed under the global
context`): `BoundedLedger.v` (`checked_add_i64_conserved_or_rejected`, `checked_add_i64_never_wraps`,
`checked_add_i64_none_iff_overflow`, `checked_sub_nonneg_conserved_or_rejected`, `checked_add_i64_matches_nat`,
`supply_credit_conserved_or_rejected`, `bounded_settlement_conserved_or_rejected`,
`bounded_fee_convert_conserved_or_rejected`); the renamed/split RBR theorems
(`rb_diagnostic_refinement_requires_commitment`, `rb_diagnostic_refinement_rejects_absent_commitment`,
`rb_full_replay_payload_equiv_split`, `rb_full_replay_payload_equiv_implies_consensus`,
`rb_full_replay_payload_consensus_coarser_than_full`). `MintingInjection.v` and `MintingHalt.v` (the halted ⇒
mint-is-identity mandate) and `Exchange.v` (1:1 conserving, requires-both-inputs) are unchanged and stay
Qed-closed.

**Cross-refs.** TM-CA-167 (the overflow-halt vector), TM-CA-151 (the digest-diagnostic scoping this names),
TM-CA-153 (the underflow admission gate this makes symmetric), DR-13 (Σ⟦s⟧ is reducer-unwritable), DR-28 (the
settlement conservation this bounds). Verification-doc §4.4.1. The one-system-token model (canonical
*phlogiston*) is untouched.

## DR-30 — Genesis is the authenticated authority trust root; admission follows verified replay

**Status:** partially superseded by DR-36 and DR-59. Genesis remains the trust root.
The separate supply payload, block-one mirror, and post-genesis initial grant are retired.

**Context.** Making the funding obligation mandatory exposed a circular bootstrap that the former
absent-wallet bypass had hidden. Block assembly checked the signer pool before executing block 1, while validator
`initial_phlogiston` and client `client_fuel_allocations` were credited only by the block-1 close hook. The first
signed deployment therefore had to spend authority that did not exist until after that deployment was admitted.
This manifested as `NoNewDeploys`, pending heartbeats, stalled finality, and downstream query failures. A second
hazard was that historical genesis replay could depend on node-local configuration. Nodes with different local
allocation lists could reconstruct different roots, and a replay cache that did not bind the allocation payload
could conceal the mismatch.

The paper's funding obligation requires available authority before execution: for every signer $`s`$, admission
requires $`\Sigma\llbracket s\rrbracket \geq \Delta_s^{\max} + f_s`$. It does not authorize an implementation to
manufacture that authority after the gate. The genesis transition is therefore the only non-circular trust root
for initial validator and client authority.

**Decision.**

1. Genesis combines every bonded validator's `initial_phlogiston` and every configured client grant by public-key
   bytes with checked `i64` addition. It omits zero totals and emits a strictly increasing, duplicate-free list of
   positive allocations.
2. The runtime writes those balances into $`\Sigma`$ before taking the final genesis checkpoint. The exact ordered
   list is serialized in `F1r3flyState.genesis_supply`; it is block-hash input, not display metadata.
3. Ceremony validators derive the same canonical list from their expected parameters and reject a candidate whose
   commitment differs. Historical replay uses the authenticated block payload, never node-local allocation
   configuration.
4. Replay rejects empty public keys, non-positive amounts, duplicates, and non-increasing order before any cache
   lookup. Non-genesis blocks must carry no genesis allocation payload. The replay-cache fingerprint binds the
   exact ordered payload, and genesis bypasses the pre-state-only state-hash cache.
5. Block 1 always installs the PoS `initialPhlogiston` draw in `@W_v`, including when `epoch_length = 1`. Rust does
   not credit $`\Sigma\llbracket v\rrbracket`$ again because the matching authority is already in the genesis root.
   Later epoch mints continue to update both the PoS draw state and $`\Sigma`$.
6. Startup rejects a non-positive epoch length, a zero cosigner limit, negative initial or epoch phlogiston,
   malformed or empty client keys, negative client grants, and duplicate-client addition overflow. Invalid economic
   parameters do not reach Rholang execution.

Configuration canonicalization and replay validation intentionally have different contracts. Configuration may
contain repeated principals and zero grants; genesis combines or removes those semantic no-ops. Once committed,
the payload must already be canonical. Replay never silently repairs authenticated bytes, because doing so would
allow different block bodies to share state or cache identity.

**Rejected alternatives.**

- Restoring an absent-wallet or deployment-kind bypass was rejected: it violates the universal funding obligation,
  treats absence as unbounded authority, and makes proposal differ from replay.
- Crediting clients or validators during block-1 close was rejected: admission precedes close, so the dependency is
  circular, and replay would still require node-local configuration.
- Reading allocation files during historical replay was rejected: files are not authenticated consensus input.
- Sorting or combining the payload inside replay was rejected: canonicalizing malformed authenticated input before
  hashing can alias distinct bodies in replay caches.
- Increasing timeouts or weakening integration assertions was rejected: those actions hide the stalled state
  machine and do not repair the authority transition.

**Invariants.**

- `GenesisCommitIsExact`: the committed allocation map equals the canonical initial authority map.
- `AdmissionRequiresGenesisAgreement`: no signed deployment is admitted before replay reconstructs the committed
  genesis authority.
- Every committed entry has a non-empty public key, positive amount, and a key strictly greater than its predecessor.
- Genesis play and replay produce the same post-state root from the same authenticated body.
- Block 1 changes the PoS draw state but does not increase the already-seeded validator supply pool.
- Changing allocation bytes changes the block hash and replay-cache key; malformed ordering cannot hit a valid
  replay entry.

**Compatibility and migration.** `genesisSupply` is an additive protobuf field, so older decoders ignore it and an
absent field decodes as an empty list. Its consensus meaning is activation-scoped: a shard enabling mandatory
cost-accounted admission must create or migrate to a genesis/state root containing the required authority. Replaying
a pre-activation genesis with an empty field remains byte-compatible, but it does not invent funding. Operators must
coordinate the genesis block, client allocations, validator phlogiston parameters, and node binary as one protocol
activation.

**Verification.** Rocq proves allocation-total permutation invariance, duplicate combination, replay-preserved
admission, admission only after verified genesis, and that the block-1 draw changes the wallet ledger without
changing genesis supply in `EndToEndAuthority.v`. TLA+
`EndToEndCostConsensus.tla` checks the safe genesis-to-finality path; the genesis-mismatch configuration must refute
`AdmissionRequiresGenesisAgreement`, and the double-credit configuration must refute
`InitialDrawDoesNotCreditSupply`. Rust tests bind allocation bytes into the block hash and replay-cache key,
reject every non-canonical shape after a valid cache entry exists, reconstruct the genesis root, reject ceremony
mismatches, and execute block 1 with `epoch_length = 1` and distinct initial/epoch amounts. The current
`block_one_epoch_issuance_does_not_repeat_genesis_allocation` regression checks that boundary. The first-block Casper
smoke and full Casper integration suite cover the original `NoNewDeploys` failure class.

**Cross-refs.** RHO Definition 19 and Appendix B.1/B.3; DR-11 (admission), DR-13 (Rust-owned supply names), DR-28
(exact settlement), TM-CA-168, UC-CA-154/162, CA-P-077, and
`cost-accounting-impl/end-to-end-authority-settlement.md`.

## DR-31 — State-bound dependent admission closes ambient-cost undercount and bounds every user evaluation

**Context.** The branch made DR-11 funding mandatory and used
`delta_sigma::demand` as the production certificate. That analyzer counts signed
layers and submitted `COMM` syntax in the normalized deployment. A send into a
persistent registry, vault, bridge, or system contract can awaken a continuation
already stored in RSpace. Those ambient continuation events are absent from the
submitted `Par`, so the structural result is a lower bound for that execution,
not a conservative upper bound. Observed regressions included structural versus
realized costs of 9 versus 123, 3 versus 258, 8 versus 113, and 108 versus 1008.
Close settlement correctly rejected the excess, but by then the proposer had
already built a block that validators could not reproduce as funded. Repeated
invalid blocks produced liveness stalls, unknown-root cascades, unauthorized
slash evidence, negative fault tolerance, and query failures.

The same branch also replaced finite `phlo_limit` execution with
`Cost::unsafe_max()` after structural admission. That made the incorrect bound
load-bearing and removed the runtime's finite resource backstop. `dev` did not
show this failure because it did not enforce this branch's new
structural-certificate-plus-exact-settlement protocol. The regression was
introduced by the incomplete composition of those branch changes, not by parent
arrival order or the multi-parent consensus theorem.

The earlier verification missed the defect because its abstraction assumed an
already-valid `CostReservation` for each `ExecutionChoice`. It proved that a
valid reservation conserves supply and that replay agrees with commit, but it
did not model how the Rust implementation produced the reservation from a term
plus resident RSpace state. Example tests concentrated on closed terms, where
submitted structure contains every continuation. This was an assumption-to-code
gap: the proof premise was true in the model and false for ambient contract
invocation.

**Decision.** Production uses the dependent-proof strategy permitted by
`continued-gslt-cost-v2.tex` §Data-dependent interaction, while retaining the
conservative structural and generic GSLT proof paths.

1. Canonically sort the complete cosigned candidate sequence and bind all proof
   evaluation to the merged pre-state, exact `BlockData`, and invalid-block map.
2. Derive a finite evaluation capacity for each funding group from effective
   supply after the fixed fee. Run proof evaluation in a spawned scratch runtime.
   An out-of-phlogiston result is an exhausted proof, never an admissible
   deployment.
3. Record the exact cost and adjacent pre/post roots for every completed
   candidate. Validate envelope identity and require the chain to begin at the
   authenticated authority root.
4. Apply exact cost-plus-fee funding against the live cross-group residual
   ledger. Remove exhausted and underfunded candidates, then rerun the retained
   sequence. The candidate set strictly shrinks on every nonterminal iteration,
   so the process terminates in at most $`n+1`$ passes.
5. Carry the result in an opaque `StateBoundAdmission` token bound to pre-state,
   block context, invalid blocks, canonical envelopes, root chain, cost, debit,
   and fee evidence. Checkpoint construction consumes that token without a
   duplicate proof run. Other checkpoint entry points must construct the token
   internally.
6. Treat the final completed bounded execution as the committed user transition.
   Preserve its exact top-level causal-event witness, continue system settlement
   from its exact post-state root, and never perform a second unconstrained play.
7. Replay reconstructs the complete cosigned envelope, derives the capacity
   from authenticated state, rejects exhaustion, and checks the same evidence.
   No proposer-supplied cost or settlement map is trusted.

For a state-bound certificate the dependent proof is exact for the authenticated
state and inputs, so $`B=\kappa`$ and unused reservation is zero. A conservative
structural or external GSLT certificate still satisfies
$`\kappa\leq B=\Delta^{\max}`$ and retains the paper's refund identity
$`B-\kappa`$. Both are instances of the same finite-bound funding rule. Direct
MeTTaIL integration remains the user-authorized exception; the GSLT traits and
proof-checker boundary are the complete integration surface it will implement.

**Rejected alternatives.** Increasing timeouts or memory ceilings does not
repair an invalid certificate. Clamping realized cost to the structural count
would make accounting dishonest. Charging only submitted sends would omit work
performed by invoked contracts. Treating `unsafe_max` as a liveness feature
admits unbounded resource consumption. Accepting an exhausted proof makes the
verdict depend on scheduler progress. Trusting proposer-supplied evidence breaks
independent validation. Serializing parent arrival does not address a
deployment-local proof error and would contradict the multi-parent design.

**Formal verification.** `StateBoundAdmission.tla` models bounded-play proof,
retention of that play as the committed transition, certificate-constrained
replay, capacity completion, schedule permutations, and settlement. TLC
explores 162 distinct safe states. Three required negative controls
independently violate the invariants when structural undercount,
duplicate-unconstrained-play drift, or exhausted admission is enabled.
`StateBoundValidatorConvergence.tla` explores three independent validators with
different arrival orders, reducer schedules, local roots, and block contexts.
The schedule set deliberately contains both reorderings of one trace and a
different event set with a different cost. The model proves that every accepted
validator uses the authenticated context, canonical order, and exact certified
causal witness and that all accepted validators agree. Its context, order, and
local-schedule controls produce counterexamples when any refinement check is
removed. `EndToEndAuthority.v`
proves exact capacity/funding equivalence,
exhaustion non-certifiability, certificate-funded commit cost, root-chain
continuity, admitted-list funding, and exact settlement conservation without
added axioms. `settlement_model.sage` exhaustively checks the fixed point over
three candidates, including termination, disjoint admitted/rejected sets,
capacity completion, and cost-plus-fee funding.

**Implementation verification.** Example tests cover resident ambient cost,
root/envelope substitution, exact block-context binding, retained-play/replay
witness equality, and the absence of a second unconstrained execution.
Property tests cover the exact cost-plus-fee boundary.
Registry, vault, bridge, replay, slashing, merge, and full Casper integration
tests exercise the concrete path. Repository CI runs workspace tests,
all-target Clippy with warnings denied, formatting, and configured TLA+ and
Rocq checks. Sage enumeration is a separate local verification step.

**Cross-refs.** Cost-Accounted Rho §§8.6–8.9; Continued Interactive GSLTs and
the Cost Monad §Data-dependent interaction; DR-5, DR-9, DR-11, DR-13, DR-28,
DR-30; TM-CA-169; CA-P-038, CA-P-039, CA-P-042, CA-P-184; UC-CA-167; and
`cost-accounting-impl/end-to-end-authority-settlement.md`.

## DR-32 — Atomic RSpace COMM is the sole calculus execution-cost event

**Context.** Reducer-entry charging treated send and receive introductions as
cost events. Whether a match was discovered by the producer or consumer path,
whether an input remained unmatched, and how Tokio scheduled reducer tasks then
changed the charged event multiset. The structural analyzer and closed examples
repeated the same assumption, so play and replay could disagree even when they
performed the same semantic Rho-calculus reductions.

**Decision.** Native COMM accounting is observed exactly once at RSpace's locked
complete-match boundary. The observer reserves one authority unit after match
selection and before the COMM event log or tuplespace is mutated. Unmatched I/O
consumes zero calculus execution units. Binary COMM and N-way join each consume
one. DR-47 separately charges canonical quantitative bytes for every RSpace
introduction and committed transfer, so unmatched stored I/O is not free in the
protocol-4 total. The stable identity uses
the matched consume/produce hashes and persistence metadata; reducer source
paths, redex identifiers, local indices, probe order, and trigger side are
excluded. Observer rejection leaves cost, event history, and RSpace state
unchanged. Play and replay install the same observer.

`delta_sigma` remains a conservative count of potential communication
introductions for closed, non-persistent submitted terms. It is not an exact
runtime event trace. Persistent I/O and unresolved dequotation are structurally
unprovable and use DR-31's finite state-bound evidence path.

**Rejected alternatives.** Canonically sorting reducer attempts cannot turn
non-semantic introductions into semantic reductions. Charging both endpoints
double-counts one COMM. Charging only one syntactic endpoint remains
trigger-dependent. Clamping play or replay cost hides a state-transition
mismatch. Moving observation after mutation makes out-of-phlogiston rejection
non-atomic.

**Verification.** `AtomicCommAccounting.tla` checks the unmatched-zero calculus
projection, exact successful-COMM execution units, join-arity independence,
trigger-order convergence, finite capacity, and replay equality.
`VaultBackedByteAccounting.tla` checks the additional introduction, transfer,
and trace-byte product sum. `AtomicCommRejection.tla` checks the
zero-capacity rollback boundary. The introduction-charging configuration is a
required counterexample to `ExactCommCost`. `AtomicCommAccounting.v` proves the
same algebraic obligations without added axioms. RSpace tests exercise both
trigger sides and join rollback; Loom exhaustively checks charge-once and
rejection-before-mutation across both two-thread arrival orders; Rholang example and property tests connect
structural bounds to exact runtime matches; DR-31 models authenticated
state-bound admission through settlement and independent-validator replay.
`VaultBackedByteAccounting.v` proves that the quantitative refinement preserves
the one-unit COMM projection, hard reservation ceiling, top-up snapshot, and
play/replay equality.

**Cross-refs.** RHO Rules 1–5 and transaction atomicity; MON
data-dependent interaction; DR-9, DR-11, DR-28, DR-31; CA-P-185…188; and
`cost-accounting-impl/end-to-end-authority-settlement.md`.

## DR-33 — Recovery is exact-occurrence, expiry-bounded, and led from finalized state

**Historical status.** DR-55 supersedes the rejected-buffer leadership rule.
DR-33 still controls exact-source admission, strict expiry, and ordinary
inclusion leadership.

**Context.** Publication-aligned linear funding makes concurrent copies of one
signed deploy contend for the same authority. Independent validators may place
that deploy in distinct source blocks before either occurrence is finalized.
The first occurrence repair serialized exact-source tombstones, but proposal
admission still projected all rejection records to a signature-wide Boolean.
One rejected source could therefore authorize another copy while a different
source remained active. Recovery also bypassed the block-height lifespan and
used authority signals from transient parent selection. Some validators could
suppress heartbeat-only blocks while they waited for a selected validator.

Those rules formed a feedback loop: multiple validators created repeated source
occurrences, merge blocks recorded multiple exact tombstones for the same
signature, and recovery kept admitting the signature until it crossed its
lifespan. The first expired recovery proposal was objectively invalid, proposal
failed on every validator, finality stopped, and later API and memory failures
were consequences of the halted shard.

**Decision.** A deploy retry is a transition over occurrence state, not a
signature flag. Let $`O_d`$ be the source occurrences visible from the selected
parent closure and $`T_d`$ its exact tombstones. The active set is
$`A_d = O_d \setminus T_d`$. Recovery requires $`A_d = \varnothing`$ and the
same strict lifespan interval as ordinary admission,
$`v_d < n < v_d + L`$. At the boundary, both local stores purge the deploy.

DR-33 originally selected one validator from each committed finalized-height
view to package rejected-buffer work. DR-55 supersedes that selection rule.
The rejected source carrier sender now keeps retry custody. Only that owner can
package the carrier after the shared floor gate opens. Distinct carrier owners
can retry independent work concurrently. Ordinary inclusion and heartbeat
proposals retain their deterministic finalized-view leader rotation.

Proposal height closes the ordinary lifespan. Required parent,
visible-source, and finalized-ancestry bodies and finalized metadata fail
closed when missing.

**Rejected alternatives.** A raw union of rejection signatures loses source
identity. Selecting the main-parent sender or the first observed parent changes
with DAG view. Allowing a validator without carrier custody to retry can create
a duplicate-occurrence storm. Owner-scoped retries remain safe only with exact
occurrence identities and eventual propagation.
Extending the lifespan for retries lets invalid blocks be created after the
validation boundary. Suppressing non-leader heartbeats deadlocks when the
elected leader is offline. Longer timeouts or a larger memory ceiling only
delay the cascade.

**Formal verification.** `DeployRecovery.tla` composes exact-source reduction,
strict proposal-height expiry, concurrent retry snapshots, delayed exact-source
visibility, owner custody, and independent owners. Its safe configuration
proves source suppression, lifespan closure, owner custody, source-distinct
parallel recovery, and bounded pending work. The missing-custody control
produces the expected counterexample.

`StaleSiblingRecovery.tla` proves owner-only rehome after exact frontier
settlement. Its non-owner control produces the expected counterexample. Rocq
proves inclusion-leader uniqueness, carrier-custody uniqueness, and independent
authorization for distinct carrier owners without added axioms.

**Implementation verification.** Snapshot construction derives both canonical
won and rejected signature sets from the same occurrence reducer. Rejected
buffer probing and selection apply the same strict expiry predicate. Received
merge validation gives rejected-buffer custody only to the source carrier
owner. A source owner can rehome a selected retry without treating an excluded
historical self-chain source as active. Other validators remain free to propose
heartbeat-only blocks. Unit, property, Loom, and integration tests cover
surviving sources, all-source tombstones, observation order, missing bodies,
expiry boundaries, owner custody, independent-owner concurrency, and per-block
rejection counts. Multi-validator system tests retain exact block-hash
agreement, finality, and resource-ceiling assertions.

**Cross-refs.** Cost-Accounted Rho deployment atomicity and linear funding;
DR-11, DR-28, DR-31, DR-32; TM-CA-171; O1–O14 in
`deploy-occurrence-specification.md`; and
`formal/tlaplus/deploy_recovery/README.md`.

## DR-34 — The active protocol has one fresh-genesis version authority

**Historical status.** DR-34 established the single-version authority chain at
protocol 3. DR-47 preserves that authority chain and supersedes only the active
version value: protocol 4 is now the sole running version because it adds the
quantitative byte certificate and witness. Versions 1 through 3 remain
non-runnable historical encodings.

**Context.** DR-6 selected fresh-genesis deployment, and D3 removed and reserved
the legacy deploy-cost wire fields. The implementation nevertheless left a
protocol-1 literal in genesis candidate construction while configuring the
running cost-accounted shard as protocol 2. Proposers emitted version-2 blocks,
but peer-interest filtering compared them with the version-1 approved genesis.
Honest validators discarded one another's blocks before validation and stopped
converging.

**Decision.** Protocol 3 is the only active protocol supported by this binary.
The configured version flows through genesis candidate construction and
approver checks. Approved-block validation rejects every noncurrent version
before startup, then the approved version is adopted into the authoritative
running shard configuration. Proposal, recovery, and peer reception all read
that same running value. Protocol 1 and protocol 2 remain historical encoding
identifiers for defensive record validation and formal composition, not runnable
accounting modes. Exact rejected-deploy dispositions begin at protocol 2; exact
per-execution state-effect provenance begins at protocol 3. Migration is a fresh
protocol-3 genesis; no block-height trigger, dual
charging engine, feature flag, or mixed-version interval exists.

**Rejected alternatives.** Accepting both versions would require retaining two
wire schemas and two execution semantics even though D3 deliberately reserved
the removed fields. Treating the approved header and local configuration as
independent authorities recreates the disagreement. Rewriting an approved
header at startup invalidates its signatures. Comparing peers with the approved
genesis while proposing from local configuration preserves the split. A
block-height switch contradicts DR-6 and makes historical replay depend on a
legacy engine that was intentionally removed.

**Formal verification.** `ProtocolVersionLifecycle.tla` covers ceremony,
approval, fail-closed admission, adoption, proposal, and reception. Its current
configuration passes end to end; legacy and unknown approved-block configurations
pass by reaching the rejected state. Five unsafe controls reproduce stale
ceremony, non-adoption, proposer bypass, the configured-current/approved-legacy receiver
split, and unsupported startup. `ProtocolVersionLifecycle.v` proves support-set
exactness, mutation-free rejection, uniform adoption, receiver agreement, and
current ceremony/recovery composition without added axioms. The capstone is
`finalized_floor_protocol_lifecycle_correct`.

**Implementation verification.** Example tests cover current adoption,
mutation-free rejection of legacy and unknown versions, approver rejection of a
mismatched candidate, approved-block validation, and receiver use of the running
version. An arbitrary-`i64` property test proves that the support predicate is
true exactly for version 3. The multi-node deploy-summary regression verifies
that every node admits the same proposal version and converges on one deploy
block.

**Cross-refs.** DR-6, DR-30, DR-31, D3 in
`cost-accounting-impl/d3-replace-phlo-with-tokens.md`,
`finalized-floor-specification.md` §5.2, and
`formal/tlaplus/deploy_recovery/README.md`.

## DR-35 — Concurrent rejection causes use a canonical semilattice join

**Context.** DR-33 made rejection authority exact-source-specific, but the
formal occurrence model represented tombstones only as an unlabeled set. The
wire record also carries a diagnostic reason. During validator-pause recovery,
concurrent descendants legitimately recorded the same exact occurrence as both
`duplicate_occurrence` and `collateral_chain_drop`: one closure saw the direct
duplicate, while another first lost the containing dependent chain. State
suppression agreed, yet `merge_occurrence_context` required byte-equal reasons
and converted this valid refinement overlap into a proposal `BugError`.
Heartbeat retries then backed off indefinitely and the finalized view stopped.

**Decision.** Rejection reason is diagnostic; `(deploy signature, source block)`
is the causal authority. Current reasons form the ordered join-semilattice

```math
r_{\bot} \prec r_{\mathrm{collateral}} \prec r_{\mathrm{merge}}
\prec r_{\mathrm{duplicate}}.
```

Here `$`r_{\bot}`$` is `Unspecified`, the fold identity and an invalid reason in
a current record. Every place that combines reasons for one exact occurrence
uses the maximum under this protocol order. Duplicate occurrence is the
strongest direct cause because it independently excludes that exact execution;
merge conflict is the next direct cause; collateral chain drop applies only
when no direct cause is known. The join is commutative, associative,
idempotent, and monotone as causal evidence grows.

**Rejected alternatives.** Rejecting reason disagreement confuses diagnostic
refinement with contradictory causal authority and deadlocks on valid DAG
concurrency. Last-writer replacement makes the block body depend on observation
order. First-writer preservation is equally order-dependent. Dropping the reason
field loses useful operator and API evidence. Encoding an unordered repeated
reason set would be expressive but unnecessarily enlarges the protocol surface:
the current classifications have a clear semantic specificity order, and all
consensus consumers require one canonical diagnostic.

**Formal verification.** `RejectionReasonConfluence.tla` explores every
interleaving of collateral, merge-conflict, and duplicate observations at two
validators. Its safe configuration proves that equal observation sets converge,
that direct causes dominate collateral causes, and that duplicate dominates all
other causes. The last-writer unsafe configuration reproduces
`Inv_EqualObservationConverges`. `RejectionReasonConfluence.v` proves the three
join laws, the precedence results, and arbitrary-list permutation invariance
without added axioms. The capstone is
`finalized_floor_rejection_reason_confluence_correct`.

**Implementation verification.** The enum owns the single `canonical_join`
operation. Both visible-tombstone reduction and final rejected-body assembly use
it. Example tests pin the precedence. Property tests range over every enum value
and prove commutativity, associativity, and idempotence. A causal sibling-block
regression constructs the exact concurrent duplicate/collateral overlap that
previously halted proposal. The validator-pause integration scenario verifies
continued proposal, finality advancement, log cleanliness, and resource bounds.

**Cross-refs.** DR-33, DR-34, TM-CA-172,
`finalized-floor-specification.md` R-REASON-CONFLUENCE,
`formal/tlaplus/deploy_recovery/README.md`, and
`docs/casper/CONSENSUS_PROTOCOL.md` §6.

## DR-36 — Paper resource roles refine the native SystemVault and RSpace architecture

**Context.** The cost-accounting papers specify the semantic extension to rho
calculus and illustrate it with small pure-rho processes. They intentionally do
not restate the F1R3node vault, registry, PoS, RSpace, replay, merge, or genesis
architecture. Early implementation stages interpreted each illustrated channel
as a new consensus store. That produced a `W_v` draw mirror, a `Sigma` balance
datum written from Rust, a separate `F_v` fee balance, close-block debit and
conversion passes, and a `genesis_supply` wire payload. Those stores duplicated
native responsibilities and created multiple sources of economic truth.

**Decision.** Persistent REV custody and ownership use the existing
`rho:vault:system` contract. Located linear execution authority uses native
RSpace stack terms and unforgeable names. The paper abstractions refine these
native roles as follows:

| Paper role | Native realization |
| --- | --- |
| wallet or persistent purse | canonical SystemVault address selected from authenticated ownership |
| available supply $`\Sigma`$ | reservable SystemVault custody plus authenticated prepaid located stacks |
| located purse | ordered `CostStack` cells at an unforgeable RSpace name |
| funding proof | a finite certificate bound to program, root, protocol, and authority presentation |
| execution draw | causal `COMM` events allocated to the certified authority regions |
| refund | unused SystemVault reservation or a located cell that was never popped |
| protocol mint | authenticated `SystemVault.protocolMint` through genesis or PoS |
| fee collection and conversion | one atomic conserving transfer from reserved payer purse to proposer vault |
| slash removal | SystemVault quarantine plus the existing PoS stake quarantine |

The invariant for authority $`a`$ is:

```math
\Sigma(a)=V(a)+L(a),
```

where $`V(a)`$ is currently reservable canonical vault custody and $`L(a)`$ is
prepaid located authority available to the execution. A finite certificate gives
bound $`B(a)`$ and deterministic fee allocation $`F(a)`$. Admission physically
reserves the vault portion and pops the selected pre-state located cells only
when:

```math
B(a)+F(a)\leq\Sigma(a).
```

The retained execution emits realized cost $`\kappa(a)`$ with
$`0\leq\kappa(a)\leq B(a)`$. Settlement burns only realized execution cost,
refunds the unused vault reservation, and transfers the fee to the proposer.
It commits atomically with the retained execution state.

**Fee linearization.** The worked example exposes fee collection at `F_v` and a
later conversion because that makes ownership flow visible in pure rho. Native
SystemVault settlement may linearize those two internal steps into one
authenticated payer-to-proposer transfer because it proves payer debit equals
proposer credit, no mint occurs, no intermediate fee state is observable, and
replay derives the same owners and amount. The blessed Exchange remains the
general two-sided swap for genuinely distinct assets or authority carriers; it
is not a mandatory second fee ledger.

**Minting and token construction.** Constructing or transferring an existing
located execution stack is an ordinary Rholang operation. Creating new canonical
REV custody remains protected by the native SystemVault protocol authority.
Genesis includes canonical vault allocations in its blessed contract execution;
there is no separate wire allocation payload or block-one mirror. PoS credits
eligible validator vaults directly and records epoch idempotence.

**Lollipop and delegation.** Lollipop $`D\multimap S`$ does not withdraw from a
vault during reduction. It consumes the already-present source authority for the
rendezvous and threads distinct continuation authority, in causal order, without
rewrapping. A user first moves custody into an unforgeable located slot through
an authenticated, conserving transfer. Passing that slot capability delegates a
bounded purse without granting general vault access.

**Replay and consensus.** The program, authenticated root, protocol version,
authority presentation, physical reservation, exact causal witness, cost, fee,
and adjacent root chain are consensus inputs. Replay independently reconstructs
them. Settlement removals are merge-visible RSpace state changes. Proposer-only
maps and scheduler-local execution traces are never authoritative.

**Superseded mechanisms.** This record supersedes the persistence and settlement
mechanisms of DR-13, DR-14, DR-27, and DR-30 wherever they prescribe
`produce_balance`, `dual_write_supply`, distinct `W_v` or `F_v` ledgers,
`convertedEpochs`, `genesis_supply`, or a block-one mirror. Their requirements
for stable signer identity, authenticated minting, conservation, bounded
arithmetic, idempotence, and replay determinism remain in force.

**Formal verification.** Rocq proves address injectivity, bounded reservation,
direct fee conservation, genesis idempotence, located-stack conservation,
lollipop authority order, and replay agreement. TLA+ checks vault reservation,
state-bound admission, physical stack settlement, mint/fee/slash interleavings,
multi-validator convergence, and end-to-end genesis-to-finality behavior, with
registered negative controls. Sage cross-checks the bounded arithmetic and
economic state machines. Loom explores atomic reservation, settlement removal,
join, and stack-frontier interleavings.

**Implementation verification.** Unit and property tests cover every linear
operator, signature algebra, reservation boundary, rollback, stack identity,
and fee allocation. Integration tests cover same-deploy and cross-deploy stack
funding, lollipop source/continuation charging, canonical genesis reconstruction,
PoS minting, slash/redemption, state-bound retained execution, play/replay root
identity, multi-parent settlement visibility, and multi-validator consensus.

## DR-37 — Replay consumes authenticated supply snapshots, never live vault queries

**Context.** Native cost admission obtains authority from the existing
SystemVault and located RSpace stacks. Proposal may query those contracts while
executing against ordinary RSpace. ReplayRSpace has a different architectural
role: it is rigged with a committed causal event sequence and must consume that
sequence exactly. Reusing the proposal-side supply reader during replay caused
SystemVault registry and balance queries to enter ReplayRSpace even though those
query events were absent from the committed `ProcessedDeploy.deploy_log`.

A multi-deployment block adds a second constraint. Let $`R_0`$ be the block
pre-state and let deployment $`d_i`$ record pre-state $`R_{i-1}`$ and
post-state $`R_i`$. The proposer already owns every intermediate root because
proposal execution created them. An independent validator initially owns only
$`R_0`$. Eagerly querying every deployment before replay therefore makes block
validity depend on node-local history: the proposer can read $`R_1`$, while the
validator reports an unknown root before it has replayed $`d_1`$.

**Decision.** Supply capture and trace replay proceed in one canonical
deployment loop. For $`d_i`$, the validator first checks that its current root is
$`R_{i-1}`$. A separate ordinary runtime resets to that already-materialized
root and captures the complete purse inventory for every required authority
lane. The snapshot also captures the certified proposer's validator-fuel purse.
ReplayRuntimeOps consumes that immutable snapshot while replaying only
$`d_i`$'s committed causal witness. Its checkpoint must equal $`R_i`$; that
checkpoint materializes $`R_i`$ locally before the loop reads the snapshot for
$`d_{i+1}`$. Missing, unexpected, or mismatched authority lanes fail closed.

```text
current := recorded block pre-state
for deployment in canonical block order:
    require deployment.pre_state = current
    snapshot := read authority from ordinary RSpace at current
    replay deployment's recorded causal witness using snapshot
    current := checkpoint and require current = deployment.post_state
```

Reporting replay follows the same boundary. It first removes admission-rejected
deployments and verifies state-bound admission at the block pre-state. It then
performs the same ordinary-read, trace-replay, checkpoint sequence for every
remaining deployment. Genesis continues to use multiplicative-unit execution
authority and the canonical SystemVault state committed by its blessed deploys.

**Trace selection.** A processed deployment contains user events followed by
native reserve, located-stack, and settlement lifecycle events. Replay validates
that every authority-bearing user event occurs in the committed combined trace
in causal order. Lifecycle-only COMMs are replayed by their system operations and
are not misclassified as user authority events. Any replayed user authority event
absent from the committed trace remains an error.

**Formal verification.** `ReplaySupplySnapshot.tla` proves authenticated
per-deployment snapshots, exact proposer-fuel depletion, exact recorded trace,
and deferred noninterference. Eight controls isolate each required guard.
`ReplayRootMaterialization.tla` adds independent producer,
validator, and reporter histories and proves that every snapshot root is locally
materialized, every snapshot uses ordinary RSpace, accepted validators have the
same terminal root, and every validator eventually decides. Its three negative
controls refute eager future-root reads, producer-history-dependent validation,
and ReplayRSpace authority queries. Rocq `ReplayRootMaterialization.v` proves the
root-prefix induction and independent-validator post-state equality without
axioms.

**Implementation verification.** Ordinary Casper replay, reporting replay,
checkpoint replay, genesis replay, and the lifecycle-trace subset regression
exercise this boundary. Native tests reject live ReplayRSpace economic queries.
The independent-validator and reporting regressions use
isolated RSpace histories containing genesis but not the producer's intermediate
root; both must materialize that root by replaying the first deployment before
reading the second deployment's purse. The directed merge-dependency regression
also confirms that sequential SystemVault settlement remains a real causal edge
rather than being erased as an allegedly side-effect-free `Nil` deployment.

**Cross-refs.** DR-31, DR-32, DR-36, DR-62, TM-CA-173, TM-CA-198,
CA-P-188, CA-P-191, CA-P-208, UC-CA-168, and UC-CA-187.

## DR-38 — Reserve and settle algebra refines to one native atomic application

**Context.** The papers express conservative funding as maximum reservation,
execution, exact charge, and refund. They assume the existing F1R3node
architecture and specify observable resource behavior, not a new persistent
ledger. The first native realization exposed those abstract phases as separate
SystemVault calls joined by a singleton `reservationStore`. Every completed paid
deploy consumed and rewrote that same RSpace datum. Multi-parent merge therefore
classified independent sibling deploys as conflicting even when their only
durable shared effect was a valid mergeable purse delta.

**Decision.** Keep maximum reservation and refund as the proof decomposition,
but refine them to one authenticated `SystemVault.applyCost` transition. The
call receives the certificate identity, canonical maximum allocations, exact
realized burn and fee allocations, and proposer address. It lexically splits
the maximum from native purses, validates exact charges, transfers fees, burns
cost, refunds the difference, and returns without publishing reservation state.
Located-stack removals and the call are enclosed by the same node checkpoint;
any failure restores the pre-deploy root.

Reservation identity remains consensus evidence binding the certificate,
system-deploy randomness, and replay. Native deploy-occurrence rules provide
duplicate protection. There is no reservation idempotency table and no parallel
economic ledger.

Each source branch must pass its maximum-bound proof at its authenticated
pre-state. Multi-parent merge then operates on completed, replay-verified durable
deltas. Funded aggregate exact debits commute; aggregate overdraw cannot finalize
as a partial mutation. This is distinct from admitting a new unexecuted parallel
deployment from an optimistic estimate.

**Formal verification.** `AtomicVaultSettlementRefinement.v` proves that the
single native operation has the same visible result as abstract reserve followed
by settle, preserves the pre-existing held balance, conserves value, rejects
insufficient bounds and realized overdraw, and makes aggregate settlement order
independent. `AtomicVaultSettlementRefinement.tla` checks branch admission,
aggregate funding, visible-state refinement, conservation, no-effect rejection,
and replay equality under both branch selection orders.
`AtomicVaultSettlementRefinementGlobalCellUnsafe.cfg` restores the singleton cell
and must refute `NoPersistentReservationState`.

**Implementation verification.** The SystemVault example checks exact two-payer
burn, proposer credit, refund, and partial-reserve rollback. Rust property tests
exercise request permutation invariance and realized-overdraw rejection. The
original `d3_same_key_benign_deploys_merge_without_precharge_conflict` consensus
regression requires both same-payer sibling deploys to survive merge.

**Cross-refs.** DR-31, DR-36, DR-37, TM-CA-174, CA-P-071, CA-P-086,
CA-P-171, CA-P-172, CA-P-192, and UC-CA-169.

## DR-39 — Certification uses the authenticated deploy normalizer environment

**Context.** Execution normalizes each deployment with bindings derived from
its verified envelope, including `rho:system:deployerId` and cosigner
introspection. State-bound certification separately called the empty-environment
compiler entry point before resolving lexical names. A funded deployment that
queried its own SystemVault through `rho:system:deployerId` consequently failed
capacity derivation even though the retained execution environment could resolve
the reference. This violated the certificate-to-execution refinement and
explained an ordinary-deployment failure class hidden by the later generic
funding-rejection status.

**Decision.** `canonical_program_for_deploy` constructs
`normalizer_env_from_cosigned_deploy` from the same authenticated `Cosigned`
envelope and passes it to `source_to_adt_with_normalizer_env`. The resulting term
then follows the existing canonical funding and lexical-resolution pipeline.
Certification, retained execution, replay admission, and replay execution all
therefore derive system bindings from the identical verified envelope; no
ambient node-local environment is consulted.

State-bound rejection remains fail-closed and consensus-visible. Diagnostic
tracing now distinguishes capacity derivation, non-expanding frontier,
exhaustion without new authenticated authority, physical reservation, and
atomic-vault rejection. Those diagnostics do not change the serialized
admission result.

**Formal verification.** `RuntimeBoundAuthority.v` proves that certification,
execution, and replay normalization are equal under one authenticated
environment and that an empty certification environment necessarily diverges
on deployer identity. `NormalizerEnvironmentRefinement.tla` checks phase
agreement, admission, execution dependency, replay equality, and eventual
completion. Its empty-environment configuration must refute
`CertificationExecutionReplayUseSameEnvironment`.

**Implementation verification.** The funded
`deployer_id_system_vault_query_should_replay_from_state_bound_checkpoint`
regression exercises parsing, authenticated normalization, state-bound capacity,
SystemVault execution, certificate settlement, checkpointing, and replay root
identity. The exact CI Clippy command passes with warnings denied.

**Cross-refs.** DR-31, DR-37, DR-38, TM-CA-175, CA-P-184, CA-P-191,
CA-P-193, and UC-CA-170.

## DR-40 — Physical settlement uses a canonical heap worklist

**Context.** Exact physical settlement is a finite proof search over each
realized authority event, the remaining atom multiset, native balances, located
stack positions, and the already selected cells for the current event. The
first implementation expressed that search as recursive Rust calls. A normal
high-fanout Rholang deployment produced more than 500 nested search frames and
overflowed the node's configured 8 MiB worker stack. Increasing that stack would
only move an implicit availability limit: the event trace is authenticated
consensus input, not a valid host-call-stack bound.

**Decision.** Physical search uses an explicit last-in-first-out heap worklist.
Each search node owns the complete immutable branch state. A persistent
reference-counted draw chain records completed events without cloning the whole
prefix for every branch. Canonically sorted candidates are pushed in reverse,
so the next popped node is exactly the candidate the recursive specification
would examine first. A failure marker is placed below all children and enters
the same canonical state key into the memo only after every child fails. Born
stack availability, one-pop-per-stack-per-event, compound-cell indivisibility,
checked arithmetic, and final `verify_physical_settlement` validation are
unchanged.

For a finite candidate tree $`T`$, the worklist visits at most $`|T|`$ nodes and
uses constant native recursion depth. Heap use is proportional to the live
frontier, failed-state memo, and persistent successful-prefix chain. This is the
native realization of the papers' decidable linear-proof-search boundary; it
does not change which certificate is selected or any serialized evidence.

**Formal verification.** `PhysicalSettlementWorklist.v` proves that complete
worklist traversal returns the same ordered solution list and the same canonical
first solution as recursive depth-first traversal. It separately proves success
and failure refinement. `PhysicalSettlementWorklist.tla` explores two
independent validators/deployments under every enabled interleaving, checks
finite-tree bounds, reference-result equality, zero native recursion, and
eventual completion. The recursive negative control must exceed a smaller
native-stack bound before completing the same event sequence.

**Implementation verification.** A 4,096-event example executes on the normal
8 MiB test stack. Proptest generates mixed authority-event sequences up to 256
events and checks exact total debit plus event-order preservation. The unchanged
high-fanout `compute_state_should_just_work` play/replay test and the complete
48-test runtime-manager suite pass at the normal stack size.

**Cross-refs.** DR-31, DR-32, DR-39, TM-CA-176, CA-P-036, CA-P-048,
CA-P-059, CA-P-194, and UC-CA-171.

## DR-41 — The $`s_0`$ limit is an embedding, not a production settlement mode

**Context.** `cost-accounted-rho.tex` presents two related limit results. Giving
every actor one distinguished signature $`s_0`$ collapses the spectral resource
model to homogeneous phlogiston, and making that signature perpetually funded
recovers pure rho reduction. Those results establish conservativity of the
extended calculus. They do not prescribe that an F1R3node validator erase
distinct signed regions, debit every communication from the deployment
envelope, or maintain a second supply ledger beside SystemVault.

Earlier staging records used “$`s_0`$ collapse” for all three ideas. That
conflation made historical diagnostics appear normative after native
`CostAuthority`, persistent `CostStack`, and exact physical settlement had
landed.

**Decision.** The mathematical $`s_0`$ embedding remains part of the formal
correspondence. It is not a runtime flag, activation mode, funding fallback, or
consensus storage design.

- Every production user deployment enters the authority-accounting protocol.
  An unsigned surface inherits its authenticated deployment payer.
- An explicit signed region replaces that inherited payer within its lexical
  scope. Ambient deployment custody cannot satisfy the region unless the
  region's signature algebra explicitly presents the same authority.
- `Sig::Unit` is the zero-demand multiplicative identity used by payer-less
  bootstrap construction and low-level reducer tests. It cannot be serialized
  as a physical stack cell and does not disable accounting around an explicit
  region.
- P8 balancing applies inside a compound funding presentation. It does not move
  cost between independently located regions or collapse them into the outer
  deployment payer.
- Native settlement selects SystemVault balance and located-stack cells from
  the complete authority of each committed atomic COMM. Play and replay bind
  the same selection to the certificate, witness, and adjacent state roots.

The production invariant for a committed event $`e`$ and an unrelated ambient
purse $`p_a`$ is:

```math
p_a\notin\operatorname{purses}(e)
\Longrightarrow
\operatorname{debit}(e,p_a)=0.
```

For an explicit region with purse $`p_r`$, sufficiency is checked at $`p_r`$;
an arbitrarily large balance at $`p_a`$ cannot make an underfunded $`p_r`$
admissible.

**Formal verification.** Rocq theorem
`explicit_regions_do_not_debit_ambient_purse` proves the invariant for arbitrary
region lists, plans, balances, and ambient purses. The existing
`debit_preserves_unselected_purse`, `admitted_event_debits_exactly`, and
`replay_preserves_purse_debit` theorems compose it with exact admission and
replay. `LocatedAuthoritySettlement.tla` checks `NoAmbientAuthority`; its
`AmbientPurseUnsafe` configuration must refute that invariant. The wallet-funded
lollipop model independently refutes slot-to-envelope payer collapse.

**Implementation verification.** The reducer regression
`explicit_region_authority_overrides_the_deploy_default` installs a distinct
default payer and proves that only the explicit region appears in the realized
authority witness. The physical-allocation regression
`explicit_region_cannot_spend_an_unrelated_default_balance` proves that an
abundant default balance cannot fund the explicit region and that adding the
matching located stack pops only that stack. Existing unit-authority,
state-bound rollback, lollipop, cross-deploy, and replay tests cover the adjacent
boundaries.

**Supersession.** This record supersedes every production claim in DR-4,
DR-13, DR-14, and their staging documents that calls envelope-only settlement
the $`s_0`$ collapse. It does not supersede the papers' $`s_0`$ limit theorems or
historical descriptions explicitly labeled as retired.

**Cross-refs.** DR-24, DR-31, DR-32, DR-36 through DR-40, E2E-009A, E2E-022,
E2E-026, `cost-accounting-as-monad-correspondence.md`, and
`cost-accounting-impl/end-to-end-authority-settlement.md`.

**Cross-refs.**
[End-to-End Authority Settlement](cost-accounting-impl/end-to-end-authority-settlement.md),
[Executable Conformance Matrix](cost-accounting-executable-conformance-matrix.md),
`formal/rocq/cost_accounted_rho/PROOF_ARTIFACTS.md`, and
`formal/tlaplus/cost_accounted_rho/README.md`.

## DR-42 — Located-stack declarations precede sibling reduction in one parallel configuration

**Context.** A first-class `CostStack` is a located linear resource in the
current Rholang configuration. The governing rules R1–R3 consume the head of a
co-present stack when a signed interaction is forced. The native evaluator
previously launched every normalized `Par` term as an independent Tokio task,
including both stack declarations and ordinary reductions. Under scheduler
pressure, a sibling COMM could therefore commit before RSpace recorded the
stack declaration that was already present in the same source configuration.
Physical settlement then correctly rejected the recorded order as unfunded.
This was an evaluator/refinement defect, not permission to reorder settlement
evidence or treat a stack as partially born.

**Decision.** Evaluation of each normalized `Par` has one semantic declaration
barrier:

1. Partition its already-canonical term sequence into `CostStack` declarations
   and ordinary reduction terms.
2. Materialize every declaration in canonical order, preserving its original
   term index, metering child, source path, and random split.
3. If a declaration fails, return the stable aggregate error without launching
   any sibling reduction.
4. After all declarations succeed, run the ordinary terms concurrently.

The barrier is local to one `Par`. A stack inside a receive continuation is not
in the current configuration and cannot be materialized until the parent COMM
releases that continuation. Thus the rule supplies present sibling authority
without crediting candidate-created or future authority.

For declaration event $`d`$, sibling reduction $`r`$, and continuation-local
declaration $`n`$, the required causal order is:

```math
d \prec r \prec n.
```

The stack transfer remains atomic: all cells become available at $`d`$, or no
cell does. Physical allocation continues to consume the recorded causal trace
without hash sorting, post-hoc event movement, ambient-payer fallback, or
per-cell birth times.

**Formal verification.** `ParallelStackMaterialization.tla` checks the
declaration barrier, nested causality, supply conservation, replay equality,
and eventual completion. Its unsafe configuration permits $`r \prec d`$ and
must refute `CausallyFundedProgramIsAccepted`; TLC and Apalache both exercise
that counterexample. `ParallelStackMaterialization.v` proves conservation for
each phase, rejection of premature parent and nested steps, exact terminal
state, scheduler-preference independence, and byte-identical replay without
axioms.

**Implementation verification.** The reducer tests inspect the actual RSpace
event log. `parallel_cost_stack_is_materialized_before_sibling_reduction`
asserts $`d \prec r`$ under a multi-threaded runtime;
`continuation_cost_stack_is_materialized_after_parent_reduction` asserts
$`r \prec n`$. The Casper same-deploy transfer regression is additionally run
on one CPU, the schedule that reliably reproduced the original rejection.

**Cross-refs.** DR-31, DR-32, DR-36, DR-38, DR-41, TM-CA-177, E2E-004,
E2E-021, REL-003 through REL-005, `StackTransferConservation.v`, and
`CausalStackOrder.v`.

## DR-43 — Exact accepted-effect provenance governs state-preserving finality

**Context.** The causal block DAG is not the replay-state graph. A merge can
name a block as a parent while rejecting one of that block's execution effects.
The first state-preservation repair represented each block with one state base.
It did not record effects that replay applied from other accepted chains.
An accepted sibling effect could therefore disappear from later provenance.

The next repair unioned every maximal DAG parent and the cached floor.
It then subtracted the current block's direct rejection evidence.
That recurrence did not match replay state construction.
It could resurrect an effect omitted by an earlier merge.

The earlier formal model encoded the same incorrect union recurrence.
It therefore proved properties about an over-approximation instead of replay
state.

**Decision.** State preservation uses exact execution-effect identities and the
actual replay constructor.

1. Each committed transition has identity
   $`(source\_block\_hash, execution\_index)`$.
   A successful execution creates an identity.
   A failed body with verified settlement also creates an identity.
   Admission rejection and a legacy failed execution create no identity.
2. Each block defines one state parent.
   A multi-parent block uses its exact `merge_base`.
   A single-parent block uses that parent.
   Genesis has no state parent.
3. Each block commits a canonical `applied_state_effects` sequence.
   The sequence identifies accepted ancestor effects that replay applies above
   the state parent.
4. Each block also commits canonical direct rejection evidence.
   Rejection evidence explains the merge decision but does not construct state.
5. Exact active state follows this positive recurrence:

```math
\operatorname{Active}(B)=
\operatorname{Active}(\operatorname{StateParent}(B))
\cup \operatorname{Applied}(B)
\cup \operatorname{Own}(B).
```

6. Validation requires a strict ancestor state parent.
   Each applied identity must name a committed effect from a strict ancestor.
   Applied and rejected identity sets must be disjoint.
7. Replay recomputes `merge_base`, applied identities, rejected identities, and
   `applied_from_scope`.
   Replay requires exact canonical equality for each field.
8. `preserves(A,D)` requires DAG ancestry and
   $`\operatorname{Active}(A) \subseteq \operatorname{Active}(D)`$.
9. Causal certification remains unchanged.
   State certification applies the existing majority and clique calculation to
   validators whose states satisfy `preserves`.
10. Protocol 6 requires this positive provenance encoding.
    Missing or malformed provenance fails closed.

The implementation follows one explicit state construction algorithm:

```text
construct_state(block):
    parent_state := state(state_parent(block))
    applied := validate(block.applied_state_effects)
    own := committed_own_effects(block)
    return parent_state union applied union own
```

Activity queries walk only the state-parent lineage.
They test inherited state, exact applied identities, and own committed effects.

Preservation queries find the meet of two state-parent lineages.
They collect positive effects introduced on both segments.
The source segment must be a subset of the destination segment.
The implementation memoizes immutable metadata, ancestry, activity, and
preservation results for one selection run.

**Rejected alternatives.** Unioning every DAG parent resurrects omitted effects.
Subtracting direct rejections cannot remove an omitted effect from later blocks.
A state parent without positive applied facts drops accepted sibling effects.
Changing the causal vote would conflate causal agreement with replay-state
admissibility.

**Formal verification.** `StateEffectProvenance.v` proves the positive
constructor and each preservation case without axioms.
It proves applied-order invariance and repeated accepted-merge preservation.
It also proves that omitted header-parent effects remain absent.
The unsafe union-parent constructor resurrects the omitted effect.

TLC exhausts the exact three-validator and two-node model.
Apalache checks the solver-oriented model through bound 8.
Both unsafe configurations reproduce header-parent effect resurrection.

**Implementation verification.** Wire and metadata round trips preserve exact
effect order and identity.
Validation rejects malformed, missing, extra, duplicate, and reordered applied
facts.
Rust covers all six three-parent orders and repeated restoration rounds.
Properties compare activity and preservation with the positive recurrence.
Loom checks atomic metadata publication and concurrent finality reads.

**Cross-refs.** DR-36, DR-38, DR-42, TM-CA-178, REL-001, REL-004,
`finalized-floor-specification.md` R-EFFECT-ID through R-EFFECT-SCAN and S28,
and `finalized-floor-verification.md` H11.

## DR-44 — Honest parent selection preserves the committed LFB state

**Context.** DR-43 made floor and LFB promotion state-preserving, but it did not
constrain proposer fork choice after promotion. The bridge integration trace
exposed the missing transition. All nodes first reached a block whose post-state
contained the funded registry effect and accepted it as LFB. Validator latest
messages then advanced along several speculative descendants; some retained the
effect and others causally included its source while rejecting the effect. The
finalizer correctly refused to promote those dropping states, yet the proposer
fed the same tips back into its estimator. Its next block therefore replayed from
a state root that did not contain the finalized registry datum. The query had no
COMM, reported zero cost, remained pending, and the network later accumulated
`UnknownRootError` and `UnauthorizedSlashDeploy` cascades.

The earlier verification missed this because `StateEffectProvenance.tla` fixed
every delivered validator tip to a state that preserved the candidate source.
It modeled exact promotion support but not the asynchronous transition in which
certificate delivery advances the local LFB while stale or effect-dropping latest
messages remain selectable. That assumption was stronger than the implementation
and made the parent-selection obligation vacuous.

**Decision.** For one immutable DAG snapshot, let `L` be the current LFB and
`J(v)` validator `v`'s valid latest message. Every value in `J` remains an input
to the estimator and causal evidence for the proposal. Direct parent compaction
may remove a latest tip only when another direct parent reaches it in the DAG;
therefore every valid latest tip is either a direct parent or lies in a direct
parent's causal closure. When `J` is empty, `{L}` is the complete parent set. The
fallback is never genesis.

Parent choice establishes causality, not the state base. The proposal pre-state
is reconstructed from the certified floor state plus the deterministic accepted
effects in the above-floor causal closure. Exact rejections may remove an
above-floor effect but may not erase an effect already represented by the floor.
The complete latest-message map still forms justifications, valid latest metadata
still determines sequence-number accounting, and a receiver replays the block's
declared parents without recomputing fork choice from its own possibly lagging
view.

**Rejected alternatives.** Invalidating a speculative descendant would make block
validity depend on when a receiver learned finality. Filtering effect-dropping or
stale latest messages would discard legitimate causal evidence and could impair
liveness without repairing replay. Filtering only the main parent would still
allow a secondary branch to affect merge state. Falling back to genesis or the
approved block would recreate state loss. Changing majority, clique, validator
weight, or `FTT` calculations would conflate causal certification with state
reconstruction.

**Formal verification.** Rocq proves non-empty causal inputs, retention of every
valid latest input, exact LFB fallback, and preservation of all non-rejected floor
effects through the rebased merge. TLC exhausts 17,169 generated / 808 distinct
local asynchronous states to depth 10. It checks direct-parent causal coverage,
exact causal-input retention, floor rebasing, and finalized-effect preservation.
The axiom-free Rocq product lifting proves arbitrary-node invariant preservation,
distinct-node commutation, and local enablement framing, while the explicit
three-validator transition system checks interacting validation and promotion races.
The floor-unprotected control violates
`Inv_ProposalPreservesSnapshotFloor`; Apalache checks the same safe and unsafe
boundaries through the same node-local transition relation.

**Implementation verification.** The Rust snapshot regressions prove that
reachability-covered tips are compacted without dropping independent siblings,
that a non-empty selected parent set is retained, and that only an empty valid set
falls back to the captured LFB. Merge-rebase regressions prove that parent fast
paths are used only when the selected cover preserves the floor. The focused
bridge scenario remains the end-to-end gate.

**Cross-refs.** DR-43, TM-CA-178, REL-001, REL-004, REL-006,
`finalized-floor-specification.md` R-PARENT-STATE/R-PARENT-EVIDENCE and S29,
and `finalized-floor-verification.md` T-STATE-PARENT.

## DR-45 — Dual-certified universal state advances the per-block replay floor

**Context.** DR-43 and DR-44 made LFB admission and future parent selection
state-preserving, but per-block floor derivation still discovered advancement
only on each declared parent's main-parent spine. The global finalizer consumes
complete causal evidence. A block can therefore receive both exact certificates,
become the local LFB, and remain a secondary ancestor of every later parent while
appearing on none of their main spines. Replay floors then remain below already
committed resource transitions.

**Decision.** Every block derives one additional candidate, the universal
certified frontier `U(B)`. For candidate `C` and declared parents `P(B)`:

```math
Universal_B(C) \iff
  \bigwedge_{P \in P(B)} C \preceq_{DAG} P.
```

`C` is eligible only when `Universal_B(C)`, the unchanged causal clique
certificate, the unchanged state-preserving clique certificate, and preservation
of every inherited parent floor all hold over the block's frozen justification
snapshot. A deterministic multi-source traversal propagates one parent identity
through every causal edge in descending `(block_number, block_hash)` order and
returns the highest eligible candidate. Strict block-number descent makes coverage
complete before selection. Missing metadata, malformed ancestry, cycles, or late
coverage fail closed.

The candidate joins inherited and main-spine candidates before the existing
sound-base selection. The decision does not alter the finalizer's main-parent
agreement propagation, hard-majority gate, maximum-clique computation, validator
weights, exact `FTT` arithmetic, or strictness. Main-parent ancestry remains a vote
scheduling relation; all-parent DAG ancestry is the state-provenance discovery
relation.

**Rejected alternatives.** Lowering the threshold, treating a causal edge as
state support, choosing the majority-observed deploy block at query time, or
requiring the LFB to reappear as main parent would respectively weaken consensus,
admit rejected state, hide divergent progress, or recreate the liveness defect.
Node-local LFB injection would make validation depend on arrival order rather than
the proposed block's bytes.

**Formal verification.** The safe `CertifiedFloorPromotion.tla` model exhausts
1,051 generated / 225 distinct states to depth 9 and proves asynchronous
arrival-order convergence and promotion safety. The main-spine-only control fails
complete-evidence promotion. Apalache independently checks the safe model through
bound 8 and finds the unsafe trace at step 3. `CertifiedFloorPromotion.v` proves
universal eligibility, causal discoverability, and selected-floor preservation
without axioms; `MainTheorem.finalized_floor_certified_promotion_correct` is the
capstone.

**Implementation verification.** Rust exercises a three-validator exact
`FTT=0.1` certificate where the candidate is secondary to every parent, all six
parent permutations, and a control in which state rejection preserves the causal
certificate but blocks promotion. A generated-DAG property varies independent
side-branch and post-merge depths plus parent order. The five-node bridge and
aggregate integration suites remain the release gates.

**Cross-refs.** DR-43, DR-44, TM-CA-179, REL-011,
`finalized-floor-specification.md` R-FLOOR/R-UNIVERSAL-FRONTIER and S30, and
`finalized-floor-verification.md` H12/T-CERTIFIED-FLOOR-PROMOTION.

## DR-46 — Compute certificate support once without changing the clique decision

**Context.** DR-45 requires complete causal discovery. The direct realization
asked storage-backed DAG ancestry once for every candidate-validator pair. AMD
uProf attributed approximately 18.13 seconds to raw ancestry queries, 15.38
seconds to the universal-frontier traversal, and 16 seconds to clique evaluation
in the 132-block complete-scan regression. Reducing its depth would restore the
old correctness hole; weakening a threshold or certificate was never admissible.

**Decision.** Transpose the same reachability relation. Seed each validator
identity at its frozen latest message, process blocks in strict descending
`(block_number, block_hash)` order, and propagate that identity to every causal
parent. For every candidate `C`, the resulting set must be exactly:

```math
Coverage_J(C) = \{v \mid C \preceq_{DAG} J(v)\}.
```

The implementation constructs the same supporter map and corresponding-weight
map and invokes the unchanged hard-majority, maximum-clique, exact-threshold
decision. Missing metadata, non-descending edges, cycles, or support arriving
after a block was processed fail closed.

A child may reuse its parent's universal-frontier result only when the child has
one parent, that parent has one predecessor, the inherited floor equals the
cached parent floor, the two frozen justification snapshots are byte-equal, and
every latest message is older than the parent. Multi-parent parents always
rescan: a merge can make a branch-local candidate universal without adding a new
latest message.

**Rejected alternatives.** A fixed candidate cap, a shallower regression, or a
test timeout would hide incomplete evidence. Memoizing pairwise storage walks
would retain their asymptotic duplication. Reusing every byte-equal snapshot is
unsound across a multi-parent merge. Importing Rholang, RSpace, or MeTTaIL
optimizations would target a subsystem the regression does not execute.

**Formal verification.** Rocq proves propagated coverage extensionally equal to
pairwise reachability, proves decision transparency, and proves the linear reuse
theorem from its explicit premises. `MainTheorem.v` exports both capstones with
no assumptions. TLC exhausts the `LatestMessageCoverage` worklist and its
descending scheduler; the unordered control violates no-late-coverage. Apalache
independently checks the safe model through bound 8 and finds the unsafe trace
through bound 4.

**Implementation verification.** Generated Rust DAGs compare coverage,
supporters, corresponding weights, and final clique verdicts with the original
pairwise oracle for every generated target. Examples reject non-descending edges
and distinguish the valid linear reuse case from multi-parent, changed-snapshot,
and non-older-evidence cases. The unchanged 132-block regression passes in 22.92
seconds, down from 63.21 seconds, without altering its candidate set.

**Cross-refs.** DR-45, TM-CA-180, REL-012,
`finalized-floor-specification.md` R-COVERAGE-EQUIVALENCE/
R-LINEAR-SNAPSHOT-REUSE and S31, and
`finalized-floor-verification.md` C13/T-COVERAGE-TRANSPARENCY.

## DR-47 — Restore quantitative RSpace bytes inside the RevVault hard ceiling

**Context.** The cost-accounted rho papers make linear authority and one
execution grade per COMM explicit, but rely on the node architecture for a
finite quantitative resource ceiling. The retired `ChargingRSpace` recognized
that serialized produces, consumes, transferred payloads, and trace records
consume finite resources. Removing its broad wrapper also removed that safety
constraint. Reintroducing only the side left waiting is unsound: producer-first
and consumer-first schedules retain different structures and can produce
different costs. Its negative refund path is likewise history-sensitive.

**Decision.** Protocol 4 charges every stable semantic produce and consume
identity at its canonical protobuf footprint before lookup or mutation.
Persistent fixed-point retries of the same identity are idempotent, while
distinct non-persistent occurrences retain multiplicity. The paid marker is
committed only after reservation succeeds. Every committed COMM
then charges one calculus execution unit plus every delivered payload byte and
its canonical trace footprint before matched-state mutation. Persistent objects
pay their original introduction once, later counterpart invocations pay their
own introductions, every delivery pays a COMM charge, and removal or peek never
creates negative credit. All arithmetic is checked.

Admission binds a versioned schedule, schedule digest, physical-authority
allocation, quantitative byte bound, and fee to one immutable SystemVault
reservation. `ProcessedDeploy.cost` is COMM execution units plus quantitative
bytes. Native settlement is independently the physical authority draw plus
quantitative bytes, followed by the conserving fee transfer and refund of the
unused maximum. The quantities coincide only in the single-cell-per-COMM
projection. An authorized concurrent top-up credits unreserved custody and
cannot enlarge an in-flight certificate.

**Rejected alternatives.** Charging only unmatched retained state makes cost
depend on arrival order. Restoring live negative refunds makes persistence and
peek history part of consensus arithmetic. Charging external wire traffic would
make validator cost depend on transport behavior rather than canonical
execution. Treating physical authority as the COMM count weakens compound and
located ownership; treating the physical cell count as the calculus execution
grade changes the paper's semantics. A feature flag or compatibility meter would
create two active accounting modes and is excluded.

**Formal verification.** `VaultBackedByteAccounting.v` proves full
producer/consumer lifecycle equality, product-sum decomposition, complete join
payload charging, persistent-delivery behavior, no removal credit, exact bounded
execution, permutation invariance, top-up conservation and reservation
immutability, physical/execution separation, settlement conservation, and replay
equality without assumptions. TLC exhaustively checks the safe lifecycle with a
two-cell join and all existing cost/accounting models. Eight negative controls
independently reproduce mutation-before-charge, trigger-side charging, join
omission, persistent recharge, peek credit, replay omission, top-up expansion,
and arithmetic wrapping. Apalache provides the independent SMT-backed bounded
cross-check.

**Implementation verification.** Canonical byte-kernel examples and proptests
cover exact encoded sizes, join arity, overflow, and arrival-order equality.
RSpace play/replay tests cover both trigger directions, persistent produce and
consume retries, peek, join, and pre-mutation rejection. Loom explores
reservation, top-up, settlement, refund, and persistent-identity interleavings.
Casper regressions bind the byte
schedule and witness to state-bound admission, distinguish processed cost from
physical settlement, and require exact replay roots and debits.

**Cross-refs.** TM-CA-181 through TM-CA-183, UC-CA-172 through UC-CA-174,
[`cost-accounting-impl/vault-backed-byte-accounting.md`](cost-accounting-impl/vault-backed-byte-accounting.md),
and protocol lifecycle version 4.

## DR-48 — Prove complete economic debit across application and protocol layers

**Context.** The atomic native settlement refinement proved physical authority,
quantitative bytes, fee transfer, refund, and aggregate order independence, but
its first state machine abstracted away application-level transfers performed by
the same deployment. A recovery fixture then selected a transfer amount using
only the application debit. Protocol 4 correctly added physical, byte, and fee
settlement to every branch, so two supposedly fitting branches exceeded the
shared 9,000,000 balance. The implementation rejected two of three siblings;
the fixture incorrectly expected one rejection.

This was a refinement-boundary failure in the verification campaign. The
component models were individually sound, but no prior capstone required the
application state transition and every protocol settlement component to share
one per-payer solvency equation. Consequently, exhaustive exploration inside
those abstractions could not discover the omitted term.

**Decision.** The consensus-visible economic debit for deployment $`d`$ and
payer $`p`$ is:

```math
D(d,p) = A(d,p) + P(d,p) + B(d,p) + F(d,p),
```

where $`A`$ is application debit, $`P`$ realized physical settlement, $`B`$
realized quantitative-byte settlement, and $`F`$ fee. Individual admission,
completed-branch merge, finality, and replay use this complete vector. For a set
of concurrent siblings $`S`$ proved against the same pre-state balance $`V(p)`$:

```math
\sum_{d \in S} D(d,p) \le V(p).
```

Application and fee credits are included in the committed conserving state
transition, but do not retroactively fund a concurrent sibling. A later
deployment may use those credits only after they exist in its authenticated
pre-state.

Exact-boundary tests must compute their capacity from the produced funding
certificate and cost witness. Application constants may shape a fixture but are
not sufficient evidence for rejection cardinality.

**Rejected alternatives.** Lowering the transfer amount without asserting the
complete witnessed debit would preserve the abstraction hole. Ignoring protocol
settlement at merge would permit real overdraw. Netting concurrent incoming
credits against outgoing siblings would create an arrival- and selection-order
dependency absent from their shared authenticated pre-state. Treating physical
and byte settlement as one unnamed burn would conceal the dimension whose
restoration exposed the defect.

**Formal verification.** `AtomicVaultSettlementRefinement.tla` now models
application debit/credit, physical burn, quantitative-byte burn, fee debit and
credit, complete aggregate funding, atomic rejection, conservation, and replay.
TLC and Apalache check the safe composition. Four independent unsafe
configurations omit exactly one debit component and each must violate
`FinalizedAggregateIsFunded`. Rocq proves the complete $`n`$-branch threshold and
the concrete migrated recovery boundary without assumptions. Sage exhausts
221,184 three-branch component and order traces and records witnesses where a
projected check would admit complete overdraw.

**Implementation verification.** A 512-case merger property varies application,
physical, byte, and fee debit while constructing a balance for which exactly two
complete branches fit. The recovery regression extracts physical settlement,
byte settlement, and fee from every produced protocol-4 witness, requires
identical siblings to have identical protocol debit, and asserts the exact
two-fit/three-overdraw boundary before checking rejection cardinality. A static
audit covers fixed-balance and exact-rejection-count fixtures and removes stale
precharge-only explanations.

**Cross-refs.** CA-P-195, TM-CA-184, UC-CA-175, CA-FR-082, and
`AtomicVaultSettlementRefinementApplicationDebitOmissionUnsafe.cfg` plus the
physical-, byte-, and fee-omission controls.

## DR-49 — Stack introduction is one failure-atomic cross-ledger transaction

**Context.** A located cost stack moves rivalrous linear cells from a source
authority into an RSpace datum. Protocol 4 also charges the datum's canonical
introduction bytes before RSpace mutation. The first implementation performed
the physical debit before the fallible byte reservation and RSpace operation,
then recorded the stack-birth witness afterward. A rejected byte charge or a
failing matched continuation could therefore expose physical authority events
without a committed produce or birth. Casper correctly rejected that orphaned
witness as `InvalidCostSettlement`.

The earlier verification campaign did not cover this composition. The located
stack model ended at successful physical reservation, while the byte model
treated physical demand as zero and represented charge plus mutation as one
atomic action. Both component models were exhaustive inside their state spaces,
but neither state space contained the real sequence
$`prepare \rightarrow chargeBytes \rightarrow mutateRSpace \rightarrow
continue \rightarrow commit`$. This was a refinement-boundary omission, not an
insufficient search bound.

**Decision.** A stack introduction uses a pending physical reservation. Pending
cells count against capacity and identity uniqueness, but are absent from the
committed event multiset, realized settlement, and stack-birth witness. The
reservation remains live across the byte-charged RSpace operation and any
matched continuation. It commits only after that operation succeeds. An
operation-local error, early return, or unwind before success drops the pending
reservation and restores exactly its cells without changing another operation's
committed state. Reducer branch tasks are joined to completion rather than
cancelled. If a different branch later makes the enclosing deployment fail, the
deployment transaction removes every stack debit and birth committed by that
deployment while the enclosing RSpace checkpoint restores its state. Attempted
quantitative work remains chargeable.

For operation $`o`$, committed cells $`C`$, pending cells $`P`$, and available
cells $`A`$, every intermediate state satisfies:

```math
A + \sum_o P(o) + \sum_o C(o) = A_0.
```

An externally valid stack birth additionally satisfies:

```math
C(o) > 0 \iff Birth(o) \land Produce(o) \in Trace.
```

RSpace records a produce that immediately matches inside `COMM.produces` as
well as, depending on the execution path, an ordinary produce event. Causal
extraction therefore flattens every matched produce before the enclosing COMM,
deduplicating by authority-event identity. This is evidence extraction only; it
does not change RSpace matching, event identity, COMM ordering, settlement, or
consensus voting.

The implementation protocol is:

```text
prepare(op, cells):
    validate identities and complete aggregate capacity
    move cells from available to pending

execute(op):
    reserve canonical introduction and COMM byte charges
    perform the RSpace operation and matched continuation

finish(op, result):
    if result succeeded:
        atomically move pending cells to committed events and publish one birth
    otherwise:
        restore the exact pending cells and publish nothing
```

Quantitative byte and ordinary reducer costs remain attempt costs: they may be
charged for failed work according to DR-47. The rollback rule applies to the
physical stack transfer because no linear resource may be debited unless its
corresponding stack birth is committed.

**Rejected alternatives.** Filtering orphaned events during Casper settlement
would hide a reducer inconsistency and could create validator-dependent supply.
Publishing the birth before RSpace mutation merely reverses the partial-commit
window. Refunding all byte work on a failed continuation would contradict the
attempt-cost semantics and make resource use depend on rollback history. A test
allowance for the orphaned witness would weaken the consensus boundary.

**Formal verification.** `StackIntroductionAtomicity.tla` exposes preparation,
byte acceptance or rejection, RSpace mutation, continuation success or failure,
commit, abort, and replay as separate interleavable actions. TLC and Apalache
check conservation, pending invisibility, exact operation abort, enclosing-
deployment rollback, attempted-byte retention, one birth per committed stack,
causal extraction, and replay equality. Six controls independently expose
pending authority, omit operation abort, omit birth, omit deployment rollback,
omit a nested matched produce, or omit replay state; each must violate its named
invariant. Rocq proves the unbounded arithmetic refinement, deployment rollback,
attempt-cost retention, and matched-produce extraction order without assumptions.
Loom explores commit, rejection and pre-mutation cancellation, competing
reservations, and enclosing-deployment rollback. It does not claim that dropping
an arbitrary future can undo an already visible RSpace mutation.

**Implementation verification.** Runtime unit and property tests require exact
capacity restoration and preservation of unrelated commits. Casper regressions
cover rejected candidate-created stacks, successful exactly funded stacks,
parallel equal stack literals with distinct identities, matched produces nested
inside a COMM, exact protocol-4 physical/byte/fee debit, and play/replay root
equality. Reducer regressions additionally prove that a later deployment error
removes all stack custody and births while preserving ordinary attempted work.

**Cross-refs.** CA-P-196, TM-CA-185, UC-CA-176, REL-014, DR-42, DR-47, and
[`cost-accounting-impl/end-to-end-authority-settlement.md`](cost-accounting-impl/end-to-end-authority-settlement.md).

## DR-50 — Evaluation and replay validation share one state-and-witness transaction

**Context.** The interpreter, Casper play path, and Casper replay path originally
owned different rollback fragments. Three composition defects followed. A parser
failure occurred before the deploy budget reset and could therefore return the
preceding deploy's witness. Reducer operator errors were classified like parser
errors and erased work already attempted by the current deploy. Finally, play or
replay could mutate RSpace successfully and only then reject malformed authority,
birth, settlement, replay-cost, or adjacent-root evidence. In replay, an adjacent-
root mismatch is detected after `create_checkpoint`, so a soft checkpoint alone
cannot restore the history repository's active root.

**Decision.** One evaluation transaction owns witness initialization, execution,
linear-authority publication, RSpace state, post-execution validation, and active-
root promotion. Its authenticated base root is $`B`$. A parser rejection before
execution returns the empty witness. A reducer rejection retains the current
attempt witness, removes every current-deploy linear stack effect, and lets the
enclosing play checkpoint restore RSpace. Every validation error after successful
execution restores the transaction's base state before returning the error.

For output witness $`W_{out}`$, attempted-work witness $`W_{attempt}`$, active
root $`H_{out}`$, and linear effects $`L_{out}`$, the rejection laws are:

```math
\begin{aligned}
ParserReject &\Rightarrow W_{out}=\varnothing \land H_{out}=B,\\
ReducerReject &\Rightarrow W_{out}=W_{attempt} \land L_{out}=\varnothing,\\
PostValidateReject &\Rightarrow H_{out}=B \land L_{out}=\varnothing.
\end{aligned}
```

The implementation uses a soft checkpoint only while the history base has not
advanced. Whole-block replay resets explicitly to the authenticated block
pre-state on any error because per-effect replay materializes intermediate roots
before comparing their recorded post-state witnesses. Content-addressed history
nodes may remain available for deduplication, but the rejected root is not the
runtime's active state and cannot feed the next replay step or settlement.

```text
evaluate(base, source, evidence):
    if parse(source) fails:
        return reject(emptyWitness, base)

    clearCurrentWitness()
    result = execute(source)
    if result is a reducer error:
        rollbackCurrentDeployLinearEffects()
        restoreRSpace(base)
        return reject(attemptWitness, base)

    candidate = checkpoint(result.state)
    if validate(candidate, evidence) fails:
        resetActiveRoot(base)
        return reject(result.witness, base)

    return accept(result.witness, candidate)
```

**Rejected alternatives.** Clearing every failure witness would refund attempted
work and contradict DR-47. Returning the live budget on a parser failure would
leak predecessor evidence. Treating post-validation as read-only would ignore
that validation occurs after state mutation and, during replay, after candidate-
root materialization. Reverting a soft checkpoint after `create_checkpoint`
would combine an old hot-store snapshot with the new history base and would not
restore $`B`$.

**Formal verification.** `EvaluationTransactionIsolation.tla` represents parser
rejection, fresh witness initialization, reducer work, play validation, replay
candidate-checkpoint publication, and replay validation as separate actions. TLC
and Apalache check all safe paths; five controls must respectively violate parser
witness freshness, reducer attempt retention, play rollback, replay rollback
after checkpoint publication, or final-state-gated merge-evidence publication.
`EvaluationTransactionIsolation.v` proves the corresponding unbounded state
projections, including pre-validation checkpoint discard and evidence
non-publication on rejection, without assumptions. `StackIntroductionAtomicity.v` separately proves
that enclosing-deployment rollback removes linear custody while retaining
attempted byte work.

**Implementation verification.** Interpreter regressions cover parser failure
after a paid deploy, reducer operator failure after attempted charging, and a
later deploy abort after committed stack introductions. Runtime-manager tests
forge a replay post-state witness, require `EffectStateMismatch`, and then prove
that the replay runtime's active checkpoint equals the block pre-state. Existing
play/replay cost, status, authority, birth, settlement, and root-tamper tests cover
the remaining validation exits.

**Cross-refs.** CA-P-197, TM-CA-186, UC-CA-177, REL-015, DR-47, DR-49, and
[`cost-accounting-impl/evaluation-transaction-isolation.md`](cost-accounting-impl/evaluation-transaction-isolation.md).

## DR-51 — Mergeable evidence is locally replayed and keyed by complete execution identity

**Context.** Multi-parent merging needs a per-deployment vector of mergeable-
channel differences. The vector is derived during execution but is not committed
in `BlockMessage`. The cache formerly used only post-state, creator, and sequence
number. Two equivocations can share those fields while having different
pre-states, processed deployment witnesses, system deployments, and mergeable
vectors. Their inserts therefore targeted one key and made lookup depend on
arrival order. Last-finalized-state synchronization also accepted a peer's
serialized vector directly even though no block commitment authenticated it.

**Decision.** Treat mergeable evidence as reconstructible, consensus-adjacent
auxiliary state. Only successful local execution or replay may publish it. The
cache key is:

```math
K(B)=\operatorname{Encode}\left(
H_{pre}(B),H_{post}(B),creator(B),sequence(B),
h_{v2}(executedPayload(B),genesis(B))
\right).
```

The payload digest is domain-separated, length-delimited, and binds every
canonical processed user and system deployment witness plus genesis mode.
Within each deployment, the event log is canonicalized as the replay-semantic
multiset proven by `rb_replay_payload_canonical_user_trace_permutation` and
`rb_replay_payload_canonical_system_trace_permutation`. This is the only
quotient: deployment order and every non-log protobuf field remain bound.
RSpace replay rigs the same multiset, and merge evidence is a function of that
multiset together with the exact pre/post roots, so two keys alias only when
their execution witnesses are already replay- and merge-equivalent.
Ordinary admission-rejected records are excluded because they did not execute.
Replay computes the candidate state and merge vectors without publishing,
validates every recorded effect, compares the exact final post-state, and then
performs the sole cache write. Any failure resets the active root to the
authenticated block pre-state and leaves the cache unchanged.

Initialization synchronizes authenticated blocks and trie nodes, not trusted
merge vectors. It ignores every incoming `MergeableEntryResponse`. A running
node retains wire compatibility by returning an empty response for a known
block; an absent block remains a silent miss. Missing entries are deterministically
replayed locally before merge. The legacy message variants remain decodable but
their payload has no authority.

The block hash is not a key component because proposal execution produces the
evidence before the final hash exists. Complete execution identity is sufficient:
distinct executions separate, while byte-identical executions derive identical
evidence and may safely share an entry.

Finalized-entry garbage collection receives the complete authenticated block,
re-derives $`K(B)`$, and deletes exactly that key. It cannot delete by the
legacy tuple. Eligibility requires finalization, strict separation beyond the
configured parent-depth horizon plus safety buffer, a nonempty child set, and
every recorded latest message advancing through a child along any DAG parent
path. Secondary-parent integration is therefore sufficient evidence of causal
advancement; a main-spine-only test is conservatively safe but leaks entries
that are no longer reachable. Missing DAG evidence fails closed. If retired evidence is needed later, only local
authenticated replay may reconstruct it.

**Rejected alternatives.** Keeping the legacy key and storing a vector list
would require every caller to identify which aliased vector matched its block,
recreating the omitted identity at lookup. Adding only block hash would make the
proposer unable to save under the final key and would duplicate a derived
transition identity. Trusting a validator signature on the peer response would
authenticate the sender, not the uncommitted vector. Replaying only when a peer
payload is absent would preserve the unauthenticated fast path.

**Formal verification.** `MergeableEvidenceAuthentication.tla` explores two
validators, two equivocations sharing the legacy tuple, both replay orders, and
peer-response races. TLC and Apalache require the safe model to preserve exact
lookup, local provenance, overwrite exclusion, and opposite-order convergence.
The legacy-key control must violate `OppositeArrivalOrdersConverge`; the peer-
trust control must violate `LocallyDerivedEvidenceOnly`.
The legacy-delete control must violate
`DeletionCommutesWithDistinctReplay`, demonstrating that retiring one legacy
alias and replaying the other cannot be reordered safely.
The vacuous-latest control must violate `RetirementRequiresEverySafetyGuard`,
demonstrating that an empty latest-message set is absence of advancement
evidence rather than proof that every validator has moved beyond the block.
The main-spine-only control must violate `SecondaryParentRetirementComplete`,
demonstrating that a latest message can advance through a child solely by a
secondary-parent path and must still permit retirement.
`MergeableEvidenceAuthentication.v` proves complete-key injectivity, separation
by every component, a concrete legacy alias, exact local publication, peer
non-publication and non-overwrite, preservation of both distinct entries,
pointwise insertion commutation, and opposite-order lookup equality without
assumptions. It also proves exact target deletion, preservation of every
distinct execution, and that retirement implies every conservative safety
guard including a concrete latest-message witness.
It additionally proves that full DAG ancestry recognizes every parent-path
witness and exhibits the secondary-parent case that a main-spine-only
recognizer misses.

**Implementation verification.** Rust examples mutate every block identity
dimension, and a generated property mutates every serialized key field. Engine
regressions feed a forged nonempty response to initialization and require an
unchanged store, then require a running node to return no evidence bytes. Loom
exhausts concurrent equivocation insertions, a local-replay/peer race, and two
validators with opposite replay orders, plus finalized retirement concurrent
with replay of a distinct legacy-key alias. Replay regressions require rejected
final-state witnesses to publish no key and missing entries to be reconstructed
locally. Runtime-manager and storage-backed garbage-collection regressions
require exact-key deletion while preserving an ineligible neighboring entry;
the DAG-policy suite exercises every finality, depth, child, empty-latest, latest-message,
and malformed-state guard.

**Cross-refs.** CA-P-198, TM-CA-187, UC-CA-178, REL-016, DR-50, and
[`cost-accounting-impl/mergeable-evidence-authentication.md`](cost-accounting-impl/mergeable-evidence-authentication.md).

## DR-52 — Close the allocator lifecycle at completed block boundaries

**Context.** The canonical integration workload crossed the aggregate RSS
ceiling even after replay caches, queues, and mergeable-evidence retention were
bounded. Live-memory gauges accounted for only a small fraction of resident
memory. Rust had dropped the per-block replay and RSpace values, but glibc kept
their free pages in process arenas. A full canonical run reached approximately
8.27 GB before guardian termination. A controlled six-node bonding path with
per-block allocator reclamation remained approximately 2.1–2.7 GB before a
separate harness assertion stopped the run.

**Decision.** Treat allocator reclamation as a distinct, non-consensus
refinement after Rust ownership ends. `BlockProcessorInstance` is the sole
incoming-block lifecycle owner. On Linux/glibc it advances one bounded atomic
completion counter and calls `RuntimeManager::trim_allocator` at the configured
interval. The default interval is one. An outer drop guard closes every local
proposal attempt, including empty, rejected, and unwound paths, after its
transient values are destroyed. The inner Casper block processor no longer owns
another counter or call.

The counter resets at the boundary instead of wrapping a lifetime total. For
any positive interval $`I`$, it remains in $`0..I-1`$ under arbitrary
concurrent completion order. An interval of zero explicitly disables the
incoming boundary. Reclamation is best-effort platform behavior and cannot be
used as a proof that all free bytes leave RSS; the integration guardian remains
the platform-refinement oracle.

**Consensus boundary.** Allocator reclamation runs after semantic block work
and has no access to consensus inputs or outputs. It cannot alter the block
hash, deploy order, RSpace root, SystemVault state, cost witness, mergeable
evidence, latest messages, committee, clique, or finalized floor. The causal
majority and state-preservation algorithms are unchanged. A memory failure may
not be repaired by shortening consensus history, reducing formal bounds,
weakening validation, or raising the guardian ceiling.

**Rejected alternatives.** Relying on Rust `Drop` ignores allocator arena
retention demonstrated by the RSS experiment. Keeping both the Casper-internal
and node-wrapper trim calls gives one completion two owners and inconsistent
defaults. A wrapping lifetime counter delays arbitrary non-power-of-two
intervals after machine-integer overflow. Trimming only every 64 blocks permits
the observed workload to breach its host ceiling before the first effective
boundary. Disabling replay caches or semantic evidence would trade a resource
symptom for correctness and recovery failures.

**Formal verification.** `BlockHeapLifecycle.tla` exhausts allocation and
completion schedules across two slots, proves the interval resident envelope,
and carries an independent committed-history reference to establish semantic
noninterference. TLC and Apalache must both reject
`BlockHeapLifecycleMissingBoundaryUnsafe.cfg`. `BlockHeapLifecycle.v` proves
the bounded counter, exact boundary, default reclamation, interval invariant,
resident envelope, and concrete missing-boundary witness without assumptions.
The Loom model exhausts compare-and-exchange schedules for intervals one, two,
and disabled reclamation.

**Implementation verification.** Node examples cover default, invalid,
disabled, explicit, boundary, corrupted-counter, and maximum-machine-integer
inputs. A proptest ranges over arbitrary `usize` counters and positive
intervals. Clippy and release builds cover both node and Casper ownership
sites. The canonical Linux/glibc integration run executes without an environment
override and records process RSS, active block count, and trim cadence under the
same aggregate guardian used by CI.

**Cross-refs.** CA-P-199, TM-CA-188, DR-51, and
[`cost-accounting-impl/block-heap-lifecycle.md`](cost-accounting-impl/block-heap-lifecycle.md).

## DR-53 — Terminal admission records are not runtime-effect records

**Context.** DR-31 made an underfunded deploy consensus-visible as a terminal
`ProcessedDeploy` so clients do not wait indefinitely and validators can
recompute the decision from the authenticated proposal pre-state. The record is
created after the admitted user sequence executes. It has no user cost, event
log, or state transition. Runtime replay correctly excludes it and therefore
produces no mergeable-channel map for it. `BlockIndex` nevertheless counted
every block-body user record when aligning the locally reconstructed metadata.
A block containing one terminal funding rejection and one executed `closeBlock`
therefore carried one valid metadata map but was incorrectly required to carry
two. Every validator failed parent indexing while proposing a successor, which
stalled the shard and left later deploys pending.

**Decision.** Define the ordered user effect projection as:

```math
E_U=[u\in U\mid u.\operatorname{admissionStatus}\ne\text{Rejected}].
```

For processed system executions $`S`$ and locally replayed merge metadata $`M`$,
require $`|M|=|E_U|+|S|`$. `BlockIndex` MUST use $`E_U`$ consistently for
cardinality, adjacent state-witness validation, user/system metadata splitting,
and execution indices. An ordinary deploy with `is_failed=true` but
`admissionStatus=Executed` remains effect-bearing because it entered the runtime
and owns a metadata position. Only a terminal pre-execution admission rejection
is excluded.

The raw block body remains unchanged. Terminal rejections retain their complete
signed envelope and consensus-visible status, while the runtime effect stream
remains an exact representation of what executed. A true missing or extra map
continues to fail closed.

**Consensus boundary.** This decision does not change clique membership,
majority weight, fork choice, block bytes, or the terminal admission decision.
It repairs the refinement from consensus-visible lifecycle records to derived
runtime effects. Failure of that refinement is availability-critical: identical
honest validators can all reject successor construction even though they agree
on the parent state.

**Rejected alternatives.** Fabricating an empty metadata map for a rejected
deploy would invent an execution. Filtering every `is_failed` record would shift
ordinary executed-failure and system indices. Removing the terminal status would
reintroduce indefinite pending. Ignoring all cardinality mismatches would admit
genuine evidence loss. Retrying proposals or increasing timeouts cannot repair a
deterministic projection error.

**Formal verification.** `AdmissionEffectAlignment.tla` checks three independent
validators, arbitrary indexing/proposal interleavings, exact metadata alignment,
and later-deploy liveness. TLC and Apalache pass the effect projection; the raw
status-counting control blocks the first validator and violates
`Inv_StatusOnlyRecordCannotBlock`. Axiom-free Rocq
`AdmissionEffectAlignment.v` proves rejection insertion transparency, executed-
failure retention, cardinality permutation invariance, exact user/system split,
and the concrete funding-rejection-plus-`closeBlock` regression.

**Implementation verification.** Rust examples pin the concrete regression,
ordinary execution failures, and projection order. A 256-case property varies
admission status, runtime failure, user record order, and system count. The
deploy-lifecycle gate runs the safe and unsafe TLC/Apalache controls and the Rust
suite. The canonical multi-node workload remains the release oracle for proposal
liveness after a terminal funding rejection.

**Cross-refs.** CA-P-200, TM-CA-189, UC-CA-180, REL-018, DR-31, DR-50,
DR-51, and
[`cost-accounting-impl/admission-effect-alignment.md`](cost-accounting-impl/admission-effect-alignment.md).

---

## DR-54 — Durable finalizer discovery follows complete causal evidence

**Status:** accepted and implemented; aggregate and canonical multi-node gates
remain release evidence.

**Context.** State-preserving cost effects made a block carried through a
secondary parent eligible for finalization. That path used complete all-parent
latest-message coverage. The durable LFB finalizer still propagated support
only through main-parent edges. It could therefore fail to enumerate the exact
target. Repeated scheduling did not help because every frozen view reproduced
the same incomplete candidate set.

**Decision.** Candidate enumeration in the durable finalizer MUST use the same
descending all-parent coverage relation as floor derivation. For frozen latest
messages $`J`$, the supporter set at candidate $`C`$ is exactly
$`\{v \mid C \preceq_{DAG} J(v)\}`$. Candidates are considered in descending
`(block_number, block_hash)` order. Enumeration does not confer finality: the
candidate's corresponding committee and supporter weights still feed the
unchanged hard-majority gate and exact mutual causal clique; the independently
filtered state supporters feed the same exact clique rule; and the candidate
must preserve the current LFB's active effects. Missing metadata or a
non-descending edge fails closed.

**Consensus boundary.** This decision changes which causally reachable blocks
are presented to the existing finality predicate. It does not change committee
membership, validator weight, clique edges, maximum-clique selection, exact FTT
arithmetic, strictness, or state-certificate refinement. A target selected for
materialization is bound to its own evidence and the frozen durable predecessor;
evidence for a causal-only rejected-state sibling cannot be substituted.
Independent validators remain parallel. Only the existing compare-and-append
publication point is linearized.

**Why the earlier verification missed it.** `CertifiedFloorPromotion` proved
complete all-parent discovery for per-block floors. `FinalizerProgress` proved
complete scanning only after assuming a finite candidate sequence. The proof
catalog had no refinement equating the durable finalizer's concrete sequence
with exhaustive all-parent pairwise reachability. Unit tests separately covered
off-main state admission and complete main-spine scanning, but did not compose a
proposal floor secondary to every selected tip with the durable materializer.

**Rejected alternatives.** Requiring the target on a main-parent spine would
discard valid multi-parent state. Lowering FTT or accepting causal-only support
would alter Casper safety. Treating prolonged deferral as success would hide a
non-materialized state. Selecting any reachable target without its own evidence
would permit target substitution. Serializing validators or disabling parallel
finalizer evaluation would not repair the missing relation.

**Formal verification.** `FinalizerFloorMaterialization.tla` composes two nodes,
independent latest-message delivery, a strict 8-of-16 boundary, a
state-rejected sibling, proposal deferral, and local materialization. TLC
exhausts 9,289 generated / 1,849 distinct states to depth 15; Apalache checks the
safe model through length 8. Main-parent-only and causal-only controls violate
their exact named invariants under both tools. Axiom-free
`FinalizerFloorMaterialization.v` proves target-bound dual certification,
target-substitution rejection, propagated/pairwise decision equivalence, unique
highest selection, and the concrete secondary-parent witness. The result is
exported by `MainTheorem.finalized_floor_materialization_target_alignment_correct`.

**Implementation verification.** The production finalizer calls
`latest_message_coverage_above`, the same fail-closed worklist used by floor
derivation. A slow exhaustive oracle independently recomputes every per-target
causal certificate, state certificate, current-floor preservation result, and
greatest eligible candidate. Examples cover strict equality, a state-rejected
sibling, a surviving secondary parent, split/reconverged tips, and advancement
from the selected secondary floor. The complete finalizer group passes 11 tests
with one deliberately ignored stress case. A property test varies branch depth,
parent order, validator order, and height bounds. Loom proves a concurrent
ambient latest-message arrival cannot retarget a frozen publication.

**Cross-refs.** CA-P-201, TM-CA-191, UC-CA-182, REL-022, finalized-floor
S41/L16, DR-43, DR-45, and DR-46.

---

## DR-55 — Rejected carrier ownership controls deploy retry custody

**Status:** accepted and implemented.

**Context.** Received merge validation reconstructed exact rejection records on
every validator. The proposer path gave the rejected deploy only to the carrier
owner. The validator path omitted its local identity and gave custody to no
validator.

The earlier repair restored the local identity. A separate finalized-view
leader still controlled retry selection. The carrier owner and the selected
leader could differ, which left the retry without an authorized proposer.

**Decision.** The sender of a rejected source carrier owns that retry. A node
buffers the deploy only when its validator identity equals the carrier sender.
Local eligible custody authorizes retry after the shared floor gate opens.

Ordinary deploy inclusion keeps its finalized-view leader. This leader limits
ordinary duplicate proposals. The leader does not override carrier-owner retry
custody.

Different owners can recover distinct rejected carriers concurrently. The
design adds no global retry lock and no validator serialization.

**Finalization boundary.** A node-local finalization marker cannot evict a
deploy. Only a write-once deploy-lifecycle terminal verdict authorizes pool and
cosigner removal. Finalization effect receipts remove entries only when that
terminal verdict already exists.

**Formal verification.** `DeployRecovery.tla` gives each retry its exact
custody source. Its safe configuration keeps both validators online, checks
eventual recovery or expiry and independent finalization progress, and permits
same-view work by distinct owners. A reachable-state control deliberately
forbids parallel owner recovery and must fail, preventing a vacuous concurrency
claim. The missing-custody control violates
`Inv_RetryHasCarrierOwnerCustody`.

`StaleSiblingRecovery.tla` checks canonical rejection observation and owner-only
buffer custody. Its missing-buffer control violates
`Inv_ObservedRejectionIsBuffered`. Its non-owner control violates
`Inv_OnlyCarrierOwnerRetries`.

Rocq proves unique authorization for one carrier and independent authorization
for distinct carrier owners. Loom checks received-merge delivery and parallel
owner recovery under all modeled thread interleavings.

**Implementation verification.** The unchanged retry-gate integration tests
pass both closed-gate and settled-gate paths. The unchanged finalization
eviction tests preserve an above-floor carrier and remove a floor-terminal
deploy.

**Cross-refs.** DR-33, TM-CA-171, O10 through O14, and
`formal/tlaplus/deploy_recovery/README.md`.

---

## DR-56 — Retry readiness uses collective parent coverage

**Status:** accepted and implemented. Focused formal and integration gates pass.
Aggregate and canonical multi-node gates remain release evidence.

**Context.** The retry packaging gate inverted two quantifiers. It required one
selected parent to cover every valid latest message. Multi-parent Casper only
requires the complete selected parent set to cover those messages collectively.
The stricter predicate deferred an authorized retry on a split frontier. The
same candidate could still package fresh work, which caused bounded retry
starvation.

**Decision.** Let $`V`$ contain the valid latest messages. Let $`P`$ contain the
selected parents. The relation $`v\preceq_{DAG}p`$ means that $`v`$ is $`p`$
or an ancestor of $`p`$. The frontier is ready exactly when:

```math
\forall v\in V.\;\exists p\in P.\;v\preceq_{DAG}p.
```

The proposer evaluates the relation with this fail-closed algorithm:

```text
function frontier_ready(parents, latest_messages, invalid_blocks):
    for each message in latest_messages:
        if invalid_blocks contains message:
            continue
        if no parent in parents descends from message:
            return false
    return true
```

The owner can package a retry after the shared floor gate opens. The owner also
must hold carrier custody. Collective coverage or lease expiry then satisfies
frontier readiness. Lease expiry cannot bypass the floor gate, carrier custody,
deploy lifespan, replay, or validation.

**Consensus boundary.** This decision changes proposer packaging only. It does
not change block validity, fork choice, committee weight, finality, wire bytes,
or replay. Validators continue to process independent parent branches in
parallel. The decision does not add a global retry lock or a recovery leader.

**Why the earlier verification missed it.** Earlier recovery models abstracted
frontier readiness as an input. They did not refine readiness to the concrete
parent and latest-message quantifiers. Serial fixtures also supplied one parent
that covered every latest message. Those fixtures could not expose the inverted
quantifiers.

**Rejected alternatives.** A serial coalescing block would reduce concurrency.
A global leader would break carrier ownership. Lease-only admission would hide
the incorrect readiness predicate. One-parent coverage remains sufficient, but
it is not necessary.

**Formal verification.** `RecoveryFrontierCoverage.tla` models selected parent
sets, valid latest messages, floor authorization, owner custody, lease expiry,
and independent ordinary work. The safe configuration requires collective
coverage. The one-parent control must violate
`CollectiveCoverageReadiesRetry`. `RecoveryFrontierCoverage.v` proves that
one-parent coverage implies collective coverage. It also proves that the
converse is false for a split frontier. The Rocq model keeps ordinary leadership
independent from retry authorization.

`StaleSiblingRecovery.tla` separately models canonical rejection observation on
all validators. Only the carrier owner receives retry custody. Its safe model
requires owner custody and forbids non-owner buffer entries.

**Implementation verification.** Rust examples cover split-frontier admission,
parent permutation, latest-message permutation, incomplete-frontier deferral,
owner custody retention, complete-frontier admission, reflexive coverage, and
lease expiry. The map-cell integration workload checks recovery across rotating
validators and a finalized settlement floor.

**Cross-refs.** CA-P-202, TM-CA-192, UC-CA-183, DR-55, option B1 in
`docs/casper/CONSENSUS_PHILOSOPHY.md`, and
`formal/tlaplus/deploy_recovery/README.md`.

---

## DR-57 — Failed-body settlement remains an exact state effect

**Status:** accepted and implemented. Focused formal and Rust checks pass.
Aggregate and multi-node gates remain release evidence.

**Context.** State-bound execution rolls back a failed user body. The node then
commits the verified SystemVault charge for attempted compute and byte work.

Earlier provenance code treated every failed execution as effect-free. Merge
indexing could omit the charge while lifecycle cleanup called the occurrence
terminal. A prior admission rejection could also shift later execution indices.

**Decision.** A processed deploy has a committed state effect when either rule
holds:

1. The admitted user body succeeds.
2. The admitted user body fails and carries both verified settlement records.

The settlement records are the funding certificate and the cost witness. A
terminal admission rejection consumes no execution index.

A legacy failed execution still consumes its historical execution slot. The
legacy failure does not originate a state effect.

Metadata keeps the historical wire field name
`successfulStateEffectIndices`. The field now records every committed effect
index. This interpretation avoids a wire-format change.

The LFB containment guard uses the same committed-effect predicate. It cannot
advance across a branch that omits a failed-body settlement.

Lifecycle settlement uses the adopted last finalized block state. A finality
marker does not prove effect membership. A carrier's frozen floor does not
prove effect membership.

Pool cleanup occurs only after exact provenance finds the occurrence effect in
the adopted state. Effect-free failures use causal adoption under their legacy
rule. Missing history keeps the occurrence pending.

**Rejected alternatives.** Dropping the failed charge violates the economic
model. Keeping the old failure filter loses a committed debit during merge.

Using finality markers for cleanup can destroy the last recovery copy. Using a
frozen proposal floor can delay or misclassify settlement.

**Formal verification.** `StateEffectProvenance.v` proves preservation for
successful effects and failed settlements. The TLA+ model carries both effects
through every parent permutation and repeated merge.

`DeployLifecycleFinalization.tla` separates a finality marker, frozen-floor
coverage, and exact adopted-state membership. Two unsafe controls prove that
the first two facts cannot authorize cleanup.

Rocq proves that marker and frozen-floor inputs cannot change the lifecycle
decision. Loom checks concurrent commit, marker, coverage, and cleanup orders.

**Implementation verification.** Rust properties generate admission, failure,
settlement, and system-deploy combinations. They check compact execution
indices and exact committed-effect projection.

Lifecycle tests check failed settlement membership, restore-horizon deferral,
and adopted-state cleanup. Floor tests check that failed-body settlement is
settled content. Storage tests check accepted and rejected exact effects.

**Cross-refs.** DR-43, DR-50, DR-53, CA-P-203, TM-CA-193, UC-CA-184,
E2E-052, and finalized-floor safety invariant S34.

---

## DR-58 — Protocol-6 replay uses its signed certified floor

**Status:** accepted and implemented. The canonical multi-node rerun remains
release evidence.

**Context.** Protocol 6 added a signed finalized-floor commitment to each
block. The proposal path could still derive a different floor from current
justifications. It then compared the derived candidate with the durable
context.

This design created two failures. Replay could use a state different from the
signed floor. An equality gate could also stop proposal while the finalizer
correctly rejected a state-dropping candidate.

`dev` does not contain this exact mismatch because it has no separate signed
floor certificate. However, `dev` also lacks the protocol-6 replay binding.
Copying its complete floor path would remove the new certificate guarantee.

**Decision.** A protocol-6 proposal captures one valid durable finalization
certificate. The certificate identifies floor $`F`$, its post-state, and its
authority context.

Proposal uses $`F`$ for sender authority, replay, merge scope, retry scope, and
the signed block commitment. A candidate $`G`$ from current finalizer evidence
does not enter proposal readiness.

The receiver verifies the certificate before replay. It requires one accepted
stored floor with the committed hash, state, and height. At least one declared
parent must DAG-descend from $`F`$.

Missing dependencies defer validation. An inconsistent binding invalidates the
block. Replay reconstructs state from $`F`$ and deterministic accepted effects
above $`F`$.

**Casper compatibility.** The decision retains `dev` concurrency between
proposal and finalization. It does not change LMD-GHOST, selected parents,
stake weights, clique membership, thresholds, or finalizer promotion.

Protocol versions before 6 retain the `dev` rule. They derive one deterministic
floor from frozen parents and justifications.

**Rejected alternatives.** Substituting $`G`$ breaks the signed replay
commitment. Requiring $`G=F`$ serializes proposal behind finalization. Always
replaying from genesis discards the certified floor optimization and scope.

**Formal verification.** `CertifiedReplayAnchor.v` proves exact committed-state
replay and candidate-evidence independence. `ProposalFloorReadiness.v` proves
that only certified authority and permit state control proposal readiness.

`StatePreservingForkChoice.tla` checks the committed replay anchor. Its
substitution control violates that invariant. `ProposalFloorReadiness.tla`
checks concurrent proposal and finalizer actions over two nodes. Candidate-gate
and finalizer-cancellation controls reproduce the liveness failures.

**Implementation verification.** Rust tests bind proposal replay to the
captured certificate. Receiver validation uses the signed commitment. A
256-case property checks exact hash, state, height, and admission identity.

The parent replay regression checks a covering parent that omits $`F`$ state.
Two Loom tests check concurrent promotion and candidate observation. The
canonical lifecycle run must prove multi-node progress before release.

**Cross-refs.** TM-CA-194, H24, T-CERTIFIED-REPLAY-ANCHOR,
T-PROPOSAL-FLOOR-READINESS, R-FINALIZATION-PROPOSAL-READINESS, and
R-CERTIFIED-REPLAY-ANCHOR.

---

## DR-59 — Initial validator allocation occurs only during authenticated genesis

**Status:** accepted, implemented, and verified.

**Context.** The PoS contract queued every successful bond for `initial_phlogiston`.
The next close-block transition credited that amount to the validator's transferable SystemVault custody.

This behavior created a repeatable subsidy. A validator could withdraw, rebond, and receive another initial allocation.

The papers define initial validator provisioning as bootstrap authority. They do not define bonding as a mint operation.
Normal validator replenishment belongs to authenticated epoch issuance after active-set selection.

**Decision.** The blessed genesis SystemVault allocation is the only `initial_phlogiston` transition.
The PoS contract does not retain a pending-initial queue.

A fresh bond or rebond transfers existing custody into stake. The operation creates no new custody.

An epoch close selects active validators before issuance. Each eligible validator receives exactly `epoch_phlogiston` once for that epoch.

Eligibility requires an epoch boundary, active membership, and no mint halt.
The retained frontier in DR-61 prevents duplicate epoch issuance.
Withdrawal, slash, redemption, and activation create no initial allocation.

A successful rebond increments the validator's bond generation. Ordinary epoch issuance does not change that generation.

**Casper compatibility.** This decision changes no voting, finality, fork choice, parent selection, or wire format.
Validators still execute independent blocks concurrently. Play and replay apply the same deterministic PoS transition.

**Invariants.** The repair requires these properties:

- Only authenticated genesis can increase `playInitialCredit` or `replayInitialCredit`.
- Bonding conserves combined liquid custody and stake, excluding ordinary execution cost.
- Epoch credit requires boundary, active membership, and a non-halted validator.
- One canonical epoch close credits each eligible validator at most once.
- Withdrawal and settlement conserve custody plus stake.
- Slash, redemption, and activation create no custody.
- Play and replay compute identical issuance and post-state roots.
- Distinct validator lifecycle transitions commute when they access disjoint custody.

**Formal verification.** `BondIssuanceLifecycle.v` proves the unbounded arithmetic and lifecycle obligations without axioms.
`BondIssuanceLifecycle.tla` explores two interleaved validators through bond, activation, withdrawal, slash, redemption, and rebond.

Nine registered controls reproduce fresh-bond, rebond, eligibility, duplicate, generation, and asymmetric replay defects.
TLC and Apalache must refute every control through its named invariant.

**Implementation verification.** Generated lifecycle traces check conservation, logical receipts, generations, replay equality, and distinct-validator commutativity.
Native runtime tests check fresh bond, epoch activation, duplicate close, completed withdrawal, and rebond.

The native tests compare every close-block play root with independent replay from the same pre-state.
The PoS contract suite checks that bonding transfers stake without minting liquid custody.

**Cross-refs.** Appendix B.1 and B.3 of `cost-accounted-rho.tex`; DR-30, DR-36, UC-CA-153, and UC-CA-154.

---

## DR-60 — Epoch close publishes all validator issuance or no state

**Status:** accepted and implemented.

**Context.** One epoch close applies rewards, withdrawals, active-set changes,
validator issuance, and its retained frontier. These effects form one financial
transaction.

The PoS fold calls `protocolMint` once for each eligible validator. An early
call can change SystemVault custody before a later call fails.

The former fold ignored a failed call. A later checkpoint could therefore
publish incomplete issuance and inconsistent replay-protection state.

**Decision.** The complete close-block system deploy is the transaction
boundary. Every successful effect publishes together at one final checkpoint.

The fold stops after the first mint failure. The fold advances the retained
frontier only after all required mint operations succeed.

Zero issuance records completion without calling the positive-only mint
primitive. This rule makes zero issuance successful and idempotent.

Every system-deploy failure restores the exact supplied pre-state root. This
rule covers contract rejection, interpreter error, and post-evaluation read
failure.

The block creator propagates the failed checkpoint attempt. It creates no block
from a failed epoch close.

A retry starts from the same pre-state and applies each eligible credit once.
Distinct validator credits remain independent and order-insensitive.

**Casper compatibility.** The repair adds no global lock and changes no vote,
clique, threshold, fork-choice, or finalization rule.

Validators can still execute disjoint proposals concurrently. Atomicity applies
only to the existing per-block system-deploy checkpoint.

**Paper alignment.** The cost-accounting papers require one funded financial
transaction to publish all effects or no effects.

Rewrite atomicity alone is insufficient. SystemVault custody and the PoS
frontier must share the deployment transaction boundary.

**Formal verification.** `EpochMintAtomicity.v` proves failure identity,
complete issuance, exact supply change, idempotence, commutation, and replay
agreement.

`EpochMintAtomicity.tla` explores three independently ordered validator mints.
Six unsafe controls reproduce partial balance, partial receipt, missing
rollback, replay, swallowed-failure, and zero-call defects.

Loom explores concurrent disjoint completion, failure, duplicate completion,
and retry publication. Generated Rust cases refine the same atomic transition.

**Implementation verification.** Native PoS tests force overflow at each
validator position. They require exact root rollback and unchanged custody.

The same tests repair the balance, retry the epoch, replay it, and repeat it.
They require one exact credit per validator.

**Cross-refs.** DR-30, DR-48, DR-50, DR-59, DR-61, CA-P-206,
TM-CA-196, UC-CA-185, and E2E-053.

---

## DR-61 — One monotonic frontier bounds epoch-mint replay protection

**Status:** accepted, implemented, and verified.

**Context.** A per-validator epoch set grows with validator count and shard
age. It retains history that the canonical close transition does not need.

An epoch close is one atomic system-deploy effect chain. The merger selects one
complete sibling close effect and rejects redundant sibling close effects.

The merger never combines partial validator mint effects from sibling closes.
Therefore, the canonical chain completes each epoch collectively or not at all.

**Decision.** PoS stores one signed integer named `mintedThroughEpoch`.
The initial value is `-1`, which means that no epoch close has completed.

A direct contract test can close epoch zero from `-1`. Production can close
epoch one from `-1` because genesis contains no epoch-zero close deploy.

After bootstrap, only epoch `f + 1` can advance frontier `f`. An epoch at or
below `f` is a successful no-op.

An epoch above `f + 1` fails without a state change. The gap reports a missing
required close effect instead of repairing history silently.

Zero issuance and an empty eligible set still advance the frontier. The close
must first complete every required operation.

Bonding, withdrawal, slashing, redemption, and rebonding never decrease or
clear the frontier. These operations never create historical catch-up issuance.

A bond that becomes canonical after a selected close waits for the next epoch.
This rule makes supply independent of sibling arrival order.

**Safety interpretation.** The frontier is a compact representation of the
logical receipt history on legal canonical traces.

Logical receipts remain useful as proof variables. Production does not store
those proof variables.

The retained value uses constant storage. Restart and replay read the same
frontier from the authenticated PoS state.

**Casper compatibility.** This decision changes no vote, threshold, clique,
fork-choice, finalization, or network rule.

The decision uses the existing atomic close-block boundary. Validators retain
parallel proposal and replay execution.

**Migration.** A new shard needs no migration. Genesis installs the frontier
with value `-1`.

A live contract migration requires an authenticated activation state and an
exact canonical height witness. Migration must reject contradictory history or
supply.

**Formal verification.** `MintedEpochRetention.v` proves bootstrap,
monotonicity, gap rejection, atomic rollback, lifecycle preservation, restart,
sibling selection, and constant storage.

`MintedEpochFrontier.tla` explores concurrent sibling close, bond, slash,
redemption, restart, duplicate close, and gap-close transitions.

Eight unsafe configurations refute incorrect initialization, rejected
bootstrap, accepted gaps, double siblings, dropped siblings, catch-up bonds,
frontier clearing, and retroactive redemption minting.

The Loom model explores concurrent mint completion, failure, retry, duplicate
completion, sibling publication, both bootstraps, and gap rejection.

Generated Rust properties refine the retained frontier against a full logical
receipt history. Native tests check exact replay roots and consecutive epochs.

A multi-parent regression checks two boundary siblings. The merged state must
contain one validator credit on every node.

**Cross-refs.** DR-3, DR-30, DR-50, DR-59, DR-60, CA-P-207,
TM-CA-197, UC-CA-186, and E2E-054.

## DR-62 — Replay captures every economic input before trace rigging

**Context.** Ordinary execution charges one fixed handler cost from the
proposer's role-separated validator-fuel purse. Admission reads that purse from
the candidate pre-state and selects a maximal affordable prefix.

Replay previously queried the validator-fuel purse after it rigged the recorded
deployment trace. ReplayRSpace accepts only communications from that trace.

The unrecorded SystemVault query could not complete. Valid blocks then stalled
or failed replay, although their authority-purse snapshots were correct.

**Decision.** A separate ordinary runtime captures one complete economic
snapshot at each authenticated deployment pre-state. Capture occurs before
ReplayRSpace receives the recorded trace.

The snapshot contains every required authority purse and the proposer
validator-fuel balance. The certificate's fee recipient selects that proposer.

The snapshot binds its root, proposer, and exact nonnegative balance. Replay
rejects a missing snapshot or any binding mismatch before user execution.

ReplayRSpace performs no registry, purse, or validator-fuel query. It consumes
only the recorded causal events for that deployment.

The snapshot is evidence for replay admission. It does not replace physical
custody or authorize a debit by itself.

The final SystemVault application checks and debits actual role-separated
custody in the replayed state. A stale or forged snapshot cannot create fuel.

Play and replay use independent transition implementations. Each session owns
its certificate cursor, so concurrent validators cannot consume another
session's replay evidence.

```text
require deployment.pre_state = current_root
snapshot := ordinary_runtime.capture(current_root, certified_proposer)
require snapshot.fuel >= handler_cost
replay_runtime.rig(recorded_trace)
replay_runtime.execute(snapshot)
require atomic_settlement_debits_actual_fuel(handler_cost)
require checkpoint = deployment.post_state
```

**Casper compatibility.** This decision changes no vote, clique, threshold,
fork-choice, finalization, or parent-selection rule. Validators retain
independent replay sessions and parallel block processing.

**Formal verification.** `ReplaySupplySnapshot.tla` models the exact six-fuel,
three-candidate failure. The safe model admits two candidates and defers one.

Eight controls independently remove runtime separation, capture order, root
binding, proposer binding, balance authenticity, settlement, or deferred
noninterference. TLC and Apalache must refute every control.

`ValidatorEconomicsReplay.v` proves independent play and replay refinement. It
also proves binding checks, exact depletion, maximal-prefix equality, and
session cursor separation without axioms.

Loom explores stale snapshot settlement, independent session cursors, and
disjoint validator settlement. Generated Rust properties use separate play and
replay machines.

**Verification lesson.** The earlier replay model covered authority-purse
snapshots but did not enumerate validator fuel as a separate economic input.

The implementation added that input after the modeled snapshot boundary. A
shared high-level assumption therefore hid a native phase-order defect.

CA-P-208 now requires an inventory of every state-dependent economic query.
Each query needs a production boundary test and an independent negative
control.

**Cross-refs.** DR-37, DR-38, DR-50, CA-P-191, CA-P-208, TM-CA-198,
UC-CA-187, and E2E-055.

## DR-63 — Every proposal retry receives a fresh state-bound admission certificate

**Context.** Proposal admission classifies one canonical candidate window
against an authenticated pre-state.

Checkpoint construction can exceed its capacity after admission completes.
The proposer previously shrank the admitted vector and reused its original
certificate.

That retry no longer represented the canonical raw candidate prefix. It could
also lose terminal rejections or drain candidates outside the successful
window.

**Decision.** The proposer canonicalizes raw user candidates once. Every
attempt selects a raw-user prefix and appends the complete dummy set.

The runtime certifies the complete window again. The opaque certificate binds
the candidate identities, authenticated context, and complete admission
partition.

The partition contains three disjoint classes: admitted, rejected, and
deferred. Each class preserves canonical window order.

One owned attempt value binds its generation, limit, window, partition, and
certificate. Checkpoint execution can consume only that attempt.

```text
users := canonicalize(raw_users)
generation := 0
limit := length(users)

repeat
  window := canonicalize(first(limit, users) + dummies)
  attempt := certify(generation, window, authenticated_context)
  result := execute_checkpoint(attempt)

  if result succeeds
    publish(attempt, result)
    drain(admitted_users(attempt) + rejected_users(attempt))
    stop

  publish_nothing()
  settle_nothing()
  drain_nothing()
  generation := generation + 1
  limit := strictly_smaller(limit)
end
```

Only the successful attempt can publish state, settle cost, package rejection
evidence, or drain terminal user candidates.

Deferred users and removed suffix users remain available. Rejected dummy
deployments remain in the published partition but never enter user storage.

Validators own independent attempt values. The repair adds no global lock and
does not change voting, finality, fork choice, or parent selection.

**Formal verification.** `CheckpointAdmissionRecertification.v` proves
partition completeness, disjointness, order, exact roots, atomic failure, and
strict retry termination.

`CheckpointAdmissionRecertification.tla` explores two independently scheduled
validators. Twelve controls isolate each forbidden stale, partial, early, or
shared-state behavior.

TLC and Apalache must refute every control. Loom explores atomic terminal
drain, failed-attempt custody, invalid partitions, and independent storage.

Rust properties check arbitrary partition classifications. Forced retry tests
check final-window rejection evidence, suffix retention, and independent peer
replay.

**Cross-refs.** DR-11, DR-31, DR-38, CA-P-209, TM-CA-199, UC-CA-188,
and E2E-056.

## DR-64 — Funding is checked before execution, and the unprovable remainder is held to the signed limit

**Status.** Adopted 2026-10-03 as decision D1 of epic 8946. Batch B2 (gap G1)
implements it.

**Terms.** Records DR-64 to DR-74 use these names.

- P1 is the Cost-Accounted Rho Calculus paper,
  `publications/cost-accounting/cost-accounted-rho.tex`, at publications
  revision `0bf78174`.
- $`R_0`$ is the original root. It is the authenticated state root that a
  candidate funds from.
- A source is one signed funding entry of an offered envelope
  (`PhloSourcePolicyV1`). Its `hold_cap` and `debit_cap` bound what the source
  can hold and debit. The total exposure bounds all sources together.
- A hold reserves part of a source balance for one candidate until
  settlement.
- $`\Delta^{\mathrm{known}}_s`$ is the demand that static analysis proves for
  the signature lane $`s`$.
- `phloLimit` and `phloPrice` are the signed limit and the signed price of the
  envelope.

**Context.** P1 §`sec:acceptance-protocol` requires the validator to compute
the demand by static analysis before any part of a deployment executes.

The offered path executed the candidate first (`evaluate_native_offered`) and
measured its obligations afterward. The static analysis (`delta_sigma`,
`static_authority_plan`) and the hold arithmetic (`FundingBranchReservation`)
existed, but no production path called them.

P1 gives a margin rule for demand that static analysis cannot resolve. An
unresolvable dereference contributes an "unknown" demand. The validator then
rejects unless the supply exceeds the known lower bound plus a configurable
safety margin (proof sketch of `thm:decidability`).

![Activity diagram of the pre-execution acceptance check. A signed offered envelope and the original root R0 enter. The check resolves lexical names from the envelope seed and runs static_authority_plan for each signature lane. A provable lane contributes its exact demand and syntactic byte charges, and an unprovable lane contributes zero. The remainder R equals the signed phloLimit minus the known demand, times phloPrice. The check fits the known holds plus the complete remainder on eligible signed sources at R0, within hold_cap, debit_cap, and total exposure. If the holds do not fit, the candidate is rejected before any runtime entry, with no effect and no charge. Otherwise it executes in a fresh offered runtime. Within bounds, settlement charges measured use and refunds the unused hold. Exhaustion of the signed limit in a capped part is a classified user failure. An overrun of an exact lane is a correctness failure with no charge.](diagrams/offered-preexecution-acceptance.svg)

(*Source: [`diagrams/offered-preexecution-acceptance.puml`](diagrams/offered-preexecution-acceptance.puml) — render with `plantuml -tsvg docs/casper/theory/diagrams/offered-preexecution-acceptance.puml`.*)

**Decision.** The signed `phloLimit` is the P1 safety margin.

1. Before execution, the producer and every validator compute the known demand
   $`\Delta^{\mathrm{known}}_s`$ for each signature lane $`s`$. They use the
   envelope and $`R_0`$ only.
2. The computation reuses `static_authority_plan`, the lexical name resolver,
   and the pure byte charge functions. A construct that the analysis cannot
   prove contributes 0 to the known demand.
3. The unprovable remainder $`R`$ is the unused part of the signed limit at the
   signed price:

   ```math
   R \;=\; \left(\mathrm{phloLimit} - \sum_{s} \Delta^{\mathrm{known}}_s\right) \cdot \mathrm{phloPrice}.
   ```

4. The producer holds the known demand and the complete remainder on the
   eligible signed sources. A partial remainder hold is not allowed.
5. If the holds do not fit the supply at $`R_0`$, the candidate is rejected
   before any runtime entry. The rejection has no effect and no charge.
6. Settlement charges the measured use. It refunds the unused hold to the
   original sources.

Unprovable constructs include data-dependent recursion, unresolved
dereference, persistent input, dynamic authority, and payload sizes that come
from stored state.

**Algorithm (literate form).** The check is one top-level chunk built from four
named chunks. The text before each chunk explains it.

```text
⟨pre-execution acceptance⟩ ≡
  ⟨compute the known demand⟩
  ⟨compute the remainder⟩
  ⟨fit the holds or reject⟩
  ⟨execute, settle, and refund⟩
```

The known demand comes only from analyzers that already exist. A lane that the
analysis cannot prove contributes nothing here, because the remainder covers
it.

```text
⟨compute the known demand⟩ ≡
  names ← resolve_lexical_names(envelope, envelope_seed)
  for each signature lane s of the envelope
    known[s] ← static_authority_plan(s, names), or 0 on UnprovableDemand
  known[s] ← known[s] + byte_charges(syntactic sends and receives of lane s)
```

The remainder is the P1 safety margin. The signer chose it when the signer
chose the limit.

```text
⟨compute the remainder⟩ ≡
  margin ← (phlo_limit − Σ known[s]) × phlo_price
```

The holds are checked against the authenticated supply before any runtime
work. This is the gate of P1 acceptance step 5.

```text
⟨fit the holds or reject⟩ ≡
  holds ← fit(known + margin, eligible_sources, hold_cap, debit_cap, total_exposure, R0)
  if holds = none
    reject(envelope, "insufficient certified funding")    -- no runtime entry
```

Execution runs with the computed bound. Settlement returns what the run did not
use.

```text
⟨execute, settle, and refund⟩ ≡
  result ← execute(envelope, resource_bound(known, margin))
  settle(result.measured_use)
  refund(holds − result.measured_use)
```

**Failure classification.** This decision adds one outcome to the failure
matrix in `cost-accounting-impl/economic-failure-policy-decisions.md`.

| Outcome | Application effect | User economic effect |
| --- | --- | --- |
| Exhaustion of the signed limit inside a limit-capped (unprovable) demand part | Roll back the failed user operation. | Retain the authorized billable prefix and the fee. Refund the unused hold. |

The existing outcome "exhaustion that contradicts a certified sufficient
bound" keeps its meaning for exact lanes. It is a correctness failure, and it
has no charge.

**Rejected alternatives.** A rule that requires a finite proof for every
recursion rejects deployments that P1 accepts with a margin. A partial
remainder hold contradicts the margin rule.

**Verification obligations.** The `EndToEndAuthority.v` branch lemmas
(`left_branch_fits_pointwise_max`, `right_branch_fits_pointwise_max`,
`pointwise_refund_is_unused_reservation`) extend to the capped remainder.

Tests show four facts. An underfunded offer never enters the runtime. A static
offer holds its exact demand. A dynamic offer holds up to the limit and gets
the rest back. Validators recompute the same decision. DR-67 covers the stack
safety of the newly reachable analyzer.

**Cross-refs.** P1 `def:funding-proof`, `thm:decidability`,
`def:conservative-demand`, `sec:acceptance-protocol`. DR-65, DR-67, DR-68.
Epic 8946 leaves `ofp-1-admission-inputs`, `ofp-2-bound-analyzer`, and
`ofp-1-proof-bound-kind`.

## DR-65 — Same-block offers that share a purse form one funding decision

**Status.** Adopted 2026-10-03 as decision D2 of epic 8946. Batch B3 (gap G2)
implements it.

**Terms.** These names extend the terms of DR-64.

- A purse $`p`$ is one physical custody key that sources draw on.
- A group $`G`$ is a set of same-block candidates that share at least one
  purse, directly or through other members.
- $`\mathrm{hold}_m(p)`$ is the DR-64 hold of member $`m`$ on purse $`p`$.
- $`\mathrm{supply}_{R_0}(p)`$ is the balance of purse $`p`$ at $`R_0`$.
- $`R_{i-1}`$ is the root after member $`i - 1`$ executes. $`R_{0}`$ precedes
  the first member.
- $`\mathrm{debit}_i(p)`$ is the settlement debit of member $`i`$ on purse
  $`p`$, and $`\mathrm{balance}_{\mathrm{after}}(p)`$ is the purse balance just
  before that debit.

**Context.** The proposer selected at most one offered deploy for each block.
Replay required one isolated candidate.

P1 §`sec:deploy-boundaries` (paragraph "Simultaneous arrival") treats
deployments that arrive together as one parallel composition. If their
combined demand exceeds the supply, neither deployment executes.

P1 also states that accepted resources are committed and unavailable to other
deployments.

![Activity diagram of the same-block funding decision. In the first partition, the proposer and every validator take the pending offered candidates in canonical order and run the DR-64 check of each candidate on its own signed sources at R0. A candidate that is not fundable alone is rejected alone and cannot sink others. Union-find over canonical physical custody keys groups the remaining candidates that share a purse. If any purse p has a group hold sum greater than its supply at R0, every member of the group is rejected with no effect. In the second partition, each accepted member i funds from R0 and executes from R(i-1) in a fresh offered runtime. At settlement, the held-capacity guard requires balance_after(p) minus debit_i(p) to be at least the holds of all later members. If the guard holds, the member settles. If it fails, the member is a classified user failure that rolls back and pays its billable work and fee. The block is then published, and validators recompute groups, holds, and guards.](diagrams/offered-same-block-funding-groups.svg)

(*Source: [`diagrams/offered-same-block-funding-groups.puml`](diagrams/offered-same-block-funding-groups.puml) — render with `plantuml -tsvg docs/casper/theory/diagrams/offered-same-block-funding-groups.puml`.*)

**Decision.**

1. A block can contain more than one offered deploy. The members execute in
   canonical order. Member $`i`$ funds from $`R_0`$ and executes from
   $`R_{i-1}`$.
2. The proposer first rejects each candidate that is invalid or unfundable on
   its own sources. Such a candidate cannot sink other candidates.
3. The remaining candidates form groups by union-find over canonical physical
   custody keys.
4. If one purse $`p`$ has
   $`\sum_{m \in G} \mathrm{hold}_m(p) > \mathrm{supply}_{R_0}(p)`$, every
   member of $`G`$ is rejected with no effect.
5. A held-capacity guard protects later members. At the settlement of member
   $`i`$, every purse $`p`$ must satisfy:

   ```math
   \mathrm{balance}_{\mathrm{after}}(p) - \mathrm{debit}_i(p) \;\geq\; \sum_{j > i} \mathrm{hold}_j(p).
   ```

   If the guard fails, member $`i`$ is a classified user failure. It rolls back
   and pays its billable work and fee from the restored balance.
6. Validators recompute the groups, holds, and guard outcomes for every
   included member. Rejections stay in the local pending lifecycle and never
   enter a block.
7. Each member runs in a fresh offered runtime, in proposal and in replay.

**Algorithm (literate form).** The decision has three named chunks.

```text
⟨same-block funding decision⟩ ≡
  ⟨reject candidates that fail alone⟩
  ⟨reject groups whose holds exceed a purse⟩
  ⟨execute accepted members with the held-capacity guard⟩
```

A candidate that fails on its own sources is removed first. That order keeps
one invalid candidate from sinking a valid group.

```text
⟨reject candidates that fail alone⟩ ≡
  for each candidate c in canonical order
    if dr64_check(c, own_sources(c), R0) = none
      reject(c)
```

Grouping follows P1's simultaneous-arrival rule. The members of a group share
at least one purse, so their holds add up on that purse.

```text
⟨reject groups whose holds exceed a purse⟩ ≡
  groups ← union_find(remaining candidates, canonical custody keys)
  for each group G
    if ∃ p : Σ_{m ∈ G} hold[m][p] > supply(R0, p)
      reject every member of G
```

The guard keeps resources that later members hold out of reach of earlier
members. This is P1's statement that accepted resources are committed.

```text
⟨execute accepted members with the held-capacity guard⟩ ≡
  root ← R0
  for each accepted member i in canonical order
    result ← execute(i, funding_root = R0, execution_root = root)
    if ∀ p : balance_after(p) − debit[i][p] ≥ Σ_{j > i} hold[j][p]
      settle(i, result)
    else
      classified_user_failure(i)        -- roll back, pay billable work and fee
    root ← root after member i
```

**Rejected alternatives.** A joint funding search over the group is not used,
because P1 sums the demands. Prefix admission, which accepts members until the
supply runs out, contradicts the simultaneous-arrival rule.

**Verification obligations.** `SameBlockFundingGroups.tla` checks
GroupAllOrNone, NoOverdraft, GateBeforeExecute, HeldCapacityGuard, and
DecisionPermutationInvariant under TLC and Apalache.

Negative controls for prefix admission, independent full capacity, and a
missing held guard must fail their named invariants. Rocq proves
`group_holds_prevent_overdraft`, `held_capacity_guard_preserves_later_holds`,
and `group_decision_permutation_invariant`. Each invariant becomes a Rust
property test.

**Cross-refs.** P1 `sec:deploy-boundaries`. DR-64, DR-72. Leaves
`ofp-3-funding-groups`, `ofp-4-held-capacity-guard`, and
`ofp-5-replay-groups`.

## DR-66 — Protocol-6 test genesis is activated, and the legacy meter is retired

**Status.** Adopted 2026-10-03 as decision D3 of epic 8946, by the user
directive "Activate tests; offered-only". Batch B5 (gap G4) implements the
test activation in stages. The offered-only production rule is implemented.

**Context.** Under offered activation, user deployments use only the offered
native path. Most Casper tests still built legacy genesis and body-only
deploys. The legacy phlo meter therefore stayed reachable only through tests.

The section "Historical replay contract" in
`cost-accounting-impl/activation-migration-policy-decisions.md` keeps the
legacy-replay requirement open. It closes only when the user approves narrower
historical support.

![Diagram that maps each kind of evaluation request to one accounting scope. User deploys use the native offered meter, which is funded, settled, and produces receipts. Genesis uses the trusted install scope, which is unmetered, carries the Unit signature, and records cost zero. System deploys keep their unchanged system scope. The exploratory estimate is a native dry run that is unfunded, host-work bounded, resets to the start root, and returns measured phlo. Internal reads of policy and token metadata use the unmetered internal read path. The RhoSpec harness uses the named inj path. Genesis replay is symmetric with genesis play. Historical body-only execution returns a compatibility error before any state reset, because the legacy meter adapter is commented out and a fresh protocol-6 genesis is required.](diagrams/offered-evaluation-scopes.svg)

(*Source: [`diagrams/offered-evaluation-scopes.puml`](diagrams/offered-evaluation-scopes.puml) — render with `plantuml -tsvg docs/casper/theory/diagrams/offered-evaluation-scopes.puml`.*)

**Decision.**

1. Test genesis uses protocol 6 with the offered-funded resource policy. Every
   test user deploy is an offered-funded envelope.
2. Under offered activation, admission rejects body-only deploys, selection
   takes offered envelopes only, and validation rejects a block that contains
   any body-only deploy.
3. The legacy meter adapter is commented out with the reason "D3: legacy phlo
   meter retired". Code is never deleted to disable it.
4. Each kind of evaluation maps to one scope, as the diagram shows. The
   user-facing exploratory estimate follows DR-71.
5. The plan approved on 2026-10-03 narrows historical support. Body-only
   historical execution returns an explicit compatibility error before any
   state reset. A fresh protocol-6 genesis is required.
6. The two legacy-charge tests follow the adopted failure policy. A classified
   user failure charges the billable prefix and the fee. A parse error is
   rejected before execution with no charge.

**Verification.** `OfferedOnlyActivation.v` proves the offered-only rule
without axioms. Active admission rejects body-only deploys, active selection
is height-independent and offered-only, active validation rejects any
body-only deploy, and every reachable active state is offered-only.

Two property tests extract these invariants.
`offered_selection_window_is_canonical_bounded_and_height_independent` checks
selection over random pending mixes and heights.
`active_validation_rejects_exactly_blocks_with_a_body_only_deploy` checks the
validation predicate.

The integration test `active_validator_rejects_a_block_with_a_body_only_deploy`
re-signs an offered block with an appended body-only deploy. The peer rejects
it as an invalid transaction and does not add it to its DAG.

**Verification obligations for the staged test activation.** The trusted
install scope leaves genesis post-state roots unchanged, and genesis costs are
0. `OfferedOnlyActivation.v` gains an evaluation-scope type with no legacy
case. `EndToEndCostConsensus.tla` gains a negative control. The full Casper
suite passes on the activated harness.

**Cross-refs.** DR-6, DR-34, DR-47, DR-71, DR-73. Leaves under
`ofp-3-test-activation`.

## DR-67 — Stack safety covers the recursion that cost accounting adds

**Status.** Adopted 2026-10-03 as decision D4 of epic 8946 ("cost-accounting
only"). Batch B2 implements it.

**Context.** Commit `c3aacd649` made cost-accounted reduction stack-safe.
DR-64 makes the static analyzer and the lexical resolver reachable in
production. Their recursion can exhaust the stack on deep terms.

**Decision.** Only the recursion that cost accounting added becomes an
explicit work stack and value stack, with a push/pop scope stack. The ports
follow the `feature/f1r3lang-mettail-only` and mettail-rust templates
(`WorklistFoldEquivalence.v`, `StackSafePDA.v`). General interpreter recursion
is reported, not changed.

**Verification obligations.** Each port has a Rocq refinement lemma: the
worklist fold equals the recursive fold. Differential property tests compare
each port with the recursive reference, which stays as a `#[cfg(test)]`
oracle.

**Cross-refs.** DR-64. Leaves `ofp-1-stack-safe-analyzer`,
`ofp-1-stack-safe-lexical`, and `ofp-1-stack-safe-signatures`.

## DR-68 — An installer-signed continuation draws on the installer's located purse

**Status.** Adopted 2026-10-03 as decision D5 of epic 8946 ("P1 Rule 4, wallet
stack"). Batch B4 (gap G3) implements it.

**Context.** The gateway flow failed with "measured funding case has no
feasible signed assignment". Three positive obligations carried the
installer's authority: one COMM, 733 introduction bytes, and 128 trace bytes.

The installer's trigger `for (@request, deployerId <= @"agent-trigger")` is
unsigned. Under P1 uniform signing (`def:sugar-uniform`), it therefore carries
the installer's region. The runtime attribution is correct.

Funding eligibility admitted only the sources of the triggering envelope
(`family_selection.rs`). The installer's purse was not a candidate source.

![Sequence diagram with the actors Installer, Gateway client, Proposer and validators, Reducer and native meter, Funding family selection, Installer located purse, and Gateway signed sources. In an earlier block, the installer's offered deploy stores a trigger continuation with the installer's region as a stored signed term. In a later block, the gateway's offered deploy sends a message that matches the stored trigger. The reducer reports the obligations of the candidate: the COMM in the installer region and the body work in the gateway region. Funding selection makes the installer's located purse eligible for installer-region obligations, authorized by the stored signed term, and the gateway's sources eligible for gateway-region obligations. If the installer lane budget, which is the balance at R0 minus earlier group holds, covers the COMM, settlement debits both purses and the gateway receives a receipt with one row per debited purse. If the budget is exhausted, the interaction does not fire and has no charge, which is the P1 Rule 4 token gating. A note states that genesis continuations carry the Unit signature, which maps to no purse.](diagrams/installer-funded-continuation-sequence.svg)

(*Source: [`diagrams/installer-funded-continuation-sequence.puml`](diagrams/installer-funded-continuation-sequence.puml) — render with `plantuml -tsvg docs/casper/theory/diagrams/installer-funded-continuation-sequence.puml`.*)

**Decision.**

1. The installer's located purse is an eligible source for obligations in the
   installer's stored signed region. The purse is the installer's wallet under
   the D2.9 wallet keying (DR-28). The stored term
   (`extend_persistent_regions`) is the authorization.
2. The installer lane's budget is the purse balance at $`R_0`$ minus the
   earlier group holds of DR-65.
3. When that budget is exhausted, the failure-matrix outcome "missing local
   capability" applies. The interaction does not fire and has no charge. This
   is the token gating of P1 Rule 4.
4. Genesis continuations carry the `Unit` signature, which maps to no purse.
   Blessed system signers are never charged.
5. The gateway test uses its intended P1 funding-slot form
   `[gateway ⊸ slot]`. Clients read installed names through the public
   `listenForContinuationAtName`.

**Verification obligations.** The three-node gateway flow passes. A TLA+
control shows that a trigger with no stored signature cannot draw an installer
purse. `FundingSlotBootstrap.v` covers the third-party stack. Each invariant
becomes a Rust property test.

**Cross-refs.** P1 `sec:rewrites` (Rules 1–5), `def:sugar-uniform`,
`def:funding-proof`. DR-28, DR-64, DR-65. Leaves
`ofp-2-rooted-region-sources` and `ofp-2-gateway-attribution`.

## DR-69 — Recovery carries offered envelopes by format plumbing only

**Status.** Adopted 2026-10-03 as decision D6 of epic 8946. Batch B5 (gap G6)
implements it.

**Context.** The offered path bypassed the rejected-deploy buffer. It also
cleared retry and in-scope recovery selection. About 35 recovery-family tests
depend on that machinery.

**Decision.** The rejected-deploy buffer, retry, and in-scope recovery carry
the offered envelope as one more deploy format. Recovery algorithms and
decisions do not change, because they are general Casper behavior.

**Verification obligations.** The recovery-family tests pass on the activated
harness. Recovery outcomes match the current outcomes for the same scenarios.

**Cross-refs.** DR-55, DR-56, DR-66. Leaf `ofp-3-recovery-offered-format`.

## DR-70 — Block reporting replays offered blocks through the offered replay path

**Status.** Adopted 2026-10-03 as decision D7 of epic 8946. Batch B5 (gap G7)
implements it.

**Context.** `reporting_casper.rs` rejected offered blocks.

**Decision.** Reporting replays an offered block through the existing offered
replay path (`certify_offered_draft`,
`replay_compute_state_envelopes_with_policy`). It reports the native evidence
and the receipts. The historical `precharge_report_shape_spec` is commented
out with a reason.

**Verification obligations.** `block_report_api_test` and
`multi_parent_casper_reporting_spec` pass on offered blocks.

**Cross-refs.** DR-66. Leaf `ofp-5-offered-reporting`.

## DR-71 — The exploratory cost estimate is a native unfunded dry run

**Status.** Adopted 2026-10-03 as decision D8 of epic 8946. Batch B5
implements it.

**Context.** DR-64 makes the signed `phloLimit` the P1 safety margin. Clients
need a measured estimate to choose that limit. The legacy exploratory estimate
used the retired meter.

**Decision.** The user-facing exploratory estimate runs the native meter
without funding, bounded by host-work limits. It resets to the starting root
and publishes nothing. It returns the measured phlo (issue #53). Internal
exploratory reads of policy and token metadata keep the unmetered read path.

**Verification obligations.** The estimate equals the measured phlo of the
same term executed as a funded offer at the same root. The dry run leaves the
root unchanged.

**Cross-refs.** DR-64, DR-66. Leaf `ofp-5-exploratory-dry-run`.

## DR-72 — A failing offered candidate is quarantined, and the proposer tries the next one

**Status.** Implemented 2026-10-04 for register item I4 of epic 8946 (batch
B0).

**Context.** Three defects let one failing offer stop offered block
production:

1. The proposer took only the first offer. A non-retryable failure returned an
   error from `create_inner`.
2. One pending envelope in an inactive format returned an error from
   selection.
3. A deploy that drained its own funding wallet failed settlement. The
   proposer error then repeated on every attempt.

P1 §`sec:deploy-boundaries` states that a rejected deployment has no effect.
It must not block other deployments.

![Activity diagram of the quarantine-and-retry loop. The proposer reads pending envelopes in canonical order. An envelope in an inactive format is quarantined at selection with a status entry that has no root. The proposer selects the first eligible offer and keeps at most ordinary_cap minus 1 alternates. It checkpoints the attempt in a fresh offered runtime. If the block is created, it is published. If the result is OfferedCandidateRejected with the attempted identity, the proposer removes that envelope from pending storage and records a bounded status entry with the identity, the root R0, the block, and the reason, at most 4096 entries and 1024 reason bytes. It then tries the next alternate. With no alternate left, it continues with no user deploys when empty-block, slashing, or recovered-slash work exists, and otherwise returns NoNewDeploys. An infrastructure error, such as storage, history, RSpace storage, lock, stream, communication, or availability, keeps its own type and propagates.](diagrams/offered-candidate-quarantine-loop.svg)

(*Source: [`diagrams/offered-candidate-quarantine-loop.puml`](diagrams/offered-candidate-quarantine-loop.puml) — render with `plantuml -tsvg docs/casper/theory/diagrams/offered-candidate-quarantine-loop.puml`.*)

**Decision.**

1. `CasperError::OfferedCandidateRejected` is a typed error. It carries the
   deploy identity, the original root, and the reason.
2. The runtime maps a failure in the candidate pipeline to this error. The
   pipeline covers the envelope, funding, metering, settlement, producer
   self-replay, and publication authorization.
3. Infrastructure failures keep their own types and still propagate. They are
   key-value store, history, lock, stream, communication, and availability
   failures, plus the history, radix-tree, and key-value classes of RSpace
   errors.
4. Budget exhaustion inside RSpace (`HostWorkRejected`, `OutOfPhlogistons`) and
   every other RSpace error belong to the candidate. Otherwise a candidate
   that exhausts its budget inside an RSpace operation stalls the proposer.
5. The proposer quarantines the envelope. It removes the envelope from pending
   storage and records a bounded status entry bound to the root.
6. The status log holds at most 4096 entries and at most 1024 reason bytes for
   each entry. It evicts the oldest entry first. A new admission of the same
   identity clears its entry.
7. The proposer then tries the next canonical offered candidate. Selection
   keeps at most `ordinary_cap − 1` alternates. One proposal therefore executes
   at most `ordinary_cap` candidates, plus one attempt without user deploys.
8. When no alternate remains, the proposal continues exactly as with no user
   deploys. It returns `NoNewDeploys` unless empty blocks, slashing deploys, or
   recovered slashes require a block.
9. An envelope in an inactive format is quarantined at selection with no root,
   because the format decision does not depend on state.

**Algorithm (literate form).** The loop has two named chunks. The function
`next_offered_attempt` implements the second chunk, and both the checkpoint
loop and its property test call it.

```text
⟨offered proposal⟩ ≡
  attempt ← next_offered_attempt(window, other_work)
  loop
    result ← checkpoint(attempt)
    if result = OfferedCandidateRejected(attempt.id, R0, reason)
      quarantine(attempt.id, R0, reason)
      attempt ← next_offered_attempt(alternates, other_work)
      continue
    return result
```

The next attempt is the next canonical alternate. With none left, the proposal
falls back to the behavior for an empty user-deploy set.

```text
⟨next offered attempt⟩ ≡
  match alternates.next()
    candidate          → Candidate(candidate)
    none, other_work   → NoUserDeploys
    none               → NoNewDeploys
```

**Scope.** The change is local to the proposer and adds no consensus-visible
state. It does not change validation, voting, finality, fork choice, or
recovery. DR-65's held-capacity guard later turns the self-drain case into a
classified user failure.

**Verification.** The TLA+ model
`formal/tlaplus/cost_accounted_rho/OfferedCandidateQuarantine.tla` covers
several proposals. It models admission and readmission, the canonical window,
checkpoints, the bounded status log, and infrastructure errors. Each behavior
chooses its fundable set, cap, and log capacity in the initial state.

TLC checks ten invariants: `TypeOK`, `NoCandidateErrorEscapes`,
`RemovedOnlyByInclusionOrQuarantine`, `QuarantinedNeverIncluded`,
`AttemptsBounded`, `LogBounded`, `LogDistinct`, `PendingNotLogged`,
`LogOrderFollowsHistory`, and `FirstFundableIncluded`.

TLC also checks the action property `QuarantineAppendsAndEvictsOldest` and the
liveness property `ProposalsTerminate`. The base configuration has 9,288
distinct states. The large configuration has 1,563,676.

Five negative controls each fail their named invariant: escape, overremove,
unbounded, noevict, and skiphead. The local gate
`scripts/check-cost-accounted-rho-tla-invariants.sh` registers all seven
configurations.

Four property tests extract the invariants into Rust:

- `envelope_rejection_log_matches_bounded_fifo_reference` compares the log
  with a reference bounded FIFO (`LogBounded`, `LogDistinct`,
  `PendingNotLogged`, `QuarantineAppendsAndEvictsOldest`).
- `offered_attempt_loop_matches_quarantine_model` drives
  `next_offered_attempt` as the checkpoint loop does (`FirstFundableIncluded`,
  `RemovedOnlyByInclusionOrQuarantine`, `QuarantinedNeverIncluded`,
  `AttemptsBounded`).
- `offered_selection_window_is_canonical_bounded_and_height_independent`
  checks the window, the alternates bound, and inactive-format quarantine.
- `candidate_failures_become_typed_rejections_and_infrastructure_passes_through`
  checks the classification over fifteen error classes
  (`NoCandidateErrorEscapes`).

The integration test
`unfundable_offer_at_queue_head_is_quarantined_and_next_offer_is_included`
shows that an unfundable head offer does not stop the next offer. It also
checks the root-bound status entry.

**Cross-refs.** P1 `sec:deploy-boundaries`. DR-63, DR-65. Leaves
`ofp-1-baseline-liveness` and `ofp-1-baseline-verify-quarantine`. Leaf
`ofp-3-rejection-status` exposes the status entry through the API.

## DR-73 — Approved-genesis version adoption applies only to policy-carrying genesis

**Status.** Implemented 2026-10-04 for register item I7 of epic 8946 (batch
B0).

**Context.** `GenesisResourcePolicy::adopt` requires the running
`casper_version` to equal the protocol version of the approved genesis policy.

The uncommitted integration diff adopted the approved header version for every
approved genesis. That change also altered the general Casper path for legacy
genesis, which carries no resource policy.

**Decision.** `hash_set_casper` computes the running version with
`adopted_casper_version` before it loads the policy. The helper matches on the
policy. A genesis with a policy adopts the approved header version. A legacy
genesis keeps the local version, exactly as on `dev`. Cost-accounted chains
keep the DR-34 authority chain.

```text
⟨adopted casper version⟩ ≡
  match genesis_policy
    some(_) → approved_header_version
    none    → local_version
```

**Verification.** `GenesisVersionAdoption.v` proves five theorems without
axioms:

- `legacy_genesis_keeps_local_version`
- `policy_genesis_adopts_header_version`
- `adopted_policy_passes_adopt_check`
- `unadopted_mismatch_fails_adopt_check`
- `policy_genesis_version_is_node_independent`

Two property tests extract them. `version_adoption_follows_the_genesis_policy`
checks the helper. `adopted_version_passes_adopt_and_unadopted_mismatch_fails`
checks the adopted version against the real `GenesisResourcePolicy::adopt`.

**Cross-refs.** DR-34, DR-47, DR-66. Leaves `ofp-1-baseline-hygiene` and
`ofp-1-baseline-verify-activation`.

## DR-74 — Native evidence encodes each causal path as a delta from the previous path

**Status.** Implemented 2026-10-04 for register item I7 of epic 8946 (batch
B0).

**Terms.**

- A causal path is the sequence of segments that identifies where one native
  operation ran. A segment is a pair of 64-bit integers.
- $`\mathrm{lcp}(a, b)`$ is the length of the longest common prefix of the
  paths $`a`$ and $`b`$.
- An entry is the encoding of one path: the prefix length $`p`$, the suffix
  length $`s`$, and the $`s`$ suffix segments.

**Context.** The native budget recording and the native operation journal
carry one causal path for each recorded attempt.

A gateway funding trace had 1,029 attempts with 529,617 path segments. Those
segments exceeded the 1 MiB recording limit, although execution performed only
about 1,000 operations.

A measurement showed that 534,964 segments repeated a prefix of the preceding
path. Only 2,463 segments were new.

Index comparisons also charged the worst-case work of a complete path for
every comparison. A second reservation before each lookup charged the same
work again.

![Diagram of the delta encoding. A previous path (0,1) (0,0) (2,2) and a current path (0,1) (0,0) (3,1) (4,4) enter a longest-common-prefix computation, which gives p equal to 2. The encoder emits the canonical v2 entry with p equal to 2, s equal to 2, and the suffix (3,1) (4,4). The decoder rebuilds the first p segments of the previous path followed by the suffix, and it rejects an entry when s is positive, p is less than the previous length, and the suffix head equals the previous path at position p. A non-maximal entry with p equal to 1, s equal to 3, and suffix (0,0) (3,1) (4,4) is rejected, because its suffix head (0,0) equals the previous path at position 1. The accepted entry decodes to the current path. A note states that the first attempt uses the empty previous path, that retries continue the chain from the last attempt, and that v1 records still decode.](diagrams/native-path-delta-encoding.svg)

(*Source: [`diagrams/native-path-delta-encoding.puml`](diagrams/native-path-delta-encoding.puml) — render with `plantuml -tsvg docs/casper/theory/diagrams/native-path-delta-encoding.puml`.*)

**Decision.**

1. New recordings use the domains `f1r3node:native-budget-recording:v2` and
   `f1r3node:native-operation-journal:v2`.
2. Each path is encoded as an entry relative to the previous path. The first
   attempt uses the empty path as its previous path. Retries continue the
   chain from the last attempt.
3. The encoder always emits the maximal shared prefix,
   $`p = \mathrm{lcp}(\mathrm{current}, \mathrm{previous})`$. The decoder
   rejects a non-maximal prefix, so each recording has exactly one encoding.
4. The decoder still accepts the v1 domains for historical records.
5. A metered key comparison charges only the inspected prefix, in chunks of
   16 segments. It returns exactly the order of the key's `Ord`, because
   `NativeIndex` uses both comparators on one tree.
6. The duplicate reservation before each operation-index lookup is removed.
   The charge before each comparison still bounds the work.

**Algorithm (literate form).** The codec has two named chunks, one for each
direction.

The encoder compares the current path with the previous one in 16-segment
chunks and charges each chunk before it compares it. It then writes the
maximal prefix length and the remaining suffix.

```text
⟨encode one path⟩ ≡
  p ← lcp(path, previous)                 -- chunked scan, charged per chunk
  emit(p, length(path) − p, path[p ..])
```

The decoder bounds both lengths before it allocates. It rejects an entry whose
suffix could have shared one more segment with the previous path. That check
is what makes the encoding unique.

```text
⟨decode one path⟩ ≡
  require p ≤ min(length(previous), maximum)
  require s ≤ maximum − p
  path ← previous[0 .. p] ++ suffix
  require s = 0 or p = length(previous) or suffix[0] ≠ previous[p]
  return path
```

**Scope.** The encoding changes the bytes of native evidence for protocol 6,
which is not yet released. Producers and validators run the same codec, so
they agree.

The consensus host-work limits are unchanged. Profiling must find the root
cause of the remaining replay cost before any limit changes.

**Verification.** `NativePathDeltaCodec.v` proves five theorems without
axioms, generically over any segment type with decidable equality:

- `encode_one_decodes` and `decode_one_canonical`, for one path.
- `delta_codec_round_trip` and `delta_codec_canonical`, for a path chain.
- `delta_encoding_unique`, which states that each chain has exactly one
  accepted encoding.

`MeteredComparison.v` proves five theorems without axioms. The chunked scan
finds exactly the first difference (`chunk_scan_result`). It never charges
beyond the compared range (`chunk_scan_charge_bounded`). An equal prefix is
charged in full (`chunk_scan_equal_prefix_charges_all`).

For every chunk size of at least 1, the chunked path comparison equals the
derived lexicographic order (`metered_lex_compare_correct`). The chunked
operation-key comparison equals `Ord for OperationKey`
(`metered_operation_compare_correct`).

Property tests extract these theorems into Rust:

- `path_delta_chain_round_trips_and_rejects_every_noncanonical_prefix` checks
  round trips over random path chains. It also checks that every non-maximal
  prefix entry of every path is rejected.
- `operation_key_metered_order_equals_ord_with_bounded_charges` and
  `native_occurrence_metered_order_with_shared_session_and_bounded_charges`
  check the order equalities and the charge bounds across the chunk boundary.
- `operation_index_comparators_agree_for_arbitrary_key_sets` checks that
  metered and prepaid index lookups give the same answers.

The example tests `canonical_recording_and_journal_round_trip`,
`historical_recording_decodes_and_noncanonical_delta_is_rejected`, and
`historical_operation_journal_decodes` cover the v1 compatibility paths.

**Cross-refs.** DR-72. Leaves `ofp-2-path-codec-parity`,
`ofp-2-limit-envelope`, and `ofp-1-baseline-verify-codec`.

## DR-75 — RSpace candidates are ordered by source hash, and only ties are digested

**Status.** Implemented 2026-10-04 for register item I1 of epic 8946 (batch
B1).

**Terms.**

- A candidate is a stored datum or a stored waiting continuation that an
  RSpace operation can match.
- $`h(c)`$ is the source hash of candidate $`c`$: the precomputed
  `Produce.hash` of a datum or `Consume.hash` of a continuation. It has
  32 bytes.
- $`d(c)`$ is the digest of $`c`$: the Blake2b-256 hash of the bincode
  encoding of the complete candidate.
- $`i(c)`$ is the index of $`c`$: its position in the store read.
- A tie run is a maximal run of two or more candidates with equal source hash
  after the candidates are sorted by $`(h(c), i(c))`$.
- $`\lvert c \rvert`$ is the encoded size of $`c`$ in bytes.

**Context.** The `dev` branch shuffles the candidates of a channel at random
(`shuffle_with_index`). Cost accounting needs play and native replay to choose
the same candidate, so this branch replaced the shuffle with a deterministic
order.

That order sorted by $`(d(c), i(c))`$. Every produce and every consume
therefore serialized and hashed every candidate on the channel, including
complete continuation bodies. System contracts register 8 to 26 methods on one
channel (ListOps 26, PoS 21, SystemVault 8), so each method call hashed every
method body again.

The metered native replay used the same order. It inspected and hashed every
candidate for each operation and charged host work for all of it.

The order changes only which candidate an operation chooses among two or more
that satisfy the same pattern. It does not need a full digest unless two
candidates have the same source hash.

![Activity diagram of the canonical candidate order. The candidates of one channel are read and indexed by store position. A list with at most one candidate is returned as read, with no sort and no digest. Otherwise phase one sorts by the pair of source hash and index, and serializes no candidate. A scan over adjacent source hashes splits the sequence into maximal runs of equal source hash. For each run of length at least two, every member is digested with Blake2b-256 over its bincode encoding, and phase two sorts the run by the pair of digest and index. A single candidate stays in place with no digest. The result is the canonical order by source hash, digest, and index, which play, directive replay, and metered native replay all compute. A note explains that Consume.hash omits peeks and the original pattern order, that Produce.hash omits the non-determinism metadata, that an index-only or source-only key would depend on the insertion order, and that metered native replay reserves host work before each allocation, comparison, scan, and digest.](diagrams/canonical-candidate-order.svg)

(*Source: [`diagrams/canonical-candidate-order.puml`](diagrams/canonical-candidate-order.puml) — render with `plantuml -tsvg docs/casper/theory/diagrams/canonical-candidate-order.puml`.*)

**Decision.**

1. The canonical candidate order is lexicographic in
   $`(h(c), d(c), i(c))`$.
2. Phase one sorts by $`(h(c), i(c))`$. It reads only the precomputed source
   hashes.
3. Phase two sorts each tie run by $`(d(c), i(c))`$. A digest is computed only
   for a member of a tie run.
4. Play, directive replay, and metered native replay call one function,
   `candidate_order::canonical_order`. Play and directive replay use the
   unmetered work implementation. Metered native replay uses
   `MeteredOrder`, which reserves host work before each allocation,
   comparison, adjacent-hash scan, and digest.
5. The legacy $`(d(c), i(c))`$ function stays only as a test oracle. The
   legacy metered body is commented out with its reason.

The digest tie-break is necessary. `Consume.hash` omits the peeks and the
original pattern order of a continuation. `Produce.hash` omits
`is_deterministic`, `output_value`, and `failed`. Two candidates with equal
source hash can therefore differ. A key of the index alone, or of the source
hash and index alone, makes the choice depend on the store's insertion order.
The verification section cites proved counterexamples for both.

**Algorithm (literate form).** The order has three named chunks.

The first chunk indexes the store read and returns short lists at once. A
list of zero or one candidate needs no work.

```text
⟨canonical order⟩ ≡
  entries ← [(c_k, k) | k ∈ 0 .. n − 1]        -- preallocated; k = store index
  if n ≤ 1: return entries
  ⟨phase one⟩
  ⟨phase two⟩
  return entries
```

Phase one compares only 32-byte source hashes. It never serializes a
candidate.

```text
⟨phase one⟩ ≡
  sort entries by (h(c), i(c))
```

Phase two finds the runs with one adjacent comparison per pair. It digests
and sorts a run only when the run has at least two members.

```text
⟨phase two⟩ ≡
  for each maximal run r of entries with equal h:
    if |r| ≥ 2:
      for c ∈ r: d(c) ← Blake2b-256(bincode(c))
      sort r by (d(c), i(c))
```

**Work.** Let $`n`$ be the number of candidates and let $`\mathcal{T}`$ be
the set of tie runs. The legacy order and the canonical order perform this
work:

```math
W_{\mathrm{legacy}} = \sum_{k=1}^{n} \lvert c_k \rvert + O(n \log n),
\qquad
W_{\mathrm{canonical}} = O(n \log n) + \sum_{r \in \mathcal{T}} \Bigl( \sum_{c \in r} \lvert c \rvert + O(\lvert r \rvert \log \lvert r \rvert) \Bigr)
```

The comparisons in $`O(n \log n)`$ read 32-byte keys. With pairwise distinct
source hashes, $`\mathcal{T}`$ is empty and no candidate is serialized. When
every candidate shares one source hash, the canonical order costs what the
legacy order costs.

**Scope.** This change is cost-accounting work. The deterministic order exists
only on this branch, for native replay, and `dev` keeps its random shuffle.
The change touches only that order and its callers.

The choice changes only when two or more candidates satisfy the same pattern.
It applies to every play execution on this branch, including genesis and
system deploys, and to native replay for protocol 6, which is not yet
released. Ordinary replay follows the recorded log and does not sort, so
historical blocks replay unchanged. Event-identity hashes are unchanged.

A golden value that the order change moves is re-derived from evidence. The
procedure dumps the COMM events of the old and the new order and finds the
first difference. It accepts the new value only when that difference is a
choice among two or more matching candidates whose canonical order differs
from the legacy digest order. Any other difference is a defect.

**Verification.** `CandidateSourceOrder.v` proves 14 theorems without axioms:

- `canonical_sort_permutation` and `canonical_sort_sorted`: the canonical
  order is a sorted permutation of its input.
- `sorted_permutation_unique`: with distinct indices, at most one sorted
  arrangement exists.
- `two_phase_is_canonical` and `lazy_two_phase_is_canonical`: the two phases,
  with or without the tie-run shortcut, give exactly the canonical order.
- `phase_one_ignores_digests`: phase one reads no digest.
- `digests_only_for_ties` and `two_phase_digests_only_for_ties`: a candidate
  is digested only if its source hash occurs at least twice.
- `distinct_sources_need_no_digest` and
  `two_phase_distinct_sources_need_no_digest`: with pairwise distinct source
  hashes, no candidate is digested.
- `candidate_order_insertion_independent`: the payload order does not depend
  on the insertion order.
- `filter_commutes_with_canonical_sort`: a filter before or after the sort
  selects the same candidates in the same order.
- `index_only_key_is_insertion_dependent` and
  `source_only_key_is_insertion_dependent`: the proved counterexamples.

The payload $`(h(c), d(c))`$ stands for the candidate value. The claim that
the order is a function of the candidate multiset therefore relies on the
collision resistance of the digest, which the model does not verify.

`NativeCandidateOrder.tla` builds two stores that hold one multiset in
different insertion orders and consumes from both with arbitrary patterns. TLC
checks `TypeOK`, `TwoPhaseIsCanonical`, `InsertionIndependent`,
`PlayReplayAgree`, `DigestsOnlyForTies`, and `FilterCommutes`. The safe
configuration has 1,343 distinct states and the large configuration has
13,011. The three negative controls fail as expected:

| Control | Mutation | Violated invariant |
|---|---|---|
| `NativeCandidateOrderIndexOnlyUnsafe` | both nodes order by index only | `InsertionIndependent` |
| `NativeCandidateOrderSourceOnlyUnsafe` | both nodes order by source hash and index | `InsertionIndependent` |
| `NativeCandidateOrderReplayNoTieBreakUnsafe` | replay omits the digest tie-break | `PlayReplayAgree` |

Property tests extract these results into Rust:

- `canonical_order_equals_reference_with_generated_collisions` compares the
  order with a reference sort that digests every candidate. Its generator uses
  a three-value source domain, so tie runs are frequent.
- `canonical_order_is_insertion_independent`,
  `filter_then_sort_equals_sort_then_filter`, and
  `ties_digest_only_run_members` check insertion independence, filter
  commutation, and the exact digest count.
- `paid_order_matches_unmetered_canonical_order` checks that metered native
  replay gives the play order and reserves every allocation first.
- `twenty_six_distinct_continuations_compute_zero_digests` is the sentinel
  for a system-contract channel.
- `shared_order_inspects_only_tie_members_and_preserves_shared_ownership`
  checks that distinct-source candidates are never inspected.
- The cut tests `every_order_reservation_cut_rejects_without_unpaid_allocation`
  and `ordering_accepts_exact_credit_and_rejects_each_smaller_dimension` use a
  fixture with a tie run, so they also cover digest reservations.

Three directive-replay tests in `rspace++/tests/comm_observer_tests/native_directive.rs`
predicted the play order with the legacy digest:
`repeated_channel_bindings_follow_play_order_not_insertion_order`,
`store_consumes_greedy_guard_veto_without_exhaustive_search`, and
`store_produce_uses_the_same_guard_veto_loop_as_play`. Their fixtures now sort
by the canonical key, the source hash and then the digest. The tests keep
their purpose: replay follows the play order, and a guard veto of the first
greedy candidate stores the operation without an exhaustive search. Before
the change, the second test failed, because the canonical order put the
guard-approved candidate first.

**Cross-refs.** DR-72, DR-74. Leaves `ofp-2-perf-ordering` and
`ofp-2-cap-root-causes`.

## DR-76 — The metered COMM observation charges only what it reads

**Status.** Implemented 2026-10-04 for cap root cause C12 of epic 8946
(batch B1, phase A).

**Terms.**

- A metered run is a sequence of reservations and reads. A reservation adds
  units to the host-work budget. A read is work that no other step meters.
- A self-metered step reserves its own work before it performs it, for
  example the COMM identity hash and the authority merge.
- Prefix coverage is the metering rule: every prefix of a run reads at most
  the units that the same prefix has reserved.
- $`\lvert v \rvert`$ is the inspection charge of a value $`v`$: the units
  that a walk over its in-memory structure reserves.

**Context.** The native observer constructs one observation for every COMM.
It runs in the producer's execution and again in typed native replay.

The charged-unit probe measured the gateway funding block under the original
caps. The producer's VerificationBytes budget tripped at 268,435,431 of
268,435,456 units. The COMM observer used 35% of that budget at the trip
point. With the cap raised, the observer's inspection of the fired
continuation used 175 MB of the producer's 655 MB demand (26.7%).

The construction (`comm_metered`) inspected three inputs before its work:
the COMM, the fired continuation, and each matched datum. It then reads
these inputs as follows:

| Input | What the construction reads | Prepayment needed |
|---|---|---|
| COMM | only inside `cost_identity_metered`, which is self-metered, and the channel count | no |
| Fired continuation | only `cost_authority`, inside the self-metered authority functions | no |
| Each datum | the unmetered `message_bytes` walk | yes |

The continuation body of a system-contract method is large, and every COMM
that fires it inspected the whole body again.

**Decision.**

1. `comm_metered` no longer inspects the COMM or the fired continuation.
   Both inspections stay in the source, commented out with their reason.
2. The datum inspections stay, because they prepay the unmetered
   `message_bytes` walk.
3. The observation values (identity, authority, measurement) are unchanged.
   The play observer and typed native replay call the same function, so
   their charges stay equal.

**Algorithm (literate form).**

```text
⟨metered COMM observation⟩ ≡
  for each matched datum d: reserve |d|            -- prepays message_bytes
  identity ← cost_identity_metered(comm)          -- self-metered
  authority ← merge(continuation.cost_authority,  -- self-metered
                    datum authorities)
  measurement ← comm_charge(comm, data)           -- reads each datum once
  return (identity, authority, measurement)
```

**Charge.** Let $`c`$ be the COMM, $`k`$ the fired continuation with body
$`b`$, guard $`g`$ and authority $`a`$, and $`d_1, \ldots, d_m`$ the matched
data. The legacy and the C12 charges differ by exactly the two removed
inspections:

```math
R_{\mathrm{legacy}} = R_{\mathrm{C12}} + \lvert c \rvert + \lvert k \rvert,
\qquad
\lvert k \rvert = \lvert b \rvert + \lvert g \rvert + \lvert a \rvert .
```

The C12 charge $`R_{\mathrm{C12}}`$ does not depend on $`b`$ or $`g`$.

**Scope.** This change is cost-accounting work. `observation_construction.rs`
exists only on this branch. The change alters host-work charges of protocol 6,
which is not yet released. It changes no evidence encoding and no
observation value.

**Verification.** `ObservationReadCoverage.v` proves six theorems without
axioms:

- `trace_covered` and `legacy_trace_covered`: both runs satisfy prefix
  coverage.
- `trace_reads_equal_legacy_reads`: both runs perform the same reads.
- `legacy_excess`: the legacy run reserves exactly
  $`\lvert c \rvert + \lvert k \rvert`$ more.
- `charge_independent_of_body_and_guard`: the C12 run does not depend on the
  body or the guard.
- `legacy_charge_depends_on_unread_body`: a proved counterexample.

Property tests in `native_runtime/tests/observation_construction.rs` extract
these results:

- `comm_observation_charge_is_independent_of_continuation_body`;
- `comm_observation_values_are_unchanged_for_every_body`;
- `comm_observation_accepts_exact_credit_and_rejects_each_smaller_dimension`;
- `legacy_comm_charge_grew_with_the_unread_body`.

The `rholang` suite passes 2,830 of 2,830 tests in release. With the original
caps, the four offered-funding API suites pass. The gateway API test still
fails at the gateway call, as before. That failure is register item G3.

**Cross-refs.** DR-72, DR-75. Leaves `ofp-2-cap-c12-comm-observer-reads` and
`ofp-2-cap-root-causes`.

## DR-77 — Tree growth charges the increment of the tree backing

**Status.** Implemented 2026-10-04 for cap root cause C13 of epic 8946
(batch B1, phase A).

**Terms.**

- $`B(n)`$ is the backing bound of a B-tree with $`n`$ entries:
  $`B(n) = N(n) \cdot s`$, where $`N(n) = 1 + \lfloor (n-1)/5 \rfloor`$ for
  $`n \geq 1`$ and $`N(0) = 0`$ bounds the node count, and $`s`$ is the
  byte size of one node (`tree_backing` in `collection_backing.rs`).
- The growth charge of a batch of $`a`$ entries added to a tree of $`n`$
  entries is $`B(n + a) - B(n)`$.
- Host-work usage accumulates: the budget never releases a reservation.

**Context.** The native runtime budget keeps several B-trees that grow
during execution:
- the introduction-authority registry;
- the introductions registry;
- the authority frontier and events;
- the pending stack transfers.

Metered native replay keeps one more: the produce-counter map, which lives
for the whole replay.

Before each insert, the budget reserved backing for the tree.

The probe of the gateway funding block showed these owned-backing
reservations as 51% of the producer's SearchStateBytes (68 MB of 133.5 MB).
That budget stood at 99.5% of its 128 MiB cap.

The helpers `reserve_registry_insert`, `reserve_tree_birth` and
`reserve_tree_batch` reserved $`B(n + 1)`$ or $`B(n + a)`$. That is the
backing of the whole tree at its new size, on every insert. Because usage
accumulates, $`n`$ single inserts reserved:

```math
R_{\mathrm{legacy}}(n) = \sum_{k=1}^{n} B(k) \;\geq\; \frac{n (n + 1)}{10}\, s ,
```

while the tree never holds more than $`B(n) \leq (1 + n/5)\, s`$. For 1,000
inserts the legacy charge reserved 100,500 nodes for a tree that needs 200.

The same file already charged the increment for persistent regions
(`extend_persistent_regions`).

**Decision.**

1. A new helper `tree_growth` in `shared::rust::collection_backing` returns
   the increment $`B(n + a) - B(n)`$ for bytes and for operations.
2. `reserve_registry_insert`, `reserve_tree_birth` and `reserve_tree_batch`
   (runtime budget) and `prepare_metered_produce_counter` (metered native
   replay) reserve that increment. The legacy lines stay in the source,
   commented out with their reason.
3. Every call site passes the current tree size before the insert, so a batch
   that builds a fresh tree ($`n = 0`$) keeps its charge $`B(a)`$.

**Algorithm (literate form).**

```text
⟨tree growth⟩ ≡
  require n + a does not overflow
  (operations', bytes') ← tree_backing(n + a)
  (operations, bytes)  ← tree_backing(n)
  return (operations' − operations, bytes' − bytes)   -- B is monotone
```

**Soundness.** `SearchStateBytes` counts canonical logical bytes retained for
a search state, and it does not use allocator metadata
([host-work budget](host-work-budget.md)). Two cases cover all trees:

- An insert-only tree (the two registries and the produce-counter map): the
  cumulative charge after $`k`$ single inserts telescopes to $`B(k)`$. By
  `checkpoint_node_count`, $`N(k)`$ bounds every node of a B-tree with $`k`$
  entries whose non-root nodes hold at least five entries, which is the
  occupancy invariant of the standard library's `BTreeMap`. Inserts never free
  nodes, so every node that the inserts allocated is covered at every prefix.
- A tree that also removes entries (the events, the pending stack transfers
  and their event identities) or clears itself (the frontier and the stack
  births): an insert charges the growth from the current size, and a removal
  refunds nothing. At every prefix the cumulative charge is at least
  $`B`$ of the largest size reached, so it covers the retained tree.

**Scope.** This change is cost-accounting work. The helpers exist only on this
branch. The change alters host-work charges of protocol 6, which is not yet
released. It changes no evidence encoding and no observable value.

**Verification.** `IncrementalTreeBacking.v` proves eight results without
axioms:

- `incremental_charges_telescope`: for every monotone projection, the charges
  of any batch sequence add up to the final value minus the initial value.
- `incremental_bytes_from_empty` and `incremental_operations_from_empty`.
- `incremental_prefix_covers_nodes`: the prefix coverage stated above.
- `legacy_cumulative_quadratic`: $`n (n + 1) \leq 10 \sum_{k \leq n} N(k)`$.
- `incremental_nodes_linear`: $`N(n) \leq 1 + n/5`$.
- `charges_cover_retained_with_removals`: for any sequence of inserts and
  removals, every prefix has charged at least $`B`$ of the current size.
- `legacy_example`: 1,050 legacy nodes against 20 for 100 inserts.

Property tests in the `tree_growth_tests` module of `accounting/mod.rs`
extract these results:

- `tree_growth_charges_telescope_to_the_final_backing` (256 random batch
  sequences);
- `incremental_reservation_covers_std_btree_allocations`. It inserts up to
  600 random keys into a standard `BTreeMap`. With the test binary's measuring
  allocator, it checks that the allocated bytes never exceed the cumulative
  reservation;
- `growth_charges_cover_retained_backing_with_removals` (256 random insert
  and removal sequences on a standard `BTreeMap`);
- `legacy_whole_tree_charge_per_insert_is_quadratic` (1,000 inserts).

**Cross-refs.** DR-76. Leaves `ofp-2-cap-c13-incremental-tree-backing` and
`ofp-2-cap-root-causes`.

## DR-78 — Produce-counter lookups charge the B-tree search bound

**Status.** Implemented 2026-10-04 for cap root cause C3 of epic 8946
(batch B1, phase A).

**Terms.**

- A *B-tree with the node bounds of the standard `BTreeMap`* has these
  properties:
  - every node holds at most 11 keys.
  - every node except the root holds at least 5 keys.
  - a nonempty root holds at least 1 key.
  - an internal node with $`k`$ keys has $`k + 1`$ children.
  - all leaves have the same depth.
- The *height* of a tree is its number of levels. A single leaf has
  height 1.
- $`h(n)`$ is the *height bound* of a map with $`n`$ entries:
  $`h(0) = 0`$, and for $`n \geq 1`$, $`h(n)`$ is the largest $`h`$ with
  $`2 \cdot 6^{h-1} - 1 \leq n`$. Equivalently,
  $`h(n) = \lfloor \log_6 \frac{n+1}{2} \rfloor + 1`$
  (`tree_height_bound`).
- $`c(n) = 11 \cdot h(n)`$ is the *search bound* (`tree_search_bound`).
- A *lookup* is one search of the map for one key. `get`, `contains_key` and
  the position search of `insert` are lookups.
- A comparison of two `Produce` keys reads only their 32-byte `hash` fields
  (`impl Ord for Produce` in `rspace++/src/rspace/trace/event.rs`).

**Context.** Metered native replay keeps the produce-counter map
(`ReplayRSpace::produce_counter`, a `BTreeMap<Produce, i32>`) for the whole
replay. Each COMM also builds a local `times_repeated` map. The legacy charge
of one lookup walked every key of the map only to charge it:

| Site | Legacy charge for a map with $`n`$ entries |
| --- | --- |
| `prepare_metered_produce_counter` (read and insert) | $`64 (n + 1) + 2n`$ operations and $`64 (n + 1) + 64n`$ bytes |
| `metered_produce_count` (read) | $`32 (n + 1) + n`$ operations and $`32 (n + 1) + 32n`$ bytes |
| `metered_comm` (`times_repeated` lookup, then insert) | the `metered_produce_count` charge for each lookup |

One lookup thus charged $`\Theta(n)`$ work. A replay that produces on $`n`$
distinct sources charged $`\Theta(n^2)`$ work in total. A `BTreeMap` search
makes $`O(\log n)`$ comparisons.

**Decision.**

1. `shared::rust::collection_backing` gets two functions:
   `tree_height_bound(n)` returns $`h(n)`$, and `tree_search_bound(n)`
   returns $`c(n)`$ (saturating).
2. A new helper `reserve_ordered_lookup(entries, hash_bytes, meter)` in
   `metered.rs` reserves $`c(n)`$ operations,
   $`2 \cdot \mathit{hash\_bytes} \cdot c(n)`$ scanned bytes, and no
   backing. That is one operation and two hash reads for each comparison.
   The metered comparator (`MeteredOrder::scan` and `sort`) uses the same
   convention.
3. Five lookups in three functions call the helper with the current map
   size:
   - two in `prepare_metered_produce_counter`: the read, and the insert that
     `publish` performs.
   - one in `metered_produce_count`.
   - two in `metered_comm`: the `times_repeated` lookup, and the insert of a
     new source.
4. The legacy lines stay in the source, commented out with their reason.

**Algorithm (literate form).** $`p`$ holds $`6^h`$, so $`2p - 1`$ is the
smallest size of a tree of height $`h + 1`$.

```text
⟨height bound⟩(n) ≡
  if n = 0 then return 0
  h ← 1; p ← 6                         -- invariant: p = 6^h
  loop
    m ← 2·p − 1                        -- least size of a tree of height h + 1
    if m overflows or m > n then return h
    h ← h + 1
    if 6·p overflows then return h     -- then 2·6·p − 1 > n as well
    p ← 6·p

⟨ordered lookup charge⟩(n, hash_bytes) ≡
  c ← 11 · ⟨height bound⟩(n)           -- saturating
  reserve(operations = c, scanned = 2 · hash_bytes · c, backing = 0)
```

![Diagram of the smallest standard BTreeMap of height 3. The root holds 1 key and 2 children. Each of the two internal nodes holds 5 keys and 6 children. Each of the 12 leaves holds 5 keys, so the tree holds 71 entries, which is 2 times 6 squared minus 1. A highlighted search path goes from the root through one internal node to one leaf, with at most 11 comparisons in each node. A first panel states the least size of a tree of height h and the height bound h of n. A second panel states the charge of one lookup: c of n equals 11 times h of n comparisons, reserved as c of n operations and 64 times c of n scanned bytes with no backing, so 2,000 entries give height 4 and 44 comparisons. A third panel states the legacy charge: one step for each entry, 66,032 operations for one lookup at 2,000 entries, and a quadratic total for a replay.](diagrams/btree-search-bound.svg)

(*Source: [`diagrams/btree-search-bound.puml`](diagrams/btree-search-bound.puml) — render with `plantuml -tsvg docs/casper/theory/diagrams/btree-search-bound.puml`.*)

**Soundness.** The argument has four steps.

1. *Size lower bound.* By induction on the height, a non-root subtree of
   height $`h \geq 1`$ holds at least $`6^h - 1`$ entries:
   - a leaf holds at least $`5 = 6 - 1`$.
   - an internal node holds at least 5 keys and 6 subtrees, so it holds at
     least $`5 + 6 (6^{h-1} - 1) = 6^h - 1`$.

   A root of height $`h + 1 \geq 2`$ holds at least 1 key and 2 subtrees, so
   it holds at least $`1 + 2 (6^h - 1) = 2 \cdot 6^h - 1`$ entries. A root of
   height 1 holds at least $`1 = 2 \cdot 6^0 - 1`$ entry.
2. *Height bound.* By step 1, a map with $`n`$ entries has height at most
   $`h(n)`$.
3. *Search bound.* A search compares the target with the keys of one node
   from the left. It stops at the first key that is not smaller than the
   target, and then it stops or descends into one child. So each level costs
   at most 11 comparisons, and one search costs at most
   $`11 \cdot \mathrm{height} \leq c(n)`$.
4. *Charged size.* The charge of each lookup uses the map size at the time
   of the search:
   - `prepare_metered_produce_counter` holds the produce-counter lock from
     the charge until `publish` inserts.
   - `metered_produce_count` holds the lock for its read.
   - `times_repeated` is local to one COMM.

An insert performs one search and then splits nodes without key
comparisons. DR-77 charges the backing of the insert.

**Determinism.** The charge uses the live map size, as the legacy charge
did. The [host-work budget](host-work-budget.md) requires that local
scheduling cannot change the cumulative use. Under native execution,
`insert_authority` adds the common footprint key `[2, 0]` to every intent
(`deterministic_reduction.rs`). Each frontier is thus one conflict
component, and its operations run one at a time in canonical causal order.
The produce-counter size at each lookup is therefore a function of the
canonical execution. The `times_repeated` map is local to one COMM.

**Limits.**

- For a map with at most 21 entries, the scanned-byte charge can be larger
  than the legacy charge: by at most 672 bytes for one lookup, and by at most
  1,344 bytes for one preparation. The cause is that the bound charges 11
  comparisons for each level, also when a node holds fewer keys. The
  operation charge is always smaller than the legacy charge.
- The model does not verify the Rust `BTreeMap` implementation. The property
  tests count the comparisons of the real `BTreeMap`.

**Out of scope.** The runtime budget's `reserve_registry_lookup` in
`accounting/mod.rs` keeps its linear charge.

- Several of its call sites pass the sum of the sizes of two or three maps
  and pay for one search in each map. One example is the stack-birth conflict
  check.
- Some call sites pay in advance for the searches of the stack-transfer
  commit. The commit runs after an `await` and can interleave with other
  commits.

A linear charge on the sum covers those searches. One B-tree bound on the sum
does not: two searches in two maps of 1,000 entries can make 88 comparisons,
but $`c(2001) = 44`$. A sound logarithmic charge for those sites needs one
charge for each searched map and a size bound that holds at the commit. The
approved C3 item does not include that change. In the gateway-block probe,
`reserve_registry_lookup` was about 2% of the proposer's
`VerificationBytes`. Update: the approved C13 item includes that change, and DR-80
implements it with one charge for each searched map.

**Scope.** This change is cost-accounting work. The metered native replay and
its produce-counter charges exist only on this branch. The change alters
host-work charges of protocol 6, which is not yet released. It changes no
evidence encoding and no observable value. The produce counts, the COMM
events, and the candidate order are unchanged.

**Verification.** `OrderedLookupBound.v` proves five results without axioms:

- `btree_size_lower_bound`: step 1 above, for subtrees and for roots.
- `btree_search_comparisons`: a left-to-right scan search in a well-formed
  tree of height $`h`$ makes at most $`11h`$ comparisons.
- `btree_height_bound`: a well-formed root of height $`h`$ with $`n`$ entries
  has $`h \leq h(n)`$, where the model's `height_bound` computes $`h(n)`$ as
  the Rust loop does.
- `search_within_size_bound`: one search makes at most $`11 \cdot h(n)`$
  comparisons.
- `linear_charge_example`: $`h(2000) = 4`$.

Property tests extract these results:

- `std_btree_get_comparisons_within_bound` in
  `shared::rust::collection_backing` (256 cases). Each case builds a map of
  up to $`2^{16}`$ entries. It uses one of five construction paths:
  - single inserts in random order.
  - single inserts in ascending order.
  - single inserts in descending order.
  - the bulk build of `collect`.
  - random inserts followed by removals.

  Each case probes present and absent keys. A counting key type counts the
  comparisons of the real `BTreeMap`.
- `std_btree_get_comparisons_within_bound_at_the_largest_size`: $`2^{16}`$
  ascending inserts, with every key probed.
- `tree_height_bound_changes_at_the_minimum_root_sizes` and
  `linear_lookup_charge_exceeds_the_bound` ($`h(2000) = 4`$,
  $`c(2000) = 44`$).
- `counter_charge_is_logarithmic` in `metered/tests.rs`. It checks map sizes
  from 0 to 2,000, on both sides of each height step. At each size, the count
  charge equals the bound. The preparation charge equals two bounds plus the
  copy and the tree growth.

The existing reservation-cut tests of the produce counter still pass. They
show that every reservation comes before the state changes.

**Cross-refs.** DR-77. Leaves `ofp-2-cap-c3-logarithmic-lookup-charge` and
`ofp-2-cap-root-causes`.

## DR-79 — Replay authority tree visits charge the B-tree bound of the live map size

**Status.** Implemented 2026-10-04 for cap root cause C14 of epic 8946
(batch B1, phase A).

**Terms.**

- The *replay authority binding* (`ReplayAuthorityBinding`) re-checks the
  cost accounting of an offered-funded deploy during native replay. For each
  COMM row, it records an authority event and updates two ledgers for each
  signature lane. A denied COMM goes into the frontier.
- A *publication* is one `prepare` followed by `publish`, or by an abort
  (`Drop`).
- A *visit* is one B-tree search that `tree_update` charges.
- $`h(n)`$ is the height bound and $`c(n) = 11 \cdot h(n)`$ is the search
  bound of a map with $`n`$ entries (DR-78).
- The maps are $`S`$ (pending stack identities), $`P`$ (pending replay
  events), $`E`$ (events), $`F`$ (frontier), $`R`$ (reserved), $`Z`$
  (realized) and $`A`$ (allocation). A debit has $`k`$ lanes.

**Context.** `tree_update` charged every visit as a search in a tree of 65
levels (`usize::BITS + 1`). That is 715 comparisons and 66 nodes for each
visit, about 63 KB of `VerificationBytes`. One granted COMM with one lane
makes 22 visits.

The probe of the gateway funding block showed these charges as 10.3% of each
replay budget's `VerificationBytes` (659 MB of 6.43 GB). They were also 5.6%
of its `SearchStateBytes`. The maps hold dozens of entries, so their height is
at most 3. The binding and its charges exist only on this branch.

**Decision.**

1. `tree_update` takes the size $`n`$ of the maps that its visits search. A
   visit charges $`c(n)`$ comparisons, the key bytes of each comparison, and
   $`h(n) + 1`$ nodes. An insert also charges $`h(n) + 1`$ nodes of
   `SearchStateBytes`. The 65-level lines stay in the source, commented out
   with their reason.
2. Each charge group gets the size of the maps that it searches:

   | Group | Visits | Searches | Charged size |
   | --- | --- | --- | --- |
   | Identity lookups | 3 | $`S`$, $`P`$, $`E`$ at prepare | $`\max(\lvert S\rvert, \lvert P\rvert, \lvert E\rvert)`$ |
   | Dominance scan (enforced allocation only) | $`\lvert R\rvert`$ | $`A`$ at prepare | $`\lvert A\rvert`$ |
   | Ledger, for each lane | 2 + 9 + 4 | $`R`$ and $`Z`$ (and $`A`$ when enforced) at prepare, publish and abort | $`\max(\lvert R\rvert, \lvert Z\rvert) + k`$, at least $`\lvert A\rvert`$ when enforced |
   | Event maps | 3 + 1, two inserts | $`P`$ at prepare, publish and abort, $`E`$ at publish | $`\max(\lvert P\rvert, \lvert E\rvert) + 1`$ |
   | Frontier (denied COMM) | 1, one insert | $`F`$ at publish | $`\lvert F\rvert + 1`$ |

3. The visit counts and the insert flags do not change.
4. The approved test "replay charges equal for play and replay" cannot hold,
   because play never uses the binding. Play charges authority work through
   `reserve_authority_identity`. Two tests replace it:
   - replays of one trace on a current-thread runtime and on multi-thread
     runtimes report the same use.
   - play and replay end with equal authority state.

**Algorithm (literate form).**

```text
⟨tree update⟩(insert, visits, n) ≡
  h ← ⟨height bound⟩(n)                      -- DR-78
  c ← 11 · h · visits
  charge VerificationOperations c
  charge VerificationBytes c · |K| + node · (h + 1) · visits
  if insert then charge SearchStateBytes node · (h + 1)

⟨publication charge⟩(state, row) ≡
  ⟨tree update⟩(false, 3, max(|S|, |P|, |E|))
  if the allocation is enforced then
    repeat |R| times ⟨tree update⟩(false, 1, |A|)
  if the row is a granted COMM with a debit of k lanes then
    L ← max(|R|, |Z|) + k
    if the allocation is enforced then L ← max(L, |A|)
    for each lane:
      ⟨tree update⟩(false, 2, L)
      ⟨tree update⟩(lane not in R, 9, L)
      ⟨tree update⟩(lane not in Z, 4, L)
    N ← max(|P|, |E|) + 1
    ⟨tree update⟩(true, 3, N)
    ⟨tree update⟩(true, 1, N)
  if the row is a denied COMM then
    ⟨tree update⟩(true, 1, |F| + 1)
```

![Activity diagram of one replay authority publication. The native session driver runs one intent at a time, because every intent carries the footprint key [2, 0]. Prepare takes the authority lock. The three identity lookups are charged at the largest of the sizes of the pending stack identities, the pending replay events and the events. When the allocation is enforced, the dominance scan is charged as one visit for each reserved entry at the allocation size. For a granted COMM with k lanes, the ledger visits are charged at the larger reserved or realized size plus k, and the event-map visits and two inserts at the larger pending or events size plus one. Then prepare validates, adds the debit to the reserved ledger and inserts the pending event. For a denied COMM, the frontier insert is charged at the frontier size plus one. A note states that every charge comes before the first mutation, and that no await, no other intent and no participant runs between prepare and publish. Publish removes the pending event, adds the debit to the realized ledger and inserts the event or frontier entry. An abort removes the pending event and subtracts the debit from the reserved ledger. Each search thus runs on a map no larger than its charged size, so it makes at most 11 times h of n comparisons and reads at most h of n nodes. A legend defines the map letters and names the two negative controls.](diagrams/replay-authority-charge-sizes.svg)

(*Source: [`diagrams/replay-authority-charge-sizes.puml`](diagrams/replay-authority-charge-sizes.puml) — render with `plantuml -tsvg docs/casper/theory/diagrams/replay-authority-charge-sizes.puml`.*)

**Soundness.** The argument has five steps.

1. *Search bound.* By `search_within_bound`, a search in a map with at most
   $`n`$ entries makes at most $`c(n)`$ comparisons and reads at most
   $`h(n)`$ nodes.
2. *Charged sizes.* Each charged size bounds its maps when the searches run:
   - The identity lookups and the dominance scan run at once, under the
     lock, at the live sizes.
   - Between prepare and publish or abort, only this publication changes
     the ledgers. The debit adds at most $`k`$ keys to $`R`$ or to $`Z`$, so
     $`\max(\lvert R\rvert, \lvert Z\rvert) + k`$ bounds both maps at every
     ledger search. Under enforcement, the allocation lookups search $`A`$.
   - $`P`$ holds the pending event until publish or abort, and $`E`$ grows
     by one entry at publish. $`F`$ grows by one entry at publish.
3. *Removals.* A removal can read one sibling on each level below the root,
   in addition to its search path. For each lane, the ledger groups charge
   15 visits for at most 10 searches. The event-map group charges 4 visits
   for 3 operations. Each spare visit is sized by the map that removes.
4. *Insert allocation.* An insert allocates at most $`h + 1`$ nodes: one on
   each level that splits, and a new root. `tree_backing` sizes a node larger
   than an internal node of the standard map.
5. *Determinism.* The live sizes are a function of the canonical execution:
   - Native execution adds the footprint key `[2, 0]` to every intent, so
     each frontier is one conflict component. Its intents run one at a time
     in canonical causal order.
   - No `.await` separates `ticket.prepare` from `completion.publish` in the
     native replay operations.
   - Participants, and their cost-stack transfers, do not run while the
     driver runs a frontier.

**Precondition.** Step 2 requires that no other publication and no
cost-stack transfer change the maps between a prepare and its publish. The
binding API allows several publications in flight, and two unit tests use
that. Under such use, a later publication can grow $`E`$ or $`Z`$ before an
earlier one publishes. The deferred searches of the earlier publication can
then exceed their charged sizes. Production never does this, by step 5. This
record documents the precondition, and the binding does not check it at run
time.

**Scope.** This change is cost-accounting work. The binding and its charges
exist only on this branch. The change alters host-work charges of protocol
6, which is not yet released. It changes no evidence encoding and no
observable value.

**Out of scope.** Four findings stay recorded in pgmcp and are not changed:

- the full dominance scan of `validate_add` depends on interleaving under
  concurrent publications.
- play-side cost-stack transfer charges use live sizes while parallel
  participants run.
- cost-stack commit searches are paid at prepare-time sizes.
- `SearchStateBytes` charges use `size_of`.

**Verification.** `OrderedLookupBound.v` adds five results without axioms:

- `height_bound_monotone`: a larger size never has a smaller height bound.
- `search_visits_le_height`: a search reads at most $`h`$ nodes.
- `search_within_bound`: step 1 above.
- `pre_operation_size_undercharges`: a negative control. A deferred search
  charged at the size before the operation charges less than the search.
- `summed_size_undercharges`: a negative control. Two 77-entry trees take 44
  comparisons, but one bound on the summed size allows 33.

Tests:

- `authority_update_charge_covers_std_btree_work` (128 cases). Each case
  builds a random authority state, with up to 700 events and frontier
  entries, 120 ledger entries and 8 lanes. The case is enforced or not,
  granted or denied, and publishes or aborts. A counting key type and the test
  allocator measure the real map work, which uses the real `sparse_ledger`
  functions. The charge covers the comparisons, 64 bytes for each comparison,
  and the allocated bytes.
- `deferred_searches_are_charged_after_the_operation`: the exact operation
  charge, at sizes on both sides of the height steps.
- `retry_comparison_exhaustion_preserves_published_authority`: updated to
  the live-size lookup charge.
- `native_replay_authority_charge_is_schedule_independent`. It replays a
  deploy with parallel branches and 17 COMMs on a current-thread runtime and
  on runtimes with 2, 4 and 8 workers. The use is identical in all 16
  dimensions.
- `native_replay_authority_state_matches_recorded_execution` (existing):
  play and replay end with equal events, realized ledger and frontier.

**Cross-refs.** DR-78. Leaves `ofp-2-cap-c14-authority-tree-bound` and
`ofp-2-cap-root-causes`.

## DR-80 — Registry lookups charge the B-tree search bound of each searched map

**Status.** Implemented 2026-10-04. It completes cap root cause C13 of epic
8946 (batch B1, phase A). DR-77 implemented the growth part.

**Context.** The approved C13 item also requires that the runtime budget's
registry lookups use the B-tree search bound of DR-78.
`reserve_registry_lookup` charged one comparison for each entry. Some call
sites charged one summed size for searches in two or three maps. After
DR-77, an insert charges only the growth of its tree, not its own search.

**Decision.**

1. `reserve_registry_lookup` charges $`c(n) = 11 \cdot h(n)`$ comparisons for
   a registry with $`n`$ entries, and the key bytes of each comparison. The
   linear lines stay in the source, commented out with their reason.
2. Each charge pays for one search in one map, at a size that holds when the
   search runs:
   - The stack-birth conflict check charges `stack_births` and
     `pending_stack_transfers` separately.
   - The event-identity check charges `events`, `pending_stack_event_ids` and
     `pending_replay_events` separately for each event.
   - `reserve_authority_identity` charges its three identity maps
     separately. It also charges the search of the event insert and of the
     frontier insert.
   - The inserts into the introduction registry and into the persistent
     introductions charge their own search.
   - The commit or the abort of a stack transfer charges the removal from
     the pending map, at its size with this transfer included.
3. The charges for searches at commit keep the existing sizes: the size now
   plus the entries that the pending transfers can add.

**Soundness.** By `OrderedLookupBound.search_within_bound`, a search in a map
with at most $`n`$ entries makes at most $`c(n)`$ comparisons. Each charge
now uses the size of the one map that it searches.
`OrderedLookupBound.summed_size_undercharges` shows why one logarithmic bound
on a summed size is not sound.

**Limits.** The searches at commit run after `produce(...).await`, and
parallel participants can commit other transfers before them. The legacy
linear charge had the same dependence on the size at commit. This dependence
stays recorded in pgmcp, and this change does not alter it.

**Verification.**

- `registry_lookup_charges_the_search_bound`: the exact charge, at sizes on
  both sides of the height steps.
- `std_btree_get_comparisons_within_bound` (DR-78): the bound holds for the
  real `BTreeMap`.

**Cross-refs.** DR-77, DR-78, DR-79. Leaf
`ofp-2-cap-c13-incremental-tree-backing`.

## DR-81 — Native continuation reads share the cached payloads

**Status.** Implemented 2026-10-04 for cap root cause C1 of epic 8946
(batch B1, phase B).

**Context.** The native hot store caches the continuations of a channel
group once, each behind a shared pointer (`Arc`). The native read
`native_continuations` copied every cached continuation on every read and
charged each copy. The consume path also read the continuations once only to
fill the cache, and then dropped the copies.

The phase A probe of the gateway funding block attributed about 66% of each
validator replay's `VerificationBytes` to the native history reads
(`history_reserve` in `native_session/history.rs`).

**Decision.**

1. A new reader `native_continuation_views` returns the shared pointers:
   - a warm read clones only the pointers, at one operation and one pointer
     size for each continuation;
   - a cold read moves each decoded continuation into its pointer, without
     a copy, and prepays its release once, when it enters the cache.
2. The candidate reader of metered native replay uses the views. It copies
   only the continuation that it selects, with `reserve_copy_and_cleanup`.
3. The consume path prefetches with `prefetch_continuations`, which fills the
   cache and copies no continuation.
4. The owned reader `native_continuations` stays for the public
   `get_continuations` API and serves as the test oracle. The replaced lines
   stay in the source, commented out with their reason.

**Soundness.** A view resolves to the same payload as a copy, so the
selection is unchanged. Every payload's release is prepaid when the payload
enters the cache, and a pointer clone allocates no payload. The last pointer
drop therefore releases a prepaid payload.

**Scope.** This change is cost-accounting work. Native replay and its hot
store paths exist only on this branch. The change alters host-work charges of
protocol 6, which is not yet released. It changes no observable value: the
selected candidate and the COMM are the same.

**Verification.** `NativeSharedReads.v` proves without axioms:

- `shared_selection_equals_deep_selection`;
- `prefetch_then_read_equals_read`;
- `cleanup_prepaid_preserved` and `every_release_was_prepaid`;
- a negative control: a cold fill without prepayment leads to an unpaid
  release.

`NativeSharedReadCleanup.tla` checks `LivePrepaid`, `EntriesPrepaid`,
`EveryReleasePrepaid` and `WarmReadsAllocateNothing`. Two unsafe controls
violate their invariants:

| Control | Mutation | Violated invariant |
| --- | --- | --- |
| `NativeSharedReadCleanupFillWithoutPrepayUnsafe` | a cold read skips the payload prepayment | `LivePrepaid` |
| `NativeSharedReadCleanupStoreWithoutPrepayUnsafe` | a cold read skips the cache-entry prepayment | `EntriesPrepaid` |

Tests in `hot_store/native/tests.rs`:

- `shared_reads_select_like_deep_reads`: views equal owned reads on a cold
  and on a warm cache.
- `shared_read_allocates_no_payload`: the warm view read allocates the same
  bytes when the payload grows 16 times.
- `prefetch_leaves_state_and_selection_unchanged`.
- `every_view_read_cut_rejects_without_filling_the_cache`.

**Cross-refs.** DR-75, DR-79. Leaf `ofp-2-cap-c1-shared-continuation-reads`.

## DR-82 — Native data reads use copy-free shard snapshots

**Status.** Implemented 2026-10-05 for cap root cause C2 of epic 8946
(batch B1, phase B).

**Context.** The native hot store caches the data of each channel in a
persistent shard (`imbl::HashMap`). The native data read copied every
cached datum of the channel on every read, and charged each copy. The
produce path also read the data once only to fill the cache, and then
dropped the copies. Together with the continuation reads of DR-81, these
reads made up most of each validator replay's `VerificationBytes`.

**Decision.**

1. A new reader `native_data_view` returns a `NativeDataView`: an O(1)
   snapshot of the persistent shard and the channel key. A view holds no
   lock and copies no datum.
   - A warm read takes the snapshot.
   - A cold read prepays the release of the decoded data, moves them into
     the cache, and takes the snapshot under the insert's write lock
     (`native_insert_new_snapshot`). Every charge is reserved before the
     insert.
2. The candidate preparation of metered native replay borrows the datums
   of the views (`Cow::Borrowed`). Only the incoming datum of a produce is
   owned. The selected datum's fields are copied as before. The views
   drop before publication.
3. The produce path prefetches with `prefetch_data`, which fills the cache
   and copies no datum.
4. The owned reader stays for the public `get_data` API and for
   installation. The replaced lines stay in the source, commented out with
   their reason.

**Soundness.** A borrowed datum is the cached datum, so the canonical order
and the selection are unchanged. A snapshot of a persistent map is not
changed by later writes. Every reservation of a cold read comes before the
insert, so a rejected reservation leaves the cache unchanged.

**Scope.** This change is cost-accounting work. Native replay and its hot
store paths exist only on this branch. The change alters host-work charges of
protocol 6, which is not yet released. It changes no observable value.

**Verification.** `NativeSharedReads.v` adds, without axioms:

- `rejected_cold_read_leaves_cache` and `accepted_cold_read_equals_read`;
- `insert_first_fills_cache_on_rejection`: a negative control. A read that
  inserts before it reserves leaves the cache filled after a rejection.

`NativeSharedReadCleanup.tla` (DR-81) also stands for the data views: a
snapshot is a shared pointer to the shard version.

Tests in `hot_store/native/tests.rs`:

- `data_view_selection_matches_owned`: views equal owned reads on a cold
  and on a warm cache.
- `data_view_allocates_no_payload`: the warm view allocates the same bytes
  when the payload grows 16 times.
- `data_view_is_a_stable_snapshot`: a later write does not change a view.
- `every_data_view_cut_rejects_without_filling_the_cache`. This test found
  a first draft that reserved the snapshot after the insert.

**Cross-refs.** DR-81. Leaf `ofp-2-cap-c2-data-views`.

## DR-83 — Continuation-shard replaces count store-owned pointers inline

**Status.** Implemented 2026-10-05 for cap root cause C5 of epic 8946
(batch B1, phase B). Its gate H8 holds: in the phase A probe,
`reserve_replace` was 6.6% of each validator replay's `VerificationBytes`
and 19.7% of its `SearchStateBytes`. The same item's C6 (H4, export at 0.9%
and 1.4%) and C11 (H9, no measurable duplicate-check site) are refuted and
change nothing.

**Context.** A replace in a persistent hot-store shard charges a copy and a
cleanup of every value in the shard. The values of a continuation shard are
vectors of store-owned shared pointers. The cleanup walk followed each
pointer into its payload, although each payload's release was prepaid when
it entered the cache: on a cold read (DR-81) and on a stored consume
(`native_store_consume` prepays the stored continuation).

**Decision.**

1. The walker gains a shared-pointer cleanup walk
   (`clone_backing::inspect_shared_pointers`). It visits each pointer and
   skips its payload, so pointers nested inside a payload are never reached.
2. `native_backing::reserve_shared_copy_and_cleanup` charges a store-owned
   pointer vector with the walker's copy walk and the shared cleanup walk.
   `reserve_shared_cleanup` charges one retired pointer the same way.
3. The continuation-shard replaces use them: `reserve_replace_shared` in
   `native_store_consume` and `native_retire_produce_match`, the copy of the
   existing pointer vector there, and the retired pointer of a match. Data and
   join shards keep the full charge. The replaced lines stay in the source,
   commented out with their reason.

**Soundness.** Every store-owned pointer's payload release was prepaid at
entry. A replace copies and drops pointers only, so the last drop releases a
prepaid payload. The shared charge equals the walked charge minus the walk
into the payloads.

**Scope.** This change is cost-accounting work. The native hot store paths
exist only on this branch. The change alters host-work charges of protocol 6,
which is not yet released. It changes no observable value.

**Verification.** `NativeSharedReads.v` adds
`shared_replace_needs_no_payload_cleanup`, without axioms. Tests in
`hot_store/native/tests.rs`:

- `shared_copy_charge_omits_only_the_payload_walk`: for payload-free
  pointers, the walked charge exceeds the shared charge by exactly the push
  of each empty payload.
- `shared_copy_charge_is_independent_of_payload`: the shared charge stays
  the same when the payload grows 16 times, stays below the walked charge,
  and covers the allocation of the pointer copy.
- `shared_cleanup_charges_the_pointer_only`.

**Cross-refs.** DR-81, DR-82. Leaf `ofp-2-cap-conditional-c5-c6-c11`.

## DR-84 — A split adds one causal-path segment

**Status.** Implemented 2026-10-05 for cap root cause C9 of epic 8946
(batch B1, phase C).

**Context.** The deterministic scheduler orders RSpace operations by their
causal paths, in the forward lexicographic order of `Vec<(u64, u64)>`. An
operation of participant `P` at step `s` has the path `P·(s, 0)`.
`ReductionContext::split` gave child `i` of a split at step `s` the
participant path `P·(s, 1)·(i, 0)`. The detached driver of commit
`c3aacd649` evaluates every continuation body in a child of `split(1)`. A
chain of `L` nested continuations therefore reached an operation path of
`2L + 1` segments. The gateway funding flow reached 1,025 segments, above the
protocol-6 limit `V6_NATIVE_PATH_SEGMENTS` of 1,024. The provisional limit
4,096 hid that excess.

**Decision.**

1. `split` gives child `i` of a split at step `s` the one segment
   `(s, i + 1)`. Operations keep `(s, 0)`. The replaced lines stay in the
   source, commented out with their reason.
2. The legacy splitter stays as the test oracle `split_legacy` under
   `#[cfg(test)]`. A thread-local test flag selects it when a session is
   built.
3. The committed limit `V6_NATIVE_PATH_SEGMENTS` stays 1,024.

![Diagram of one participant plan and its causal paths under the two splitters. The plan has an operation at step 0, a split into two children at step 1, an operation of child 0, a split of child 1 into one grandchild, an operation of the grandchild, and an operation at step 2 after the children rejoin. The legacy splitter gives child i the segments (s,1) (i,0), and its sorted operation paths are (0,0), then (1,1) (0,0) (0,0), then (1,1) (1,0) (0,1) (0,0) (0,0), then (2,0). Its longest path has 5 segments. The compacted splitter gives child i the segment (s, i + 1), and its sorted operation paths are (0,0), then (1,1) (0,0), then (1,2) (0,1) (0,0), then (2,0). Its longest path has 3 segments. Both columns lead to a note that every operation has the same rank, by render_order_isomorphism, and that k nested splits give k + 1 segments instead of 2k + 1. A red box shows the rejected naive fusion (s, i): Op 1 and Child 1 0 both give (1,0), by naive_fusion_collides. A note explains that the step counter prevents that pair in the program, but that the chosen fusion keeps the order without that invariant, because (s, i + 1) is greater than (s, 0). A legend names the five colours.](diagrams/detached-path-compaction.svg)

(*Source: [`diagrams/detached-path-compaction.puml`](diagrams/detached-path-compaction.puml) — render with `plantuml -tsvg docs/casper/theory/diagrams/detached-path-compaction.puml`.*)

**Soundness.** Describe each generated path by its units: an operation at
step `s`, or child `i` of a split at step `s`. Let `legacy(u)` and
`compact(u)` be the paths of the two splitters for the unit list `u`. For all
unit lists `u` and `v`:

```math
\mathrm{cmp}\bigl(\mathrm{compact}(u), \mathrm{compact}(v)\bigr)
  = \mathrm{cmp}\bigl(\mathrm{legacy}(u), \mathrm{legacy}(v)\bigr)
```

At the first unit where `u` and `v` differ, both renderings decide the
comparison in the same way:

| Units | Legacy segments | Compacted segments | Result in both |
| --- | --- | --- | --- |
| two operations | `(s, 0)` and `(t, 0)` | `(s, 0)` and `(t, 0)` | `s` against `t` |
| operation and child | `(s, 0)` and `(t, 1)` | `(s, 0)` and `(t, j + 1)` | `s` against `t`, and the operation first when `s = t` |
| two children | `(s, 1)·(i, 0)` and `(t, 1)·(j, 0)` | `(s, i + 1)` and `(t, j + 1)` | `s` against `t`, then `i` against `j` |

Equal units give equal segments in both renderings. The proof therefore
needs no premise on the plan. The scheduler orders every frontier in the
same way, so the schedule, the event log and the post-state root do not
change. The naive fusion `(s, i)` would give the operation `(s, 0)` and the
first child of a split at step `s` the same path. A participant uses one step
counter for its operations and its splits, so the program never makes that
pair. The chosen fusion does not depend on that invariant.

Two orders keyed by paths change. `OperationKey` compares the path length
first, so the shape of its lookup index changes. `NativeIndex::keys`
iterates in insertion order, so only the lookup charges change, and they
shrink with the shorter paths. `JournalKey` compares the segments
lexicographically, so its order does not change.

**Scope.** This change is cost-accounting work. The detached driver and the
native evidence exist only on this branch. Native evidence records shorter
paths in the same format, which is a protocol-6 encoding change. Protocol 6
is not yet released. The change alters no schedule, event log or root.

**Verification.** `DetachedPathCompaction.v` proves without axioms:

- `fuse_render`: `fuse` maps every legacy path to the compacted path of the
  same units;
- `render_order_isomorphism` and `fuse_order_isomorphism`;
- `fuse_injective`;
- `fuse_halves_participant_depth` and `funding_flow_depth`: 513 segments
  instead of 1,025 for 512 nested splits;
- `naive_fusion_collides`: a negative control for the fusion `(s, i)`.

Tests:

- `compacted_paths_preserve_vec_order` (`deterministic_reduction.rs`): 64
  seeded random plans generate 1,227 paths, nested up to four splits. Every
  pair of compacted paths compares like the legacy pair, by vector order and
  by the shared-root comparison of `CausalPath`. Distinct steps have distinct
  paths.
- `funding_flow_depth_at_most_513` (`deterministic_reduction.rs`): 512
  nested splits give 513 segments, and the legacy splitter gives 1,025.
- `compacted_schedule_equals_legacy_schedule` (native execution tests):
  native play of four corpus terms with recursion, a fork tree, joins, peeks
  and a persistent send gives the same event log and the same root with both
  splitters. Each term records a shorter longest path with the compacted
  splitter, so the legacy flag took effect.

**Cross-refs.** DR-74. Leaf `ofp-2-cap-c9-path-compaction`.

## DR-85 — Causal paths carry a chained digest, and the recording index compares digests

**Status.** Implemented 2026-10-05 for cap root cause C7a of epic 8946
(batch B1, phase C). Its gate H5 holds: in the phase B probe, the recording
occurrence index was 33.6% of the producer execution budget's
`VerificationBytes` (169 MB of 502 MB).

**Context.** The native recorder detects a repeated occurrence with an
index of the recorded occurrences (`NativeBudgetRecorder::occurrences`). The
index was keyed by `NativeBudgetOccurrence` (session, path, stage), and a
comparison read the two paths segment by segment up to their first
difference. The recorded paths share long prefixes, so each comparison on a
search path read most of the path. The index also held a second copy of each
path.

**Decision.**

1. Each node of a `CausalPath` stores the digest of its path. `push_back`
   computes it in constant time from the parent digest:
   - the root digest is Blake2b-256 over the domain tag
     `f1r3fly/causal-path/v1` and the node-kind byte 0;
   - a child digest is Blake2b-256 over the domain tag, the node-kind byte 1,
     the parent digest and the two segment words in big-endian order.
2. The recording index is keyed by `OccurrenceKey`: the session, the digest,
   the depth, the last segment and the stage. A comparison charges a fixed
   90 bytes and 7 operations, whatever the path depth.
3. `reserve_record` copies the path once, for the attempt, instead of twice.
   Its operation and `SearchStateBytes` charges count one copy.
   `capture_recording` sums the depths of the keys.

The replaced lines stay in the source, commented out with their reason.

**Soundness.** Two paths have equal chained digests exactly when they are
equal, if the child digest determines its parent digest and segment and no
child digest equals the root digest. Blake2b-256 collision resistance and
the node-kind byte justify that premise. The digest-keyed index therefore
reports a repeat exactly when the path-keyed index does. The index order
changes from path order to digest order, but only lookups use it:
`NativeIndex::keys` iterates in insertion order.

A digest collision would make the producer reject its own execution as a
repeat. Validators do not use the recorder: they check the decoded journal
independently. A collision therefore cannot make a validator accept a wrong
value.

**Scope.** This change is cost-accounting work. The native recorder and the
causal-path digests exist only on this branch. The recording content and its
encoding do not change. The change alters host-work charges of protocol 6,
which is not yet released.

**Verification.** `NativePathTrie.v` (part 1) proves without axioms:

- `chain_snoc`: the digest of `p ++ [s]` is the child digest of the digest
  of `p` and `s`;
- `digest_chain_correct`: equal digests exactly when the paths are equal;
- `occurrence_key_correct`: equal occurrence keys exactly when the sessions,
  paths and stages are equal;
- `unchained_digest_collides`: a negative control. A key that hashes only the
  last segment gives two different paths the same key.

Tests:

- `path_digest_folds_the_segments` (`operation_context.rs`): the digest of a
  path folds the child digest over its segments, and paths with a shared
  prefix have equal digests exactly when they are equal.
- `digest_keyed_occurrences_match_path_keyed_index` (`tests/index.rs`): on
  generated occurrence sequences with frequent repeats, the digest-keyed
  index reports the same repeats as the path-keyed index.
- `digest_keyed_lookup_charge_is_independent_of_depth` (`tests/index.rs`): a
  comparison charges 90 bytes, and a lookup stays within the index height
  bound for paths of depth 1 and 1,000. The path-keyed lookup at depth 1,000
  charges more than 100 times the digest-keyed lookup.

**Cross-refs.** DR-74, DR-84. Leaf `ofp-2-cap-c7a-path-digests`.

## DR-86 — Native evidence interns its causal paths in one hash-consed trie

**Status.** Implemented 2026-10-05 for cap root cause C7b of epic 8946
(batch B1, phase C).

**Context.** The native evidence decoder rebuilt every causal path of a
recording and its journal as a vector. A delta entry copied the shared prefix
of the previous path and appended the suffix (DR-74). The journal check
counted the sum of the path depths against `total_path_segments`. The journal
index, `check_link`, `bind_trace` and the replay slot lookup compared whole
paths. In the gateway funding flow before C9, the depth sum was about
1.04 M, above the limit of 262,144, although the chain added only about 2.5 k
new segments. The phase B probe attributed about 18% of each validator
replay's `VerificationBytes` to path comparisons: the journal index (6.7%)
and the trace binding and slot lookup (11.7%).

**Decision.**

1. A recording owns one hash-consed trie of its causal paths
   (`NativePathTrie`). A node is a (parent, segment) pair, interned once. Each
   node stores the chained digest of C7a and a jump table of ancestors.
   Occurrences and journal rows name nodes (`PathId`) instead of holding path
   vectors.
2. The decoder interns each delta suffix below the ancestor at the prefix
   depth. It admits the suffix length against the limit Σ s_i before it
   allocates a node. It checks the canonical prefix before it creates the
   first suffix node. The new field `total_path_segments` of the recording
   wire limits holds the limit, set to `deploy_log_items`. A recording and its
   journal share the counter. The wire format does not change.
3. The journal check limits the node count. Journal keys and `check_link`
   compare node ids. `bind_trace` orders the slots by a depth-first walk of
   the trie that visits children by increasing segment. The replay finds a
   slot by the digest key of the live path in a sorted index.
4. The producer records copied paths as before. When it captures its
   evidence, it builds the same trie in the wire order of the evidence. Its
   evidence therefore equals the decode of its encoding, ids included.
5. The committed journal limit `total_path_segments` stays `deploy_log_items`.

The replaced lines stay in the source, commented out with their reason. The
encode and decode functions of the journal take the recording's trie, so two
calls in `offered_evidence.rs` change.

**Soundness.**

- The trie decoder accepts exactly the chains that the vector decoder
  accepts, and each node denotes the decoded path. Node ids are equal exactly
  when the paths are equal, so the uniqueness checks and the links decide as
  before.
- The walk lists the paths in strictly increasing lexicographic order and
  lists every node. The rows share one session and have distinct paths, so
  the walk gives the order of the replaced sort. An operation at the empty
  path, the root, comes first, as before.
- Equal digest keys mean equal paths under the collision premise of DR-85.
  The index rejects a duplicate key.
- The node count is at most Σ s_i, so `deploy_log_items` bounds the trie.
  The check relaxes: Σ s_i ≤ Σ depth, so evidence that passed before still
  passes.
- Each child search charges the B-tree search bound of the children map
  (DR-78). Each new node charges its vector growth, its tree growth (DR-77)
  and its digest. The walk, the index and the lookups charge their actual
  work. Every charge comes before the work.

**Scope.** This change is cost-accounting work. The native evidence codec,
the journal check and native replay exist only on this branch. The Casper
changes pass the recording's trie to the journal codec and set the new limit.
The evidence bytes, the event log and the roots do not change. Protocol 6 is
not yet released.

**Verification.** `NativePathTrie.v` (part 2) proves without axioms:

- `node_identity_is_path_equality`;
- `child_spec`, `intern_spec` and `path_of_ancestor`;
- `trie_decode_denotes_paths` against `NativePathDeltaCodec.decode_all`;
- `trie_nodes_bounded_by_suffixes`;
- `trie_dfs_is_lex_sort`, `preorder_unique` and `preorder_complete`;
- `materialized_segments_quadratic` and
  `materialized_segments_exceed_journal_limit`: a negative control. A
  staircase chain of 724 paths has 724 new segments but 262,450
  materialized segments.

Tests:

- `trie_decode_round_trips_v1_v2` (`wire.rs`): the trie encoder writes the
  bytes of the segment encoder, and the v2 and v1 decoders rebuild the
  encoder's trie with the same ids for a recording chain and a journal chain.
- `delta_limit_checked_before_allocation` (`wire.rs`): a suffix header above
  the remaining limit fails with no node and no `SearchStateBytes` charge, and
  the limit counts a recording and its journal together.
- `trie_order_equals_vec_sort` and `ids_equal_iff_paths_equal`
  (`tests/path_trie.rs`), with ancestor, prefix and limit tests.
- `replay_digest_lookup_equals_binary_search` (`tests/checked_trace.rs`): the
  digest index finds the slot of every operation path and no slot for its
  prefixes, extensions and other paths.

**Cross-refs.** DR-74, DR-77, DR-78, DR-85. Leaf
`ofp-2-cap-c7b-decoded-path-trie`.

## DR-87 — The acquisition demand holds one entry for each obligation key

**Status.** Implemented 2026-10-05 for cap root cause C8 of epic 8946
(batch B1, phase C).

**Context.** `prepare_acquisition_demand` pushed one resource entry for each
located occurrence of a measured class. In the gateway funding flow, 2,899
occurrences of a few keys gave 5,798 entries in the required and fresh parts
of the witness, 5,798 authority nodes and about 4.76 MiB of key bytes. These
counts exhausted the protocol-6 limits for obligations (4,096), authority
nodes (4,096) and key bytes (262,144), and the limits were raised
provisionally.

**Decision.**

1. The demand holds one entry for each distinct key: a purse identity and a
   class. Each entry holds the summed quantity of its occurrences. The
   entries are sorted by the identity encoding and the class.
2. The purse identity is the canonical obligation-key encoding of the
   purse's location and authority (`PhloResource::encoded_obligation_key`).
   Two purses on one channel with different authorities therefore stay
   distinct. Two signatures with one authority value merge. The spec text
   names (purse channel, class) as the key. The authority is added, because
   the payment exactness that the spec requires needs it.
3. The entry limit applies to the distinct keys. A zero quantity and an
   overflowing quantity sum are rejected when the demand is prepared. The
   later checks rejected the same inputs before.
4. The demand maps each occurrence to its entry (`occurrence_entry`). The
   prepaid selection still works on occurrences, so the evidence positions
   do not change. The binding maps each position to its entry and keeps a
   quantity bound for each occurrence, so it decides as before.
5. Settlement already merged the obligations of each key before it encoded
   them. It now receives distinct entries.
6. The committed limits stay at their original values.

The replaced lines stay in the source, commented out with their reason.

**Soundness.** Aggregation keeps the counted quantity of every key, the
priced value, the counted discharge and the exhaustion of supply. Payments
stay exact. The obligations and the funding case are equal as sets, and the
funding case is put in canonical order before family selection and capture,
so its bytes do not change. Validators compute the same aggregation from the
same evidence.

**Scope.** This change is cost-accounting work. The acquisition demand, the
prepaid binding and the settlement of offered deploys exist only on this
branch. The Casper changes are in the cost-accounting tree: the demand
binding and the acquisition limits. No encoding changes. The accepted set
grows: an offer whose occurrences exceeded the limits, but whose distinct
keys fit, is now accepted. This is a protocol-6 rule change, and protocol 6
is not yet released. Prepaid draws are not aggregated: each draw still
derives two keys. Phase D (S11) measures that remaining use.

**Verification.** `AggregatedAcquisitionDemand.v` proves without axioms:

- `aggregate_preserves_counts` and `aggregate_expansion_permutation`;
- `aggregate_preserves_weighted_usage`;
- `aggregate_preserves_partition` and `aggregate_preserves_exhaustion`;
- `aggregate_keys_distinct`;
- `aggregated_payment_exact`;
- `occurrence_key_work_exceeds_aggregated`: a negative control. 2,899
  occurrences of eight one-node keys need 5,798 authority nodes, above the
  limit of 4,096, and their aggregate needs 16.

Tests in `acquisition/tests/aggregation.rs`:

- `aggregated_witness_checks_like_occurrence_witness`: on generated purses,
  including one channel with two authorities, the entries have distinct keys
  and sum their occurrences, the result does not depend on the row order,
  and the aggregated demand checks, discharges and projects its obligations
  like the per-occurrence demand. At the distinct entry limit, the
  aggregated witness passes and a larger per-occurrence witness fails.
- `settlement_encodes_each_key_once`: a demand with 64 copies of each
  occurrence gives one obligation for each key and the fee, encodes each key
  once, and does the encoding work of a demand with one occurrence per key.
- `funding_flow_has_nine_keys`: two purse identities and four classes in
  2,904 occurrences give nine obligations under the original limits, which
  the per-occurrence demand exceeds.

The existing acquisition tests now assert the aggregated entry counts. The
gateway funding flow itself is measured in the phase C probe.

**Cross-refs.** DR-77, DR-85, DR-86. Leaf `ofp-2-cap-c8-aggregated-demand`.

## DR-88 — Candidate selection charges only what the matcher reads

**Status.** Implemented 2026-10-05 for Phase D item D-A1 of epic 8946
(D-M1 and D-M6 of the Phase D plan).

**Context.** Native replay selects candidates in
`metered_match_data` (`native_candidate/metered.rs`) and checks
installations in `install` (`native_session/installation.rs`). Before each
match attempt the caller inspected the whole pattern and the whole datum.
Before the commit check it inspected the whole continuation, and it copied
every matched datum into an owned vector only to pass a slice to
`check_commit_metered`. In the gateway funding block the continuation
inspection alone charged 177.2 MB of VerificationBytes per validator
replay (157.3 MB on the produce path, 19.9 MB on the consume path), the
pattern and datum inspections 14.7 MB, and the copies 27.3 MB.

**Decision.**

1. `Match::get_metered` and `Match::check_commit_metered` carry a written
   contract: an implementation reserves every read of the pattern, the
   datum, the continuation and the matched data before it performs the read.
   The rholang `Matcher` complies: `fold_match` reserves a copy of every
   pair it touches, `free_check` reserves every surplus target, and the
   commit check reserves one operation and then walks only the guard. Every
   test matcher complies.
2. The caller no longer inspects the pattern, the datum or the continuation
   before these calls. The inspections are commented out with their reason.
3. `check_commit_metered` takes the matched data by reference
   (`&[&A]`). The caller builds a vector of references, charged as a buffer
   of `n` pointers, instead of copying every matched datum. The owned-copy
   code is commented out with its reason. The unmetered `check_commit` keeps
   its owned signature.

**Soundness.** The removed inspections prepaid no read: every read of the
pattern, the datum and the guard is reserved by the self-metered callee
before it happens, so prefix coverage holds for the new runs as for the
legacy runs. The reference vector is the only remaining work of the old
copy step, and it is reserved before it is built. The decisions do not
change: the commit check over borrowed data evaluates the same guard on the
same bindings. Every charge depends only on value sizes and `size_of`, so
producer self-replay and validator replays compute it identically.

**Scope.** This change is cost-accounting work: native replay and its
metered matcher interface exist only on this branch. The play path
(`Match::get`, `Match::check_commit`) does not change. No encoding, root or
event changes.

**Verification.** `CandidateReadCoverage.v` proves without axioms, over the
event model of `ObservationReadCoverage`:

- `attempt_trace_covered`, `legacy_attempt_trace_covered`,
  `commit_trace_covered` and `legacy_commit_trace_covered`;
- `attempt_reads_equal_legacy_reads` and `legacy_attempt_excess`: the
  attempt reads the same and reserves one pattern and one datum inspection
  less;
- `legacy_commit_excess` and `legacy_commit_reads`: the commit check
  reserves the continuation inspection, the owned vector and the copies less
  and the reference vector more, and the copies were real work;
- `attempt_charge_independent_of_unread_tails`,
  `commit_charge_independent_of_body` and
  `commit_refs_charge_independent_of_matched_sizes`;
- negative controls `legacy_commit_charge_depends_on_unread_body`,
  `legacy_attempt_charge_depends_on_unread_tails` and
  `legacy_commit_copy_excess`.

Tests:

- rspace++ `native_candidate/metered/tests.rs`:
  `match_attempt_charge_excludes_unread_pattern_and_datum` (4 KiB unread
  tails on the pattern and the datum leave a failed attempt's charge
  unchanged), `commit_check_charge_is_independent_of_continuation_body` (a
  4 KiB continuation leaves a successful match's charge unchanged), and the
  negative control `legacy_selection_charge_grew_with_unread_values`.
- rholang `matcher/match.rs`: `commit_guard_by_reference_matches_owned_guard`
  (borrowed and owned checks decide alike for empty, constant, bound-variable,
  cross-binding, out-of-range and non-boolean guards),
  `commit_check_charge_is_independent_of_continuation_body` (also: without a
  guard the check charges exactly one operation, whatever the matched data),
  and the negative control `legacy_commit_charge_grew_with_unread_body`.
- The existing every-cut and differential tests of candidate selection and
  installation pass unchanged.

**Cross-refs.** DR-76 (C12, the same read-coverage rule for the COMM
observation), DR-81, DR-82. Leaf `ofp-2-cap-d-a1-commit-check-reads`.

## DR-89 — Replay authority charges only the reads and copies it makes

**Status.** Implemented 2026-10-05 for Phase D item D-A2 of epic 8946
(D-O4 and D-O5 of the Phase D plan).

**Context.** Two replay-authority charges did not follow the work.

1. `ReplayAuthorityBinding::prepare` walked the whole recorded observation
   of every COMM row and reserved a copy of its authority, whatever the
   branch. Only the retry branch compares the recorded observation, and only
   a new row copies the authority: into the event when it is granted, into
   the frontier at publication when it is not.
2. `reserve_native_result_backing` reserved a copy and cleanup of the whole
   events map (its tree backing and every event's shared byte-observation
   payload) and of every byte-observation row payload. `capture_result`
   copies only each event's id, authority and debit
   (`authority_events`). In replay the rows come from the evidence and are
   not copied. In play the rows are copied as shared pointers whose payload
   releases were prepaid when the rows were born.

In the gateway funding block these charges were 39.8 MB of VerificationBytes
per validator replay and 24.1 MB in producer execution.

**Decision.**

1. `prepare` no longer walks the observation or reserves the authority copy
   up front; both lines are commented out with their reason. The retry
   branch inspects the saved and the recorded observation before it compares
   them. The `(None, false)` branch reserves the authority copy and the copy
   of the shared observation pointer that the event or the frontier keeps.
2. `reserve_native_result_backing(copies_rows)` reserves, for each event,
   the copy and cleanup of its id, its authority and its debit
   (`backing::reserve_event_copies`), and keeps the realized ledger, the stack
   births and the result vectors. When rows are copied (play, no evidence), it
   reserves the pointer-slice copy and the shared-pointer cleanup
   (`inspect_shared_pointer_slice`, the C5 rule of DR-83). `capture_result`
   passes `evidence.is_none()`.
3. The shared walker gains `inspect_shared_pointer_slice`, the slice form of
   `inspect_shared_pointers`. The rholang wrappers `reserve` and
   `reserve_slice` are enabled outside tests.

**Soundness.** Each branch of `prepare` reserves every read and copy it
performs before it performs it: the comparison reads both observations, the
event or the frontier copies the authority and the shared pointer. The
result backing covers every copy `capture_result` makes: event ids,
authorities and debits with their releases, the realized ledger and the
births, and in play the row pointers. Row payload releases were prepaid at
birth (`reserve_observation_birth` reserves the shared allocation as owned
backing, and the canonical authority is built under owned backing). The
charges depend only on value sizes and on whether evidence is present, which
every replay of a block agrees on.

**Scope.** Cost-accounting work: the native replay authority and its result
backing exist only on this branch. The error order changes only on replays
that are already invalid (a row that fails an identity check is now rejected
before the observation walk is reserved). No encoding, root or event changes.

**Verification.** `ReplayAuthorityPrepareCoverage.v` proves without axioms:
`prepare_trace_covered`, `legacy_prepare_trace_covered`,
`prepare_reads_equal_legacy_reads`, `legacy_prepare_excess`,
`prepare_charge_independent_of_unread_values` and the negative control
`legacy_prepare_inspects_unread_observation`. `ResultBackingCoverage.v`
proves `result_charge_covers_play_copies`,
`result_charge_covers_replay_copies`, `legacy_result_trace_covered`,
`result_reads_equal_legacy_reads`, `legacy_result_excess`,
`shared_row_cleanup_releases_no_payload` and the negative control
`legacy_result_charge_counts_unreturned_payloads`.

Tests in `replay_authority/tests/backing.rs`:

- `prepare_rejects_each_smaller_dimension_without_publication`: for a
  granted, a frontier and a retry row, a budget one unit short in any
  dimension rejects the prepare and publishes nothing.
- `result_backing_charges_row_pointers_not_payloads`: the replay result
  backing does not depend on the rows; the play charge for a row does not
  depend on its payload.
- `event_result_backing_covers_counted_allocations`: the bytes the result
  copies allocate (counted by a measuring allocator) stay within the reserved
  SearchStateBytes.
- Negative controls `legacy_prepare_charge_walked_the_recorded_observation`
  and `legacy_result_backing_walked_unreturned_payloads`.

The existing replay-authority tests, including the retry exhaustion and the
checkpoint and result rejection tests, pass unchanged.

**Cross-refs.** DR-79 (C14), DR-83 (C5), DR-88. Leaf
`ofp-2-cap-d-a2-replay-authority-backing`.

## DR-90 — The stored consume's duplicate check compares source hashes first

**Status.** Implemented 2026-10-05 for Phase D item D-A4 of epic 8946
(D-S4 of the Phase D plan; the conditional fix C11 of phase B, whose gate is
now met).

**Context.** `native_store_consume` (`hot_store/native.rs`) decides whether
a new waiting continuation duplicates a stored one by comparing their
identities: the Debug text of the patterns, the body, the persistence flag
and the peeks. It formatted the new continuation and every stored
continuation of the channel group on every store. In the gateway funding
block the formatting charged about 151 MB of VerificationBytes and 22 MB of
SearchStateBytes per validator replay (4.6% of VerificationBytes).

**Decision.**

1. For each stored continuation the check first compares its source hash
   with the new continuation's source hash (one operation and 64 bytes).
   A stored continuation with a different hash is not a duplicate.
2. Only on equal hashes does the check format and compare the identities.
   The new continuation's identity is formatted at most once, the first time
   it is needed.
3. The replaced code stays in the source, commented out with its reason.

**Soundness.** The source hash of a consume is the hash of the sorted channel
hashes, the sorted pattern encodings, the body encoding and the flag
(`native_source::consume`, equal to the legacy `Consume::create`). Every
continuation of one cache entry has the same channels. Equal identities
therefore give equal patterns, body and flag, hence equal sorted encodings
and an equal hash, whatever the hash function. So a different hash rules out
a duplicate, and the decision is the decision of the identity scan. Equal
hashes do not imply equal identities (patterns in another order, other
peeks), so the identity comparison stays for hash ties. Floats are stored as
raw bits, so Debug equality and encoding equality agree.

**Scope.** Cost-accounting work: the native stored consume exists only on
this branch. No encoding, root or event changes; the decision does not
change.

**Verification.** `NativeDuplicatePrefilter.v` proves without axioms, for an
arbitrary hash function: `identity_eq_implies_source_hash_eq`,
`prefiltered_duplicate_equals_scan`,
`prefilter_builds_identities_only_on_hash_ties`, and the negative control
`permuted_patterns_share_hash` (a hash-only check would be wrong).

Tests in `hot_store/native/tests.rs`:

- `prefiltered_duplicate_decision_matches_identity_scan` (256 cases): the
  stored-consume decision equals the identity scan for exact copies,
  permuted patterns, changed peeks and new continuations, with real sources.
- `identity_equality_implies_source_hash_equality` (256 cases).
- `distinct_source_hashes_format_no_identity`: with four stored
  continuations of other hashes, 4 KiB bodies leave the charge unchanged.
- Negative control `legacy_duplicate_check_formatted_every_stored_continuation`.

The existing stored-consume test, which stores continuations with equal
(default) sources, passes unchanged.

**Cross-refs.** DR-83 (C5). Leaf `ofp-2-cap-d-a4-duplicate-prefilter`.
