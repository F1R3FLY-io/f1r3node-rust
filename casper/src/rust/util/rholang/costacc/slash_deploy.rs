// See casper/src/main/scala/coop/rchain/casper/util/rholang/costacc/SlashDeploy.scala

use std::collections::HashMap;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use crypto::rust::public_key::PublicKey;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{GPrivate, GUnforgeable, Par};
use models::rust::block::state_hash::StateHash;
use models::rust::block_hash::BlockHash;
use models::rust::utils::new_gstring_par;
use rholang::rust::interpreter::accounting::Sig;
use rholang::rust::interpreter::rho_type::{
    Extractor, RhoBoolean, RhoByteArray, RhoNil, RhoString,
};
use rholang::rust::interpreter::system_processes::BlockData;
use rspace_plus_plus::rspace::history::Either;

use crate::rust::errors::CasperError;
use crate::rust::rholang::runtime::RuntimeOps;
use crate::rust::util::rholang::replay_failure::ReplayFailure;
use crate::rust::util::rholang::supply::{self, supply_channel};
use crate::rust::util::rholang::system_deploy::SystemDeployTrait;
use crate::rust::util::rholang::system_deploy_user_error::SystemDeployUserError;

/// Env-channel key under which the Cost-Accounted Rho Stage-C `slash` contract
/// publishes the resolved OFFENDER public key (the validator whose invalid
/// block triggered the slash, `invalidBlocks.get(blockHash)`) for the Rust
/// `Σ⟦v⟧`-zero. Mirrors `CloseBlockDeploy::MINT_LIST_ENV_KEY`: the channel is a
/// slash-deploy-RNG-derived `GPrivate` (Rust-constructed, so Rust knows it
/// exactly; user-Rholang-unforgeable — DR-13), passed into the slash source via
/// [`SlashDeploy::env`]. [`SlashDeploy::post_eval`] reads the published pk and
/// zeros exactly that offender's supply pool `Σ⟦offender⟧ = from_sig(Ground(pk))`.
///
/// CRITICAL: the offender is NOT [`SlashDeploy::pk`] — that field is the slash
/// ISSUER (the proposer's identity, `validator_identity.public_key`, used for
/// the deployer-id env). The offender is resolved INSIDE Rholang from the
/// invalid-block evidence and published here; resolving it in Rust would
/// duplicate the `invalidBlocks` lookup. Publishing it (the closeBlock
/// `mintList` pattern) keeps the offender derivation single-sourced in Rholang
/// and replay-symmetric (replay re-resolves the same offender from the same
/// `invalid_block_hash` carried in `SystemDeployData::Slash`).
pub const SLASHED_PK_ENV_KEY: &str = "sys:casper:slashedPk";

#[derive(Clone)]
pub struct SlashDeploy {
    pub invalid_block_hash: BlockHash,
    pub pk: PublicKey,
    /// Epoch at which the slash takes effect. By the §9 authorization
    /// predicate this must equal both the offender's evidence epoch and the
    /// current epoch of the block carrying the slash; see
    /// `slashing_authorization::received_slash_deploy_authorized`.
    pub target_activation_epoch: i64,
    pub initial_rand: Blake2b512Random,
}

impl SlashDeploy {
    /// The deterministic, slash-deploy-RNG-derived, user-unforgeable channel
    /// onto which the `slash` contract publishes the resolved offender pk.
    /// Derived from a FIXED split path (`split_byte(SLASHED_PK_RNG_PATH)`) of
    /// the slash deploy seed so it is disjoint from the return channel (which
    /// uses `rand().next()` directly, no split) — no aliasing — and
    /// byte-identical on play and replay (the seed is
    /// `generate_slash_deploy_random_seed`, identical on both paths for the same
    /// proposer + seq_num + invalid block hash).
    pub fn slashed_pk_channel(&self) -> Par {
        const SLASHED_PK_RNG_PATH: i8 = 0x2c; // fixed, disjoint from the return channel stream
        let id: Vec<u8> = self
            .rand()
            .split_byte(SLASHED_PK_RNG_PATH)
            .next()
            .into_iter()
            .map(|b| b as u8)
            .collect();
        Par::default().with_unforgeables(vec![GUnforgeable {
            unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id })),
        }])
    }

    fn mk_slashed_pk_channel(&self) -> (String, Par) {
        (SLASHED_PK_ENV_KEY.to_string(), self.slashed_pk_channel())
    }

    /// Shared implementation of the Stage-C `Σ⟦v⟧`-zero, run on the LIVE
    /// post-slash runtime by [`SystemDeployTrait::post_eval`] (play) /
    /// [`Self::post_eval_replay`] (replay).
    ///
    /// Reads the offender pk the `slash` contract published on
    /// [`SLASHED_PK_ENV_KEY`] (the LAST datum, mirroring `closeBlock`'s mint
    /// list) and, if present, zeros `Σ⟦offender⟧ = from_sig(Ground(pk))` via
    /// `supply::produce_balance(chan, 0, …)` (stageb-minting-halt-interface.md
    /// Decision 4: the spec-complete "all remaining phlogiston is removed",
    /// idempotent — a read-modify-replace to 0). An ABSENT pk (a no-op /
    /// idempotent slash, or absent evidence) zeros NOTHING — there is no
    /// offender to zero (and an already-zero `Σ⟦v⟧` would be unchanged anyway).
    ///
    /// CONSENSUS-CRITICAL replay symmetry: byte-identical on play and replay.
    /// The offender pk is resolved deterministically in Rholang from the same
    /// `invalid_block_hash` (carried in `SystemDeployData::Slash`) on both paths,
    /// and the produce `random_state` ([`supply::slash_random_state`]) is derived
    /// from the slash deploy's replay-stable `initial_rand`. `is_replay` gates
    /// the [`ReplayFailure::ReplaySupplyMismatch`] write-readback integrity guard
    /// (Decision 6.3): after the zero the balance read back MUST equal 0.
    async fn zero_offender_supply(
        &self,
        runtime_ops: &mut RuntimeOps,
        _block_data: &BlockData,
        _pre_state_hash: &StateHash,
        is_replay: bool,
    ) -> Result<(), CasperError> {
        let chan_pub = self.slashed_pk_channel();
        let published = runtime_ops.get_data_par(&chan_pub).await;

        // The offender pk is the LAST datum produced on the channel (the slash
        // contract publishes it exactly once per positive-bond slash). An absent
        // pk ⇒ no Σ⟦v⟧ zero this slash (the no-op / idempotent path).
        let offender = published
            .iter()
            .rev()
            .find_map(|p| RhoByteArray::unapply(p));

        if let Some(pk_bytes) = offender {
            let chan = supply_channel(&Sig::Ground(pk_bytes.clone()));
            let random_state = supply::slash_random_state(&self.rand());
            // Spec-complete "all remaining phlogiston is removed": replace the
            // pool datum with 0 (idempotent — zeroing an already-zero pool is a
            // no-op write to the same value).
            supply::produce_balance(runtime_ops, &chan, 0, random_state).await?;

            if is_replay {
                // Decision 6.3: write-readback integrity — the zeroed balance
                // must read back as 0. Sibling of `ReplayCostMismatch`; surfaces
                // a `produce_balance` divergence before the post-state root check.
                let readback = supply::read_balance(runtime_ops, &chan).await;
                if readback != 0 {
                    return Err(CasperError::ReplayFailure(
                        ReplayFailure::replay_supply_mismatch(hex::encode(&pk_bytes), 0, readback),
                    ));
                }
            }
        }

        Ok(())
    }

    /// Replay-side entry point for the Stage-C `Σ⟦v⟧`-zero. Identical to the
    /// play-side `post_eval` write path (same offender resolution, same
    /// `produce_balance` to 0, same deterministic `random_state`) but with the
    /// `ReplaySupplyMismatch` write-readback integrity guard enabled (Decision
    /// 6.3). Invoked from `replay_block_system_deploy`'s `Slash` branch SYMMETRIC
    /// with the play-side `post_eval` auto-call in `RuntimeOps::play_system_deploy`.
    ///
    /// CONSENSUS-CRITICAL: must mutate the live store byte-identically to the
    /// play-side `post_eval`.
    pub async fn post_eval_replay(
        &self,
        runtime_ops: &mut RuntimeOps,
        block_data: &BlockData,
        pre_state_hash: &StateHash,
    ) -> Result<(), CasperError> {
        self.zero_offender_supply(runtime_ops, block_data, pre_state_hash, true)
            .await
    }
}

impl SystemDeployTrait for SlashDeploy {
    type Output = (RhoBoolean, Either<RhoString, RhoNil>);
    type Result = ();

    fn source() -> &'static str {
        r#"
          new rl(`rho:registry:lookup`),
          poSCh,
          deployerId(`sys:casper:deployerId`),
          invalidBlockHash(`sys:casper:invalidBlockHash`),
          sysAuthToken(`sys:casper:authToken`),
          slashedPk(`sys:casper:slashedPk`),
          return(`sys:casper:return`)
          in {
            rl!(`rho:system:pos`, *poSCh) |
            for(@(_, PoS) <- poSCh) {
              @PoS!("slash", *deployerId, *invalidBlockHash.hexToBytes(), *sysAuthToken, *slashedPk, *return)
            }
        }"#
    }

    fn process_result(
        value: <Self::Output as Extractor>::RustType,
    ) -> Either<SystemDeployUserError, Self::Result> {
        match value {
            (true, _) => Either::Right(()),
            (false, Either::Left(error_msg)) => Either::Left(SystemDeployUserError::new(error_msg)),
            _ => Either::Left(SystemDeployUserError::new(
                "Slashing failed unexpectedly".to_string(),
            )),
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn rand(&self) -> Blake2b512Random {
        self.initial_rand.clone()
    }

    fn env(&mut self) -> HashMap<String, Par> {
        let mut env = HashMap::new();

        let (d_key, d_value) = self.mk_deployer_id(&self.pk);
        env.insert(d_key, d_value);

        env.insert(
            "sys:casper:invalidBlockHash".to_string(),
            new_gstring_par(hex::encode(&self.invalid_block_hash), Vec::new(), false),
        );

        let (sys_key, sys_value) = self.mk_sys_auth_token();
        env.insert(sys_key, sys_value);

        let (slashed_key, slashed_value) = self.mk_slashed_pk_channel();
        env.insert(slashed_key, slashed_value);

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

    /// PLAY-side invocation (RuntimeOps::play_system_deploy). Zeros the offender's
    /// supply pool `Σ⟦offender⟧` (the Stage-C two-effect slash; the `@W_v` drain
    /// + `mintingHalted` + quarantine are the Rholang half). The replay-side
    /// invocation (`replay_block_system_deploy`'s `Slash` branch) calls
    /// `post_eval_replay` so the `ReplaySupplyMismatch` write-readback guard
    /// activates there.
    fn post_eval<'a>(
        &'a self,
        runtime_ops: &'a mut RuntimeOps,
        block_data: &'a BlockData,
        pre_state_hash: &'a StateHash,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), CasperError>> + Send + 'a>>
    {
        Box::pin(async move {
            self.zero_offender_supply(runtime_ops, block_data, pre_state_hash, false)
                .await
        })
    }
}
