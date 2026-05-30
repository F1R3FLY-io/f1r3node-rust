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
discriminant (default `0` for back-compatibility). The Rocq `sig` gains `SGround`; `ASHash` is repurposed
as the `#P` axis.

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
3. **fee ≠ cost:** the `F_v` collection (the spec's flat `FeeExtract`, one transferred token per processed
   deploy) is SEPARATE from the WD-D2 settlement debit (the burned COST). The committed D2 gate/settlement is
   unchanged by StageD. PoS owns only the conversion ELIGIBILITY (`active ∧ ¬mintingHalted ∧
   ¬convertedEpochs`) + `convertedEpochs` idempotency, publishing the eligible list on
   `sys:casper:feeConvertList`. (Scope note: the OD-4 `@W_v` draw-wallet purse mirror is not performed — the
   convert credits the consensus pool `Σ⟦v⟧` only; see the `workstream-c-economic.md` Stage D realization
   note for the replay-rig rationale and the follow-on Rust→PoS seam.)

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
gain two phases after the mint + settlement: (3a) COLLECTION — credit `F_v(proposer) += count` (the flat
`FeeExtract`, `count = block.body.deploys.len()`, threaded play-side via `fee_credits`, recomputed replay-side
from `terms.len()` by `recompute_fee_credits` — same recompute-from-block discipline as the settlement debit);
(3b) CONVERSION — read the eligible `[(v, epochIdx)]` list PoS published on `sys:casper:feeConvertList`, and
for each eligible `v` credit `Σ⟦v⟧ += f` and zero `F_v` (`f = read F_v(v)`; `f ≤ 0 ⇒ skip`, DR-4). Disjoint
replay-stable `random_state` paths (`fee_collect_random_state` `-0x2e`, `fee_convert_random_state` `-0x2d`,
disjoint from mint `lo≥0` / debit `-0x2b` / slash `-0x2c` / mint-list `0x2a`) + the `ReplaySupplyMismatch`
readback guard on every fee write. The cost ≠ fee separation holds: the fee is a transferred token on `F_v`,
the cost is the burned settlement debit on `Σ⟦s⟧`. Play/replay symmetry exercised by
`fee_collection_and_convert_is_play_replay_deterministic` + `fee_convert_converted_epochs_idempotent_deterministic`.

**Alternatives considered.** (a) literal nested-send messages, one per token — rejected (O(n) gate-read
bottleneck); (b) a Rust-injected supply name `@sigSupplyCh` bound into `VB`'s continuation — rejected
(re-exposes `Σ⟦v⟧` to the Rholang layer, enlarging the trusted surface); (c) a `sysAuthToken`-gated
`sigChannelOps` system process resolving sig→channel — recorded as a future refinement for in-Rholang
minting contracts (ERC-20-style), unnecessary while the only authorized writer is Rust.
