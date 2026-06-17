# §D2.9 — The Funding Flow: `Σ⟦signer⟧ == Σ⟦wallet⟧`

**Status:** IMPLEMENTED (`feature/cost-accounted-rho`, commit `3a4e03eb`). This is the
pedagogical, end-to-end companion to the authoritative implementation contract in
[`wd-d2-acceptance-gate.md`](wd-d2-acceptance-gate.md) §D2.9 and decision record
[DR-13 §D2.9-refinement](../cost-accounting-decision-records.md). It traces a single deploy's fuel
from genesis seeding, through the acceptance gate, to the settlement debit and its replay
re-verification, and explains the one invariant the whole flow exists to uphold:

> **A deploy's cost is debited from its signer's own wallet** — the supply pool keyed by the
> signer's **ground public key** — so `Σ⟦signer⟧ == Σ⟦wallet⟧`.

---

## 1. Terms & symbols

Every symbol is defined here before use (per the documentation guidelines). All quantities are
non-negative integers in one phlogiston unit (DR-9: token-per-COMM; `Σ` and `Δ` share the unit).

| Symbol / term | Definition |
|---|---|
| `pk` | A signer's **public key** (Ed25519 / secp256k1), the bytes of `Cosigner.pk`. |
| `wire_sig` | The per-deploy **wire signature** — the bytes a signer produces over the deploy data. *Fresh every deploy.* |
| `Cosigned` | A deploy's signed envelope: an ordered, verified set of cosigners `{(pkᵢ, sigᵢ)}` (a 1-element set is the legacy single-signer case). |
| `deploy_id` | The deploy's **on-chain identity**, `Blake2b256(DEPLOY_SIGNATURE_DOMAIN ‖ wire_sig)`. Must be unique **per deploy** ⇒ derived from `wire_sig`. |
| `envelope_sig` | `Sig::Quote(Blake2b256(DEPLOY_SIGNATURE_DOMAIN ‖ wire_sig))` — the `deploy_id` basis (a `#P`-style process hash). **Not** a funding key. |
| `funding_sig` | The **funding key** (§D2.9): `Sig::Ground(pk)` for a single signer; the left-associated `Sig::And`-fold of `Sig::Ground(pkᵢ)` over the *filtered* (non-placeholder) signer set for multi-sig. A *ground* signature `g` (the paper: `g` = "an Ed25519 public key / a secp256k1 key hash"). |
| `Σ⟦s⟧` | The **supply pool** keyed by signature `s` — a single balance datum `(TOKEN_TAG, n)` on the content-addressed channel `from_sig(s)` (DR-13). Written only by the Rust `supply::produce_balance`. |
| `@W_v` / wallet | The signer's genesis-seeded supply pool. For user deploys the wallet is `Σ⟦Ground(pk)⟧`; the formal model `WalletNaming.v` keys the validator wallet `@W_v := @(*walletTag, pk)` by the public key. |
| `Δ_s` (demand) | The **static demand** of a deploy under signature `s`: the count of token-consuming COMMs (one per send / receive), `delta_sigma::demand`. |
| `Σ_s` (supply) | The pool balance `read_balance(from_sig(s))` (absent ⇒ `0`). |
| margin | An economic floor (`min_phlo_price`); by F-B it rides **only** the data-dependent `unknown` branch of the funding check, never the resolvable-demand correctness gate. |
| strict / non-strict shard | A shard-genesis activation mode. **Strict** enforces the funding obligation against present pools (and rejects absent ones); **non-strict** early-admits an absent pool unenforced (the transitional default — byte-identical to pre-cost-accounting behavior). |
| placeholder cosigner | An empty-`sig` member of an M-of-N threshold envelope (listed but did **not** sign). EXCLUDED from `funding_sig` (security, §5). |
| `effectiveΣ` | The Split/Join effective supply of a compound signature: `effectiveΣ_{s₁∘s₂} = Σ_{s₁∘s₂} + min(Σ_{s₁}, Σ_{s₂})` — a compound demand may draw from the combined pool or a matched component pair. |

---

## 2. The invariant — and why the wire-signature keying broke it

A deploy carries **one** `Cosigned`, but the runtime needs **two** keys from it, with opposite
requirements:

- the **`deploy_id`** must be *unique per deploy* — so it is derived from the `wire_sig` (a fresh
  value every deploy) via `envelope_sig`;
- the **funding key** must be *stable per signer identity* — so the pool a signer funds from is the
  same wallet across all of that signer's deploys.

The original implementation derived **both** from `envelope_sig` (the wire signature). Because the
wire signature changes every deploy, a deploy's funding pool `Σ⟦envelope_sig⟧` was a *fresh,
genesis-absent channel* — never the wallet genesis actually seeds (`Σ⟦Ground(pk)⟧`). The acceptance
gate's absent-pool branch then admitted the deploy *unenforced and undebited*: the wallet was never
touched and the linear `Σ ≥ Δ` proof never bound. §D2.9 decouples the two keys: `deploy_id` stays
`wire_sig`-derived (byte-identical on-chain identity), while the funding key becomes
`funding_sig = Sig::Ground(pk)`, so the pool the gate reads, proves `Σ ≥ Δ` against, and debits **is**
the genesis-seeded wallet.

![§D2.9 funding flow — a deploy's Cosigned is verified at ingress, the funding key funding_sig = Sig::Ground(pk) is derived (decoupled from the wire-sig deploy_id), the acceptance gate reads the signer's wallet Σ⟦Ground(pk)⟧ and admits iff Σ ≥ Δ (Def 19; margin only on the unknown branch), the CloseBlock settlement debits post = pre − ΣΔ from that wallet, and replay reconstructs the cosigner set via to_cosigned() and re-derives the same funding_sig for a byte-identical debit map. The invariant Σ⟦signer⟧ == Σ⟦wallet⟧ holds end-to-end.](../diagrams/d2-9-funding-flow-sequence.svg)

(*Source: [`diagrams/d2-9-funding-flow-sequence.puml`](../diagrams/d2-9-funding-flow-sequence.puml) — render with `plantuml -tsvg docs/theory/diagrams/d2-9-funding-flow-sequence.puml`.*)

The decoupling — one `Cosigned`, two keys — is the heart of the fix:

![The deploy_id / funding_sig decoupling — one Cosigned { wire_sig, pk } fans out to two derivations: envelope_sig = Sig::Quote(Blake2b256(DOMAIN ‖ wire_sig)) feeds the deploy_id (the stable, byte-identical on-chain identity), while funding_sig = Sig::Ground(pk) feeds the supply pool Σ⟦Ground(pk)⟧ (the wallet the gate proves Σ ≥ Δ against and debits). The pre-§D2.9 edge envelope_sig → supply pool is struck out in red: the wire-sig pool was always absent, so the wallet was never debited.](../diagrams/deploy-id-funding-decoupling.svg)

(*Source: [`diagrams/deploy-id-funding-decoupling.puml`](../diagrams/deploy-id-funding-decoupling.puml) — render with `plantuml -tsvg docs/theory/diagrams/deploy-id-funding-decoupling.puml`.*)

---

## 3. Stage-by-stage walkthrough

The flow has five stages. Each is presented in literate-programming form (Knuth): the intent in
prose, then the algorithm in pseudocode keyed to the real functions.

### 3.1 Genesis — seed the signer's wallet

A shard's genesis trust-root (`wallets.txt`, threaded as `client_fuel_allocations`) names each
client by **public key** and an initial balance. At block 1, `CloseBlockDeploy` seeds that balance
onto the client's ground-pubkey pool — the same `Sig::Ground(pk)` the gate will later key by.

```
for (client_pk, balance) in client_fuel_allocations:           # close_block_deploy.rs
    chan ← supply_channel(Sig::Ground(client_pk))              # = from_sig(Ground(pk)) = Σ⟦Ground(pk)⟧
    produce_balance(chan, balance, random_state)               # the wallet, content-addressed, Rust-only
```

On a **non-strict** shard with empty `client_fuel_allocations`, no wallet is seeded, every pool is
absent, and the post-state is byte-identical to pre-cost-accounting (§6).

### 3.2 Funding-key derivation (with the placeholder filter)

At deploy admission and at runtime install, the funding key is derived by the single shared
`accounting::funding_sig` — the *one* function the gate, the runtime install, and the replay
recompute all call, so they can never drift:

```
funding_sig(cosigned):                                          # accounting/mod.rs
    funders ← [ s.pk.bytes for s in cosigned.signers if s.sig is non-empty ]   # placeholder filter (§5)
    match funders:
        [pk]        ⇒ Sig::Ground(pk)                           # single signer
        [pk₁,…,pkₖ] ⇒ And( … And(Ground(pk₁), Ground(pk₂)) …, Ground(pkₖ) )    # left-assoc fold
```

The `deploy_id` is derived separately, from the wire signature, and is byte-identical to the
pre-§D2.9 install (the decoupling):

```
set_deploy_signature_funded(wire_sig, funding_sig):            # accounting/mod.rs
    deploy_id      ← envelope_sig(wire_sig)                    # UNCHANGED — on-chain identity
    self.signature ← funding_sig                              # the supply / settlement key (§D2.9)
    install_signer_channels(funding_sig)                      # per-redex lane attribution
```

### 3.3 The acceptance gate — the linear proof `Σ ≥ Δ`

The gate (`acceptance.rs::build_candidate_with_logic` + `admit_by_funding`) keys each deploy by its
`funding_sig`, reads the wallet `Σ⟦Ground(pk)⟧` once, and admits the largest canonical-order prefix
of each per-signer group whose cumulative demand fits the supply. The funding predicate is the
paper's **Definition 19** — a *bare* inequality for resolvable demand; by **F-B** the margin rides
only the data-dependent `unknown` branch (Theorem 20):

```
is_funded(Δ, Σ, margin):                                       # delta_sigma.rs
    required ← Δ.known_lower_bound + (margin if Δ.unknown else 0)
    return Σ ≥ required                                        # resolvable ⇒ Σ ≥ Δ exactly (Def 19)
```

A first non-fitting deploy rejects it **and all after it** in the group (§7.7 reject-both).

### 3.4 Settlement debit — `post = pre − ΣΔ`

After all user deploys execute, `CloseBlockDeploy::dual_write_supply` debits each signer wallet by
the gate's admitted demand (`checked_sub`, underflow ⇒ invalid block), then carves the conserving
flat `FeeExtract`. The close-block stages — mint, cost debit, fee carve, fee convert — are disjoint,
replay-stable, and conserving:

![Close-block supply stages — Stage 1 mints Σ⟦v⟧ at an epoch boundary (green), Stage 2 debits the cost ΣΔ from the signer wallet Σ⟦Ground(pk)⟧ (red, BURN), Stage 3 carves the flat FeeExtract from Σ⟦c⟧ into the validator fee pool F_v (a paired red-out / green-in CONSERVING transfer), and Stage 3b converts F_v into Σ⟦v⟧ (green, backed by the carve). Every write is a read-modify-replace of a single datum on a disjoint replay-stable random_state path, guarded by a ReplaySupplyMismatch readback, so play and replay produce a byte-identical state.](../diagrams/close-block-stages-sequence.svg)

(*Source: [`diagrams/close-block-stages-sequence.puml`](../diagrams/close-block-stages-sequence.puml) — render with `plantuml -tsvg docs/theory/diagrams/close-block-stages-sequence.puml`.*)

### 3.5 Replay re-verification

Replay reconstructs the full *verified* cosigner set from the block via `Cosigned::to_cosigned()`
and re-derives the **same** `funding_sig` (the no-drift guarantee), so the recomputed settlement-debit
map is byte-identical to the play side (`recompute_settlement_debits_with_logic`).

---

## 4. Multi-signature & strict compound

A multi-sig deploy's `funding_sig` is the `And`-fold of the cosigners' `Sig::Ground(pkᵢ)` atoms, so
its funding components are exactly the cosigners' wallets `Σ⟦Ground(pkᵢ)⟧`. The cost is debited
**balanced** — each cosigner's wallet is debited equally (a compound token is a *matched pair*, one
from each pool; the ratified P8).

Genesis seeds the individual cosigner wallets but **not** the combined `Σ⟦And(…)⟧` pool, so under
enforcement a compound deploy funds from the **effective** supply
`effectiveΣ = Σ_compound(absent ⇒ 0) + min(Σ_l, Σ_r)`. This exposed a latent pre-§D2.9 bug: the
replay recompute keyed its strict re-verification on the compound pool's *raw* presence (absent),
so a strict compound deploy was play-admitted (on `effectiveΣ`) but replay-rejected — a play/replay
**fork**. §D2.9 keys both the strict re-verification *and* the settle-filter on `effectiveΣ`
(`effective_supply_with`), with the settle-filter keeping a group iff `strict ∨ present(own pool)` —
mirroring the play side exactly (non-strict stays byte-identical).

**Over-admission re-check (TM-CA-164).** The strict re-verification above only guarantees a compound group
has a *positive* effective supply; it does NOT bound the group's *cumulative demand* against that supply on
replay. Because `compute_settlement_debits` residual-caps a compound pair-draw at `min(Σ_l, Σ_r)`, an
over-demand `ΣΔ > effectiveΣ` (a malicious proposer stuffing more compound deploys from one cosigner set than
the gate would admit) is silently absorbed into per-pool debits ≤ balance — so the per-pool `debit > balance`
replay check (`recompute_and_verify_admission`) cannot catch it (it catches single-sig only, whose own-pool
debit is *uncapped* `= ΣΔ`). The deploys still execute (unmetered-for-liveness), so the cosigners would
oversubscribe their shared component wallets by `ΣΔ − effectiveΣ` un-funded units. The recompute therefore
ALSO re-imposes the gate's per-group bound on the RAW cumulative demand: for every enforced group it asserts
`Σ(cost + fee) ≤ effectiveΣ`, raising `ReplayAdmissionMismatch` otherwise — matching the gate's static
per-group `effective`, so it never forks a gate-admitted block (equality is admissible). Test:
`compound_over_admission_rejected_on_replay`. (Cross-group sharing of one component across *distinct* cosigner
sets is a separate, tracked follow-up — see TM-CA-164.)

![Strict compound effective supply — a strict multi-sig deploy whose combined Σ⟦And(…)⟧ pool is genesis-absent funds from effectiveΣ = Σ_compound (absent ⇒ 0) + min(Σ_l, Σ_r). Keying the replay recompute on the raw compound-pool presence (the pre-§D2.9 bug) rejects on replay what play admitted — a fork (red). §D2.9 keys on the effective supply, admits, settles balanced from the component pair (left −= k, right −= k, P8), and keeps the group in the settle-filter iff strict ∨ present(own pool), so play and replay are byte-identical (green).](../diagrams/strict-compound-effective-supply.svg)

(*Source: [`diagrams/strict-compound-effective-supply.puml`](../diagrams/strict-compound-effective-supply.puml) — render with `plantuml -tsvg docs/theory/diagrams/strict-compound-effective-supply.puml`.*)

---

## 5. Security — the placeholder filter (R1-F4 / TM-CA-162)

A Phase-2 **threshold** envelope (M-of-N) may list members who did *not* sign, as empty-`sig`
**placeholder** cosigners (`Cosigned::from_signed_data_threshold`). If `funding_sig` folded those in,
a deploy could key funding to — and so debit — an **unsigned victim's** wallet `Σ⟦Ground(victim_pk)⟧`.
`funding_sig` therefore **excludes** empty-`sig` signers: the *filtered* funder count (not
`is_compound()`) drives the funding arity, so a 1-of-2 threshold with one real signer + one
placeholder funds **only** the real signer's wallet. This is the threat
[TM-CA-162](../cost-accounting-threat-model.md); ingress `from_proto_cosigned` independently verifies
every non-placeholder `sig` against its `pk`, so a forger cannot present a victim's `pk` with a valid
`sig` either. Test: `threshold_placeholder_victim_wallet_is_never_debited`.

---

## 6. Migration & compatibility

| Shard mode | Behavior |
|---|---|
| **Non-strict + empty `client_fuel_allocations`** (default) | Every user deploy keys to `Σ⟦Ground(pk)⟧`, which is absent ⇒ the gate's early-admit branch admits unenforced with no debit ⇒ **post-state byte-identical to pre-§D2.9.** No genesis / integration golden moves. |
| **Strict shard** | Seed `Σ⟦Ground(client_pk)⟧` via `client_fuel_allocations` (already pubkey-keyed). This is where the linear `Σ ≥ Δ` proof and the data-dependent precharge actually bind; an unfunded signer is rejected. |
| **Genesis (block 0) + all system deploys** | Routed through `evaluate_system_source` (not `evaluate_cosigned`); they bypass the gate and never install a funding sig — the s₀ always-funded path. Unchanged. |

---

## 7. Reconciliation with the formal model

The §D2.9 fix does **not** change any formal artifact. The Rocq model was *already* pubkey-keyed:
`WalletNaming.v` keys the wallet `@W_v := @(*walletTag, validatorPk)` by the public key (modeled as
`SGround : list bool → sig`), with `wallet_name_injective` proved axiom-free, and **no** artifact
ties a pool to a wire signature. The paper's funding key is an *abstract parameter*; §D2.9 simply
instantiates it as `Sig::Ground(pk)` — exactly the pubkey naming the model already proves injective.
The implementation's wire-signature keying was the **outlier**; §D2.9 reconciles the code with its own
model. (See [`cost-accounted-rho-verification.md`](../cost-accounted-rho-verification.md) §12(iv).)

![§D2.9 reconciliation — the Rocq model WalletNaming.v already keys the validator wallet @W_v := @(*walletTag, validatorPk) by the public key (SGround : list bool → sig), proved injective and axiom-free. The Rust implementation's funding_sig = Sig::Ground(pk) → Σ⟦Ground(pk)⟧ instantiates that abstract funding-key parameter (a blue "instantiates" arrow from model to code). The pre-§D2.9 wire-sig Quote(hash) keying was the outlier the code corrected. Conclusion: no formal-model change is needed — §D2.9 brings the code into line with the model.](../diagrams/d2-9-walletnaming-reconciliation.svg)

(*Source: [`diagrams/d2-9-walletnaming-reconciliation.puml`](../diagrams/d2-9-walletnaming-reconciliation.puml) — render with `plantuml -tsvg docs/theory/diagrams/d2-9-walletnaming-reconciliation.puml`.*)

---

## 8. Test map

| Property | Test | Location |
|---|---|---|
| A signer's deploy debits exactly `Σ⟦Ground(signer_pk)⟧` by `Δ` (+ conservation) | `deploy_funds_from_signer_ground_pubkey_wallet` | `acceptance.rs::tests` |
| An unfunded signer is rejected under strict | `unfunded_signer_rejected_under_strict` | `acceptance.rs::tests` |
| Multi-sig funds balanced over cosigner wallets, play == replay | `multi_sig_funds_balanced_over_cosigner_ground_pubkey_wallets` | `acceptance.rs::tests` |
| A threshold placeholder victim's wallet is never debited | `threshold_placeholder_victim_wallet_is_never_debited` | `acceptance.rs::tests` |
| `funding_sig` shape; `deploy_id` decoupling preserved | `funding_sig_tests` (`funding_sig_single_is_ground`, `set_deploy_signature_funded_preserves_deploy_id_and_installs_ground`, …) | `accounting/mod.rs` |
| End-to-end gate → settlement → replay byte-identity | `gate_decision_replay_determinism` | `casper/tests/.../runtime_manager_test.rs` |

---

## 9. References & citations

- Implementation contract: [`wd-d2-acceptance-gate.md`](wd-d2-acceptance-gate.md) §D2.9; decision
  record [DR-13 §D2.9-refinement](../cost-accounting-decision-records.md); threat
  [TM-CA-162 / TM-CA-163](../cost-accounting-threat-model.md); use cases
  [UC-CA-160…163](../cost-accounting-use-cases.md); verification note
  [`cost-accounted-rho-verification.md`](../cost-accounted-rho-verification.md) §12(iv).
- Spec basis — `publications/cost-accounting/cost-accounted-rho.tex`: the signature grammar
  (`g` = an Ed25519 public key / secp256k1 key hash), the per-actor signature-indexed pools (§4.6/§4.7),
  and a deploy consuming its signer's pool. **Caveat:** `publications/` is read-only and **not** in
  this working tree, so these `.tex` clause references are the design pass's citations and are
  *unverified against the real paper* — confirm before relying on them; do not edit any `.tex`.
- Reflective higher-order calculus (the `#P` quote / reflection substrate): L. G. Meredith and
  M. Radestock, "A reflective higher-order calculus," *ENTCS* 141(5):49–67, 2005,
  [doi:10.1016/j.entcs.2005.05.016](https://doi.org/10.1016/j.entcs.2005.05.016).
