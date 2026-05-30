// See casper/src/main/scala/coop/rchain/casper/util/rholang/costacc/CloseBlockDeploy.scala

use std::collections::HashMap;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{GPrivate, GUnforgeable, Par};
use models::rust::block::state_hash::StateHash;
use rholang::rust::interpreter::accounting::Sig;
use rholang::rust::interpreter::rho_type::{RhoBoolean, RhoByteArray, RhoList, RhoNil, RhoNumber, RhoString, RhoTuple2};
use rholang::rust::interpreter::system_processes::BlockData;
use rspace_plus_plus::rspace::history::Either;

use crate::rust::errors::CasperError;
use crate::rust::rholang::runtime::RuntimeOps;
use crate::rust::util::rholang::replay_failure::ReplayFailure;
use crate::rust::util::rholang::supply::{self, supply_channel};
use crate::rust::util::rholang::system_deploy::SystemDeployTrait;
use crate::rust::util::rholang::system_deploy_user_error::SystemDeployUserError;

/// Env-channel key under which `closeBlock` publishes the authoritative
/// per-validator mint list `[(pk, amount)]` for the current block's epoch /
/// genesis-block-1 mint (Cost-Accounted Rho, Stage B). The channel is a
/// deploy-RNG-derived `GPrivate` (Rust-constructed, so Rust knows it exactly;
/// user-Rholang-unforgeable — no bytes→GPrivate surface primitive, DR-13
/// security), passed into the close-block source via [`CloseBlockDeploy::env`].
/// `post_eval` reads the published list NON-destructively and mirrors each
/// amount into `Σ⟦v⟧`.
pub const MINT_LIST_ENV_KEY: &str = "sys:casper:mintList";

/// Cost-Accounted Rho Stage D — env-channel key for the per-epoch fee→v
/// conversion ELIGIBLE list `[(pk, epochIdx)]` that `closeBlock`'s
/// `convertFeesToValidators` fold publishes (the economic loop, spec
/// tex:3095-3100). It carries the validators ELIGIBLE for conversion this epoch
/// (`active ∧ ¬mintingHalted ∧ ¬convertedEpochs`); PoS records `(pk, epochIdx)`
/// in `convertedEpochs` (idempotency) and emits the pair. `post_eval` reads this
/// list NON-destructively and, for each eligible `pk`, reads its Rust-held fee
/// pool `F_v` count `f` (`supply::fee_collection_channel`), and — when `f > 0` —
/// mirrors it 1:1 into `Σ⟦v⟧` (`produce_balance(Σ⟦v⟧, old + f)`) and ZEROES
/// `F_v`. PoS owns ELIGIBILITY + idempotency; the fee balances + the `Σ⟦v⟧`
/// mirror are the Rust dual-write half (DR-13). Same deploy-RNG-derived,
/// user-unforgeable GPrivate discipline as the mint list.
pub const FEE_CONVERT_LIST_ENV_KEY: &str = "sys:casper:feeConvertList";

// Currently we use parentHash as initial random seed
#[derive(Clone)]
pub struct CloseBlockDeploy {
    pub initial_rand: Blake2b512Random,
    /// WD-D2 settlement debits: per-pool `ΣΔ_s` to subtract from `Σ⟦s⟧` so
    /// `post = pre − ΣΔ_admitted` (cost-accounted-rho §7.7 / handoff Decision
    /// 4c). Keyed by `SigKey` (= `Sig::lane_hash`). Populated by the acceptance
    /// gate on the PLAY path (block_creator.rs); RECOMPUTED from
    /// `block.body.deploys` on REPLAY (replay_runtime.rs) — the debit amounts are
    /// NOT serialized into the block, so this defaults EMPTY for genesis /
    /// non-cost-accounted / replay-reconstructed close deploys and is filled in
    /// by the recompute before `post_eval_replay`.
    pub settlement_debits:
        std::collections::BTreeMap<crate::rust::util::rholang::acceptance::SigKey, crate::rust::util::rholang::acceptance::SettlementDebit>,
    /// Cost-Accounted Rho Stage D: the per-block FEE credit (the spec's
    /// `FeeExtract` — flat one token per processed deploy, tex:2509-2521),
    /// collected into the PROPOSING validator's fee channel `F_v`. `None` for
    /// genesis / replay-reconstructed / slashing-test close deploys (no fee);
    /// populated by the block proposer's `create()` and RECOMPUTED on replay
    /// from `block.body.deploys` (the amount is NOT serialized into the block —
    /// it rides the same recompute-from-block discipline as `settlement_debits`).
    /// SIBLING of `settlement_debits`: the fee (a transferred token) is DISTINCT
    /// from the cost (the burned settlement debit) — cost ≠ fee (D.0).
    pub fee_credits: Option<crate::rust::util::rholang::acceptance::FeeCredit>,
}

impl CloseBlockDeploy {
    /// Construct a close-block deploy carrying NO settlement debits (the common
    /// case: genesis, replay-reconstructed close deploys, slashing/merging test
    /// fixtures — none of which compute WD-D2 debits; only the block proposer's
    /// `create()` populates `settlement_debits`, and replay RECOMPUTES them).
    /// Keeps the many literal-construction sites stable as the struct gains the
    /// `settlement_debits` field.
    pub fn new(initial_rand: Blake2b512Random) -> Self {
        Self {
            initial_rand,
            settlement_debits: std::collections::BTreeMap::new(),
            fee_credits: None,
        }
    }

    /// The deterministic, deploy-RNG-derived, user-unforgeable channel onto
    /// which `closeBlock` publishes its mint list. Derived from a FIXED split
    /// path (`split_byte(MINT_LIST_RNG_PATH)`) of the close-block deploy seed so
    /// it is disjoint from the return channel (which uses `rand().next()`
    /// directly, no split) — no aliasing — and byte-identical on play and
    /// replay (the seed is `generate_close_deploy_random_seed_from_*`, identical
    /// on both paths for the same proposing validator + seq_num).
    pub fn mint_list_channel(&self) -> Par {
        const MINT_LIST_RNG_PATH: i8 = 0x2a; // fixed, disjoint from the return channel stream
        let id: Vec<u8> = self
            .rand()
            .split_byte(MINT_LIST_RNG_PATH)
            .next()
            .into_iter()
            .map(|b| b as u8)
            .collect();
        Par::default().with_unforgeables(vec![GUnforgeable {
            unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id })),
        }])
    }

    fn mk_mint_list_channel(&self) -> (String, Par) {
        (MINT_LIST_ENV_KEY.to_string(), self.mint_list_channel())
    }

    /// Stage D: the deterministic, deploy-RNG-derived, user-unforgeable channel
    /// onto which `closeBlock` publishes the per-epoch fee→v conversion ELIGIBLE
    /// list `[(pk, epochIdx)]`. Derived from a FIXED split path
    /// (`split_byte(FEE_CONVERT_LIST_RNG_PATH)`) DISJOINT from the mint-list
    /// channel (`0x2a`) and the return channel (no split) — so the env channels
    /// never alias — and byte-identical on play and replay (same close-block seed).
    pub fn fee_convert_list_channel(&self) -> Par {
        const FEE_CONVERT_LIST_RNG_PATH: i8 = 0x2b; // disjoint from mint-list 0x2a
        let id: Vec<u8> = self
            .rand()
            .split_byte(FEE_CONVERT_LIST_RNG_PATH)
            .next()
            .into_iter()
            .map(|b| b as u8)
            .collect();
        Par::default().with_unforgeables(vec![GUnforgeable {
            unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id })),
        }])
    }

    fn mk_fee_convert_list_channel(&self) -> (String, Par) {
        (
            FEE_CONVERT_LIST_ENV_KEY.to_string(),
            self.fee_convert_list_channel(),
        )
    }

    /// Shared implementation of the Stage-B supply mint + WD-D2 settlement
    /// debit, run on the LIVE post-closeBlock runtime by
    /// [`SystemDeployTrait::post_eval`] (play) / [`Self::post_eval_replay`]
    /// (replay). Two phases, in order:
    ///
    /// 1. **Stage-B mint** (credit): read the `closeBlock`-published mint list
    ///    `[(pk, amount)]` and mirror each amount into `Σ⟦v⟧ = from_sig(Ground(pk))`
    ///    (epoch/genesis-block-1 mint).
    /// 2. **WD-D2 settlement debit** (charge): for each `(sig_key, debit)` in
    ///    `debits` (a deterministic `BTreeMap`), subtract `debit.amount` (= the
    ///    gate's static `ΣΔ_s`) from the pool `Σ⟦s⟧` so `post = pre − ΣΔ_admitted`
    ///    (cost-accounted-rho §7.7 / handoff Decision 4c). closeBlock is ALWAYS
    ///    the last system deploy, so every admitted user deploy has executed by
    ///    now. The debit runs AFTER the mint loop so a channel that is both
    ///    minted and debited this block ends at `mint − debit` deterministically.
    ///
    /// CONSENSUS-CRITICAL replay symmetry: the debit MUST be byte-identical on
    /// play and replay. Play passes `debits = &self.settlement_debits` (threaded
    /// by the block proposer); replay passes the SAME map RECOMPUTED from
    /// `block.body.deploys` (the debit amounts are not serialized into the
    /// block) — the same deterministic function ⇒ the same map ⇒ identical writes.
    ///
    /// `is_replay` gates the [`ReplayFailure::ReplaySupplyMismatch`] write-readback
    /// integrity guard (Decision 6.3) on BOTH phases: after each write the balance
    /// read back MUST equal the just-written value. A settlement `checked_sub`
    /// underflow is a HARD error (`expect`) — the gate guarantees `ΣΔ_s ≤ Σ_s`, so
    /// underflow here is a gate-invariant violation, never reachable in a valid
    /// block.
    async fn dual_write_supply(
        &self,
        runtime_ops: &mut RuntimeOps,
        _block_data: &BlockData,
        _pre_state_hash: &StateHash,
        debits: &std::collections::BTreeMap<
            crate::rust::util::rholang::acceptance::SigKey,
            crate::rust::util::rholang::acceptance::SettlementDebit,
        >,
        is_replay: bool,
    ) -> Result<(), CasperError> {
        let list_chan = self.mint_list_channel();
        let published = runtime_ops.get_data_par(&list_chan).await;

        // The list is the LAST datum produced on the channel (closeBlock writes
        // it exactly once per close); an absent list ⇒ no MINT this block (the
        // common non-epoch, non-block-1 path). Do NOT early-return here — the
        // WD-D2 settlement DEBIT loop below must still run even when nothing is
        // minted (a block with admitted user deploys but no epoch/genesis mint).
        let mut mints = match published
            .iter()
            .rev()
            .find_map(|p| RhoList::unapply(p))
        {
            Some(list) => decode_mint_list(&list)?,
            None => Vec::new(),
        };

        // Canonical (pk-ascending) order so the per-mint `random_state`
        // derivation (indexed) is independent of the fold/iteration order on
        // both play and replay.
        mints.sort_by(|a, b| a.0.cmp(&b.0));

        let close_rand = self.rand();
        for (index, (pk, amount)) in mints.iter().enumerate() {
            let chan = supply_channel(&Sig::Ground(pk.clone()));
            let old_n = supply::read_balance(runtime_ops, &chan).await;
            let new_n = old_n
                .checked_add(*amount)
                .expect("phlogiston supply overflow");
            let random_state = supply::mint_random_state(&close_rand, index as i64);
            supply::produce_balance(runtime_ops, &chan, new_n, random_state).await?;

            if is_replay {
                // Decision 6.3: write-readback integrity — the just-written
                // balance must read back as `new_n`. Sibling of
                // `ReplayCostMismatch`; surfaces a `produce_balance` divergence
                // before the post-state root check would.
                let readback = supply::read_balance(runtime_ops, &chan).await;
                if readback != new_n {
                    return Err(CasperError::ReplayFailure(
                        ReplayFailure::replay_supply_mismatch(hex::encode(pk), new_n, readback),
                    ));
                }
            }
        }

        // ── WD-D2 settlement debit (charge), AFTER the mint loop ─────────────
        // Iterate the debit map in its deterministic BTreeMap order (by SigKey)
        // so the per-debit `random_state` index is identical on play and replay.
        // Each debit subtracts the gate's static `ΣΔ_s` from the pool `Σ⟦s⟧`;
        // a `checked_sub` underflow is a gate-invariant violation (the gate only
        // admits a prefix whose cumulative Δ ≤ effective Σ), hence a hard error.
        for (index, (_sig_key, debit)) in debits.iter().enumerate() {
            if debit.amount == 0 {
                // A zero-amount debit is a no-op (no admitted demand on this
                // pool); skip it so the channel datum is untouched.
                continue;
            }
            let old_n = supply::read_balance(runtime_ops, &debit.channel).await;
            let new_n = old_n.checked_sub(debit.amount).expect(
                "phlogiston supply underflow on settlement debit — gate invariant violated \
                 (admitted ΣΔ_s exceeds Σ_s)",
            );
            let random_state = supply::debit_random_state(&close_rand, index as i64);
            supply::produce_balance(runtime_ops, &debit.channel, new_n, random_state).await?;

            if is_replay {
                // Write-readback integrity (Decision 6.3), symmetric with the
                // mint loop: the debited balance must read back as `new_n`.
                let readback = supply::read_balance(runtime_ops, &debit.channel).await;
                if readback != new_n {
                    return Err(CasperError::ReplayFailure(
                        ReplayFailure::replay_supply_mismatch(
                            format!("debit:{}", hex::encode(_sig_key)),
                            new_n,
                            readback,
                        ),
                    ));
                }
            }
        }

        // ── Stage D fee COLLECTION (phase 3a), AFTER mint + debit ─────────────
        // The spec's FeeExtract (tex:2509-2521): credit the proposing validator's
        // FEE pool `F_v = fee_collection_channel(pk)` by the per-block deploy count
        // (one token per processed deploy). `F_v` is a Rust-nameable,
        // content-addressed, reducer-unwritable pool (DR-13) — DISTINCT from
        // `Σ⟦v⟧` (gate pool) and `@W_v` (draw). cost ≠ fee (D.0): the fee is a
        // SEPARATE token TRANSFERRED to the validator, never the burned settlement
        // debit. Play: `fee_credits` threaded by the proposer (`block.body.deploys`
        // count); replay: RECOMPUTED from `terms.len()` (= `block.body.deploys`),
        // byte-identical — the same recompute-from-block discipline as
        // `settlement_debits`. Disjoint `fee_collect_random_state` path; readback
        // guard (TM-CA-160 fee-credit play/replay divergence).
        if let Some(fee) = &self.fee_credits {
            if fee.amount > 0 {
                let chan = supply::fee_collection_channel(&fee.recipient_pk);
                let old_n = supply::read_balance(runtime_ops, &chan).await;
                let new_n = old_n
                    .checked_add(fee.amount)
                    .expect("fee supply overflow on FeeExtract collection credit");
                let random_state = supply::fee_collect_random_state(&close_rand);
                supply::produce_balance(runtime_ops, &chan, new_n, random_state).await?;

                if is_replay {
                    let readback = supply::read_balance(runtime_ops, &chan).await;
                    if readback != new_n {
                        return Err(CasperError::ReplayFailure(
                            ReplayFailure::replay_supply_mismatch(
                                format!("feeCollect:{}", hex::encode(&fee.recipient_pk)),
                                new_n,
                                readback,
                            ),
                        ));
                    }
                }
            }
        }

        // ── Stage D fee→v CONVERSION Σ⟦v⟧ mirror (phase 3b), AFTER collection ──
        // Read the per-epoch ELIGIBLE list `[(pk, epochIdx)]` that closeBlock's
        // `convertFeesToValidators` fold published (validators that are
        // active ∧ ¬halted ∧ ¬convertedEpochs; the `convertedEpochs` record is
        // done Rholang-side in the fold). For each eligible `pk`, read its FEE
        // pool `F_v` count `f`; when `f > 0`, 1:1-convert it into the reducer-
        // unwritable gate pool `Σ⟦v⟧` (`produce_balance(Σ⟦v⟧, old + f)`) and ZERO
        // `F_v`. DR-4: `f == 0` ⇒ NO Σ⟦v⟧ credit (no one-sided mint). Conserves:
        // the `Σ⟦v⟧ += f` credit is EXACTLY the `f` fees that leave `F_v` (Rocq
        // `fee_convert_credit_is_backed` / `fee_collection_conserves`).
        //
        // Replay symmetry: the eligible list is recomputed identically by the
        // Rholang fold from the SAME pre-state PoS state (active/halted/converted
        // ledgers), and each `f` is read from the SAME pre-state `F_v` pool — so
        // the convert writes are byte-identical play↔replay. Sorted by pk so the
        // per-credit `random_state` index is fold-order-independent.
        let fee_list_chan = self.fee_convert_list_channel();
        let fee_published = runtime_ops.get_data_par(&fee_list_chan).await;
        let mut eligible = match fee_published
            .iter()
            .rev()
            .find_map(|p| RhoList::unapply(p))
        {
            Some(list) => decode_mint_list(&list)?,
            None => Vec::new(),
        };
        // Each entry is `(pk, epochIdx)`; sort by pk for fold-order independence.
        eligible.sort_by(|a, b| a.0.cmp(&b.0));

        for (index, (pk, _epoch_idx)) in eligible.iter().enumerate() {
            let fee_chan = supply::fee_collection_channel(pk);
            let f = supply::read_balance(runtime_ops, &fee_chan).await;
            if f <= 0 {
                // DR-4: an eligible validator with no collected fees gets no
                // convert credit (never a one-sided mint). Leave F_v untouched.
                continue;
            }
            // Credit Σ⟦v⟧ += f (the conserving 1:1 convert).
            let supply_chan = supply_channel(&Sig::Ground(pk.clone()));
            let old_v = supply::read_balance(runtime_ops, &supply_chan).await;
            let new_v = old_v
                .checked_add(f)
                .expect("phlogiston supply overflow on fee-convert credit");
            let credit_rand = supply::fee_convert_random_state(&close_rand, (index as i64) * 2);
            supply::produce_balance(runtime_ops, &supply_chan, new_v, credit_rand).await?;

            // Zero F_v (the converted fees have left the fee pool). A distinct
            // index parity keeps the convert-credit and the F_v-zero produce
            // identities distinct even on the same close seed.
            let zero_rand = supply::fee_convert_random_state(&close_rand, (index as i64) * 2 + 1);
            supply::produce_balance(runtime_ops, &fee_chan, 0, zero_rand).await?;

            if is_replay {
                // Write-readback integrity (Decision 6.3), symmetric with the
                // mint/debit/collect loops: TM-CA-160 fee-credit play/replay guard.
                let readback_v = supply::read_balance(runtime_ops, &supply_chan).await;
                if readback_v != new_v {
                    return Err(CasperError::ReplayFailure(
                        ReplayFailure::replay_supply_mismatch(
                            format!("feeConvert:{}", hex::encode(pk)),
                            new_v,
                            readback_v,
                        ),
                    ));
                }
                let readback_f = supply::read_balance(runtime_ops, &fee_chan).await;
                if readback_f != 0 {
                    return Err(CasperError::ReplayFailure(
                        ReplayFailure::replay_supply_mismatch(
                            format!("feeZero:{}", hex::encode(pk)),
                            0,
                            readback_f,
                        ),
                    ));
                }
            }
        }

        Ok(())
    }
}

/// Decode the Rholang-published mint list `List[(GByteArray pk, GInt amount)]`
/// into `Vec<(pk_bytes, amount)>`. Total over the published pars: a malformed
/// element is a consensus error (the close-block deploy is the sole producer of
/// this channel and always publishes well-formed pairs).
fn decode_mint_list(list: &[Par]) -> Result<Vec<(Vec<u8>, i64)>, CasperError> {
    let mut out = Vec::with_capacity(list.len());
    for entry in list {
        let (pk_par, amt_par) = RhoTuple2::unapply(entry).ok_or_else(|| {
            CasperError::RuntimeError("mint list entry was not a (pk, amount) tuple".to_string())
        })?;
        let pk = RhoByteArray::unapply(&pk_par).ok_or_else(|| {
            CasperError::RuntimeError("mint list pk was not a byte array".to_string())
        })?;
        let amount = RhoNumber::unapply(&amt_par).ok_or_else(|| {
            CasperError::RuntimeError("mint list amount was not an integer".to_string())
        })?;
        out.push((pk, amount));
    }
    Ok(out)
}

impl SystemDeployTrait for CloseBlockDeploy {
    type Output = (RhoBoolean, Either<RhoString, RhoNil>);
    type Result = ();

    fn source() -> &'static str {
        r#"
        new rl(`rho:registry:lookup`),
        poSCh,
        sysAuthToken(`sys:casper:authToken`),
        mintList(`sys:casper:mintList`),
        feeConvertList(`sys:casper:feeConvertList`),
        return(`sys:casper:return`)
        in {
          rl!(`rho:system:pos`, *poSCh) |
          for(@(_, PoS) <- poSCh) {
             @PoS!("closeBlock", *sysAuthToken, *mintList, *feeConvertList, *return)
          }
        }"#
    }

    fn process_result(value: (bool, Either<String, ()>)) -> Either<SystemDeployUserError, ()> {
        match value {
            (true, _) => Either::Right(()),
            (false, Either::Left(error_msg)) => Either::Left(SystemDeployUserError::new(error_msg)),
            _ => Either::Left(SystemDeployUserError::new("<no cause>".to_string())),
        }
    }

    fn as_any(&self) -> &dyn std::any::Any { self }

    fn rand(&self) -> Blake2b512Random { self.initial_rand.clone() }

    fn env(&mut self) -> HashMap<String, Par> {
        let mut env = HashMap::new();

        let (sys_key, sys_value) = self.mk_sys_auth_token();
        env.insert(sys_key, sys_value);

        let (mint_key, mint_value) = self.mk_mint_list_channel();
        env.insert(mint_key, mint_value);

        // Stage D: the fee→v conversion eligible list channel (PoS publishes the
        // eligible `(v, epochIdx)`; post_eval reads it to mirror each F_v into Σ⟦v⟧).
        let (fee_convert_key, fee_convert_value) = self.mk_fee_convert_list_channel();
        env.insert(fee_convert_key, fee_convert_value);

        let (ret_key, ret_value) = self.mk_return_channel();
        env.insert(ret_key, ret_value);

        env
    }

    fn return_channel(&mut self) -> Result<Par, CasperError> {
        match self.env().get("sys:casper:return") {
            Some(par) => Ok(par.clone()),
            None => Err(CasperError::RuntimeError(
                "Return channel not found. This is a compile time error.".to_string(),
            )),
        }
    }

    fn post_eval<'a>(
        &'a self,
        runtime_ops: &'a mut RuntimeOps,
        block_data: &'a BlockData,
        pre_state_hash: &'a StateHash,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<(), CasperError>> + Send + 'a>,
    > {
        // PLAY-side invocation (RuntimeOps::play_system_deploy). The settlement
        // debits are threaded on `self.settlement_debits` (populated by the block
        // proposer's acceptance gate). The replay-side invocation
        // (replay_block_system_deploy) calls `post_eval_replay` with the
        // RECOMPUTED map so the `ReplaySupplyMismatch` write-readback guard
        // activates there.
        Box::pin(async move {
            self.dual_write_supply(
                runtime_ops,
                block_data,
                pre_state_hash,
                &self.settlement_debits,
                false,
            )
            .await
        })
    }
}

impl CloseBlockDeploy {
    /// Replay-side entry point for the Stage-B supply mint + WD-D2 settlement
    /// debit. Identical to the play-side `post_eval` write path (same mint, same
    /// debit, same `produce_balance`, same deterministic `random_state`) but with
    /// the `ReplaySupplyMismatch` write-readback integrity guard enabled
    /// (Decision 6.3).
    ///
    /// `debits` is the settlement-debit map RECOMPUTED by `replay_deploys` from
    /// `block.body.deploys` (the play-side `self.settlement_debits` is empty on a
    /// replay-reconstructed close deploy — the debit amounts are not serialized
    /// into the block). Passing the recomputed map (rather than `self`) is what
    /// makes the debit byte-identical play↔replay.
    ///
    /// CONSENSUS-CRITICAL: must mutate the live store byte-identically to the
    /// play-side `post_eval` given the same (recomputed) debit map.
    pub async fn post_eval_replay(
        &self,
        runtime_ops: &mut RuntimeOps,
        block_data: &BlockData,
        pre_state_hash: &StateHash,
        debits: &std::collections::BTreeMap<
            crate::rust::util::rholang::acceptance::SigKey,
            crate::rust::util::rholang::acceptance::SettlementDebit,
        >,
    ) -> Result<(), CasperError> {
        self.dual_write_supply(runtime_ops, block_data, pre_state_hash, debits, true)
            .await
    }
}
