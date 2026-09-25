use std::collections::HashMap;
use std::sync::Arc;

use models::rust::block_hash::BlockHash;
use models::rust::deploy_envelope::{DeployEnvelope, DeployEnvelopeRef};
use models::rust::signed_phlo_deploy::OfferedFundedDeploy;
use models::rust::validator::Validator;
use rholang::rust::interpreter::accounting::native_phlo_rules::NativeBudgetTraceLimits;
use rholang::rust::interpreter::accounting::phlo_controls::PhloScheduleBinding;
use rholang::rust::interpreter::accounting::NativeRuntimeConfig;
use rholang::rust::interpreter::host_work::HostWorkBudget;
use rholang::rust::interpreter::interpreter::EvaluateResult;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::system_processes::BlockData;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::trace::event::Event;

use super::CheckedDirectWalletPolicy;
use crate::rust::errors::CasperError;
use crate::rust::rholang::runtime::RuntimeOps;
use crate::rust::util::rholang::costacc::genesis_resource_policy::AdoptedResourcePolicy;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

pub struct NativeFundedExecutionContext {
    pub block_data: BlockData,
    pub invalid_blocks: HashMap<BlockHash, Validator>,
    pub trace: NativeBudgetTraceLimits,
    pub host_work: HostWorkBudget,
}

pub struct NativeFundedAttempt<'policy, 'a> {
    policy: &'policy CheckedDirectWalletPolicy<'a, OfferedFundedDeploy>,
    adopted: AdoptedResourcePolicy,
    runtime: RuntimeOps,
    evaluation: EvaluateResult,
    replay_log: Arc<[Event]>,
    execution_root: [u8; 32],
}

impl<'policy, 'a> NativeFundedAttempt<'policy, 'a> {
    pub fn policy(&self) -> &'policy CheckedDirectWalletPolicy<'a, OfferedFundedDeploy> {
        self.policy
    }

    pub fn evaluation(&self) -> &EvaluateResult { &self.evaluation }

    pub fn runtime(&mut self) -> &mut RuntimeOps { &mut self.runtime }
}

fn check_execution_identity(authorized: &[u8], requested: &[u8]) -> Result<(), CasperError> {
    if authorized != requested {
        return Err(CasperError::InvalidCostSettlement(
            "native execution envelope differs from the verified funding policy".to_owned(),
        ));
    }
    Ok(())
}

pub(super) fn check_execution_root(authorized: &[u8], actual: &[u8]) -> Result<(), CasperError> {
    if authorized != actual {
        return Err(CasperError::InvalidCostSettlement(
            "native execution runtime differs from its authorized state root".to_owned(),
        ));
    }
    Ok(())
}

impl CheckedDirectWalletPolicy<'_, OfferedFundedDeploy> {
    fn prepare_execution(
        &self,
        envelope: &DeployEnvelope,
        adopted: &AdoptedResourcePolicy,
        schedule: &PhloScheduleBinding<'_>,
        limits: NativeBudgetTraceLimits,
        budget: HostWorkBudget,
    ) -> Result<NativeRuntimeConfig, CasperError> {
        if !matches!(envelope.view(), DeployEnvelopeRef::OfferedFunded(_)) {
            return Err(CasperError::InvalidCostSettlement(
                "native wallet execution requires an offered funded envelope".to_owned(),
            ));
        }
        let signed = self.policy().policy().signed_intent();
        let identity = signed
            .envelope()
            .envelope_commitment()
            .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
        check_execution_identity(identity.as_ref(), envelope.identity().as_bytes())?;
        let controls = signed.intent().bound().consent().family().cases()[0]
            .obligations
            .execution()
            .controls();
        let contract = adopted.bind_execution_contract(controls, schedule)?;
        Ok(NativeRuntimeConfig::new(contract, limits, budget))
    }
}

impl RuntimeManager {
    pub async fn evaluate_native_funded<'policy, 'a>(
        &self,
        policy: &'policy CheckedDirectWalletPolicy<'a, OfferedFundedDeploy>,
        envelope: &DeployEnvelope,
        adopted: &AdoptedResourcePolicy,
        schedule: &PhloScheduleBinding<'_>,
        context: NativeFundedExecutionContext,
    ) -> Result<NativeFundedAttempt<'policy, 'a>, CasperError> {
        let config = policy.prepare_execution(
            envelope,
            adopted,
            schedule,
            context.trace,
            context.host_work,
        )?;
        let root = policy.snapshot().wallets().pre_state_root();
        let mut runtime = RuntimeOps::new(self.spawn_runtime().await);
        runtime.runtime.set_block_data(context.block_data).await;
        runtime
            .runtime
            .set_invalid_blocks(context.invalid_blocks)
            .await;
        runtime
            .runtime
            .reset(&Blake2b256Hash::from_bytes(root.to_vec()))
            .await?;
        check_execution_root(&root, &runtime.runtime.get_root().await.bytes())?;
        let checkpoint = runtime.runtime.create_soft_checkpoint().await;
        let evaluation = runtime.evaluate_native_envelope(envelope, config).await?;
        let replay_log = runtime.runtime.take_event_log().await.into();
        if !evaluation.errors.is_empty() {
            runtime.runtime.revert_to_soft_checkpoint(checkpoint).await;
        }
        Ok(NativeFundedAttempt {
            policy,
            adopted: adopted.clone(),
            runtime,
            evaluation,
            replay_log,
            execution_root: root,
        })
    }
}

mod settlement;
pub use settlement::{NativeAttemptSettlementInput, NativeAttemptSettlementLimits};

mod replay;
pub use replay::{NativeFundedReplayInput, NativeFundedReplayLimits};

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    proptest! {
        #[test]
        fn native_execution_guards_refine_exact_root_and_envelope_binding(
            root in any::<[u8; 32]>(), identity in any::<[u8; 32]>(),
            other_root in any::<[u8; 32]>(), other_identity in any::<[u8; 32]>(),
            keep_root in any::<bool>(), keep_identity in any::<bool>(),
        ) {
            let requested_root = if keep_root { root } else { other_root };
            let requested_identity = if keep_identity { identity } else { other_identity };
            prop_assert_eq!(check_execution_root(&root, &requested_root).is_ok(), root == requested_root);
            prop_assert_eq!(check_execution_identity(&identity, &requested_identity).is_ok(), identity == requested_identity);
            prop_assert_eq!(
                check_execution_root(&root, &requested_root).is_ok()
                    && check_execution_identity(&identity, &requested_identity).is_ok(),
                root == requested_root && identity == requested_identity,
            );
        }
    }
}
