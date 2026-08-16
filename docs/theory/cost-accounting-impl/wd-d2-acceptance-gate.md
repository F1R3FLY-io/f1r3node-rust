# WD-D2 acceptance gate and settlement debit (historical design)

> **Retired implementation design; do not implement from this document.** Its
> discovery that funding identity must derive from verified signer public keys
> remains valid. Its channel-balance supply store, `dual_write_supply`, and
> close-block settlement were replaced by canonical SystemVault reservation,
> authenticated located-stack pops, retained state-bound execution, and
> replay-symmetric settlement. The normative protocol is
> [`end-to-end-authority-settlement.md`](end-to-end-authority-settlement.md).

**Status:** Historical WD-D2 design retained for the funding-key incident and
the evolution from structural-only admission to dependent state-bound proof.

## D2.1 — Gate placement
Call `certify_state_bound_admission` from `block_creator.rs::create` after `compute_parents_post_state` and before committed checkpoint execution. Proof evaluation occurs in a spawned scratch runtime and cannot mutate the committed root. The proposer collects deployments through unordered containers, so proof production re-imposes the canonical `valid_after_block_number`, `time_stamp`, and signature ordering before grouping. Gate every signed block-body envelope, including proposer heartbeat/dummy deployments. Replay has no origin discriminator, so proposal cannot exempt a signed deployment class. Only rejected user signatures participate in user-deploy storage bookkeeping. The opaque result is consumed by `compute_deploys_checkpoint_cosigned_admitted`, avoiding a second proof evaluation while binding checkpoint inputs to the proof context.

## D2.2 — Structural kernel and production state-bound algorithm
`pub async fn admit_by_funding(deploys: Vec<Cosigned<DeployData>>, supply_reader: &dyn SupplyReader) -> Result<AdmissionOutcome, CasperError>` where `AdmissionOutcome` carries admitted envelopes, rejected signatures, realized-cost reservation debits, and fee debits. `SupplyReader` binds certificates to the merged pre-state root and interprets an absent pool as zero.
1. **Canonicalize** the input by the block_creator.rs:315-324 comparator (verbatim).
2. Per deploy (canonical order) build a `Candidate { deploy, sig_key, demand }`:
   - **Envelope `Sig`** via an EXTRACTED shared helper `Cosigned::envelope_sig` (refactor of mod.rs:890-961 + runtime.rs:1487-1498 into ONE function so gate, runtime-install, and replay never drift): single-signer ⇒ `Sig::Quote(Blake2b256(DEPLOY_SIGNATURE_DOMAIN ‖ sig))`; compound ⇒ left-assoc `Sig::And` fold of `Sig::Quote(Blake2b256(COMPOUND_DEPLOY_SIGNATURE_DOMAIN ‖ sig_i))`. (The envelope `Sig` drives the `deploy_id` basis ONLY; per §D2.9 below, `sig_key` and the supply channel are keyed by `funding_sig` — getting `funding_sig` wrong mis-keys the pool, so it MUST match the runtime install and replay.)
     > **⚠ SUPERSEDED by §D2.9 (the funding-key correction).** Keying the supply pool by `envelope_sig` (the per-deploy **wire signature** hash) was the bug: genesis seeds wallets at `Σ⟦Sig::Ground(pk)⟧` (the **public key**), so `Σ⟦envelope_sig⟧ ≠ Σ⟦wallet⟧` and no deploy ever debited its wallet. The supply pool is now keyed by `funding_sig` (the signer's GROUND public key), so `Σ⟦signer⟧ == Σ⟦wallet⟧`. The `envelope_sig` derivation survives ONLY as the (unchanged) `deploy_id` basis. See §D2.9.
   - **Funding `Sig`** (§D2.9 — the supply/settlement key): `funding_sig` = `Sig::Ground(pk)` (single signer) / the left-associated `Sig::And`-fold of `Sig::Ground(pkᵢ)` over the *filtered* (non-placeholder) signer set (multi). Keyed by the signers' GROUND public keys, so `Σ⟦funding_sig⟧ == Σ⟦Ground(pk)⟧ == Σ⟦wallet⟧` (the genesis seed). `envelope_sig` (above) is now the `deploy_id` basis ONLY; the gate/supply/settlement key below is `funding_sig`.
   - `sig_key = delta_sigma::sig_key(&funding_sig) = funding_sig.lane_hash()`.
   - `Δ_s^max = delta_sigma::demand(&desugar_for_funding(&Compiler::source_to_adt(&deploy.data.term)?), &funding_sig).certified_upper_bound`. Accept only when `demand_bound` supplies a matching proof-bearing finite bound. `unknown` / `Unprovable` is rejected; an economic margin cannot prove boundedness.
   - Malformed term (`source_to_adt` error) ⇒ REJECTED (the runtime would fail it too).
3. Group into `BTreeMap<SigKey, Vec<Candidate>>` (deterministic group iteration; per-group `Vec` preserves canonical order).
4. Build `Decomposition { compound, left, right }` (lane hashes) for each compound `Sig::And` envelope (per internal `And` node for n≥3). `Threshold/Plus/With` ⇒ no decomposition (the runtime forms only `And` today). Post-§D2.9 the `Sig::And` folds over `Sig::Ground(pkᵢ)` leaves, so the component pools are the cosigners' wallets `Σ⟦Ground(pkᵢ)⟧` (which genesis seeds; the combined `Σ⟦And(…)⟧` pool is not seeded — hence the effective-supply fold in step 6).
5. **Per-group supply read ONCE**: `Σ_s = supply::decode_balance_datum(&runtime_manager.get_data(pre_state_hash.clone(), &supply_channel(&funding_sig)).await?)` (the wallet channel `Σ⟦Ground(pk)⟧`). (Use `get_data` — gate has a hash, not a live `RuntimeOps`; same decoder as `read_balance`.) Read each distinct channel (groups + decomposition components) exactly once into `raw: BTreeMap<SigKey,i64>`.
6. **LIVE cross-group residual ledger (§D2.9 TM-CA-165)**: seed `remaining = raw.clone()` and index decompositions by compound (`index_decompositions`). Each group's admission cap is its effective supply read from THIS ledger, which is drawn DOWN as successive groups are admitted — so two DISTINCT cosigner sets sharing a component wallet `Σ⟦Ground(s)⟧` cannot each be admitted against `s`'s full balance (linearity: no contraction). *[Pre-TM-CA-165 this was a single static `effective = effective_supply_with(&raw, &decompositions)` re-read per group — the cross-group over-admission bug, since the shared stack was never decremented across groups.]*
7. **Per-group prefix admission (reject-both) against the LIVE ledger**: for each group in `SigKey` order, compute the effective residual from the live component ledger. Reserve `Δ_s^max + fee` only when the finite certificate verifies and the residual dominates it. On the first non-fitting candidate, reject it and all later candidates in the group. Draw the reservation from the in-memory ledger so later groups cannot double-reserve a shared component. This draw is an admission proof, not the supply mutation; realized settlement is computed after execution.
8. Return; feed `admitted` to execution in canonical order. **No global lock/barrier** — groups are independent.

Those steps define the structural proof kernel and settlement algebra. The
production wrapper `admit_with_state_bound_evidence` first evaluates the
canonical sequence from the authenticated root under finite capacities derived
from the same effective-supply groups. Each completed candidate yields its exact
cost and adjacent pre/post roots. Capacity exhaustion removes the candidate;
exact cost-plus-fee underfunding removes the corresponding group suffix. Because
removal changes the state observed by later deployments, the retained sequence
is evaluated again until neither set changes. Every nonterminal pass strictly
shrinks the sequence, so the fixed point terminates in at most $`n+1`$ passes.

## D2.3 — Realized settlement
`post Σ⟦s⟧ = pre − Σκ_s − fee`, where the completed bounded play derives
`ProcessedDeploy.cost = κ_s`, is retained as the committed transition, and is
independently reproduced by certificate-constrained replay. State-bound admission has exact bound
`B_s = κ_s`; structural or external conservative certificates require
`0 ≤ κ_s ≤ Δ_s^max`. Play and replay both call
`recompute_state_bound_settlement_debits` before the replay-symmetric
`CloseBlockDeploy::dual_write_supply` mutation. The unused reservation
`B_s − κ_s` remains in the wallet.
- **Write:** `old = read_balance(chan); new = old.checked_sub(amount).expect("supply underflow — gate invariant violated"); produce_balance(chan, new, debit_random_state(close_rand, idx))`. Use a **disjoint RNG path** from the mint loop (`supply::debit_random_state`, mirroring the mint's `split_byte(0x2a)`) so mint+debit to a shared channel get distinct datum identities; read-modify-replace keeps the trie root deterministic. Replay adds the `ReplaySupplyMismatch` write-readback guard.
- **Compound-debit deferral (TRACKED FOLLOW-ON → D3/funding-slots):** for compound envelopes the Split/Join credit (`effectiveΣ = Σ_compound + min(Σ_l,Σ_r)`) could exceed the compound's own pool. D2 **debits each lane's OWN channel only** and correspondingly **caps compound admission at `Σ_compound`** (component-pair credit treated as non-spendable in D2) — safe (never admits unfunded / never underflows), conservatively under-live for multi-sig (rare; the runtime forms only `And`). The full multi-pool draw-allocation is a funding-slot mechanism, out of D2's consensus-gate scope. **Single-signer (the only shape the pool carries today, all §7.4 examples) is EXACT: `Σ⟦s⟧ -= ΣΔ_s`.**
  > **⚠ SUPERSEDED (#12 Split/Join + §D2.9).** The deferral above is no longer in force: the Split/Join `effectiveΣ = Σ_compound + min(Σ_l, Σ_r)` is spendable, a compound deployment funds from the matched component pair, and `compute_settlement_debits` settles the multi-pool draw without underflow. Replay keys re-verification on `effectiveΣ`, not raw `Σ_compound` presence. Post-§D2.9 the components are the cosigners' wallets and the multi-sig cost is balanced equally across them (P8).

## D2.4 — Replay determinism: `ReplayAdmissionMismatch`
After reset and before execution, replay reconstructs the complete cosigned envelopes and authority-derived finite capacities from `block.body.deploys` and `start_hash`. Every admitted deployment must be well formed, complete without exhaustion, and be fundable in canonical order against the recomputed residual ledger. A malformed or exhausted admitted envelope is `ReplayAdmissionMismatch`, never zero demand. Replay verifies cost, status, event log, and adjacent post-state roots before recomputing cost-plus-fee settlement. The reject direction relies on the post-state root because rejected bodies are not present in the block. These checks compose with `ReplayCostMismatch` and `ReplaySupplyMismatch` to guard pre-state, execution, and post-state views.

## D2.5 — Ingress price is not a proof input
`CasperShardConf.min_phlo_price` remains the ingress economic floor. It is not passed to `admit_by_funding`, cannot turn a lower bound into a finite upper bound, and does not change the authority inequality. Certified demand and supply share one integer authority unit.

## D2.6 — Proof evaluation is not execution-on-receipt
The spec mandates that rejected deployments make no committed state change. The
state-bound proof satisfies that rule: it starts only during block assembly,
uses a spawned runtime rooted at authenticated state and has finite
authority-derived capacity. Rejected iterations expose no committed transition;
the final completed iteration becomes the block's user transition and settlement
continues from its exact post-state root. It is not an ingress cache or a source
of proposer-only authority. Replay consumes its serialized event witness and
derives the result independently.

## D2.7 — Formal obligations
- **Rocq (D5a already discharges):** `funding_decidable` (Def 19 — `is_funded` is a total decision procedure), `competing_funding_at_most_one_succeeds` (Remark 21 — reject-both/no-double-spend). **NEW:** `admit_prefix_maximal` (the per-group prefix is the largest canonical prefix with cumulative `Δ ≤ Σ`; induction, residual monotone), `reject_both_sound` (corollary), `settlement_conserves`/`accept_commit_conserves` (`post=pre−ΣΔ ∧ ΣΔ=Σ consumed`), `sigma_s_balance_eq_stack_count`+`funding_check_balance_sound` (handoff Decision 8). Register in the heredoc.
- **TLA+:** `EvalScheduling.tla` `AcceptanceGate(group)` + `NoDoubleSpendAtBlock`, `RejectBothOnOversubscription`, `GateBeforeExecute`, `SupplyConservation`, `SupplyOnlyWrittenByMint`; `RuntimeBudgetReplay.tla` `admission_decision_schedule_independent`.
- **TLA+ state-bound refinement:** `StateBoundAdmission.tla` checks proof completion, single-play evidence commitment, constrained replay agreement, finite-capacity funding, schedule-independent cost, exact settlement, and eventual done-or-rejected; three negative controls expose structural ambient undercount, duplicate unconstrained play, and exhausted admission. `StateBoundValidatorConvergence.tla` gives validators local reducer schedules with both different orders and different event/cost traces, then checks strict root/block-context binding, canonical deployment order, exact certified-witness reproduction, and accepted-validator agreement. Its negative controls expose context substitution, unchecked arrival-order execution, and acceptance of scheduler-local execution in place of certificate replay.
- **Rocq state-bound refinement:** `EndToEndAuthority.v` proves capacity/funding equivalence, exhaustion non-certifiability, certificate-funded committed cost, adjacent-root continuity, funded admitted lists, and exact settlement conservation.
- **Sage:** `supply_accounting_model.sage` (mint/admit/settle interleavings: no negative balance, post=pre−ΣΔ, oversubscription⇒reject-both).
- **Sage fixed point:** `settlement_model.sage` exhaustively checks termination, capacity completion, admitted/rejected disjointness, and exact cost-plus-fee funding over three candidates.

## D2.8 — Threat/UC (dedup against DR-13)
TM-CA-153 (double-spend/oversubscription), UC-CA-151 (commit-Δ + replay), UC-CA-152 (settlement post=pre−ΣΔ) already cover D2. Update TM-CA-153 "Blocked by" to cite BOTH the in-pass residual AND `replay_admission_mismatch` (the proposer-side gate is necessary-not-sufficient; replay re-verifies). Note in UC-CA-151 the replay asymmetry (admitted direction re-checked; reject direction via the post-state root).

## D2.9 — Funding key = the signer's GROUND public key (`Σ⟦signer⟧ == Σ⟦wallet⟧`)

**Status:** IMPLEMENTED + verified (this correction supersedes the §D2.2 `envelope_sig` keying). **Consensus-critical** — changes which pool every deploy debits. Spec basis: `cost-accounted-rho.tex` §"signature grammar" (`g` = *"an Ed25519 public key, a secp256k1 key hash"*) + §1485-1495 (per-actor pools) + §1613 (a deploy consumes its signer's pool); corroborated by `typed_value.tex`, `reputation.tex`, `continued-gslt-cost-v2.tex`, `rent_and_shard_splitting.tex`, `proofs-as-processes-continued.tex`; refuted by none. Confirmed directly by Greg + Mike.

### The bug
A deploy's funding pool was keyed by `envelope_sig` = `Sig::Quote(Blake2b256(DOMAIN ‖ wire_sig))` — the per-deploy **wire signature**, a fresh value every deploy. Genesis seeds wallets at `Σ⟦Sig::Ground(pk)⟧` (the signer's **public key**; `close_block_deploy.rs:228/514`). The two channels never coincide (`hash(wire_sig) ≠ pk`), so a deploy's pool was always ABSENT ⇒ the gate's absent-pool branch admitted it unenforced with no debit ⇒ **the wallet was never debited** and the linear `Σ ≥ Δ` proof never bound.

### The fix — decouple `deploy_id` from the funding key
`envelope_sig` is overloaded: it derived BOTH the `deploy_id` (needs per-deploy uniqueness — the wire sig gives that) AND the supply key (needs per-signer identity — the pubkey gives that). Split them:
- **`deploy_id`** stays **wire-sig-derived** (`envelope_sig_single`/`_compound`) — byte-identical to before; on-chain deploy identity never moves.
- **The funding key** becomes **`funding_sig`** = `Sig::Ground(pk)` for single-sig, `Sig::And` fold of `Sig::Ground(pkᵢ)` for multi-sig — keyed by the signers' GROUND public keys, so the pool the gate reads / proves `Σ ≥ Δ` against / debits IS the genesis-seeded wallet `Σ⟦Ground(pk)⟧`.

The runtime install gains `set_deploy_signature_funded(wire_sig, funding_sig)` / `set_deploy_signatures_funded(...)` (`accounting/mod.rs`): `deploy_id` from the wire sig, `self.signature`/signer-channels from `funding_sig`. The legacy `set_deploy_signature(s)` are now thin wrappers passing the wire-sig `envelope_sig*` (so every test/bench caller is byte-identical). `evaluate_cosigned` (`runtime.rs`) derives `funding_sig(cosigned)` and calls the `_funded` variants. The gate (`build_candidate_with_logic`) and the replay recompute (`recompute_settlement_debits_with_logic`) both key by the SINGLE shared `accounting::funding_sig` — the no-drift guarantee; replay reconstructs the full verified cosigner set via `Cosigned::to_cosigned()` ⇒ byte-identical settlement-debit map.

### Security — the placeholder filter (R1-F4)
`funding_sig` excludes empty-`sig` PLACEHOLDER cosigners (the un-signed members of an M-of-N threshold envelope, `from_signed_data_threshold`). Without this, a deploy could key funding to an UNSIGNED victim's pubkey wallet. The FILTERED funder count (not `is_compound()`) drives the funding arity, so a 1-of-2 threshold with one real signer + one placeholder funds ONLY the real signer's wallet. (Ingress `from_proto_cosigned` already verifies every non-placeholder `sig` against its `pk`.)

### Multi-sig (P8-balanced) and the compound recompute fix
A multi-sig deploy funds from the cosigners' wallets `Σ⟦Ground(pkᵢ)⟧`, balanced (each cosigner debited equally — a compound token is a matched pair, one from each pool; ratified P8). Genesis seeds the individual pubkey wallets, not the compound `Σ⟦And(…)⟧` pool, so the compound group funds from `effectiveΣ_compound = Σ⟦compound⟧(absent ⇒ 0) + min(Σ⟦left⟧, Σ⟦right⟧)`.

This exposed a **latent pre-existing bug**: replay keyed compound re-verification on the compound pool's raw presence instead of effective supply. Once component wallets were funded, a compound deploy could be play-admitted by the effective fold but replay-rejected by raw absence. Proposal and replay now run the identical effective-supply fold and canonical cross-group residual ledger; absence contributes zero and never disables enforcement.

### Provisioning
- **Clients:** seed `Σ⟦Ground(client_pk)⟧` via `client_fuel_allocations`; an omitted client has zero effective supply and is rejected.
- **Validator heartbeat/dummy deployments:** pass through the same signed-deployment gate and use the validator's initial-phlogiston wallet.
- **Protocol system deploys:** route through `evaluate_system_source` and their separately verified system transition.

### Tests
`accounting/mod.rs::funding_sig_tests` covers shape and deploy-ID decoupling. `acceptance.rs::tests` covers signer-wallet binding, absent and drained rejection, zero-cost fee gating, balanced multi-sig settlement, placeholder exclusion, malformed replay rejection, and play/replay equality. `runtime_manager_test::gate_decision_replay_determinism` covers the end-to-end seam.

## Impl sequencing + files
1. **Extract `Cosigned::envelope_sig`** (the `deploy_id` basis) **+ derive `funding_sig`** (the supply key — the `_funded` install variants, §D2.9) — both via shared functions used by gate + runtime-install + replay so they never drift; unit-test single-sig `envelope_sig ⇒ Sig::Quote` / `funding_sig ⇒ Sig::Ground(pk)`, 2-signer `⇒ Sig::And`. 2. **`acceptance.rs`** (`admit_by_funding` + `AdmissionOutcome` + `SettlementDebit`; tests `per_signature_group_gate`, `reject_both_on_oversubscription`, §7.4 boundary). 3. **Wire into `block_creator.rs::create`** (gate user_deploys after pre-state; union gate-rejected; populate `settlement_debits`; post-gate empty-block-skip). 4. **`CloseBlockDeploy` debit** (field + `dual_write_supply` debit loop + `supply::debit_random_state`; play threads, replay recomputes). 5. **`ReplayAdmissionMismatch`** in `replay_failure.rs`. 6. **`replay_runtime.rs::replay_deploys`** recompute + `replay_admission_mismatch` + feed debit map (test `gate_decision_replay_determinism`). 7. **Margin** = `min_phlo_price` (no new param). 8. **Formal** (Rocq `admit_prefix_maximal`/`reject_both_sound`/`settlement_conserves`; TLA+; Sage; register in proof script). 9. **Doc deltas**.

**Critical files:** `block_creator.rs` (gate seam ~:790, union rejected, populate debits), `costacc/close_block_deploy.rs` (debit loop in `dual_write_supply` :76, symmetric :207/:232), `rholang/replay_runtime.rs` (`replay_admission_mismatch` + debit recompute in `replay_deploys` :111), `accounting/delta_sigma.rs` (consumed read-only), `util/rholang/supply.rs` (read/debit + add `debit_random_state`), NEW `util/rholang/acceptance.rs`.

**Risks (mitigated):** non-deterministic gate (pure analyzer + BTreeMap + canonical re-sort + proof-bearing reservation + replay recompute); HashSet nondeterminism (re-sort); envelope-Sig drift (one extracted function); debit underflow (compound single-pool cap); play/replay debit asymmetry (replay recomputes the same map).

## Tracked follow-ons (NOT consensus-critical, explicitly scoped out of D2)
- **D2-perf:** speculative execution-on-receipt + `committed` I/O gate (latency optimization; gate is spec-conformant without it).
- **D2→D3 compound multi-pool debit:** full Split/Join spend drawing from component pairs (D2 caps compounds at their own pool; safe + single-signer-exact).
