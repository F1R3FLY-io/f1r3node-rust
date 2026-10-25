use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::Event as CasperEvent;
use models::rust::casper::protocol::offered_processed_deploy::OfferedProcessedDeploy;
use models::rust::cost_protocol_limits::offered_funded_v6_limits;
use models::rust::deploy_envelope::{DeployEnvelope, DeployEnvelopeRef};
use models::rust::native_cost_evidence::NativeCostFailureClass;
use models::rust::native_wallet_receipt::{NativeWalletReceiptLimits, NativeWalletReceiptV1};
use models::rust::phlo_grant_creation::VerifiedGrantCreationV1;
use models::rust::phlo_intent::PhloConversionCompositionV2;
use models::rust::phlo_wire::PhloWireLimits;
use models::rust::validator::Validator;
use rholang::rust::interpreter::accounting::owner_funding::OwnerFundingBinding;
use rholang::rust::interpreter::interpreter::EvaluateResult;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::system_processes::BlockData;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::merger::merging_logic::NumberChannelsEndVal;
use rspace_plus_plus::rspace::trace::event::Event;

use super::block_gate::offered_member_limits;
use super::held_capacity::LaterHolds;
use super::{
    NativeFundedExecutionContext, NativeGrantSettlementInput, NativeOfferedAttempt, OfferedGate,
};
use crate::rust::errors::CasperError;
use crate::rust::rholang::runtime::RuntimeOps;
use crate::rust::util::event_converter;
use crate::rust::util::rholang::acceptance::prepare_offered_candidate;
use crate::rust::util::rholang::costacc::genesis_resource_policy::AdoptedResourcePolicy;
use crate::rust::util::rholang::costacc::offered_evidence::encode_measured_runtime_recording;
use crate::rust::util::rholang::costacc::offered_grants::{
    offered_grant_issue_call_limits, offered_grant_transition_limits,
};
use crate::rust::util::rholang::costacc::production_limits::{
    offered_funded_v6_prepaid_inventory_limits, offered_funded_v6_prepaid_receipt_limits,
    offered_funded_v6_production_limits, offered_funded_v6_trace_limits,
};
use crate::rust::util::rholang::costacc::vault_payer::vault_payer;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

pub struct OfferedRun {
    evaluation: EvaluateResult,
    replay_log: Arc<[Event]>,
    grant_issuances: Vec<VerifiedGrantCreationV1>,
    grant_issue_log: Arc<[Event]>,
    gate: OfferedGate,
    owner: Option<Arc<OwnerFundingBinding>>,
    post_root: [u8; 32],
    number_channels: NumberChannelsEndVal,
}

impl OfferedRun {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn from_parts(
        evaluation: EvaluateResult,
        replay_log: Arc<[Event]>,
        grant_issuances: Vec<VerifiedGrantCreationV1>,
        grant_issue_log: Arc<[Event]>,
        gate: OfferedGate,
        owner: Option<Arc<OwnerFundingBinding>>,
        post_root: [u8; 32],
        number_channels: NumberChannelsEndVal,
    ) -> Self {
        OfferedRun {
            evaluation,
            replay_log,
            grant_issuances,
            grant_issue_log,
            gate,
            owner,
            post_root,
            number_channels,
        }
    }

    pub fn user_log(&self) -> Vec<CasperEvent> {
        self.replay_log
            .iter()
            .chain(self.grant_issue_log.iter())
            .cloned()
            .map(event_converter::to_casper_event)
            .collect()
    }

    pub fn post_root(&self) -> [u8; 32] { self.post_root }

    pub fn number_channels(&self) -> &NumberChannelsEndVal { &self.number_channels }

    pub fn phlo_used(&self) -> Option<u64> { self.evaluation.native_phlo_usage }

    pub fn failed(&self) -> bool { !self.evaluation.errors.is_empty() }

    pub fn evaluation(&self) -> &EvaluateResult { &self.evaluation }

    pub fn recording(&self) -> Result<(Vec<u8>, Vec<u8>), CasperError> {
        let recording = encode_measured_runtime_recording(
            &self.evaluation,
            offered_funded_v6_production_limits().recording,
        )?;
        Ok((recording.budget, recording.journal))
    }
}

pub struct SettledOffer {
    pub processed: OfferedProcessedDeploy,
    pub final_root: [u8; 32],
    pub failure_class: NativeCostFailureClass,
    pub debits: Vec<([u8; 32], u128, u128)>,
}

fn invalid(message: &str) -> CasperError { CasperError::RuntimeError(message.to_string()) }

fn root_bytes(root: &Blake2b256Hash) -> Result<[u8; 32], CasperError> {
    root.bytes()
        .as_slice()
        .try_into()
        .map_err(|_| invalid("a state root is not 32 bytes"))
}

impl RuntimeManager {
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn run_offered_envelope(
        &self,
        envelope: &DeployEnvelope,
        funding_root: [u8; 32],
        execution_root: [u8; 32],
        gate: OfferedGate,
        later: &LaterHolds,
        block_data: &BlockData,
        invalid_blocks: &HashMap<BlockHash, Validator>,
        adopted: &AdoptedResourcePolicy,
    ) -> Result<OfferedRun, CasperError> {
        let DeployEnvelopeRef::OfferedFunded(signed) = envelope.view() else {
            return Err(invalid("an offered run needs an offered-funded envelope"));
        };
        let limits = offered_member_limits();
        let prepared =
            prepare_offered_candidate(signed, adopted, funding_root, execution_root, limits)?;
        if !matches!(
            prepared.intent.conversion,
            PhloConversionCompositionV2::NoConversion
        ) {
            return Err(invalid(
                "offered conversion requires authenticated production composition",
            ));
        }
        let selected = prepared.selected_terms(adopted)?;
        let mut attempt = self
            .evaluate_native_offered(
                &prepared,
                envelope,
                &selected,
                limits,
                gate,
                NativeFundedExecutionContext {
                    block_data: block_data.clone(),
                    invalid_blocks: invalid_blocks.clone(),
                    trace: offered_funded_v6_trace_limits(),
                },
                later,
            )
            .await?;
        let post_root = root_bytes(&attempt.runtime.runtime.create_checkpoint().await.root)?;
        let number_channels = attempt
            .runtime
            .get_number_channels_data(&attempt.evaluation.mergeable)
            .await?;
        Ok(OfferedRun {
            evaluation: attempt.evaluation,
            replay_log: attempt.replay_log,
            grant_issuances: attempt.grant_issuances,
            grant_issue_log: attempt.grant_issue_log,
            gate: attempt.gate,
            owner: attempt.owner,
            post_root,
            number_channels,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn settle_offered_envelope(
        &self,
        envelope: &DeployEnvelope,
        funding_root: [u8; 32],
        settlement_root: [u8; 32],
        run: &OfferedRun,
        later: &LaterHolds,
        rolled_back: bool,
        block_data: &BlockData,
        invalid_blocks: &HashMap<BlockHash, Validator>,
        adopted: &AdoptedResourcePolicy,
    ) -> Result<Option<SettledOffer>, CasperError> {
        let DeployEnvelopeRef::OfferedFunded(signed) = envelope.view() else {
            return Err(invalid(
                "an offered settlement needs an offered-funded envelope",
            ));
        };
        let limits = offered_member_limits();
        let prepared =
            prepare_offered_candidate(signed, adopted, funding_root, settlement_root, limits)?;
        let grant_limits = offered_grant_transition_limits();
        let grant_context = match prepared.intent.grant_uses.is_empty() {
            true => None,
            false => {
                let authenticated_time = u64::try_from(block_data.time_stamp)
                    .map_err(|_| invalid("offered grant use requires nonnegative block time"))?;
                let grant_snapshot =
                    self.capture_offered_grants_from_signed(settlement_root, signed, grant_limits)?;
                let verified = grant_snapshot.verified_source_payers(
                    settlement_root,
                    signed,
                    authenticated_time,
                    grant_limits,
                )?;
                Some((grant_snapshot, verified, authenticated_time))
            }
        };
        let settlement_hash = prost::bytes::Bytes::copy_from_slice(&settlement_root);
        let snapshot = self
            .read_direct_offered_wallet_snapshot(
                envelope,
                &settlement_hash,
                grant_context.as_ref().map(|(_, verified, _)| verified),
            )
            .await?;
        if snapshot.wallets().pre_state_root() != settlement_root {
            return Err(invalid(
                "offered wallet snapshot belongs to another settlement root",
            ));
        }
        let snapshot = snapshot.capped_by_later_holds(later.holds());
        let mut payers: BTreeMap<String, [u8; 32]> = snapshot
            .wallets()
            .authorization()
            .payers()
            .iter()
            .map(|(custody, payer)| (payer.address.to_base58(), *custody))
            .collect();
        if let Some(owner) = &run.owner {
            for (custody, entry) in owner.table().entries() {
                let payer =
                    vault_payer(&entry.signature).map_err(|error| invalid(&error.to_string()))?;
                payers.insert(payer.address.to_base58(), *custody);
            }
        }
        let mut runtime = RuntimeOps::new(
            self.spawn_offered_play_runtime(offered_grant_issue_call_limits())
                .await?,
        );
        runtime.runtime.set_block_data(block_data.clone()).await;
        runtime
            .runtime
            .set_invalid_blocks(invalid_blocks.clone())
            .await;
        runtime
            .runtime
            .reset(&Blake2b256Hash::from_bytes(settlement_root.to_vec()))
            .await?;
        let attempt = NativeOfferedAttempt {
            preflight: &prepared,
            adopted: adopted.clone(),
            block_data: block_data.clone(),
            runtime,
            evaluation: run.evaluation.clone(),
            replay_log: run.replay_log.clone(),
            grant_issuances: run.grant_issuances.clone(),
            grant_issue_log: run.grant_issue_log.clone(),
            gate: run.gate.clone(),
            owner: run.owner.clone(),
        };
        let candidate = attempt
            .produce_native_offered_candidate_with_rooted_prepaid(
                self,
                envelope.clone(),
                &snapshot,
                later,
                grant_context
                    .as_ref()
                    .map(
                        |(snapshot, verified, authenticated_time)| NativeGrantSettlementInput {
                            snapshot,
                            verified,
                            authenticated_time: *authenticated_time,
                            limits: grant_limits,
                        },
                    ),
                offered_funded_v6_production_limits(),
                offered_funded_v6_prepaid_inventory_limits(),
                offered_funded_v6_prepaid_receipt_limits(),
                Some(rolled_back),
            )
            .await?;
        let Some(candidate) = candidate else {
            return Ok(None);
        };
        let protocol = offered_funded_v6_limits();
        let evidence = candidate
            .processed
            .evidence(protocol.evidence)
            .map_err(|error| invalid(&error))?;
        let receipt =
            NativeWalletReceiptV1::decode(evidence.wallet_settlement, NativeWalletReceiptLimits {
                wire: PhloWireLimits {
                    total_bytes: protocol.evidence.field_bytes,
                    field_bytes: protocol.evidence.field_bytes,
                },
                payers: protocol.envelope.payload.funding.sources,
            })
            .map_err(|error| invalid(&error.to_string()))?;
        let mut debits = Vec::with_capacity(receipt.rows.len());
        for row in &receipt.rows {
            if row.resource_rev == 0 && row.fee_rev == 0 {
                continue;
            }
            let address = std::str::from_utf8(row.address)
                .map_err(|_| invalid("a wallet receipt address is not text"))?;
            let custody = payers
                .get(address)
                .ok_or_else(|| invalid("a wallet receipt row names no authorized purse"))?;
            debits.push((*custody, row.resource_rev, row.fee_rev));
        }
        debits.sort_unstable_by_key(|(custody, _, _)| *custody);
        if debits.windows(2).any(|pair| pair[0].0 == pair[1].0) {
            return Err(invalid("two wallet receipt rows name one purse"));
        }
        let failure_class = evidence.failure_class;
        Ok(Some(SettledOffer {
            processed: candidate.processed,
            final_root: candidate.final_root,
            failure_class,
            debits,
        }))
    }
}
