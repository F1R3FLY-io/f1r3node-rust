// See casper/src/main/scala/coop/rchain/casper/ReportingCasper.scala

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use block_storage::rust::dag::block_dag_key_value_storage::BlockDagKeyValueStorage;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use models::rust::block::state_hash::StateHash;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, ProcessedDeploy, ProcessedSystemDeploy, SystemDeployData,
};
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::system_processes::{BlockData, Definition};
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::reporting_rspace::{ReportingEvent, ReportingRspace};
use rspace_plus_plus::rspace::rspace::RSpaceStore;
use shared::rust::ByteString;

/// Deploy details + reporting events
#[derive(Clone, Debug)]
pub struct DeployReportResult {
    pub processed_deploy: ProcessedDeploy,
    pub events: Vec<Vec<ReportingEvent<Par, BindPattern, ListParWithRandom, TaggedContinuation>>>,
}

/// System deploy details + reporting events
#[derive(Clone, Debug)]
pub struct SystemDeployReportResult {
    pub processed_system_deploy: SystemDeployData,
    pub events: Vec<Vec<ReportingEvent<Par, BindPattern, ListParWithRandom, TaggedContinuation>>>,
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
    block_store: KeyValueBlockStore,
    block_dag_storage: BlockDagKeyValueStorage,
    external_services: rholang::rust::interpreter::external_services::ExternalServices,
}

#[async_trait]
impl ReportingCasper for RhoReporterCasper {
    async fn trace(&self, block: &BlockMessage) -> Result<ReplayResult, String> {
        use crate::rust::genesis::genesis::Genesis;
        use crate::rust::util::proto_util;

        let reporting_rspace = ReportingRuntime::create_reporting_rspace(self.rspace_store.clone())
            .map_err(|e| format!("Failed to create reporting rspace: {}", e))?;

        let mergeable_tags = std::sync::Arc::new(Genesis::default_mergeable_tags());
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

        let genesis = self
            .block_store
            .get_approved_block()
            .map_err(|e| format!("Failed to get approved block: {}", e))?;

        let is_genesis = genesis
            .as_ref()
            .map(|g| block.block_hash == g.candidate.block.block_hash)
            .unwrap_or(false);

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

        let replay_terms = block
            .body
            .deploys
            .iter()
            .filter(|deploy| !deploy.is_admission_rejected())
            .cloned()
            .collect::<Vec<_>>();
        if !is_genesis {
            self.verify_admission(&pre_state_hash, &replay_terms, &block_data.sender.bytes)
                .await?;
        }

        let replay = self
            .replay_deploys(
                &mut reporting_runtime,
                &pre_state_hash,
                &replay_terms,
                &block.body.system_deploys,
                if is_genesis {
                    crate::rust::rholang::replay_runtime::ReplayBlockKind::Genesis
                } else {
                    crate::rust::rholang::replay_runtime::ReplayBlockKind::Ordinary
                },
                &block_data,
                seen_invalid_blocks,
            )
            .await?;
        let expected_post_state = block.body.state.post_state_hash.to_vec();
        if replay.post_state_hash != expected_post_state {
            return Err(format!(
                "reporting replay post-state {} differs from block post-state {}",
                hex::encode(&replay.post_state_hash),
                hex::encode(expected_post_state)
            ));
        }
        Ok(replay)
    }
}

impl RhoReporterCasper {
    /// Replay deploys and collect reporting events.
    ///
    /// L-30-COV-2 (Phase 7 whole-review) note: reporting is a
    /// read-only trace pass — it does NOT append to a WAL and does
    /// NOT trigger snapshot writes.  The `ReportingRuntime` wraps a
    /// `RhoRuntimeImpl` whose reducer's `wal` is present but its
    /// `SnapshotWriter` hook (populated only in the primary
    /// `runtime.rs` boot path) stays `None`.  As a result, invoking
    /// `report(block)` on a validator whose primary runtime is
    /// configured with WAL+snapshots will replay the fs-native
    /// syscalls (fd allocation, reads, stats) but the resulting
    /// entries stay in the buffer for the deploy's local scope and
    /// are dropped when the reporting runtime is torn down.  This
    /// is desired: reporting is a debug/introspection tool, and
    /// admin-triggered reports must not perturb the on-disk
    /// snapshot cadence of the primary chain runtime.
    async fn replay_deploys(
        &self,
        runtime: &mut ReportingRuntime,
        start_hash: &Blake2b256Hash,
        terms: &[ProcessedDeploy],
        system_deploys: &[ProcessedSystemDeploy],
        block_kind: crate::rust::rholang::replay_runtime::ReplayBlockKind,
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

        // H-P7-7 review fix (Phase 7 whole-review round): mirror the
        // URN-filter toggle that the primary `replay_deploys` in
        // `casper::rholang::replay_runtime.rs` performs for genesis
        // replay.  Reporting typically operates on post-genesis
        // blocks, but if a report is ever requested for the genesis
        // block the reporting runtime re-executes the FsGenesis
        // ProcessedDeploy which binds `rho:io:fs:native:*` URNs.
        // Without the exemption those bindings fail with a
        // ReduceError just as they did in the pre-slice-31-round-2
        // primary `replay_deploys`.
        //
        // Cost-accounted merge: use `block_kind == ReplayBlockKind::Genesis`
        // as the genesis-mode signal (replaces fileio's earlier
        // `!with_cost_accounting` parameter — cost-accounted removed
        // that param and channels the same signal through block_kind).
        // RAII guard drops on all exit paths including panic unwind.
        let _filter_exemption =
            if block_kind == crate::rust::rholang::replay_runtime::ReplayBlockKind::Genesis {
                Some(runtime.runtime.exempt_fs_native_urn_filter())
            } else {
                None
            };

        if block_kind == crate::rust::rholang::replay_runtime::ReplayBlockKind::Ordinary
            && !crate::rust::rholang::replay_runtime::has_exactly_one_successful_terminal_close(
                system_deploys,
            )
        {
            return Err(
                "ordinary reporting replay requires exactly one successful terminal close deploy"
                    .to_string(),
            );
        }

        let mut deploy_results = Vec::new();
        let mut current_root: StateHash = start_hash.to_bytes_prost();
        for (idx, term) in terms.iter().enumerate() {
            tracing::debug!(
                target: "f1r3fly.casper.reporting",
                deploy_index = idx,
                total_deploys = terms.len(),
                "Replaying deploy for report"
            );

            let effect = format!("user:{}", hex::encode(&term.deploy.sig));
            let validate_witness =
                crate::rust::rholang::replay_runtime::ReplayRuntimeOps::validate_effect_pre_state(
                    &effect,
                    &term.pre_state_hash,
                    &term.post_state_hash,
                    &current_root,
                )
                .map_err(|error| format!("reporting effect pre-state failed: {error}"))?;
            let purse_snapshot =
                if block_kind == crate::rust::rholang::replay_runtime::ReplayBlockKind::Ordinary {
                    Some(self.purse_snapshot_at_root(term, &current_root).await?)
                } else {
                    None
                };
            runtime
                .replay_deploy_e(block_kind, term, purse_snapshot.as_ref())
                .await
                .map_err(|error| format!("reporting user deploy replay failed: {error}"))?;
            let events = runtime
                .get_report()
                .map_err(|error| format!("reporting event collection failed: {error}"))?;
            let actual_post = runtime.create_checkpoint().await.root.to_bytes_prost();
            if validate_witness {
                crate::rust::rholang::replay_runtime::ReplayRuntimeOps::validate_effect_post_state(
                    &effect,
                    &term.post_state_hash,
                    &actual_post,
                )
                .map_err(|error| format!("reporting effect post-state failed: {error}"))?;
            }
            current_root = actual_post;

            deploy_results.push(DeployReportResult {
                processed_deploy: term.clone(),
                events,
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

            let effect = format!("system:{idx}");
            let (recorded_pre, recorded_post) = system_deploy.state_hashes();
            let validate_witness =
                crate::rust::rholang::replay_runtime::ReplayRuntimeOps::validate_effect_pre_state(
                    &effect,
                    recorded_pre,
                    recorded_post,
                    &current_root,
                )
                .map_err(|error| format!("reporting effect pre-state failed: {error}"))?;
            runtime
                .replay_block_system_deploy(block_data, system_deploy)
                .await
                .map_err(|error| format!("reporting system deploy replay failed: {error}"))?;
            let events = runtime
                .get_report()
                .map_err(|error| format!("reporting event collection failed: {error}"))?;
            let actual_post = runtime.create_checkpoint().await.root.to_bytes_prost();
            if validate_witness {
                crate::rust::rholang::replay_runtime::ReplayRuntimeOps::validate_effect_post_state(
                    &effect,
                    recorded_post,
                    &actual_post,
                )
                .map_err(|error| format!("reporting effect post-state failed: {error}"))?;
            }
            current_root = actual_post;

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

    async fn purse_snapshot_at_root(
        &self,
        term: &ProcessedDeploy,
        root: &StateHash,
    ) -> Result<crate::rust::util::rholang::acceptance::ReplayPurseSnapshot, String> {
        use rholang::rust::interpreter::matcher::r#match::Matcher;
        use rholang::rust::interpreter::rho_runtime::create_runtime_from_kv_store;
        use rspace_plus_plus::rspace::r#match::Match;

        use crate::rust::genesis::genesis::Genesis;
        use crate::rust::rholang::runtime::RuntimeOps;

        let matcher: Arc<Box<dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>>> =
            Arc::new(Box::new(Matcher));
        let runtime = create_runtime_from_kv_store(
            self.rspace_store.clone(),
            Arc::new(Genesis::default_mergeable_tags()),
            true,
            &mut Vec::new(),
            matcher,
            self.external_services.clone(),
        )
        .await;
        let mut runtime_ops = RuntimeOps::new(runtime);
        runtime_ops
            .runtime
            .reset(&Blake2b256Hash::from_bytes_prost(root))
            .await
            .map_err(|error| format!("reporting purse snapshot reset failed: {error}"))?;
        let reader = crate::rust::util::rholang::acceptance::RuntimeOpsSupplyReader {
            runtime_ops: &runtime_ops,
            pre_state_root: root
                .as_ref()
                .try_into()
                .expect("consensus state roots are Blake2b-256"),
        };
        crate::rust::util::rholang::acceptance::replay_purse_snapshot(term, &reader)
            .await
            .map_err(|error| format!("reporting purse snapshot failed: {error}"))
    }

    async fn verify_admission(
        &self,
        start_hash: &Blake2b256Hash,
        terms: &[ProcessedDeploy],
        fee_recipient: &[u8],
    ) -> Result<(), String> {
        use rholang::rust::interpreter::matcher::r#match::Matcher;
        use rholang::rust::interpreter::rho_runtime::create_runtime_from_kv_store;
        use rspace_plus_plus::rspace::r#match::Match;

        use crate::rust::genesis::genesis::Genesis;
        use crate::rust::rholang::runtime::RuntimeOps;

        let matcher: Arc<Box<dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>>> =
            Arc::new(Box::new(Matcher));
        let runtime = create_runtime_from_kv_store(
            self.rspace_store.clone(),
            Arc::new(Genesis::default_mergeable_tags()),
            true,
            &mut Vec::new(),
            matcher,
            self.external_services.clone(),
        )
        .await;
        let mut runtime_ops = RuntimeOps::new(runtime);
        runtime_ops
            .runtime
            .reset(start_hash)
            .await
            .map_err(|error| format!("reporting admission reset failed: {error}"))?;
        let reader = crate::rust::util::rholang::acceptance::RuntimeOpsSupplyReader {
            runtime_ops: &runtime_ops,
            pre_state_root: start_hash
                .bytes()
                .try_into()
                .expect("consensus state roots are Blake2b-256"),
        };
        crate::rust::util::rholang::acceptance::verify_state_bound_replay_admission(
            terms,
            fee_recipient,
            &reader,
        )
        .await
        .map_err(|error| format!("reporting admission verification failed: {error}"))?;
        Ok(())
    }
}

/// Factory function to create noop reporting casper
pub fn noop() -> Arc<dyn ReportingCasper> { Arc::new(NoopReportingCasper) }

/// Factory function to create rho reporter with real reporting capability
pub fn rho_reporter(
    rspace_store: &RSpaceStore,
    block_store: &KeyValueBlockStore,
    block_dag_storage: &BlockDagKeyValueStorage,
    external_services: rholang::rust::interpreter::external_services::ExternalServices,
) -> Arc<dyn ReportingCasper> {
    Arc::new(RhoReporterCasper {
        rspace_store: rspace_store.clone(),
        block_store: block_store.clone(),
        block_dag_storage: block_dag_storage.clone(),
        external_services,
    })
}

/// ReportingRuntime wraps RhoRuntimeImpl with ReportingRspace to enable event collection
pub struct ReportingRuntime {
    runtime: rholang::rust::interpreter::rho_runtime::RhoRuntimeImpl,
    space: RhoReportingRspace,
}

impl ReportingRuntime {
    /// Get reporting events from the space
    pub fn get_report(
        &self,
    ) -> Result<
        Vec<Vec<ReportingEvent<Par, BindPattern, ListParWithRandom, TaggedContinuation>>>,
        RSpaceError,
    > {
        self.space.get_report()
    }

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
        block_kind: crate::rust::rholang::replay_runtime::ReplayBlockKind,
        processed_deploy: &ProcessedDeploy,
        purse_snapshot: Option<&crate::rust::util::rholang::acceptance::ReplayPurseSnapshot>,
    ) -> Result<(), crate::rust::errors::CasperError> {
        use crate::rust::rholang::replay_runtime::ReplayRuntimeOps;

        let mut replay_ops = ReplayRuntimeOps::new_from_runtime(self.runtime.clone());

        replay_ops
            .replay_deploy_e_with_snapshot(block_kind, processed_deploy, purse_snapshot)
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
        .await;

        rholang::rust::interpreter::rho_runtime::bootstrap_registry(&runtime).await;

        Ok(ReportingRuntime {
            runtime,
            space: reporting_space,
        })
    }
}
