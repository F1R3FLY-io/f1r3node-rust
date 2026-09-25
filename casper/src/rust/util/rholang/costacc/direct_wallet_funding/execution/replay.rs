use std::collections::HashMap;
use std::sync::Arc;

use models::rust::deploy_envelope::{DeployEnvelope, DeployEnvelopeRef};
use models::rust::normalizer_env::normalizer_env_from_envelope;
use models::rust::signed_phlo_deploy::OfferedFundedDeploy;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::phlo_controls::PhloScheduleBinding;
use rholang::rust::interpreter::accounting::{
    funding_sig, NativeBudgetRecording, NativeOperationJournalLimits, NativeOperationRecord,
    NativeOperationTraceLimits, RuntimeBudget,
};
use rholang::rust::interpreter::interpreter::InterpreterImpl;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::rho_runtime::{create_native_replay_env, RhoRuntime};
use rholang::rust::interpreter::system_processes::DeployData;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::trace::event::Event;
use tokio::sync::RwLock;

use super::{CheckedDirectWalletPolicy, NativeFundedAttempt, NativeFundedExecutionContext};
use crate::rust::errors::CasperError;
use crate::rust::rholang::runtime::RuntimeOps;
use crate::rust::util::rholang::costacc::genesis_resource_policy::AdoptedResourcePolicy;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;
use crate::rust::util::rholang::tools::Tools;

#[derive(Clone, Copy)]
pub struct NativeFundedReplayInput<'a> {
    pub recording: &'a NativeBudgetRecording,
    pub operations: &'a Arc<[NativeOperationRecord]>,
    pub events: &'a Arc<[Event]>,
}

#[derive(Clone, Copy, Debug)]
pub struct NativeFundedReplayLimits {
    pub journal: NativeOperationJournalLimits,
    pub trace: NativeOperationTraceLimits,
}

fn invalid(error: impl std::fmt::Display) -> CasperError {
    CasperError::InvalidCostSettlement(error.to_string())
}

fn settlement_root<T>(before: T, after: T, failed: bool) -> T {
    if failed {
        before
    } else {
        after
    }
}

impl NativeFundedAttempt<'_, '_> {
    pub fn replay_input(&self) -> Result<NativeFundedReplayInput<'_>, CasperError> {
        Ok(NativeFundedReplayInput {
            recording: self
                .evaluation
                .native_budget_recording
                .as_ref()
                .ok_or_else(|| invalid("native funded attempt has no budget recording"))?,
            operations: self
                .evaluation
                .native_operation_recording
                .as_ref()
                .ok_or_else(|| invalid("native funded attempt has no operation recording"))?,
            events: &self.replay_log,
        })
    }
}

impl RuntimeManager {
    pub async fn replay_native_funded<'policy, 'a>(
        &self,
        policy: &'policy CheckedDirectWalletPolicy<'a, OfferedFundedDeploy>,
        envelope: &DeployEnvelope,
        adopted: &AdoptedResourcePolicy,
        schedule: &PhloScheduleBinding<'_>,
        context: NativeFundedExecutionContext,
        input: NativeFundedReplayInput<'_>,
        limits: NativeFundedReplayLimits,
    ) -> Result<NativeFundedAttempt<'policy, 'a>, CasperError> {
        let host = context.host_work.clone();
        let config = policy.prepare_execution(
            envelope,
            adopted,
            schedule,
            context.trace,
            context.host_work,
        )?;
        let controls = policy
            .policy()
            .policy()
            .signed_intent()
            .intent()
            .bound()
            .consent()
            .family()
            .cases()[0]
            .obligations
            .execution()
            .controls();
        let identity = envelope.identity().as_bytes().try_into().map_err(invalid)?;
        let trace = adopted
            .bind_execution_contract(controls, schedule)?
            .check_operation_journal(
                identity,
                input.recording,
                input.operations.clone(),
                limits.journal,
                &host,
            )
            .map_err(invalid)?
            .bind_trace(input.events.clone(), limits.trace, &host)
            .map_err(invalid)?;
        let DeployEnvelopeRef::OfferedFunded(signed) = envelope.view() else {
            return Err(invalid("native replay requires an offered funded envelope"));
        };
        let cost = RuntimeBudget::new(Cost::unsafe_max());
        cost.set_deploy_id_funded(identity, funding_sig(signed));
        cost.reset_for_native_execution(config)?;
        let before =
            Blake2b256Hash::from_bytes(policy.snapshot().wallets().pre_state_root().to_vec());
        let history = Arc::new(self.history_repo.reset(&before).map_err(invalid)?);
        super::check_execution_root(&before.bytes(), &history.root().bytes())?;
        let parsed = InterpreterImpl::parse_source(
            &envelope.body().term,
            normalizer_env_from_envelope(envelope),
            Some(&host),
        )?;
        let mut replay = create_native_replay_env(
            trace,
            history,
            Arc::new(Box::new(Matcher)),
            host,
            Arc::new(RwLock::new(HashMap::new())),
            self.mergeable_tags.clone(),
            &mut Vec::new(),
            cost,
            self.external_services.clone(),
        )
        .await?;
        *replay.block_data.write().await = context.block_data.clone();
        replay
            .set_invalid_blocks(context.invalid_blocks.clone())
            .await;
        *replay.deploy_data.write().await = DeployData::from_envelope(envelope);
        let evaluation = replay
            .evaluate(parsed, Tools::user_envelope_rng(envelope))
            .await?;
        let exported = replay.export().await?;
        let root = settlement_root(
            before,
            exported.root().clone(),
            !evaluation.errors.is_empty(),
        );
        let mut runtime = RuntimeOps::new(self.spawn_runtime().await);
        runtime.runtime.set_block_data(context.block_data).await;
        runtime
            .runtime
            .set_invalid_blocks(context.invalid_blocks)
            .await;
        runtime
            .runtime
            .set_deploy_data(DeployData::from_envelope(envelope))
            .await;
        runtime.runtime.reset(&root).await?;
        super::check_execution_root(&root.bytes(), &runtime.runtime.get_root().await.bytes())?;
        Ok(NativeFundedAttempt {
            policy,
            adopted: adopted.clone(),
            runtime,
            evaluation,
            replay_log: input.events.clone(),
            execution_root: root
                .bytes()
                .try_into()
                .map_err(|_| invalid("native replay state root must be 32 bytes"))?,
        })
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    proptest! {
        #[test]
        fn funded_replay_state_selection_preserves_rollback(
            before in any::<[u8; 32]>(), after in any::<[u8; 32]>(),
            other in any::<[u8; 32]>(), failed in any::<bool>(),
        ) {
            let selected = settlement_root(before, after, failed);
            prop_assert_eq!(selected, if failed { before } else { after });
            if failed {
                prop_assert_eq!(selected, settlement_root(before, other, failed));
            }
            prop_assert!(super::super::check_execution_root(&selected, &selected).is_ok());
            prop_assert_eq!(
                super::super::check_execution_root(&selected, &other).is_ok(),
                selected == other,
            );
            let mut changed = selected;
            changed[0] ^= 1;
            prop_assert!(super::super::check_execution_root(&selected, &changed).is_err());
            prop_assert!(super::super::check_execution_root(&selected, &selected[..31]).is_err());
            if !failed && before != after {
                prop_assert!(super::super::check_execution_root(&selected, &before).is_err());
            }
        }
    }
}
