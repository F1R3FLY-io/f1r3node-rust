use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use crypto::rust::public_key::PublicKey;
use models::rhoapi::Par;
use models::rust::casper::protocol::casper_message::BlockMessage;
use models::rust::normalizer_env::normalizer_env_from_deploy;
use prost::bytes::Bytes;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::system_processes::BlockData;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;

use super::body::StateHash;
use super::concurrent_record::ConcurrentRecord;
use super::record::ExecutionRecord;
use crate::rust::errors::CasperError;
use crate::rust::util::event_converter;
use crate::rust::util::rholang::costacc::close_block_deploy::CloseBlockDeploy;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;
use crate::rust::util::rholang::system_deploy_enum::SystemDeployEnum;
use crate::rust::util::rholang::system_deploy_util;
use crate::rust::util::rholang::tools::Tools;

#[async_trait]
pub trait ExecutionBackend: Send + Sync {
    type Record: ExecutionRecord;

    async fn execute(
        &self,
        pre_root: &StateHash,
        block: &BlockMessage,
    ) -> Result<Self::Record, CasperError>;
    async fn replay(
        &self,
        record: &Self::Record,
        block: &BlockMessage,
    ) -> Result<StateHash, CasperError>;
}

pub struct ConcurrentBackend {
    runtime_manager: Arc<RuntimeManager>,
}

impl ConcurrentBackend {
    pub fn new(runtime_manager: Arc<RuntimeManager>) -> Self {
        ConcurrentBackend { runtime_manager }
    }
}

struct Prepared {
    terms: Vec<(usize, Par, Blake2b512Random)>,
    failed: Vec<bool>,
    budget: i64,
    parsing: i64,
}

struct Outcome {
    failed: Vec<bool>,
    exhausted: bool,
    cost: u64,
}

pub(super) fn block_data(block: &BlockMessage) -> BlockData {
    BlockData {
        time_stamp: block.header.timestamp,
        block_number: block.body.state.block_number,
        sender: PublicKey::from_bytes(&block.sender),
        seq_num: block.seq_num,
    }
}

fn prepare(block: &BlockMessage) -> Prepared {
    let mut prepared = Prepared {
        terms: Vec::new(),
        failed: vec![false; block.body.deploys.len()],
        budget: 0,
        parsing: 0,
    };
    for (i, processed) in block.body.deploys.iter().enumerate() {
        prepared.budget = prepared
            .budget
            .saturating_add(processed.phlo_limit().max(0));
        prepared.parsing = prepared
            .parsing
            .saturating_add(processed.term().len() as i64);
        let Some(legacy) = processed.as_legacy() else {
            prepared.failed[i] = true;
            continue;
        };
        let deploy = &legacy.deploy;
        let parsed = normalizer_env_from_deploy(deploy)
            .map_err(|e| e.to_string())
            .and_then(|env| {
                Compiler::source_to_adt_with_normalizer_env(&deploy.data.term, env)
                    .map_err(|e| e.to_string())
            });
        match parsed {
            Ok(par) => prepared.terms.push((
                i,
                par,
                Tools::unforgeable_name_rng(&deploy.pk, deploy.data.time_stamp),
            )),
            Err(_) => prepared.failed[i] = true,
        }
    }
    prepared
}

pub(crate) fn exhausts(error: &InterpreterError) -> bool {
    match error {
        InterpreterError::OutOfPhlogistonsError => true,
        InterpreterError::AggregateError { interpreter_errors } => {
            interpreter_errors.iter().any(exhausts)
        }
        _ => false,
    }
}

async fn run_concurrently(runtime: &RhoRuntimeImpl, prepared: &Prepared) -> Outcome {
    let mut failed = prepared.failed.clone();
    let _legacy_scope = runtime.cost.enter_legacy_scope();
    runtime
        .cost
        .set(Cost::create(prepared.budget, "v6 block budget"));
    if runtime
        .cost
        .charge_legacy(Cost::create(prepared.parsing, "v6 parsing"))
        .is_err()
    {
        return Outcome {
            failed,
            exhausted: true,
            cost: prepared.budget.max(0) as u64,
        };
    }
    let handles: Vec<_> = prepared
        .terms
        .iter()
        .cloned()
        .map(|(i, par, rand)| {
            let reducer = runtime.reducer.clone();
            tokio::spawn(async move { (i, reducer.inj(par, rand).await) })
        })
        .collect();
    let mut exhausted = false;
    for handle in handles {
        match handle.await {
            Ok((_, Ok(()))) => {}
            Ok((i, Err(error))) => {
                failed[i] = true;
                exhausted |= exhausts(&error);
            }
            Err(_) => exhausted = true,
        }
    }
    let left = runtime.cost.get().value.max(0);
    let cost = if exhausted {
        prepared.budget
    } else {
        prepared.budget - left
    };
    Outcome {
        failed,
        exhausted,
        cost: cost.max(0) as u64,
    }
}

fn envelope_ids(block: &BlockMessage) -> Vec<Bytes> {
    block
        .body
        .deploys
        .iter()
        .map(|deploy| Bytes::copy_from_slice(deploy.identity_bytes()))
        .collect()
}

pub(super) fn close_block(block: &BlockMessage) -> SystemDeployEnum {
    SystemDeployEnum::Close(CloseBlockDeploy {
        initial_rand: system_deploy_util::generate_close_deploy_random_seed_from_pk(
            PublicKey::from_bytes(&block.sender),
            block.seq_num,
        ),
    })
}

fn interp(error: InterpreterError) -> CasperError { CasperError::InterpreterError(error) }

fn root(hash: &StateHash) -> Blake2b256Hash { Blake2b256Hash::from_bytes_prost(hash) }

#[async_trait]
impl ExecutionBackend for ConcurrentBackend {
    type Record = ConcurrentRecord;

    async fn execute(
        &self,
        pre_root: &StateHash,
        block: &BlockMessage,
    ) -> Result<ConcurrentRecord, CasperError> {
        let mut runtime = self.runtime_manager.spawn_runtime().await?;
        runtime.reset(&root(pre_root)).await.map_err(interp)?;
        runtime.set_block_data(block_data(block)).await;
        runtime.set_invalid_blocks(HashMap::new()).await;
        let fallback = runtime.create_soft_checkpoint().await;
        let prepared = prepare(block);
        let outcome = run_concurrently(&runtime, &prepared).await;
        let log = runtime.take_event_log().await;
        let mid_root: Bytes = if outcome.exhausted {
            runtime.revert_to_soft_checkpoint(fallback).await;
            pre_root.clone()
        } else {
            runtime.create_checkpoint().await.root.to_bytes_prost()
        };
        let (post_root, _, system_deploys) = self
            .runtime_manager
            .compute_state(
                &mid_root,
                vec![],
                vec![close_block(block)],
                block_data(block),
                None,
            )
            .await?;
        Ok(ConcurrentRecord {
            target: block.block_hash.clone(),
            pre_root: pre_root.clone(),
            mid_root,
            post_root,
            envelope_ids: envelope_ids(block),
            comm_log: log
                .into_iter()
                .map(event_converter::to_casper_event)
                .collect(),
            cost: outcome.cost,
            aborted: outcome.exhausted,
            failed: outcome.failed,
            system_deploys,
        })
    }

    async fn replay(
        &self,
        record: &ConcurrentRecord,
        block: &BlockMessage,
    ) -> Result<StateHash, CasperError> {
        let mismatch =
            |what: &str| CasperError::RuntimeError(format!("v6 record replay mismatch: {}", what));
        let mut runtime = self.runtime_manager.spawn_replay_runtime().await?;
        runtime
            .reset(&root(&record.pre_root))
            .await
            .map_err(interp)?;
        runtime.set_block_data(block_data(block)).await;
        runtime.set_invalid_blocks(HashMap::new()).await;
        runtime
            .rig(
                record
                    .comm_log
                    .iter()
                    .map(event_converter::to_rspace_event)
                    .collect(),
            )
            .await
            .map_err(interp)?;
        let prepared = prepare(block);
        let outcome = run_concurrently(&runtime, &prepared).await;
        let mid_root = if record.aborted {
            if !outcome.exhausted {
                return Err(mismatch("record claims the block budget was exhausted"));
            }
            if record.mid_root != record.pre_root {
                return Err(mismatch("an aborted record must leave the pre-state"));
            }
            record.pre_root.clone()
        } else {
            if outcome.exhausted {
                return Err(mismatch("replay exhausted the block budget"));
            }
            runtime.check_replay_data().await.map_err(interp)?;
            let mid = runtime.create_checkpoint().await.root.to_bytes_prost();
            if mid != record.mid_root {
                return Err(mismatch("user-phase root"));
            }
            if outcome.cost != record.cost {
                return Err(mismatch("cost"));
            }
            mid
        };
        self.runtime_manager
            .replay_compute_state(
                &mid_root,
                vec![],
                record.system_deploys.clone(),
                &block_data(block),
                None,
                false,
            )
            .await
    }
}

#[cfg(test)]
mod tests {
    use models::rust::casper::protocol::casper_message::{
        Body, F1r3flyState, Header, ProcessedDeploy,
    };

    use super::*;
    use crate::rust::util::construct_deploy;

    fn empty_block(timestamp: i64, block_number: i64, seq_num: i32) -> BlockMessage {
        BlockMessage {
            block_hash: Bytes::new(),
            header: Header {
                parents_hash_list: vec![],
                timestamp,
                version: 1,
                extra_bytes: Bytes::new(),
            },
            body: Body {
                state: F1r3flyState {
                    pre_state_hash: Bytes::new(),
                    post_state_hash: Bytes::new(),
                    bonds: vec![],
                    block_number,
                },
                deploys: vec![],
                rejected_deploys: vec![],
                system_deploys: vec![],
                extra_bytes: Bytes::new(),
                applied_from_scope: vec![],
                merge_base: Bytes::new(),
            },
            justifications: vec![],
            sender: vec![0xAAu8; 33].into(),
            seq_num,
            sig: Bytes::new(),
            sig_algorithm: String::new(),
            shard_id: String::new(),
            extra_bytes: Bytes::new(),
        }
    }

    #[test]
    fn exhausts_recognizes_out_of_phlogistons() {
        assert!(exhausts(&InterpreterError::OutOfPhlogistonsError));
    }

    #[test]
    fn exhausts_recurses_through_aggregate_errors() {
        let aggregate = InterpreterError::AggregateError {
            interpreter_errors: vec![
                InterpreterError::ReduceError("unrelated".to_string()),
                InterpreterError::OutOfPhlogistonsError,
            ],
        };
        assert!(
            exhausts(&aggregate),
            "an aggregate carrying a phlo exhaustion must still be flagged as exhausted",
        );
    }

    #[test]
    fn exhausts_returns_false_for_other_errors() {
        assert!(!exhausts(&InterpreterError::ReduceError(
            "boom".to_string()
        )));
        let aggregate = InterpreterError::AggregateError {
            interpreter_errors: vec![InterpreterError::ReduceError("boom".to_string())],
        };
        assert!(
            !exhausts(&aggregate),
            "an aggregate without a phlo exhaustion must not be flagged",
        );
    }

    #[test]
    fn prepare_marks_an_unparseable_deploy_as_failed() {
        let bad = construct_deploy::source_deploy(
            "not valid rholang garbage".to_string(),
            0,
            Some(1000),
            None,
            None,
            None,
            None,
        )
        .expect("sign the unparseable deploy");
        let good =
            construct_deploy::basic_deploy_data(42, None, None).expect("sign the valid deploy");
        let mut block = empty_block(0, 0, 1);
        block.body.deploys = vec![
            ProcessedDeploy {
                deploy: bad,
                cost: models::rhoapi::PCost { cost: 0 },
                deploy_log: vec![],
                is_failed: false,
                system_deploy_error: None,
            },
            ProcessedDeploy {
                deploy: good,
                cost: models::rhoapi::PCost { cost: 0 },
                deploy_log: vec![],
                is_failed: false,
                system_deploy_error: None,
            },
        ]
        .into_iter()
        .map(Into::into)
        .collect();

        let prepared = prepare(&block);
        assert_eq!(prepared.failed.len(), 2);
        assert!(
            prepared.failed[0],
            "the unparseable deploy must be marked failed"
        );
        assert!(
            !prepared.failed[1],
            "the valid deploy must not be marked failed"
        );
        assert_eq!(
            prepared.terms.len(),
            1,
            "only the parseable deploy contributes a term",
        );
        assert_eq!(
            prepared.terms[0].0, 1,
            "the surviving term keeps its original index",
        );
    }

    #[test]
    fn prepare_sums_the_phlo_budget_across_deploys() {
        let one = construct_deploy::source_deploy(
            "@1!(1)".to_string(),
            0,
            Some(50),
            None,
            None,
            None,
            None,
        )
        .expect("sign deploy one");
        let two = construct_deploy::source_deploy(
            "@2!(2)".to_string(),
            1,
            Some(70),
            None,
            None,
            None,
            None,
        )
        .expect("sign deploy two");
        let mut block = empty_block(0, 0, 1);
        block.body.deploys = vec![
            ProcessedDeploy {
                deploy: one,
                cost: models::rhoapi::PCost { cost: 0 },
                deploy_log: vec![],
                is_failed: false,
                system_deploy_error: None,
            },
            ProcessedDeploy {
                deploy: two,
                cost: models::rhoapi::PCost { cost: 0 },
                deploy_log: vec![],
                is_failed: false,
                system_deploy_error: None,
            },
        ]
        .into_iter()
        .map(Into::into)
        .collect();

        let prepared = prepare(&block);
        assert_eq!(
            prepared.budget, 120,
            "budget is the sum of each deploy's phlo limit (50 + 70)",
        );
    }

    #[test]
    fn block_data_mirrors_the_block_header_and_sender() {
        let block = empty_block(1_700_000_000, 42, 7);
        let data = block_data(&block);
        assert_eq!(data.time_stamp, 1_700_000_000);
        assert_eq!(data.block_number, 42);
        assert_eq!(data.seq_num, 7);
        assert_eq!(data.sender.bytes.as_ref(), block.sender.as_ref());
    }
}
