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

use crate::rust::errors::CasperError;
use crate::rust::util::event_converter;
use crate::rust::util::rholang::costacc::close_block_deploy::CloseBlockDeploy;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;
use crate::rust::util::rholang::system_deploy_enum::SystemDeployEnum;
use crate::rust::util::rholang::system_deploy_util;
use crate::rust::util::rholang::tools::Tools;

use super::body::{Record, StateHash};

#[async_trait]
pub trait ExecutionBackend: Send + Sync {
    async fn execute(
        &self,
        pre_root: &StateHash,
        block: &BlockMessage,
    ) -> Result<Record, CasperError>;
    async fn replay(&self, record: &Record, block: &BlockMessage)
        -> Result<StateHash, CasperError>;
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

fn block_data(block: &BlockMessage) -> BlockData {
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
        let deploy = &processed.deploy;
        prepared.budget = prepared
            .budget
            .saturating_add(deploy.data.phlo_limit.max(0));
        prepared.parsing = prepared
            .parsing
            .saturating_add(deploy.data.term.len() as i64);
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

fn exhausts(error: &InterpreterError) -> bool {
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
    runtime
        .cost
        .set(Cost::create(prepared.budget, "v6 block budget"));
    if runtime
        .cost
        .charge(Cost::create(prepared.parsing, "v6 parsing"))
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

fn close_block(block: &BlockMessage) -> SystemDeployEnum {
    SystemDeployEnum::Close(CloseBlockDeploy {
        initial_rand: system_deploy_util::generate_close_deploy_random_seed_from_pk(
            PublicKey::from_bytes(&block.sender),
            block.seq_num,
        ),
    })
}

fn interp(error: InterpreterError) -> CasperError {
    CasperError::InterpreterError(error)
}

fn root(hash: &StateHash) -> Blake2b256Hash {
    Blake2b256Hash::from_bytes_prost(hash)
}

#[async_trait]
impl ExecutionBackend for ConcurrentBackend {
    async fn execute(
        &self,
        pre_root: &StateHash,
        block: &BlockMessage,
    ) -> Result<Record, CasperError> {
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
        Ok(Record {
            target: block.block_hash.clone(),
            pre_root: pre_root.clone(),
            mid_root,
            post_root,
            envelope_sigs: block
                .body
                .deploys
                .iter()
                .map(|d| d.deploy.sig.clone())
                .collect(),
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
        record: &Record,
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
