// See casper/src/main/scala/coop/rchain/casper/ReportingCasper.scala

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use block_storage::rust::dag::block_dag_key_value_storage::BlockDagKeyValueStorage;
use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
// Changed by DR-134 (DR-70): a block report also holds offered deploys.
// use models::rust::casper::protocol::casper_message::{
//     BlockMessage, ProcessedDeploy, ProcessedSystemDeploy, SystemDeployData,
// };
use models::rust::casper::protocol::casper_message::{
    BlockMessage, ProcessedDeploy, ProcessedSystemDeploy, ProcessedUserDeploy, SystemDeployData,
};
use models::rust::casper::protocol::offered_processed_deploy::OfferedProcessedDeploy;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::system_processes::{BlockData, Definition};
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
// Changed by DR-134 (DR-70): the reporting logger of an offered user run.
// use rspace_plus_plus::rspace::reporting_rspace::{ReportBatch, ReportingRspace};
use rspace_plus_plus::rspace::reporting_rspace::{
    ReportBatch, ReportPhase, ReportingLogger, ReportingRspace,
};
use rspace_plus_plus::rspace::rspace::RSpaceStore;
use shared::rust::ByteString;

// Added by DR-134 (DR-70).
use crate::rust::api::block_api::{offered_settlement_receipt, OfferedSettlementReceipt};
use crate::rust::util::rholang::costacc::genesis_resource_policy::AdoptedResourcePolicy;
use crate::rust::util::rholang::costacc::offered_only::{
    is_legacy_metered_genesis, BODY_ONLY_DEPLOYS_UNSUPPORTED, LEGACY_METERED_GENESIS_UNSUPPORTED,
};
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

/// Reporting events for one phase-segment of a deploy's replay.
type DeployReportEvents = Vec<ReportBatch<Par, BindPattern, ListParWithRandom, TaggedContinuation>>;

/// Added by DR-134 (DR-70): the Casper state that the replay of an offered
/// block needs. The reporter is built before Casper starts, so the block
/// report API passes this state with each request.
#[derive(Clone, Copy)]
pub struct ReportContext<'a> {
    pub runtime_manager: &'a RuntimeManager,
    pub adopted: Option<&'a AdoptedResourcePolicy>,
}

/// Added by DR-134 (DR-70): the committed native cost evidence of an offered
/// deploy and its settlement receipt. The report carries them next to the
/// event batches of the user run, which a dev report has for every user
/// deploy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfferedDeployReport {
    pub native_cost_evidence: Vec<u8>,
    pub receipt: Option<OfferedSettlementReceipt>,
}

impl OfferedDeployReport {
    /// Added by DR-134 (DR-70): the proto form in the block report. The REV
    /// amounts are decimal strings, because they are 128-bit values.
    pub fn to_proto(&self) -> models::casper::OfferedDeployReport {
        models::casper::OfferedDeployReport {
            native_cost_evidence: self.native_cost_evidence.clone().into(),
            receipt: self
                .receipt
                .as_ref()
                .map(|receipt| models::casper::OfferedSettlementReport {
                    phlo_used: receipt.phlo_used,
                    fresh_phlo: receipt.fresh_phlo,
                    retained_phlo: receipt.retained_phlo,
                    phlo_limit: receipt.phlo_limit,
                    phlo_price: receipt.phlo_price,
                    fee_rev: receipt.fee_rev.to_string(),
                    rev_spent: receipt.rev_spent.to_string(),
                    rev_ceiling: receipt.rev_ceiling.to_string(),
                    purses: receipt
                        .purses
                        .iter()
                        .map(|purse| models::casper::OfferedPurseReport {
                            address: purse.address.clone().into(),
                            resource_rev: purse.resource_rev.to_string(),
                            fee_rev: purse.fee_rev.to_string(),
                            post_balance: purse.post_balance,
                        })
                        .collect(),
                }),
        }
    }
}

/// Deploy details + reporting events
#[derive(Clone, Debug)]
pub struct DeployReportResult {
    // Changed by DR-134 (DR-70): the deploy is legacy or offered.
    // pub processed_deploy: ProcessedDeploy,
    pub processed_deploy: ProcessedUserDeploy,
    pub events: DeployReportEvents,
    // Added by DR-134 (DR-70): set only for an offered deploy.
    pub offered: Option<OfferedDeployReport>,
}

/// System deploy details + reporting events
#[derive(Clone, Debug)]
pub struct SystemDeployReportResult {
    pub processed_system_deploy: SystemDeployData,
    pub events: DeployReportEvents,
}

/// Aggregated replay results
#[derive(Clone, Debug)]
pub struct ReplayResult {
    pub deploy_report_result: Vec<DeployReportResult>,
    pub system_deploy_report_result: Vec<SystemDeployReportResult>,
    pub post_state_hash: ByteString,
}

type RhoReportingRspace = ReportingRspace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

/// Trait for reporting casper functionality
#[async_trait]
pub trait ReportingCasper: Send + Sync {
    async fn trace(&self, block: &BlockMessage) -> Result<ReplayResult, String>;

    /// Added by DR-134 (DR-70): `trace` with the Casper state that an offered
    /// block needs. A reporter that does not replay offered blocks keeps the
    /// behavior of `trace`.
    async fn trace_in(
        &self,
        block: &BlockMessage,
        _context: ReportContext<'_>,
    ) -> Result<ReplayResult, String> {
        self.trace(block).await
    }
}

/// No-op implementation that returns empty results
pub struct NoopReportingCasper;

#[async_trait]
impl ReportingCasper for NoopReportingCasper {
    async fn trace(&self, _block: &BlockMessage) -> Result<ReplayResult, String> {
        Ok(ReplayResult {
            deploy_report_result: Vec::new(),
            system_deploy_report_result: Vec::new(),
            post_state_hash: ByteString::from("empty".as_bytes()),
        })
    }
}

/// Real implementation using RhoReporter
pub struct RhoReporterCasper {
    rspace_store: RSpaceStore,
    block_dag_storage: BlockDagKeyValueStorage,
    replay_lock: Arc<crate::rust::util::rholang::runtime_manager::ReplayLock>,
    external_services: rholang::rust::interpreter::external_services::ExternalServices,
}

#[async_trait]
impl ReportingCasper for RhoReporterCasper {
    async fn trace(&self, block: &BlockMessage) -> Result<ReplayResult, String> {
        // Changed by DR-134 (DR-70): `trace_block` selects the replay by the
        // kind of block. Without Casper state it cannot report an offered block.
        self.trace_block(block, None).await
    }

    // Added by DR-134 (DR-70).
    async fn trace_in(
        &self,
        block: &BlockMessage,
        context: ReportContext<'_>,
    ) -> Result<ReplayResult, String> {
        self.trace_block(block, Some(context)).await
    }
}

impl RhoReporterCasper {
    /// Added by DR-134 (DR-66 item 5, DR-70): the kind of block selects the
    /// replay before any permit, runtime or reset, as validation does. A
    /// genesis and a block without user deploys keep the replay of dev. A
    /// body-only user deploy after genesis and a genesis that the legacy meter
    /// produced return the compatibility error. An offered block replays
    /// through the offered replay path.
    async fn trace_block(
        &self,
        block: &BlockMessage,
        context: Option<ReportContext<'_>>,
    ) -> Result<ReplayResult, String> {
        let is_genesis = block.header.parents_hash_list.is_empty();
        let mut legacy_deploys = Vec::with_capacity(block.body.deploys.len());
        let mut offered_deploys = Vec::with_capacity(block.body.deploys.len());
        for deploy in &block.body.deploys {
            match deploy {
                ProcessedUserDeploy::Legacy(legacy) => legacy_deploys.push(legacy.clone()),
                ProcessedUserDeploy::Offered(offered) => offered_deploys.push(offered),
            }
        }
        let legacy_metered_genesis = is_genesis && is_legacy_metered_genesis(&legacy_deploys);
        match (
            is_genesis,
            legacy_deploys.is_empty(),
            offered_deploys.as_slice(),
            context,
        ) {
            (true, _, [], _) if legacy_metered_genesis => {
                Err(LEGACY_METERED_GENESIS_UNSUPPORTED.to_string())
            }
            (true, _, [], _) => self.trace_legacy(block, legacy_deploys).await,
            (true, _, _, _) => Err("offered-funded replay is not active for this block".to_string()),
            (false, false, _, _) => Err(BODY_ONLY_DEPLOYS_UNSUPPORTED.to_string()),
            (false, true, [], _) => self.trace_legacy(block, legacy_deploys).await,
            // Changed by DR-134 (DR-65, gap G2): an offered block carries a
            // batch of offered members.
            // (false, true, [offered], Some(context)) => {
            //     self.trace_offered(block, offered, context).await
            // }
            // (false, true, [_], None) => Err(
            //     "an offered block report needs the runtime manager and the adopted policy of Casper"
            //         .to_string(),
            // ),
            // (false, true, _, _) => {
            //     Err("offered-funded replay requires one isolated user candidate".to_string())
            // }
            (false, true, offered @ [_, ..], Some(context)) => {
                self.trace_offered(block, offered, context).await
            }
            (false, true, [_, ..], None) => Err(
                "an offered block report needs the runtime manager and the adopted policy of Casper"
                    .to_string(),
            ),
        }
    }

    // Changed by DR-134 (DR-65, gap G2): the report of an offered block with a
    // batch of offered members. The former single-member version follows,
    // commented out.
    //
    // /// Added by DR-134 (DR-70): reports an offered block through the offered
    // /// replay path of validation. `certify_offered_draft` replays the offered
    // /// deploy natively and checks its committed evidence and settlement. The
    // /// system deploys then replay on the reporting runtime from the settled
    // /// root, as `replay_offered_system_continuation` does, so their events
    // /// are reported. The native replay of the user run logs its events to a
    // /// reporting logger, so the offered deploy reports them in a USER batch.
    // async fn trace_offered(
    //     &self,
    //     block: &BlockMessage,
    //     offered: &OfferedProcessedDeploy,
    //     context: ReportContext<'_>,
    // ) -> Result<ReplayResult, String> {
    //     use crate::rust::genesis::genesis::Genesis;
    //     use crate::rust::util::proto_util;
    //     use crate::rust::util::rholang::costacc::offered_grants::{
    //         grant_issue_definition, offered_grant_issue_call_limits,
    //     };
    //
    //     let adopted = context.adopted.ok_or_else(|| {
    //         "offered-funded replay requires authenticated adopted resource policy".to_string()
    //     })?;
    //     let _replay_permit = self
    //         .replay_lock
    //         .acquire_reporting()
    //         .await
    //         .map_err(|error| format!("Replay semaphore closed: {}", error))?;
    //     let dag = self
    //         .block_dag_storage
    //         .get_representation()
    //         .map_err(|e| format!("Failed to get DAG representation: {}", e))?;
    //     // The invalid-blocks map of validation: the senders of the block's own
    //     // slash targets (interpreter_util.rs, validate_block_checkpoint_with_policy).
    //     let slashed_hashes: Vec<models::rust::block_hash::BlockHash> = block
    //         .body
    //         .system_deploys
    //         .iter()
    //         .filter_map(|system_deploy| match system_deploy {
    //             ProcessedSystemDeploy::Succeeded {
    //                 system_deploy:
    //                     SystemDeployData::Slash {
    //                         invalid_block_hash, ..
    //                     },
    //                 ..
    //             } => Some(invalid_block_hash.clone()),
    //             _ => None,
    //         })
    //         .collect();
    //     let invalid_blocks = proto_util::slashed_block_senders(&dag, &slashed_hashes)
    //         .map_err(|error| format!("Failed to read the slashed block senders: {}", error))?;
    //     let pre_state_hash = proto_util::pre_state_hash(block);
    //     let block_data = BlockData::from_block(block);
    //     // Changed by DR-134 (DR-70): the native replay of the user run logs its
    //     // stored and matched operations to dev's reporting logger, so the
    //     // report shows the user run's events in a USER batch, as dev's report
    //     // shows a user deploy's events. The logger does not change the replay.
    //     // let certificate = context
    //     //     .runtime_manager
    //     //     .certify_offered_draft(
    //     //         offered,
    //     //         &pre_state_hash,
    //     //         &block_data,
    //     //         &invalid_blocks,
    //     //         adopted,
    //     //     )
    //     //     .await
    //     let user_run_events = Arc::new(std::sync::Mutex::new(Vec::new()));
    //     let user_run_logger: rholang::rust::interpreter::rho_runtime::RhoSpaceLogger =
    //         Box::new(ReportingLogger {
    //             report: Arc::new(std::sync::Mutex::new(Vec::new())),
    //             soft_report: user_run_events.clone(),
    //         });
    //     let certificate = context
    //         .runtime_manager
    //         .certify_offered_draft_with_logger(
    //             offered,
    //             &pre_state_hash,
    //             &block_data,
    //             &invalid_blocks,
    //             adopted,
    //             Some(user_run_logger),
    //         )
    //         .await
    //         .map_err(|error| {
    //             tracing::warn!(
    //                 target: "f1r3fly.casper.reporting",
    //                 deploy_id = %hex::encode(offered.identity_bytes()),
    //                 error = %error,
    //                 "Offered deploy replay failed; aborting block report"
    //             );
    //             format!(
    //                 "Offered deploy replay failed for {}: {}",
    //                 hex::encode(offered.identity_bytes()),
    //                 error
    //             )
    //         })?;
    //     let receipt =
    //         offered_settlement_receipt(offered, offered.identity_bytes(), block.block_hash.clone())
    //             .map_err(|error| {
    //                 format!("Failed to decode the offered settlement receipt: {}", error)
    //             })?;
    //
    //     let reporting_rspace = ReportingRuntime::create_reporting_rspace(self.rspace_store.clone())
    //         .map_err(|e| format!("Failed to create reporting rspace: {}", e))?;
    //     let mut offered_system_processes =
    //         vec![grant_issue_definition(offered_grant_issue_call_limits())];
    //     let mut reporting_runtime = ReportingRuntime::create_reporting_runtime(
    //         reporting_rspace,
    //         Genesis::default_mergeable_tags_arc(),
    //         &mut offered_system_processes,
    //         rholang::rust::interpreter::external_services::ExternalServices::noop(),
    //     )
    //     .await
    //     .map_err(|e| format!("Failed to create reporting runtime: {}", e))?;
    //     // Added by DR-134 (DR-70): the events of the registry bootstrap open
    //     // the report of the first deploy, as on dev, and the USER batch of the
    //     // user run follows. The USER batch is present even when it is empty, so
    //     // the node finds a marked report for every offered deploy.
    //     let mut offered_events = reporting_runtime
    //         .get_report()
    //         .map_err(|error| format!("Failed to collect the bootstrap report: {}", error))?;
    //     offered_events.push(ReportBatch {
    //         phase: ReportPhase::User,
    //         events: std::mem::take(
    //             &mut *user_run_events
    //                 .lock()
    //                 .map_err(|_| "the user-run report lock is poisoned".to_string())?,
    //         ),
    //     });
    //     let settled_root = Blake2b256Hash::from_bytes(certificate.settled_root().to_vec());
    //     let mut result = Self::replay_deploys(
    //         &mut reporting_runtime,
    //         &settled_root,
    //         &[],
    //         &block.body.system_deploys,
    //         with_cost_accounting(&block.header.parents_hash_list),
    //         &block_data,
    //         invalid_blocks,
    //     )
    //     .await?;
    //     result.deploy_report_result.insert(0, DeployReportResult {
    //         processed_deploy: ProcessedUserDeploy::Offered(offered.clone()),
    //         // Changed by DR-134 (DR-70): the bootstrap and USER batches.
    //         // events: Vec::new(),
    //         events: offered_events,
    //         offered: Some(OfferedDeployReport {
    //             native_cost_evidence: offered.native_cost_evidence_bytes().to_vec(),
    //             receipt,
    //         }),
    //     });
    //     Ok(result)
    // }
    //

    /// Added by DR-134 (DR-65, DR-70): reports an offered block through the
    /// offered replay path of validation. `certify_offered_batch` certifies
    /// the members in batch order, each from the settled root of the member
    /// before it, and the native replay of each user run logs its events to
    /// the reporting logger of its member. The events of the registry bootstrap
    /// open the report of the first member, as on dev, and each member reports
    /// its user run in its own USER batch. The system deploys then replay on
    /// the reporting runtime from the settled root of the last member.
    async fn trace_offered(
        &self,
        block: &BlockMessage,
        offered: &[&OfferedProcessedDeploy],
        context: ReportContext<'_>,
    ) -> Result<ReplayResult, String> {
        use crate::rust::genesis::genesis::Genesis;
        use crate::rust::util::proto_util;
        use crate::rust::util::rholang::costacc::offered_grants::{
            grant_issue_definition, offered_grant_issue_call_limits,
        };

        let adopted = context.adopted.ok_or_else(|| {
            "offered-funded replay requires authenticated adopted resource policy".to_string()
        })?;
        let _replay_permit = self
            .replay_lock
            .acquire_reporting()
            .await
            .map_err(|error| format!("Replay semaphore closed: {}", error))?;
        let dag = self
            .block_dag_storage
            .get_representation()
            .map_err(|e| format!("Failed to get DAG representation: {}", e))?;
        // The invalid-blocks map of validation: the senders of the block's own
        // slash targets (interpreter_util.rs, validate_block_checkpoint_with_policy).
        let slashed_hashes: Vec<models::rust::block_hash::BlockHash> = block
            .body
            .system_deploys
            .iter()
            .filter_map(|system_deploy| match system_deploy {
                ProcessedSystemDeploy::Succeeded {
                    system_deploy:
                        SystemDeployData::Slash {
                            invalid_block_hash, ..
                        },
                    ..
                } => Some(invalid_block_hash.clone()),
                _ => None,
            })
            .collect();
        let invalid_blocks = proto_util::slashed_block_senders(&dag, &slashed_hashes)
            .map_err(|error| format!("Failed to read the slashed block senders: {}", error))?;
        let pre_state_hash = proto_util::pre_state_hash(block);
        let block_data = BlockData::from_block(block);
        let mut user_run_events = Vec::with_capacity(offered.len());
        let mut loggers: Vec<Option<rholang::rust::interpreter::rho_runtime::RhoSpaceLogger>> =
            Vec::with_capacity(offered.len());
        for _ in offered {
            let events = Arc::new(std::sync::Mutex::new(Vec::new()));
            loggers.push(Some(Box::new(ReportingLogger {
                report: Arc::new(std::sync::Mutex::new(Vec::new())),
                soft_report: events.clone(),
            })));
            user_run_events.push(events);
        }
        let certificates = context
            .runtime_manager
            .certify_offered_batch(
                &pre_state_hash,
                offered,
                &block_data,
                &invalid_blocks,
                adopted,
                loggers,
            )
            .await
            .map_err(|error| {
                tracing::warn!(
                    target: "f1r3fly.casper.reporting",
                    error = %error,
                    "Offered batch replay failed; aborting block report"
                );
                format!("Offered batch replay failed: {}", error)
            })?;
        let settled = certificates
            .last()
            .map(|certificate| certificate.settled_root())
            .ok_or_else(|| "an offered block report certified no member".to_string())?;

        let reporting_rspace = ReportingRuntime::create_reporting_rspace(self.rspace_store.clone())
            .map_err(|e| format!("Failed to create reporting rspace: {}", e))?;
        let mut offered_system_processes =
            vec![grant_issue_definition(offered_grant_issue_call_limits())];
        let mut reporting_runtime = ReportingRuntime::create_reporting_runtime(
            reporting_rspace,
            Genesis::default_mergeable_tags_arc(),
            &mut offered_system_processes,
            rholang::rust::interpreter::external_services::ExternalServices::noop(),
        )
        .await
        .map_err(|e| format!("Failed to create reporting runtime: {}", e))?;
        // The events of the registry bootstrap open the report of the first
        // member, as on dev. The USER batch of each member is present even when
        // it is empty, so the node finds a marked report for every member.
        let mut bootstrap = Some(
            reporting_runtime
                .get_report()
                .map_err(|error| format!("Failed to collect the bootstrap report: {}", error))?,
        );
        let mut member_reports = Vec::with_capacity(offered.len());
        for (member, events) in offered.iter().zip(&user_run_events) {
            let receipt = offered_settlement_receipt(
                member,
                member.identity_bytes(),
                block.block_hash.clone(),
            )
            .map_err(|error| {
                format!("Failed to decode the offered settlement receipt: {}", error)
            })?;
            let mut member_events = bootstrap.take().unwrap_or_default();
            member_events.push(ReportBatch {
                phase: ReportPhase::User,
                events: std::mem::take(
                    &mut *events
                        .lock()
                        .map_err(|_| "the user-run report lock is poisoned".to_string())?,
                ),
            });
            member_reports.push(DeployReportResult {
                processed_deploy: ProcessedUserDeploy::Offered((*member).clone()),
                events: member_events,
                offered: Some(OfferedDeployReport {
                    native_cost_evidence: member.native_cost_evidence_bytes().to_vec(),
                    receipt,
                }),
            });
        }
        let settled_root = Blake2b256Hash::from_bytes(settled.to_vec());
        let mut result = Self::replay_deploys(
            &mut reporting_runtime,
            &settled_root,
            &[],
            &block.body.system_deploys,
            with_cost_accounting(&block.header.parents_hash_list),
            &block_data,
            invalid_blocks,
        )
        .await?;
        member_reports.append(&mut result.deploy_report_result);
        result.deploy_report_result = member_reports;
        Ok(result)
    }

    /// Changed by DR-134 (DR-70): the replay of dev, for a genesis and for a
    /// block without user deploys. `trace_block` splits the deploys.
    async fn trace_legacy(
        &self,
        block: &BlockMessage,
        legacy_deploys: Vec<ProcessedDeploy>,
    ) -> Result<ReplayResult, String> {
        use crate::rust::genesis::genesis::Genesis;
        use crate::rust::util::proto_util;

        // Changed by DR-134 (DR-70): `trace_block` splits the deploys.
        // let legacy_deploys = block
        //     .body
        //     .deploys
        //     .iter()
        //     .map(|deploy| {
        //         deploy.as_legacy().cloned().ok_or_else(|| {
        //             "offered-funded reporting requires independent native replay".to_string()
        //         })
        //     })
        //     .collect::<Result<Vec<_>, _>>()?;

        let _replay_permit = self
            .replay_lock
            .acquire_reporting()
            .await
            .map_err(|error| format!("Replay semaphore closed: {}", error))?;
        let reporting_rspace = ReportingRuntime::create_reporting_rspace(self.rspace_store.clone())
            .map_err(|e| format!("Failed to create reporting rspace: {}", e))?;

        let mergeable_tags = Genesis::default_mergeable_tags_arc();
        let mut extra_system_processes = Vec::new();
        let mut reporting_runtime = ReportingRuntime::create_reporting_runtime(
            reporting_rspace,
            mergeable_tags,
            &mut extra_system_processes,
            self.external_services.clone(),
        )
        .await
        .map_err(|e| format!("Failed to create reporting runtime: {}", e))?;

        let dag = self
            .block_dag_storage
            .get_representation()
            .map_err(|e| format!("Failed to get DAG representation: {}", e))?;

        let invalid_blocks_set = dag.invalid_blocks();

        let pre_state_hash_bytes = proto_util::pre_state_hash(block);
        let pre_state_hash = Blake2b256Hash::from_bytes_prost(&pre_state_hash_bytes);

        let block_data = BlockData::from_block(block);

        let unseen_blocks_set =
            proto_util::unseen_block_hashes(&dag, &block.justifications, Some(&block.block_hash))
                .map_err(|e| format!("Failed to get unseen block hashes: {}", e))?;

        let seen_invalid_blocks: HashMap<
            models::rust::block_hash::BlockHash,
            models::rust::validator::Validator,
        > = invalid_blocks_set
            .iter()
            .filter(|block_metadata| !unseen_blocks_set.contains(&block_metadata.block_hash))
            .map(|block_metadata| {
                (
                    block_metadata.block_hash.clone(),
                    block_metadata.sender.clone(),
                )
            })
            .collect();

        Self::replay_deploys(
            &mut reporting_runtime,
            &pre_state_hash,
            &legacy_deploys,
            &block.body.system_deploys,
            with_cost_accounting(&block.header.parents_hash_list),
            &block_data,
            seen_invalid_blocks,
        )
        .await
    }
}

impl RhoReporterCasper {
    /// Replay deploys and collect reporting events
    async fn replay_deploys(
        runtime: &mut ReportingRuntime,
        start_hash: &Blake2b256Hash,
        terms: &[ProcessedDeploy],
        system_deploys: &[ProcessedSystemDeploy],
        with_cost_accounting: bool,
        block_data: &BlockData,
        invalid_blocks: HashMap<
            models::rust::block_hash::BlockHash,
            models::rust::validator::Validator,
        >,
    ) -> Result<ReplayResult, String> {
        runtime
            .reset(start_hash)
            .await
            .map_err(|error| format!("Failed to reset reporting runtime: {}", error))?;

        runtime.set_block_data(block_data.clone()).await;
        runtime.set_invalid_blocks(invalid_blocks).await;

        // Reporting replay of the genesis block re-executes blessed
        // deploys that bind FS native URNs (fs_generator composes the
        // FsGenesis bundle via `new fsRead(`rho:io:fs:native:1.0.0/
        // read`), ...`).  Those URNs are gated behind the reducer's
        // `filter_fs_native_urns` flag (slice 5.32) which defaults to
        // TRUE in every freshly-created reducer — including this
        // reporting runtime.  Mirrors the play-side (slice 5.33) and
        // state-replay (slice 5.35) toggles.  Non-genesis reporting
        // keeps the default so user deploys that would reject under
        // regular replay also reject here.
        let _fs_filter_guard = if !with_cost_accounting {
            Some(crate::rust::rholang::runtime::FsNativeFilterGuard::disable(
                &runtime.runtime.reducer,
            ))
        } else {
            None
        };

        let mut deploy_results = Vec::new();
        for (idx, term) in terms.iter().enumerate() {
            tracing::debug!(
                target: "f1r3fly.casper.reporting",
                deploy_index = idx,
                total_deploys = terms.len(),
                "Replaying deploy for report"
            );

            runtime
                .replay_deploy_e(with_cost_accounting, term)
                .await
                .map_err(|error| {
                    // Logged where it is raised: the failure now aborts the whole report, and
                    // several callers of the block-report API discard the error, so this is the
                    // only record that survives regardless of which one asked.
                    tracing::warn!(
                        target: "f1r3fly.casper.reporting",
                        deploy_index = idx,
                        deploy_sig = %hex::encode(&term.deploy.sig),
                        error = %error,
                        "Deploy replay failed; aborting block report"
                    );
                    format!(
                        "Deploy replay failed at index {} for {}: {}",
                        idx,
                        hex::encode(&term.deploy.sig),
                        error
                    )
                })?;
            let events = runtime.get_report().map_err(|error| {
                format!(
                    "Failed to collect deploy report at index {}: {}",
                    idx, error
                )
            })?;

            deploy_results.push(DeployReportResult {
                // Changed by DR-134 (DR-70): the deploy is legacy or offered.
                // processed_deploy: term.clone(),
                processed_deploy: ProcessedUserDeploy::Legacy(term.clone()),
                events,
                // Added by DR-134 (DR-70).
                offered: None,
            });
        }

        let mut system_deploy_results = Vec::new();
        for (idx, system_deploy) in system_deploys.iter().enumerate() {
            tracing::debug!(
                target: "f1r3fly.casper.reporting",
                system_deploy_index = idx,
                total_system_deploys = system_deploys.len(),
                "Replaying system deploy for report"
            );

            runtime
                .replay_block_system_deploy(block_data, system_deploy)
                .await
                .map_err(|error| {
                    tracing::warn!(
                        target: "f1r3fly.casper.reporting",
                        system_deploy_index = idx,
                        error = %error,
                        "System deploy replay failed; aborting block report"
                    );
                    format!("System deploy replay failed at index {}: {}", idx, error)
                })?;
            let events = runtime.get_report().map_err(|error| {
                format!(
                    "Failed to collect system deploy report at index {}: {}",
                    idx, error
                )
            })?;

            let system_deploy_data = match system_deploy {
                ProcessedSystemDeploy::Succeeded { system_deploy, .. } => system_deploy.clone(),
                ProcessedSystemDeploy::Failed { .. } => SystemDeployData::Empty,
            };

            system_deploy_results.push(SystemDeployReportResult {
                processed_system_deploy: system_deploy_data,
                events,
            });
        }

        let checkpoint = runtime.create_checkpoint().await;
        let post_state_hash = ByteString::from(checkpoint.root.to_bytes_prost());

        Ok(ReplayResult {
            deploy_report_result: deploy_results,
            system_deploy_report_result: system_deploy_results,
            post_state_hash,
        })
    }
}

/// Factory function to create noop reporting casper
pub fn noop() -> Arc<dyn ReportingCasper> { Arc::new(NoopReportingCasper) }

/// Factory function to create rho reporter with real reporting capability
pub fn rho_reporter(
    rspace_store: &RSpaceStore,
    block_dag_storage: &BlockDagKeyValueStorage,
    replay_lock: Arc<crate::rust::util::rholang::runtime_manager::ReplayLock>,
    external_services: rholang::rust::interpreter::external_services::ExternalServices,
) -> Arc<dyn ReportingCasper> {
    Arc::new(RhoReporterCasper {
        rspace_store: rspace_store.clone(),
        block_dag_storage: block_dag_storage.clone(),
        replay_lock,
        external_services,
    })
}

/// Genesis is the only block without parents, and it replays without cost
/// accounting because no precharge or refund system deploy is wrapped around it.
fn with_cost_accounting(parent_hashes: &[prost::bytes::Bytes]) -> bool { !parent_hashes.is_empty() }

/// ReportingRuntime wraps RhoRuntimeImpl with ReportingRspace to enable event collection
pub struct ReportingRuntime {
    runtime: rholang::rust::interpreter::rho_runtime::RhoRuntimeImpl,
    space: RhoReportingRspace,
}

impl ReportingRuntime {
    /// Get reporting events from the space, segmented and tagged with
    /// the phase that was in force when each segment was flushed.
    pub fn get_report(&self) -> Result<DeployReportEvents, RSpaceError> { self.space.get_report() }

    /// Reset the runtime to a specific state hash
    pub async fn reset(
        &mut self,
        root: &Blake2b256Hash,
    ) -> Result<(), rholang::rust::interpreter::errors::InterpreterError> {
        self.runtime.reset(root).await
    }

    /// Set block data for the runtime
    pub async fn set_block_data(&self, block_data: BlockData) {
        RhoRuntime::set_block_data(&self.runtime, block_data).await;
    }

    /// Set invalid blocks for the runtime
    pub async fn set_invalid_blocks(
        &self,
        invalid_blocks: std::collections::HashMap<
            models::rust::block_hash::BlockHash,
            models::rust::validator::Validator,
        >,
    ) {
        RhoRuntime::set_invalid_blocks(&self.runtime, invalid_blocks).await;
    }

    /// Create a checkpoint and return the root hash
    pub async fn create_checkpoint(&mut self) -> rspace_plus_plus::rspace::checkpoint::Checkpoint {
        RhoRuntime::create_checkpoint(&mut self.runtime).await
    }

    /// Replay a deploy and collect reporting events
    pub async fn replay_deploy_e(
        &mut self,
        with_cost_accounting: bool,
        processed_deploy: &ProcessedDeploy,
    ) -> Result<(), crate::rust::errors::CasperError> {
        use crate::rust::rholang::replay_runtime::ReplayRuntimeOps;

        let mut replay_ops = ReplayRuntimeOps::new_from_runtime(self.runtime.clone());

        replay_ops
            .replay_deploy_e(with_cost_accounting, processed_deploy)
            .await?;

        self.runtime = replay_ops.runtime_ops.runtime;

        Ok(())
    }

    /// Replay a system deploy and collect reporting events
    pub async fn replay_block_system_deploy(
        &mut self,
        block_data: &BlockData,
        processed_system_deploy: &models::rust::casper::protocol::casper_message::ProcessedSystemDeploy,
    ) -> Result<(), crate::rust::errors::CasperError> {
        use crate::rust::rholang::replay_runtime::ReplayRuntimeOps;

        // Create ReplayRuntimeOps from the runtime
        let mut replay_ops = ReplayRuntimeOps::new_from_runtime(self.runtime.clone());

        // Replay the system deploy
        replay_ops
            .replay_block_system_deploy(block_data, processed_system_deploy)
            .await?;

        // Update the runtime from replay_ops
        self.runtime = replay_ops.runtime_ops.runtime;

        Ok(())
    }
}

/// Factory functions for creating ReportingRuntime
impl ReportingRuntime {
    /// Create a ReportingRspace from RSpaceStore
    pub fn create_reporting_rspace(store: RSpaceStore) -> Result<RhoReportingRspace, RSpaceError> {
        use rholang::rust::interpreter::matcher::r#match::Matcher;
        use rspace_plus_plus::rspace::r#match::Match;

        let matcher: Arc<Box<dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>>> =
            Arc::new(Box::new(Matcher));

        RhoReportingRspace::create(store, matcher)
    }

    /// Create a ReportingRuntime from a ReportingRspace
    ///
    /// Bootstraps registry without checkpoint
    /// `createCheckpoint` is called at the end of `replayDeploys`, not here.
    /// The reporting space is ephemeral and reset to `preStateHash` before replay.
    pub async fn create_reporting_runtime(
        reporting_space: RhoReportingRspace,
        mergeable_tags: std::sync::Arc<
            std::collections::HashMap<
                Par,
                rspace_plus_plus::rspace::merger::merging_logic::MergeType,
            >,
        >,
        extra_system_processes: &mut Vec<Definition>,
        external_services: rholang::rust::interpreter::external_services::ExternalServices,
    ) -> Result<Self, String> {
        use rholang::rust::interpreter::rho_runtime::create_replay_rho_runtime;

        let runtime = create_replay_rho_runtime(
            reporting_space.clone(),
            mergeable_tags,
            false,
            extra_system_processes,
            external_services,
        )
        .await
        .map_err(|e| e.to_string())?;

        rholang::rust::interpreter::rho_runtime::bootstrap_registry(&runtime).await;

        Ok(ReportingRuntime {
            runtime,
            space: reporting_space,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn genesis_deploys_are_not_cost_accounted() {
        assert!(!with_cost_accounting(&[]));
        assert!(with_cost_accounting(&[prost::bytes::Bytes::from_static(
            b"parent",
        )]));
    }
}
