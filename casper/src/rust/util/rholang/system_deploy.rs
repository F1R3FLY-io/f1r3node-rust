// See casper/src/main/scala/coop/rchain/casper/util/rholang/SystemDeploy.scala

use std::collections::HashMap;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use crypto::rust::public_key::PublicKey;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{GDeployerId, GPrivate, GSysAuthToken, GUnforgeable, Par};
use models::rust::block::state_hash::StateHash;
use rholang::rust::interpreter::rho_type::Extractor;
use rholang::rust::interpreter::system_processes::BlockData;
use rspace_plus_plus::rspace::history::Either;

use super::system_deploy_user_error::{SystemDeployPlatformFailure, SystemDeployUserError};
use crate::rust::errors::CasperError;
use crate::rust::rholang::runtime::RuntimeOps;

pub trait SystemDeployTrait: Send + Sync {
    type Output: Extractor;
    type Result;

    fn source() -> &'static str;

    fn process_result(
        value: <Self::Output as Extractor>::RustType,
    ) -> Either<SystemDeployUserError, Self::Result>;

    fn as_any(&self) -> &dyn std::any::Any;

    fn rand(&self) -> Blake2b512Random;

    fn env(&mut self) -> HashMap<String, Par>;

    fn return_channel(&mut self) -> Result<Par, CasperError>;

    fn mk_return_channel(&mut self) -> (String, Par) {
        (
            "sys:casper:return".to_string(),
            Par::default().with_unforgeables(vec![GUnforgeable {
                unf_instance: Some(UnfInstance::GPrivateBody(GPrivate {
                    id: self.rand().next().into_iter().map(|b| b as u8).collect(),
                })),
            }]),
        )
    }

    fn mk_deployer_id(&self, pk: &PublicKey) -> (String, Par) {
        (
            "sys:casper:deployerId".to_string(),
            Par::default().with_unforgeables(vec![GUnforgeable {
                unf_instance: Some(UnfInstance::GDeployerIdBody(GDeployerId {
                    public_key: pk.bytes.to_vec(),
                })),
            }]),
        )
    }

    fn mk_sys_auth_token(&self) -> (String, Par) {
        (
            "sys:casper:authToken".to_string(),
            Par::default().with_unforgeables(vec![GUnforgeable {
                unf_instance: Some(UnfInstance::GSysAuthTokenBody(GSysAuthToken {})),
            }]),
        )
    }

    /// Post-evaluation Rust-side settlement hook, run on the LIVE runtime AFTER
    /// the system deploy's Rholang source has fully evaluated but BEFORE the
    /// post-state checkpoint is taken (Cost-Accounted Rho, Stage B Decision 2.5;
    /// `stageb-minting-halt-interface.md`). The default is a NO-OP, so existing
    /// system deploys (slash, pre-charge, refund, …) are unaffected.
    ///
    /// `CloseBlockDeploy` overrides it to dual-write the per-validator supply
    /// pool `Σ⟦v⟧ = from_sig(Ground(pk))` for the epoch / genesis-block-1 mint
    /// (the `@W_v` purse half is the Rholang `mintPhlogiston`; the `Σ⟦v⟧` half
    /// is `supply::produce_balance`, because `Σ⟦v⟧` is unnameable in Rholang —
    /// handoff Decision 1/3).
    ///
    /// CONSENSUS-CRITICAL: this hook MUST run IDENTICALLY on play
    /// (`RuntimeOps::play_system_deploy`) and replay
    /// (`ReplayRuntimeOps::replay_block_system_deploy`) — same recompute, same
    /// `produce_balance` writes, same deterministic `random_state` — or play and
    /// replay diverge into a consensus fork. `pre_state_hash` is the system
    /// deploy's pre-state (carried for cross-checks / diagnostics); `runtime_ops`
    /// is the live (post-eval) runtime whose hot store the writes land in and
    /// which is then checkpointed.
    #[allow(unused_variables)]
    fn post_eval<'a>(
        &'a self,
        runtime_ops: &'a mut RuntimeOps,
        block_data: &'a BlockData,
        pre_state_hash: &'a StateHash,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), CasperError>> + Send + 'a>>
    {
        Box::pin(async move { Ok(()) })
    }

    fn extract_result(&self, output: &Par) -> Either<SystemDeployUserError, Self::Result> {
        match <Self::Output as Extractor>::unapply(output) {
            Some(value) => Self::process_result(value),
            None => {
                let error = SystemDeployPlatformFailure::UnexpectedResult(vec![output.clone()]);
                Either::Left(SystemDeployUserError::from(error))
            }
        }
    }
}
