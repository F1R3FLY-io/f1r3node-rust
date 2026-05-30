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

// Currently we use parentHash as initial random seed
#[derive(Clone)]
pub struct CloseBlockDeploy {
    pub initial_rand: Blake2b512Random,
}

impl CloseBlockDeploy {
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

    /// Shared implementation of the Stage-B supply dual-write, run on the LIVE
    /// post-closeBlock runtime by [`SystemDeployTrait::post_eval`]. Reads the
    /// `closeBlock`-published mint list `[(pk, amount)]` from the live store and
    /// mirrors each amount into the per-validator supply pool
    /// `Σ⟦v⟧ = from_sig(Ground(pk))` via [`supply::produce_balance`]
    /// (read-modify-replace, single datum).
    ///
    /// `is_replay` gates the [`ReplayFailure::ReplaySupplyMismatch`] write-readback
    /// integrity guard (Decision 6.3): on replay, after writing `new_n`, the
    /// balance read back from `Σ⟦v⟧` MUST equal `new_n` (a `produce_balance`
    /// malfunction would otherwise silently diverge the post-state). The full
    /// play↔replay supply equality is additionally enforced by the post-state
    /// root comparison the replay validator already performs.
    async fn dual_write_supply(
        &self,
        runtime_ops: &mut RuntimeOps,
        _block_data: &BlockData,
        _pre_state_hash: &StateHash,
        is_replay: bool,
    ) -> Result<(), CasperError> {
        let list_chan = self.mint_list_channel();
        let published = runtime_ops.get_data_par(&list_chan).await;

        // The list is the LAST datum produced on the channel (closeBlock writes
        // it exactly once per close); an absent list ⇒ no mint this block (the
        // common non-epoch, non-block-1 path) ⇒ nothing to mirror.
        let mut mints = match published
            .iter()
            .rev()
            .find_map(|p| RhoList::unapply(p))
        {
            Some(list) => decode_mint_list(&list)?,
            None => return Ok(()),
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
        return(`sys:casper:return`)
        in {
          rl!(`rho:system:pos`, *poSCh) |
          for(@(_, PoS) <- poSCh) {
             @PoS!("closeBlock", *sysAuthToken, *mintList, *return)
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
        // PLAY-side invocation (RuntimeOps::play_system_deploy). The replay-side
        // invocation (replay_block_system_deploy) calls `post_eval_replay` so the
        // `ReplaySupplyMismatch` write-readback guard activates there.
        Box::pin(async move {
            self.dual_write_supply(runtime_ops, block_data, pre_state_hash, false)
                .await
        })
    }
}

impl CloseBlockDeploy {
    /// Replay-side entry point for the Stage-B supply dual-write. Identical to
    /// the play-side `post_eval` write path (same recompute, same
    /// `produce_balance`, same deterministic `random_state`) but with the
    /// `ReplaySupplyMismatch` integrity guard enabled (Decision 6.3).
    /// CONSENSUS-CRITICAL: must mutate the live store byte-identically to the
    /// play-side `post_eval`.
    pub async fn post_eval_replay(
        &self,
        runtime_ops: &mut RuntimeOps,
        block_data: &BlockData,
        pre_state_hash: &StateHash,
    ) -> Result<(), CasperError> {
        self.dual_write_supply(runtime_ops, block_data, pre_state_hash, true)
            .await
    }
}
