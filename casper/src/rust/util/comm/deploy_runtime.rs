use std::process::exit;
use std::time::{SystemTime, UNIX_EPOCH};

use crypto::rust::private_key::PrivateKey;
use crypto::rust::public_key::PublicKey;
// Changed by DR-134 (DR-66 item 2): `deploy` signs an owner-direct offered
// envelope in place of a body-only deploy.
// use crypto::rust::signatures::signed::Signed;
use futures::FutureExt;
use models::casper::{
    BlockQuery, BlocksQuery, BondStatusQuery, ContinuationAtNameQuery, FindDeployQuery,
    IsFinalizedQuery, MachineVerifyQuery, VisualizeDagQuery,
};
// Changed by DR-134 (DR-66 item 2).
// use models::rust::casper::protocol::casper_message::DeployData;
use models::rust::cost_deploy_data::DeployData;
use models::rust::phlo_schedule::PhloGenesisPolicy;
use prost::bytes::Bytes;

use crate::rust::util::comm::grpc_deploy_service::DeployService;
use crate::rust::util::comm::grpc_propose_service::ProposeService;
use crate::rust::util::comm::listen_at_name::{self, Name};
use crate::rust::util::comm::ServiceResult;
use crate::rust::util::rholang::costacc::owner_offer::{
    default_resource_schedule, owner_direct_offer,
};

pub struct DeployRuntime;

impl DeployRuntime {
    /// Like Scala: gracefulExit(program: F[Either[Seq[String], String]])
    async fn graceful_exit<F>(program: F)
    where F: std::future::Future<Output = ServiceResult<String>> {
        let attempted = program.await;

        // Note: usage of the sync IO is not a good practice in the async context. The reason of such impl
        // is because ProposeService API is async and that DeployRuntime is used only in CLI context where it will not influence the performance.
        match attempted {
            Ok(msg) => {
                println!("{msg}");
            }
            Err(errors) => {
                for e in errors {
                    eprintln!("{e}");
                }
                exit(1);
            }
        }
    }

    pub async fn propose<S: ProposeService>(mut svc: S, print_unmatched_sends: bool) {
        Self::graceful_exit(
            svc.propose(print_unmatched_sends)
                .map(|res| res.map(|rs| format!("Response: {rs}"))),
        )
        .await
    }

    pub async fn get_block<S: DeployService>(svc: &mut S, hash: String) {
        Self::graceful_exit(svc.get_block(BlockQuery { hash })).await
    }

    pub async fn get_blocks<S: DeployService>(svc: &mut S, depth: i32) {
        Self::graceful_exit(svc.get_blocks(BlocksQuery { depth })).await
    }

    pub async fn visualize_dag<S: DeployService>(
        svc: &mut S,
        depth: i32,
        show_justification_lines: bool,
    ) {
        Self::graceful_exit(svc.visualize_dag(VisualizeDagQuery {
            depth,
            show_justification_lines,
            start_block_number: 0,
        }))
        .await
    }

    pub async fn machine_verifiable_dag<S: DeployService>(svc: &mut S) {
        Self::graceful_exit(svc.machine_verifiable_dag(MachineVerifyQuery {})).await
    }

    pub async fn listen_for_continuation_at_name<S: DeployService + Clone>(
        svc: &mut S,
        names: Vec<Name>,
    ) {
        // Use graceful_exit like other methods, but with polling logic
        Self::graceful_exit(async {
            listen_at_name::listen_at_names_until_changes(names, |pars| {
                let mut value = svc.clone();
                async move {
                    value
                        .listen_for_continuation_at_name(ContinuationAtNameQuery {
                            depth: i32::MAX,
                            names: pars,
                        })
                        .await
                }
            })
            .await
            .map(|res| format!("{:?}", res))
        })
        .await
    }

    pub async fn find_deploy<S: DeployService>(svc: &mut S, deploy_id: &[u8]) {
        Self::graceful_exit(svc.find_deploy(FindDeployQuery {
            deploy_id: Bytes::copy_from_slice(deploy_id),
        }))
        .await
    }

    /// Accepts a Rholang source file and deploys it
    // Changed by DR-134 (DR-66 item 2): the deploy is an owner-direct offer.
    // It signs the genesis schedule of the shard: the schedule of
    // `resource_policy` (the hexadecimal `resource-policy` of the shard's
    // genesis configuration) or, without it, the default resource schedule of
    // the shipped configurations.
    #[allow(clippy::too_many_arguments)]
    pub async fn deploy_file_program<S: DeployService>(
        svc: &mut S,
        phlo_limit: i64,
        phlo_price: i64,
        valid_after_block: i64,
        private_key: &PrivateKey,
        file: &str,
        shard_id: &str,
        resource_policy: Option<&str>,
    ) {
        Self::graceful_exit(async {
            // Try reading file (Scala: Try(Source.fromFile(...)).toEither)
            let code = match tokio::fs::read_to_string(file).await {
                Ok(code) => code,
                Err(e) => return Err(vec![format!("Error with given file:\n{e}")]),
            };

            let now_ms = match SystemTime::now().duration_since(UNIX_EPOCH) {
                Ok(duration) => duration.as_millis() as i64,
                Err(e) => return Err(vec![format!("Clock error: {e}")]),
            };

            // Changed by DR-134 (DR-66 item 2): the body-only deploy is
            // replaced by the owner-direct offer below.
            // let d = DeployData {
            //     term: code,
            //     time_stamp: now_ms,
            //     phlo_price,
            //     phlo_limit,
            //     valid_after_block_number: valid_after_block,
            //     shard_id: shard_id.to_string(),
            //     expiration_timestamp: None,
            // };
            //
            // // Signed(d, Secp256k1, privateKey)
            // let signed = match Signed::create(
            //     d,
            //     Box::new(crypto::rust::signatures::secp256k1::Secp256k1),
            //     private_key.clone(),
            // ) {
            //     Ok(signed) => signed,
            //     Err(e) => return Err(vec![format!("Failed to sign deploy: {e}")]),
            // };
            let Ok(phlo_limit) = u64::try_from(phlo_limit) else {
                return Err(vec!["The phlo limit must be nonnegative".to_string()]);
            };
            let Ok(phlo_price) = u64::try_from(phlo_price) else {
                return Err(vec!["The phlo price must be nonnegative".to_string()]);
            };
            let genesis_policy = match resource_policy {
                Some(encoded) => {
                    let bytes = hex::decode(encoded)
                        .map_err(|e| vec![format!("Invalid resource policy hex: {e}")])?;
                    Some(
                        PhloGenesisPolicy::decode(&bytes)
                            .map_err(|e| vec![format!("Invalid resource policy: {e}")])?,
                    )
                }
                None => None,
            };
            let schedule = match &genesis_policy {
                Some(policy) => policy
                    .schedule()
                    .map_err(|e| vec![format!("Invalid resource policy schedule: {e}")])?,
                None => default_resource_schedule(shard_id),
            };
            let body = DeployData {
                term: code,
                language: "rholang".to_string(),
                time_stamp: now_ms,
                valid_after_block_number: valid_after_block,
                shard_id: shard_id.to_string(),
                expiration_timestamp: None,
                authority_presentations: Vec::new(),
            };
            let signed =
                match owner_direct_offer(private_key, body, phlo_limit, phlo_price, &schedule) {
                    Ok(signed) => signed,
                    Err(e) => return Err(vec![format!("Failed to sign deploy: {e}")]),
                };

            let resp = match svc.deploy(signed).await {
                Ok(resp) => resp,
                Err(e) => return Err(e),
            };
            Ok(format!("Response: {resp}"))
        })
        .await
    }

    pub async fn last_finalized_block<S: DeployService>(svc: &mut S) {
        Self::graceful_exit(svc.last_finalized_block()).await
    }

    pub async fn is_finalized<S: DeployService>(svc: &mut S, block_hash: String) {
        Self::graceful_exit(svc.is_finalized(IsFinalizedQuery { hash: block_hash })).await
    }

    pub async fn bond_status<S: DeployService>(svc: &mut S, public_key: &PublicKey) {
        Self::graceful_exit(svc.bond_status(BondStatusQuery {
            public_key: Bytes::from(public_key.bytes.clone()),
        }))
        .await
    }

    pub async fn status<S: DeployService>(svc: &mut S) { Self::graceful_exit(svc.status()).await }
}
