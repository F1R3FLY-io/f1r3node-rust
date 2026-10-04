use std::collections::BTreeSet;

mod genesis;
mod vm;
pub use genesis::{GenesisSettings, VaultAllocation, initialize_runtime};
pub use vm::DeterministicVm;

use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use cordial_consensus::{
    Chain, CommittedExecution, DurableBlocklace, ExecutionReceipt, MAX_PAYLOAD_BYTES,
};
use crypto::rust::hash::blake2b256::Blake2b256;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::signed::Signed;
use k256::ecdsa::VerifyingKey;
use models::casper::DeployDataProto;
use models::rust::casper::protocol::casper_message::DeployData;
use prost::Message;
use rholang::rust::interpreter::{
    external_services::ExternalServices, system_processes::BlockData,
};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use serde::{Deserialize, Serialize};

pub const MAX_BATCH_DEPLOYS: usize = 64;
pub const MAX_BATCH_PHLO: i64 = 10_000_000;
pub const MAX_DEPLOY_BYTES: usize = MAX_PAYLOAD_BYTES - 1024;

#[derive(Clone, PartialEq, Message)]
pub struct DeployBatch {
    #[prost(uint32, tag = "1")]
    pub version: u32,
    #[prost(int64, tag = "2")]
    pub timestamp: i64,
    #[prost(message, repeated, tag = "3")]
    pub deploys: Vec<DeployDataProto>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum DeployOutcome {
    Succeeded,
    Failed,
    Duplicate,
    Rejected,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DeployResult {
    pub id: Option<[u8; 32]>,
    pub outcome: DeployOutcome,
    pub cost: u64,
    pub rejection: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecutionSummary {
    pub version: u32,
    pub timestamp: Option<i64>,
    pub rejection: Option<String>,
    pub deploys: Vec<DeployResult>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Store(#[from] cordial_consensus::Error),
    #[error("Invalid Cordial deploy batch: {0}")]
    Input(String),
    #[error("Cordial execution failed: {0}")]
    Runtime(String),
}

pub struct CommittedRholang {
    runtime: RuntimeManager,
    chain: Chain,
    vm: DeterministicVm,
}

pub fn encode_batch(timestamp: i64, deploys: Vec<Signed<DeployData>>) -> Result<Vec<u8>, Error> {
    let bytes = DeployBatch {
        version: 1,
        timestamp,
        deploys: deploys.into_iter().map(DeployData::to_proto).collect(),
    }
    .encode_to_vec();
    decode_batch(&bytes).map_err(|code| Error::Input(code.into()))?;
    Ok(bytes)
}

fn decode_batch(bytes: &[u8]) -> Result<DeployBatch, &'static str> {
    if bytes.len() > MAX_PAYLOAD_BYTES {
        return Err("batch-limit");
    }
    let batch = DeployBatch::decode(bytes).map_err(|_| "invalid-batch")?;
    if batch.version != 1
        || batch.timestamp < 0
        || batch.deploys.len() > MAX_BATCH_DEPLOYS
        || batch.encode_to_vec() != bytes
    {
        return Err("invalid-batch");
    }
    Ok(batch)
}

fn validate_deploy(
    proto: DeployDataProto,
    shard: &str,
    timestamp: i64,
    index: i64,
) -> Result<Signed<DeployData>, &'static str> {
    if proto.sig_algorithm != "secp256k1"
        || proto.deployer.len() != 65
        || proto.deployer.first() != Some(&4)
    {
        return Err("unsupported-signature");
    }
    let deploy = DeployData::from_proto(proto).map_err(|_| "invalid-signature")?;
    let data = &deploy.data;
    if data.shard_id != shard {
        return Err("shard-mismatch");
    }
    if data.phlo_limit <= 0
        || data.phlo_limit > MAX_BATCH_PHLO
        || data.phlo_price <= 0
        || data.phlo_limit.checked_mul(data.phlo_price).is_none()
    {
        return Err("invalid-phlo");
    }
    if data.time_stamp < 0
        || data.time_stamp > timestamp
        || data.expiration_timestamp.is_some_and(|value| value < 0)
        || data.is_expired_at(timestamp)
    {
        return Err("invalid-deploy-time");
    }
    if data.valid_after_block_number < 0 || data.valid_after_block_number >= index {
        return Err("invalid-deploy-height");
    }
    Ok(deploy)
}

fn deploy_id(deploy: &Signed<DeployData>) -> [u8; 32] {
    let mut bytes = b"cordial:rholang-deploy:v1\0".to_vec();
    bytes.extend_from_slice(&deploy.sig);
    Blake2b256::hash(bytes)
        .try_into()
        .expect("Blake2b256 produces 32 bytes")
}

#[async_trait::async_trait]
impl cordial_consensus::CommittedExecutor for CommittedRholang {
    async fn execute_next(
        &self,
        store: &mut DurableBlocklace,
    ) -> Result<bool, cordial_consensus::Error> {
        CommittedRholang::execute_next(self, store)
            .await
            .map(|receipt| receipt.is_some())
            .map_err(|error| cordial_consensus::Error::Execution(error.to_string()))
    }
}

impl cordial_consensus::Application for CommittedRholang {
    fn validate_submission(
        &self,
        payload: &[u8],
    ) -> Result<cordial_consensus::Submission, cordial_consensus::Error> {
        if payload.len() > MAX_DEPLOY_BYTES {
            return Err(cordial_consensus::Error::Packet(
                "deploy size limit exceeded".into(),
            ));
        }
        let proto = DeployDataProto::decode(payload)
            .map_err(|_| cordial_consensus::Error::Packet("invalid deploy protobuf".into()))?;
        let deploy = validate_deploy(proto, &self.chain.spec().shard, current_time()?, i64::MAX)
            .map_err(|reason| cordial_consensus::Error::Packet(reason.into()))?;
        Ok(cordial_consensus::Submission {
            id: deploy_id(&deploy),
            payload: DeployData::to_proto(deploy).encode_to_vec(),
        })
    }

    fn proposal_payload(
        &self,
        submissions: &[cordial_consensus::Submission],
    ) -> Result<(Vec<u8>, usize), cordial_consensus::Error> {
        let mut deploys = Vec::new();
        let mut phlo = 0i64;
        let mut bytes = 64usize;
        for submission in submissions.iter().take(MAX_BATCH_DEPLOYS) {
            let proto = DeployDataProto::decode(submission.payload.as_slice()).map_err(|_| {
                cordial_consensus::Error::Corrupt("invalid persisted deploy".into())
            })?;
            let deploy = DeployData::from_proto(proto).map_err(|_| {
                cordial_consensus::Error::Corrupt("invalid persisted deploy signature".into())
            })?;
            if deploy_id(&deploy) != submission.id {
                return Err(cordial_consensus::Error::Corrupt(
                    "persisted deploy identity mismatch".into(),
                ));
            }
            let Some(next) = phlo
                .checked_add(deploy.data.phlo_limit)
                .filter(|next| *next <= MAX_BATCH_PHLO)
            else {
                break;
            };
            let size = bytes + submission.payload.len() + 8;
            if size > MAX_PAYLOAD_BYTES {
                break;
            }
            phlo = next;
            bytes = size;
            deploys.push(deploy);
        }
        if deploys.is_empty() {
            return Ok((vec![], 0));
        }
        let included = deploys.len();
        let payload = encode_batch(current_time()?, deploys)
            .map_err(|error| cordial_consensus::Error::Execution(error.to_string()))?;
        Ok((payload, included))
    }
}

fn current_time() -> Result<i64, cordial_consensus::Error> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| {
            cordial_consensus::Error::Configuration("system clock is before Unix epoch".into())
        })?
        .as_millis()
        .try_into()
        .map_err(|_| cordial_consensus::Error::Configuration("system clock overflow".into()))
}

impl CommittedRholang {
    pub fn new(mut runtime: RuntimeManager, chain: Chain) -> Result<Self, Error> {
        runtime.external_services = ExternalServices::noop();
        Ok(Self {
            runtime,
            chain,
            vm: DeterministicVm::start()?,
        })
    }

    pub async fn execute_next(
        &self,
        store: &mut DurableBlocklace,
    ) -> Result<Option<ExecutionReceipt>, Error> {
        let checkpoint = store.execution_checkpoint();
        if checkpoint.chain != self.chain.fingerprint() {
            return Err(Error::Runtime("execution chain mismatch".into()));
        }
        if !self
            .runtime
            .has_root(&Blake2b256Hash::from_bytes(checkpoint.state.to_vec()))
            .map_err(|error| Error::Runtime(error.to_string()))?
        {
            return Err(Error::Runtime("execution pre-state is unavailable".into()));
        }
        let Some(request) = store.next_execution()? else {
            return Ok(None);
        };
        let mut summary = ExecutionSummary {
            version: 1,
            timestamp: None,
            rejection: None,
            deploys: vec![],
        };
        if request.payload.is_empty() {
            return self.finish(store, &request, request.pre_state, summary, vec![]);
        }
        let batch = match decode_batch(&request.payload) {
            Ok(batch) => batch,
            Err(code) => {
                summary.rejection = Some(code.into());
                return self.finish(store, &request, request.pre_state, summary, vec![]);
            }
        };
        let index = i64::try_from(request.index)
            .map_err(|_| Error::Runtime("execution height overflow".into()))?;
        let sequence = i32::try_from(request.index)
            .map_err(|_| Error::Runtime("VM sequence limit reached".into()))?;
        summary.timestamp = Some(batch.timestamp);
        let mut terms = Vec::new();
        let mut slots = Vec::new();
        let mut ids = Vec::new();
        let mut seen = BTreeSet::new();
        let mut phlo = 0i64;
        for proto in batch.deploys {
            let deploy =
                match validate_deploy(proto, &self.chain.spec().shard, batch.timestamp, index) {
                    Ok(deploy) => deploy,
                    Err(code) => {
                        summary.deploys.push(DeployResult {
                            id: None,
                            outcome: DeployOutcome::Rejected,
                            cost: 0,
                            rejection: Some(code.into()),
                        });
                        continue;
                    }
                };
            let id = deploy_id(&deploy);
            if !seen.insert(id) || store.has_executed_deploy(&id)? {
                summary.deploys.push(DeployResult {
                    id: Some(id),
                    outcome: DeployOutcome::Duplicate,
                    cost: 0,
                    rejection: None,
                });
                continue;
            }
            phlo = phlo
                .checked_add(deploy.data.phlo_limit)
                .ok_or_else(|| Error::Input("batch phlo overflow".into()))?;
            if phlo > MAX_BATCH_PHLO {
                summary.rejection = Some("batch-phlo-limit".into());
                summary.deploys.clear();
                return self.finish(store, &request, request.pre_state, summary, vec![]);
            }
            slots.push(summary.deploys.len());
            summary.deploys.push(DeployResult {
                id: Some(id),
                outcome: DeployOutcome::Succeeded,
                cost: 0,
                rejection: None,
            });
            ids.push(id);
            terms.push(deploy);
        }
        if terms.is_empty() {
            return self.finish(store, &request, request.pre_state, summary, vec![]);
        }
        let creator = VerifyingKey::from_sec1_bytes(&request.object.creator.0)
            .map_err(|_| Error::Runtime("invalid committed creator".into()))?;
        let context = BlockData {
            time_stamp: batch.timestamp,
            block_number: index,
            sender: PublicKey::from_bytes(creator.to_encoded_point(false).as_bytes()),
            seq_num: sequence,
        };
        let runtime = self.runtime.clone();
        let pre_state = prost::bytes::Bytes::copy_from_slice(&request.pre_state);
        let (root, processed, system) = self
            .vm
            .run(async move {
                runtime
                    .compute_state(&pre_state, terms, vec![], context, None)
                    .await
            })
            .await?
            .map_err(|error| Error::Runtime(error.to_string()))?;
        if processed.len() != slots.len() || !system.is_empty() {
            return Err(Error::Runtime(
                "VM result does not match submitted deploys".into(),
            ));
        }
        for ((position, processed), expected) in slots.into_iter().zip(processed).zip(&ids) {
            if &deploy_id(&processed.deploy) != expected {
                return Err(Error::Runtime("VM reordered deploy results".into()));
            }
            summary.deploys[position].outcome =
                if processed.is_failed || processed.system_deploy_error.is_some() {
                    DeployOutcome::Failed
                } else {
                    DeployOutcome::Succeeded
                };
            summary.deploys[position].cost = processed.cost.cost;
        }
        let post_state: [u8; 32] = root
            .as_ref()
            .try_into()
            .map_err(|_| Error::Runtime("invalid VM state root".into()))?;
        if !self
            .runtime
            .has_root(&Blake2b256Hash::from_bytes(post_state.to_vec()))
            .map_err(|error| Error::Runtime(error.to_string()))?
        {
            return Err(Error::Runtime("execution post-state is unavailable".into()));
        }
        self.finish(store, &request, post_state, summary, ids)
    }

    fn finish(
        &self,
        store: &mut DurableBlocklace,
        request: &CommittedExecution,
        post_state: [u8; 32],
        summary: ExecutionSummary,
        deploy_ids: Vec<[u8; 32]>,
    ) -> Result<Option<ExecutionReceipt>, Error> {
        let result =
            serde_json::to_vec(&summary).map_err(|error| Error::Runtime(error.to_string()))?;
        let receipt = ExecutionReceipt {
            index: request.index,
            object: request.object.clone(),
            pre_state: request.pre_state,
            post_state,
            result,
            deploy_ids,
        };
        store.acknowledge_execution(receipt.clone())?;
        Ok(Some(receipt))
    }
}
