# Stage B Minting + Stage C Halt Interface — Authoritative Design

**Status:** Authoritative design (grounded against `feature/cost-accounted-rho @ 53bcc16e`; spec law `publications/cost-accounting/cost-accounted-rho.tex`). **Binding contract for the C-Stage-B implementation agent and the Stage-C halt interface.** Builds on DR-3, DR-13, [supply-realization-c-d-handoff.md](supply-realization-c-d-handoff.md), and Stage A (`5f4a235b`). Supersedes the Stage B/C sketch in [workstream-c-economic.md](workstream-c-economic.md) where they differ.

**Foundational fact (verified):** there is **no** `produce_balance`/`read_balance`/`TOKEN_TAG` in the tree yet (only doc mentions). The RSpace write primitive that exists is `DebruijnInterpreter::produce` (reduce.rs:325), reachable on a live system runtime as `runtime_ops.runtime.reducer.space.produce(chan, ListParWithRandom, persistent)`. `SignatureChannel::from_sig` exists (accounting/mod.rs:1544). Stage B **creates** the supply read/write helpers and co-locates them with the close-block system deploy.

## Decision 1 — Per-validator economic state + dual-credit

Two distinct channels, written by ONE authorized deploy:

| Channel | Representation | Writer | Reader | Role |
|---|---|---|---|---|
| `@W_v := @(*walletTag, validatorPk)` | a `MakeMint` purse of `amount` (Rholang message) | Rholang `mintPhlogiston` (sysAuthToken-gated) | `VB ≜ for(phlo<=@W_v){*phlo}` | DR-3 halt (drain ⇒ VB blocks ⇒ offline) + spec anchor |
| `Σ⟦v⟧ := from_sig(Sig::Ground(pk)).par` | single balance datum `(TOKEN_TAG, n)` | **Rust `produce_balance`** (same sysAuthToken-bearing deploy) | WD-D2 gate (O(1), block assembly) + `read_balance` | the quantity the gate commits `Δ_s` against |

**KEEP the `MakeMint` purse on `@W_v`** (StageA shipped it; do not simplify to a presence-handle). Rationale: spec App.B models the wallet as a message carrying a token stack (tex 2388-2391), `*phlo` releases the *stack* (tex 2680-2683); the DR-3 halt is drain-based (consume the purse ⇒ VB re-blocks, tex 3036-3038); the amount on `@W_v` is not consensus-load-bearing (the gate never reads `@W_v`), so carrying it is free. *Runner-up (presence-handle): rejected.*

**Invariant I-DUAL:** the `amount` deposited as the `@W_v` purse equals the `amount` added to the `Σ⟦v⟧` balance, in the same deploy. The spec flow `mint→@W_v→VB draws→*phlo releases stack onto Σ⟦s⟧` is realized as: the Rholang half (`mint→@W_v→VB`) kept verbatim as free infrastructure (tex 2701-2705: the bootstrap is *not* a `ca_step`); the `*phlo`-releases-onto-`Σ⟦s⟧` half is the Rust `produce_balance` (because `Σ⟦v⟧` is unnameable in Rholang — handoff Decision 1). Both writes derive from the same `amount` literal; asserted by `MintingInjection.v`.

## Decision 2 — Bond vs epoch mint; the one-deploy dual-write

- **B.1a confirmed:** `bond` (PoS.rhox:330) is a USER deploy (no `sysAuthToken`) ⇒ cannot mint inline. StageA already made `bond` record stake + install `VB` (empty `@W_v` ⇒ DR-3 halt until funded). Minting happens on the next authorized `closeBlock`.
- **`closeBlock`** (PoS.rhox:731, sysAuthToken-gated): non-epoch branch (`blockNumber % epochLength != 0`) currently a no-op; epoch branch a `runMVar` state pipeline (`commitCurrentEpochRewards`→…→`pickActiveValidators`→`stateUpdateCh`).
- **DECISION:** minting lives in `closeBlock`, folding over `allBonds`, mint to `pk` iff `active ∧ ¬mintingHalted ∧ ¬mintedEpochs.contains((pk, epochIndex))`, `epochIndex = blockNumber / epochLength`. Steady epoch path mints `epochPhlogiston` (newly-bonded validators get their first mint here — the catch-up is the same loop). The **genesis bonded set** is funded `initialPhlogiston` on **block 1** via the same fold run on the non-epoch branch at `epochIndex=0` (factor a shared `mintPhlogistonToValidators(@state,@amount,@epochIdx,ret)`; lift the non-epoch branch to the `runMVar`/`stateUpdateCh` pattern). *Runner-up (separate newly-bonded set + initialPhlogiston for catch-up): rejected — spec draws no such distinction (tex 2365-2367).*
- **Co-location seam (DECISIVE):** add a default-no-op `async fn post_eval(&self, runtime_ops, block_data, pre_state_hash)` to `SystemDeployTrait`; `CloseBlockDeploy::post_eval` performs the `Σ⟦v⟧` writes. Invoke it in `play_system_deploy` (runtime.rs ~1015-1018, between `play_system_deploy_internal` and `create_checkpoint`) and in `replay_block_system_deploy`'s CloseBlock branch (replay_runtime.rs ~529) — symmetric play/replay, same live runtime, so the writes land in the checkpointed state. *Runner-up (inline behind `downcast_ref::<CloseBlockDeploy>`): rejected — duplicates logic across play/replay.* `post_eval` recomputes the mint set **independently in Rust** from the pre-state (same predicate + amount as the Rholang fold), then per `pk`: `chan=from_sig(Ground(pk)); old=read_balance(chan); produce_balance(chan, old.checked_add(amount).expect("overflow"))`.

## Decision 3 — Idempotency under multi-parent merge

**`"mintedEpochs": Set[(Pk, Int)]`** in PoS state (per-validator-per-epoch; NOT `Set[Int]`, which is too coarse for bond-catch-up). Genesis-init `{}`. *Runner-up `lastMintedEpoch: Map[Pk,Int]` noted.* A replayed/duplicated epoch mint is a no-op on BOTH channels: the Rholang fold checks `¬mintedEpochs.contains((pk,epochIndex))` before `mintPhlogiston` (no second `@W_v` purse); the Rust `post_eval` recomputes the same predicate (guarded-out ⇒ no `produce_balance`). `produce_balance` is read-modify-**replace** of a single datum (not append), so even an accidental re-exec over-writes to the same `new_n`. Distinct validators' channels are content-addressed-disjoint (`WalletNaming.v` injectivity). Lemma `epoch_mint_idempotent_on_balance`: `In (v,e) (minted_epochs st) → balance_of (epoch_mint st v e amt) v = balance_of st v`.

## Decision 4 — Halt interface (Stage C implements; Stage B exposes)

**Slash zeroes/halts via the `@W_v` drain + the `mintingHalted` flag + a `Σ⟦v⟧` zero.** The halt is a *liveness* halt (drain `@W_v` ⇒ VB blocks ⇒ validator can't propose ⇒ never runs its own gate); `mintingHalted` enforces it across epochs (the Stage-B fold skips `v ∈ mintingHalted`). **DECISION: slash ALSO zeros `Σ⟦v⟧`** (the slash deploy's Rust `post_eval` writes `produce_balance(from_sig(Ground(pk)), 0)`) — the spec-complete realization of "all remaining phlogiston is removed" (tex 3030-3033), cheap, and it eliminates the residual-funding edge case. *Runner-up (VB-block + `mintingHalted` only, no `Σ⟦v⟧` zero): sufficient for consensus safety, rejected as the interface default.*

| Quantity | `slash(v)` | Mechanism |
|---|---|---|
| `@W_v` | drain the purse (VB re-blocks) | Rholang `for(purse <- @W_v){Nil}`, guarded by `¬mintingHalted.contains(v)` (first slash only) |
| `Σ⟦v⟧` | zero | Rust `post_eval`: `produce_balance(from_sig(Ground(pk)), 0)` (idempotent) |
| `mintingHalted` | insert `v` | Rholang `state.set("mintingHalted", …add(v))` (idempotent) |
| stake/quarantine | bond→0, remove active, quarantine | Stage C (workstream-c-economic.md) |

**`redeemSlashed(@validatorPk,@outcome,@sysAuthToken,return)`** (Stage C; sysAuthToken + PoS-multisig per DR-7): Vindicated/Guilty → remove `v` from `mintingHalted` + remove stale `mintedEpochs (v,e≥current)`; Burned → keep halted. **Redemption writes NEITHER `Σ⟦v⟧` NOR `@W_v` directly** — it clears `mintingHalted` and lets the *normal next-epoch mint* re-fund (tex 2380-2383), keeping ALL phlogiston creation on the single authorized path. `MintingHalt.v` `halted_validator_supply_not_increased` proves nothing is credited while halted.

## Decision 5 — Rust `produce_balance`/`read_balance` mechanism

New module `casper/src/rust/util/rholang/supply.rs`:
```rust
pub const TOKEN_TAG: &str = "phlo";
pub fn supply_channel(sig: &Sig) -> Par { SignatureChannel::from_sig(sig).par }  // the ONE channel-keying fn
pub fn decode_balance_datum(data: &[Par]) -> i64;                                // pure; find (GString(TOKEN_TAG), GInt(n)); 0 if none
pub async fn read_balance(runtime_ops: &RuntimeOps, chan: &Par) -> i64;          // get_data_par then decode
pub async fn produce_balance(runtime_ops: &mut RuntimeOps, chan: &Par, n: i64);  // consume existing datum, produce (TOKEN_TAG, n)
```
- **Single-datum invariant:** `produce_balance` first consumes any existing balance datum, then produces the new one (channel holds exactly one). Read-modify-replace; `checked_add` overflow → `.expect("phlogiston supply overflow")`.
- **Shared decoder:** WD-D1's `supply(s)` MUST call `supply::read_balance` / `decode_balance_datum` (not re-decode) — handoff Decision 5.
- **Authority:** Rust-only, riding the close-block deploy's `mk_sys_auth_token` (no Rholang-reachable path) — handoff Decision 3.
- **Integration invariant:** `supply_channel(s) == s.lane_pool channel` (WD-D0's `Sig::lane_pool`, accounting/mod.rs:1450) — same `from_sig` basis. Add test `supply_channel_equals_lane_pool_channel`.
- **`random_state`** for the produce: derived deterministically from the close-block deploy's `rand()` advanced per validator (byte-identical play/replay).

## Decision 6 — Replay-determinism

Channel (`from_sig` pure), amount (genesis const), validator set + predicate (deterministic pre-state read + sorted `allBonds` fold), `epochIndex` (pure of block number), produce `random_state` (deterministic) are all replay-stable; `post_eval` runs symmetrically on play+replay. **`generate_epoch_mint_deploy_random_seed` (StageA) is now DORMANT** (minting folds into `closeBlock`, not a standalone deploy) — harmless, retained for a future slush-grant deploy. New **`ReplaySupplyMismatch`** in `replay_failure.rs` (sibling of `ReplayCostMismatch`): assert each minted validator's recomputed `new_n` matches the play-time post-state balance.

## Decision 7 — Formal obligations

- **`MintingInjection.v`** (extend): `supply_write_injective_in_pk` (`from_sig∘Ground` injective ⇒ disjoint pools; reuse `WalletNaming.v`'s blake2b injectivity), `epoch_mint_idempotent_on_balance` (Decision 3), `user_ca_step_does_not_increase_balance` (corollary of existing `user_ca_step_does_not_mint` at the balance layer).
- **`MintingHalt.v`** (NEW): `halted_validator_supply_not_increased` + `halted_validator_not_minted`. Keep **independent of `MainTheorem.v`** (G-coordination). Stage B touches NO slashing-tree files ⇒ safe to land now regardless of G (which is settled, `52f488a3`).
- Register all in `_CoqProject` + the `Print Assumptions` heredoc (`scripts/check-cost-accounted-rho-proofs.sh`, add `MintingHalt`). Zero admits/axioms.
- **TLA+:** `EvalScheduling.tla` `SupplyOnlyIncreasedByMint`, `HaltedValidatorSupplyNonIncreasing`; `SlashFlow.tla` `mintingHalted` var + `EpochMint` action + `Inv_HaltedNotMinted` + `Inv_NoDoubleCreditUnderMerge`.
- **Sage:** new `supply_accounting_model.sage` — no negative balance, no double-credit under merge, `post=pre−ΣΔ`.

## Decision 8 — Threat/UC rows (dedup against DR-13)

DR-13 already covers mint-replay/double-credit (**TM-CA-154**) and balance/commit/settlement (UC-CA-150/151/152) — do NOT duplicate. New: **TM-CA-155** (unauthorized mint — sysAuthToken gate + Rust-only writer), **TM-CA-156** (halted-validator residual-supply funding — slash zeros `Σ⟦v⟧` + `mintingHalted`), **TM-CA-157** (redemption double-credit/unauthorized — supersedes the stale "TM-CA-152 unauthorized redemption" label in workstream-c). **UC-CA-153** (epoch mint funds active validators), **UC-CA-154** (bond-then-first-close + genesis block-1 funding). Rows in `cost-accounting-threat-model.md` / `cost-accounting-use-cases.md` (no TODO markers — CI-gated).

## Decision 9 — Doc deltas

- **workstream-c-economic.md Stage B:** dual-write (Rholang `mintPhlogiston`→`@W_v`; Rust `CloseBlockDeploy::post_eval`→`Σ⟦v⟧`); `mintedEpochs: Set[(Pk,Int)]`; genesis block-1 `initialPhlogiston` path; note `generate_epoch_mint_deploy_random_seed` dormant. **Stage C halt interface:** slash adds `Σ⟦v⟧`-zero + `mintingHalted`; redeem clears flag + stale epochs, no direct restore; supersede stale TM/UC labels (→ TM-CA-157, UC-CA-153/154).
- **supply-realization-c-d-handoff.md:** producer = `CloseBlockDeploy::post_eval` (not a standalone slash_deploy.rs sibling); helpers in `supply.rs`; `TOKEN_TAG="phlo"`; shared symbols `supply::read_balance`/`decode_balance_datum`.
- **decision-records.md:** no new DR; add a DR-3 sub-bullet (slash zeros `Σ⟦v⟧`) + a DR-13 note (producer seam = `CloseBlockDeploy::post_eval`, helper `supply.rs`).

## Implementation sequencing
1. `supply.rs` (TOKEN_TAG, supply_channel, read_balance/decode_balance_datum, produce_balance) + `supply_channel_equals_lane_pool_channel` test; register in `mod.rs`.
2. `post_eval` (default no-op) on `SystemDeployTrait`; call in `play_system_deploy` + `replay_block_system_deploy` CloseBlock branch.
3. `CloseBlockDeploy::post_eval` (recompute mint set, dual-write `Σ⟦v⟧`).
4. Rholang `closeBlock`: add `mintedEpochs:{}` + `mintingHalted:{}` genesis-init; the `mintEpochPhlogiston` fold (epoch branch) + genesis block-1 `initialPhlogiston` path; lift non-epoch branch to `runMVar`.
5. `ReplaySupplyMismatch` in `replay_failure.rs`.
6. Proofs: extend `MintingInjection.v`, new `MintingHalt.v`, `_CoqProject` + heredoc.
7. TLA+/Sage/threat-UC/doc deltas.
8. (Stage C, later) consumes `mintingHalted` + `supply::produce_balance` for slash's `Σ⟦v⟧`-zero and `redeemSlashed`.

## Critical files
- `casper/src/main/resources/PoS.rhox` (closeBlock 731-879, mintPhlogiston 464-507, bond 330-385, state-init 198-214, slash 646-729)
- `casper/src/rust/rholang/runtime.rs` (play_system_deploy 1005-1066 — post_eval call site)
- `casper/src/rust/rholang/replay_runtime.rs` (replay_block_system_deploy CloseBlock 517-547 — symmetric post_eval)
- `rholang/src/rust/interpreter/accounting/mod.rs` (from_sig 1544-1562, Sig::lane_pool 1450 — shared channel basis)
- `formal/rocq/cost_accounted_rho/theories/MintingInjection.v` (extend; MintingHalt.v new alongside)
