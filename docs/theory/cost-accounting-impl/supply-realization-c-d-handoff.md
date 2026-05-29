# Per-Signature Token-Supply Realization (C↔D handoff)

**Status:** Authoritative design (grounded against `feature/cost-accounted-rho @ b670d3fb`). **This document is the binding contract for the C-StageA/B (supply producer) and WD-D1/D2 (supply consumer) implementation agents.** Where it conflicts with `workstream-c-economic.md` / `workstream-d-acceptance.md`, this document and DR-13 govern (those docs have been reconciled to match). Spec (LAW): `publications/cost-accounting/cost-accounted-rho.tex`.

**Verdict (hypotheses tested):** channel = `from_sig(s)` (**confirmed**); balance representation (**confirmed and strengthened** — the runtime already uses `Token::Count{sig, remaining:u64}`, accounting/mod.rs:1156-1164); the consensus decrement is the **acceptance commit at block assembly**, not a runtime per-COMM RSpace mutation (the "decrement at reduction" hypothesis is **refuted** by spec tex 1677-1729).

---

## Decision 1 — Channel: `Σ⟦s⟧ = from_sig(s)` is THE single canonical signature→name map

`Σ⟦s⟧ ≜ SignatureChannel::from_sig(s).par` (accounting/mod.rs:1237-1255) is the one map used identically by (a) the Appendix-A calculus translation, (b) the supply PRODUCER (C), and (c) the WD-D2 CONSUMER. No second convention.

- **Spec basis.** Appendix A `Σ⟦g⟧ = quote(H_g)` (tex 1849-1869, Remark 1877-1884); `Σ⟦s₁∘s₂⟧ = quote(⌊Σ⟦s₁⟧⌋ | ⌊Σ⟦s₂⟧⌋)`; `K⟦s:S⟧ = send(Σ⟦s⟧, K⟦S⟧)` (tex 1915-1918); funding-slot deposit `Σ⟦slot⟧!(…)` (tex 1147-1151).
- **Code.** `from_sig(Ground(b)|Quote(b)) = Par{unforgeables:[GPrivate{id: Blake2b256(b)}]}` (the g/#P axis collapses at the channel — equal bytes ⇒ equal channel, DR-1); `And/Threshold/Plus/With` → `concatenate_pars` + canonical `ParSortMatcher::sort_match` (mod.rs:1256-1311), permutation-invariant per the spec's reflection-of-parallel-composition.
- **Security mechanism (load-bearing).** `GPrivate` ids are minted only by `new` from the per-deploy RNG, and FixedChannels are hardcoded URIs — **there is no bytes→GPrivate surface primitive**. The lone `to_par(UnforgPrivate{…})` (web_api.rs ~1593) is a read-only query builder for `getDataAtName`, not an in-language name injector. So **no Rholang process — including VB — can name `Σ⟦Ground(pk)⟧`**. The only writer is Rust constructing that exact `Par` and `produce`-ing to it, behind the `GSysAuthToken` root (system_processes.rs:709-714; `mk_sys_auth_token`, system_deploy.rs:55). This realizes the spec's unforgeability requirement (Remark 14, tex 1164-1172) at the substrate.
- **Rejected runner-up:** a FixedChannel `rho:phlo:supply:<pkhex>` — nameable in-language ⇒ forgeable supply ⇒ violates guard #3.

## Decision 2 — Representation: a single balance-carrying datum (not literal nested-send messages)

Supply on `Σ⟦s⟧` is **one** datum `Σ⟦s⟧!( (TOKEN_TAG, n) )`, `n: Long` = the count of available `s`-layers; `TOKEN_TAG` is a fixed genesis-scoped discriminator. `supply(s) = n` (0 if absent).

- **(i) Fidelity.** Def 17 (tex 1574-1582) defines `Σ_s` as a layer COUNT (`Σ_s(())=0`, `Σ_s(T|U)=Σ_s(T)+Σ_s(U)`); the funding obligation is the single integer inequality `Σ_s ≥ Δ_s` (tex 1590, 1613). The §B.1 decomposition equivalence (`s:s:s:() ≡ s:()|s:()|s:()`, tex 2394+) makes only the count observable; the nested-send chain (tex 2845-2849) is one encoding of the count, a balance `n` is an equivalent one. The runtime already coalesces stacks to a balance (`Token::Count{sig,remaining}`, mod.rs:1156-1164, via `Token::coalesced`) — the balance is the runtime's own normal form. Discharged by `sigma_s_balance_eq_stack_count` (Decision 8).
- **(ii) Performance.** Literal messages make `supply(s)` O(n) per gate read per block (materialize all `Par`s via `get_data`) and O(n) inserts/merge on the epoch mint — at n≈10³–10⁶/validator this is a per-block bottleneck (violates extension-guard #2). The balance datum is O(1) read, one `produce` per mint, one datum to merge.
- **(iii) Units.** Δ_s = layer count (`i64`, small); Σ_s = the balance `n` (same unit); token-per-COMM (DR-9) = one `BillableTokenEvent{kind:SourceStep, weight:1}` per rendezvous (mod.rs:162,168-174). Gate compares `cumulative Δ_s ≤ effectiveΣ_s` — one `i64` comparison, identical units (Def funding-obligation tex 1590; §7.6 step 4 tex 1635).
- **Compound signatures.** `effectiveΣ_{s₁∘s₂} = Σ_{s₁∘s₂} + min(Σ_{s₁},Σ_{s₂})` reads the compound channel `from_sig(And(..))` AND each component `from_sig(s_i)` — each a separate O(1) balance, preserving Appendix A eq:app-st-signed-compound (tex 1971-1983).

## Decision 3 — Producer: Rust mint writes the balance directly to `from_sig(Ground(pk))`; `@W_v`/`VB` retained as the spec's draw/halt structure (DISTINCT channels)

The seam that previously caused confusion: **`@W_v` (the validator's *draw* channel, spec eq:38) is NOT `Σ⟦v⟧` (the *supply pool* the gate reads, spec fuel gate `for(t←Σ⟦v⟧){…}`, tex 1953-1958).** They are different channels with different roles.

1. **Authority + amount stay in the `sysAuthToken`-gated path.** `mintPhlogiston(@validatorPk, @amount, @sysAuthToken, return)` first does `sysAuthTokenOps!("check", *sysAuthToken, *ok)` (true iff `GSysAuthToken`, system_processes.rs:709-714) — reachable only from Rust system deploys (epoch/bond mint) that always hold `pk`.
2. **The `Σ⟦v⟧` supply write is a Rust `produce`** co-located in the same system deploy: compute `chan = from_sig(&Sig::Ground(pk_bytes)).par`, read `old_n` (0 if absent), `new_n = old_n + amount` (epoch-keyed idempotent), `produce` the single datum `(TOKEN_TAG, new_n)`. Rholang never names `Σ⟦v⟧` (Decision 1) ⇒ unforgeable supply.
3. **`@W_v` and `VB ≜ for(phlo<-@W_v){VH | *phlo}`** (tex eq:39/35) are installed verbatim by C-StageA as (i) the DR-3 halt mechanism (drain `@W_v` ⇒ `VB` blocks ⇒ validator offline, tex 3030-3036) and (ii) the spec-faithful structural anchor — but they are **not on the D acceptance read path** (the gate is lifted to static block-assembly analysis per Remark 11 / DR-11).
- **Rejected:** (a) Rust-injected free name `@sigSupplyCh` into VB's continuation — re-exposes `Σ⟦v⟧` to Rholang, enlarges trusted surface (guard #3). (c) a `sigChannelOps` system process — extra attack surface for zero benefit (the only authorized caller is Rust, which calls `from_sig` directly); recorded as a future refinement for in-Rholang minting contracts.

**Producer shapes** — Rust (C-StageB epoch/bond mint deploy, sibling to slash_deploy.rs):
```
let chan: Par = SignatureChannel::from_sig(&Sig::Ground(pk_bytes.clone())).par;
let old_n: i64 = read_balance(pre_state, &chan);                       // decode (TOKEN_TAG, n); 0 if absent
let new_n: i64 = old_n.checked_add(amount).expect("supply overflow"); // epoch-keyed idempotent
produce_balance(&chan, TOKEN_TAG, new_n);                             // single datum, replace
```
Rholang `mintPhlogiston` keeps the authorization shape + `@W_v` bookkeeping; the `Σ⟦v⟧` write is the Rust step (it cannot name `Σ⟦v⟧`). `read_balance` is the SAME decoder WD-D1 `supply()` uses (shared helper, Decision 5).

## Decision 4 — Consumer (WD-D2): read balance from merged pre-state; decrement = acceptance commit at block assembly

- **(a) Read.** `supply(s) -> i64` = `RuntimeManager::get_data(merged_pre_state_hash, &from_sig(s).par)` (runtime_manager.rs:969) → decode the single `(TOKEN_TAG, n)` → `n` (0 if empty). This is §7.6 step 3 "compute `Σ_c` from the available token stack" (tex 1633-1634).
- **(b) Decrement = acceptance commit (DECISIVE).** Spec tex 1677-1687 (duplicate deploy): deploy 1 accepted ⇒ "three tokens **committed**"; deploy 2 sees "`Σ_Alice = 0` (already committed to the first)" ⇒ rejected. Acceptance "precedes all execution… no interleaving" (tex 1726-1729). In `admit_by_funding` (`prepare_user_deploys`, casper/src/rust/blocks/proposer/block_creator.rs): read `Σ_s` once per group; maintain an **in-pass residual** `residual_s = effectiveΣ_s`; walk the group in canonical order (block_creator.rs:315-324), admit-and-`residual_s -= Δ_s` while `Δ_s ≤ residual_s`, reject the first that doesn't fit and all after it (§7.7 reject-both, tex 1696-1712). No RSpace write per deploy.
- **(c) Single consensus decrement / no double-count.** Three views of one quantity: pre-state balance (read), in-pass residual (acceptance bookkeeping), post-state balance (settlement). They must agree: `post_balance(s) = pre_balance(s) − Σ_{admitted} Δ_s`, written by the SAME Rust `produce_balance` (debit) — never an in-language write. The per-COMM `weight:1` events (DR-9) are diagnostic and are the mechanism by which `reconcile()` arrives at `consumed = Δ_s`; they do not independently mutate the balance. **Exactly one consensus decrement: the settlement debit = admitted `Δ_s`.**
- **(d) Gate-before-speculate (DR-11).** O(AST) gate at assembly; only admitted deploys execute/settle; execution-on-receipt is speculative to a soft checkpoint (runtime.rs `create_soft_checkpoint`/`revert_to_soft_checkpoint`) that never feeds acceptance/commit; I/O gated on `committed`. The per-channel reducer lock (rspace.rs) is orthogonal (data channels, not `Σ⟦s⟧`). Disjoint signatures ⇒ disjoint `Σ⟦s⟧` ⇒ zero gate contention (§7.6).

## Decision 5 — Replay-determinism

- **Write:** `from_sig(Ground(pk))` is pure (Blake2b256 + canonical sort); `amount` is a genesis constant or a deterministic `allBonds` epoch fold; epoch-key idempotency ⇒ replayed mint is a no-op (`MintingInjection.v`).
- **Read:** `get_data(merged_pre_state_hash, …)` is deterministic (merged pre-state hash is already a consensus quantity — cf. `compute_parents_post_state_regression_spec.rs`); decode is pure.
- **Decrement:** `admit_by_funding` is a pure fold (pure `Δ_s`, `BTreeMap` groups, canonical order, genesis `margin`).
- **Replay check:** add `ReplayFailure::ReplayAdmissionMismatch` (casper/src/rust/util/rholang/replay_failure.rs, sibling of `ReplayCostMismatch`) and `replay_admission_mismatch` (replay_runtime.rs ~442) — recompute `admit_by_funding` against the same merged pre-state hash + genesis constants; assert admitted/rejected match. Existing `replay_cost_mismatch` continues to guard `total_cost == consumed == Δ_s` (the gate↔runtime bridge).

## Decision 6 — Security / threat model

| Attack | Blocked by |
|---|---|
| Forge supply (write `Σ⟦s⟧` from Rholang) | D1: no bytes→GPrivate primitive; only Rust `produce_balance` writes, only on the `sysAuthToken` path |
| Name/drain another validator's `Σ⟦v'⟧` | D1: unforgeable channel; Rust writer computes `from_sig` only for the authorized `pk` (allBonds fold) |
| Mint replay / multi-parent double-credit | D3/D5: epoch-key idempotency; `MintingInjection.v` idempotency; content-addressed Produce |
| Double-spend `Σ_s` across deploys in a block | D4(b): in-pass residual + reject-both (§7.7) |
| Forge `sysAuthToken` | system_processes.rs:709-714 accepts only `GSysAuthToken` (no in-language constructor) |
| Non-deterministic gate ⇒ fork | D5: pure analyzer + genesis margin + canonical order + replay recompute |
| Speculative I/O leak / pre-gate commit | DR-11/D4(d): soft checkpoint, I/O gated on `committed` |
| `TOKEN_TAG` confusion | D2: fixed tag; only Rust writes the channel |

**New rows (next-free; live max TM-CA-151 / UC-CA-149):** TM-CA-152 (forge supply — protected), TM-CA-153 (double-spend/oversubscription — protected), TM-CA-154 (mint replay — protected); UC-CA-150 (balance datum O(1), 0 when absent), UC-CA-151 (commit `Δ_s` + replay match), UC-CA-152 (settlement `post=pre−ΣΔ` = runtime consumed).

## Decision 7 — Spec-conformance verdict

**FAITHFUL** to §4.6 / Appendix A / §7, with two **guarded encoding choices**: (1) balance datum vs literal sends — guard #1 `sigma_s_balance_eq_stack_count`, #2 O(1) vs O(n), #3 fewer messages + single Rust writer; (2) acceptance-time commit vs runtime per-COMM peeling — guard #1 the spec mandates acceptance-time commit (tex 1677-1729) + s₀-collapse lifts the gate to static analysis (Remark 11 / DR-11), #2 avoids per-rendezvous writes, #3 keeps `Σ⟦s⟧` unwritable from the reducer. Neither is a deviation.

## Decision 8 — Formal obligations (register every new lemma in scripts/check-cost-accounted-rho-proofs.sh)

- **`LinearLogicResources.v` (WD-D5a):** pure `delta_s` + `funding_decidable` (tex 1599) + `delta_s_tensor_additive`; reuse `ll_no_double_spend_single_witness` (:359) for "≤1 of competing proofs succeeds" (tex 1731-1741). **New:** `sigma_s_balance_eq_stack_count` (the balance `n` = `Σ_s` of a depth-`n` stack — the fidelity lemma), `funding_check_balance_sound` (`is_funded` over the balance ⇔ `Σ_s ≥ Δ_s`).
- **`MintingInjection.v` (C):** `supply_write_injective_in_pk` (`from_sig∘Ground` injective ⇒ disjoint pools), `epoch_mint_idempotent_on_balance`, `halted_validator_supply_not_increased` (ties to `MintingHalt.v`, DR-3).
- **`TokenConservation.v` (C↔D bridge):** `accept_commit_conserves` (`post = pre − ΣΔ_admitted` ∧ `ΣΔ_admitted = Σ reconcile.consumed`).
- **`RuntimeBudgetRefinement.v` (WD-D0):** `rb_pool` / `rb_pool_total_cost = Σ rb_total_cost` (the supply balances seed per-lane `initial_tokens`).
- **`ChannelSeparation.v` (WD-D0):** `lane_pool_disjoint` (corollary of `fuel_gate_no_app_channel_overlap` :179).
- **`Settlement.v` / `MultiSignerRefinement.v` (D4.1):** reinterpret `pos_charge`/`pos_refund` as wallet-draw/commit over balances; keep distinctness/FIFO lemmas verbatim (`fifo_drain_conservation` → `Σ released + Σ committed = Σ reserved`).

**TLA+:** `EvalScheduling.tla` — `AcceptanceGate(group)` over a per-signature `supply` var; invariants `NoDoubleSpendAtBlock`, `RejectBothOnOversubscription`, `GateBeforeExecute`, **new** `SupplyConservation` (`post = pre − ΣΔ`), `SupplyOnlyWrittenByMint`. `RuntimeBudgetReplay.tla` — `admission_decision_schedule_independent` (mirror `ConsumedAndVerdictScheduleIndependent` :503).

**Sage:** new `supply_accounting_model.sage` — adversarial search over (mint, admit, settle) interleavings: no negative balance, no double-credit under merge, `post=pre−ΣΔ`, oversubscription ⇒ reject-both.

## Coordination / non-blockers

- **No true blockers for the supply seam.** `$$posPubKey$$` (PoS.rhox:149) is **NOT** a parse blocker — it is inside a Rholang string literal `createUnfVault!(*posVaultCh, "$$posPubKey$$")`; template substitution is plain `str::replace("$$key$$", val)` (compile_rholang_source.rs:55-58), and since `posPubKey` is absent from the `pos_generator` macro list (standard_deploys.rs:241-270) the literal survives and **still parses**. It is a pre-existing latent correctness bug (the vault human-control key is the literal string), orthogonal to this seam; fix when wiring `@W_v`.
- **Ordering:** B core → C economic → D acceptance. C's supply write must land before WD-D2's `supply()` is non-trivially testable.
- **Integration invariant:** WD-D0's `Sig::lane_hash` must share one function with `from_sig`'s channel-keying basis so the lane key and the supply channel agree (avoid drift — make them one function).
