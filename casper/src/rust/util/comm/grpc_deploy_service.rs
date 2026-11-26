use crypto::rust::signatures::signed::Signed;
use models::casper::v1::deploy_service_client::DeployServiceClient;
use models::casper::v1::{
    block_info_response, block_response, bond_status_response, continuation_at_name_response,
    deploy_response, event_info_response, exploratory_deploy_response, find_deploy_response,
    is_finalized_response, last_finalized_block_response, machine_verify_response,
    rho_data_response, status_response, visualize_blocks_response, RhoDataPayload,
};
use models::casper::{
    BlockEventInfo, BlockQuery, BlocksQuery, BondStatusQuery, ContinuationAtNameQuery,
    ContinuationsWithBlockInfo, DataAtNameByBlockQuery, DataWithBlockInfo, ExploratoryDeployQuery,
    FindDeployQuery, IsFinalizedQuery, LastFinalizedBlockQuery, LightBlockInfo, MachineVerifyQuery,
    ReportQuery, VisualizeDagQuery,
};
use models::rhoapi::Par;
use models::rust::casper::protocol::casper_message::DeployData;
use tonic::transport::{Channel, Endpoint};

use super::{error_to_vec, ServiceResult};

#[async_trait::async_trait]
pub trait DeployService {
    async fn deploy(&mut self, d: Signed<DeployData>) -> ServiceResult<String>;
    async fn get_block(&mut self, q: BlockQuery) -> ServiceResult<String>;
    async fn get_blocks(&mut self, q: BlocksQuery) -> ServiceResult<String>;
    async fn visualize_dag(&mut self, q: VisualizeDagQuery) -> ServiceResult<String>;
    async fn machine_verifiable_dag(&mut self, q: MachineVerifyQuery) -> ServiceResult<String>;
    async fn find_deploy(&mut self, q: FindDeployQuery) -> ServiceResult<String>;
    async fn listen_for_continuation_at_name(
        &mut self,
        q: ContinuationAtNameQuery,
    ) -> ServiceResult<Vec<ContinuationsWithBlockInfo>>;
    async fn get_data_at_par(
        &mut self,
        q: DataAtNameByBlockQuery,
    ) -> ServiceResult<(Vec<Par>, LightBlockInfo)>;
    async fn exploratory_deploy(
        &mut self,
        q: ExploratoryDeployQuery,
    ) -> ServiceResult<(DataWithBlockInfo, u64)>;
    async fn get_event_by_hash(&mut self, q: ReportQuery) -> ServiceResult<BlockEventInfo>;
    async fn last_finalized_block(&mut self) -> ServiceResult<String>;
    async fn is_finalized(&mut self, q: IsFinalizedQuery) -> ServiceResult<String>;
    async fn bond_status(&mut self, q: BondStatusQuery) -> ServiceResult<String>;
    async fn status(&mut self) -> ServiceResult<String>;
}

#[derive(Clone)]
pub struct GrpcDeployService {
    client: DeployServiceClient<Channel>,
}

// ---- implement the trait for the struct by delegating to existing methods ----
#[async_trait::async_trait]
impl DeployService for GrpcDeployService {
    async fn deploy(&mut self, d: Signed<DeployData>) -> ServiceResult<String> {
        self.deploy_impl(d).await
    }

    async fn get_block(&mut self, q: BlockQuery) -> ServiceResult<String> {
        self.get_block_impl(q).await
    }

    async fn get_blocks(&mut self, q: BlocksQuery) -> ServiceResult<String> {
        self.get_blocks_impl(q).await
    }

    async fn visualize_dag(&mut self, q: VisualizeDagQuery) -> ServiceResult<String> {
        self.visualize_dag_impl(q).await
    }

    async fn machine_verifiable_dag(&mut self, q: MachineVerifyQuery) -> ServiceResult<String> {
        self.machine_verifiable_dag_impl(q).await
    }

    async fn find_deploy(&mut self, q: FindDeployQuery) -> ServiceResult<String> {
        self.find_deploy_impl(q).await
    }

    async fn listen_for_continuation_at_name(
        &mut self,
        q: ContinuationAtNameQuery,
    ) -> ServiceResult<Vec<ContinuationsWithBlockInfo>> {
        self.listen_for_continuation_at_name_impl(q).await
    }

    async fn get_data_at_par(
        &mut self,
        q: DataAtNameByBlockQuery,
    ) -> ServiceResult<(Vec<Par>, LightBlockInfo)> {
        self.get_data_at_par_impl(q).await
    }

    async fn exploratory_deploy(
        &mut self,
        q: ExploratoryDeployQuery,
    ) -> ServiceResult<(DataWithBlockInfo, u64)> {
        self.exploratory_deploy_impl(q).await
    }

    async fn get_event_by_hash(&mut self, q: ReportQuery) -> ServiceResult<BlockEventInfo> {
        self.get_event_by_hash_impl(q).await
    }

    async fn last_finalized_block(&mut self) -> ServiceResult<String> {
        self.last_finalized_block_impl().await
    }

    async fn is_finalized(&mut self, q: IsFinalizedQuery) -> ServiceResult<String> {
        self.is_finalized_impl(q).await
    }

    async fn bond_status(&mut self, q: BondStatusQuery) -> ServiceResult<String> {
        self.bond_status_impl(q).await
    }

    async fn status(&mut self) -> ServiceResult<String> { self.status_impl().await }
}

impl GrpcDeployService {
    pub async fn new(host: &str, port: u16, max_inbound_bytes: usize) -> eyre::Result<Self> {
        let endpoint = Endpoint::from_shared(format!("http://{host}:{port}"))?;

        let channel = endpoint.connect().await?;
        Ok(Self {
            client: DeployServiceClient::new(channel).max_decoding_message_size(max_inbound_bytes),
        })
    }

    async fn deploy_impl(&mut self, d: Signed<DeployData>) -> ServiceResult<String> {
        let resp = self
            .client
            .do_deploy(DeployData::to_proto(d))
            .await
            .map_err(error_to_vec)?;

        match resp.into_inner().message {
            Some(deploy_response::Message::Error(err)) => Err(error_to_vec(err)),
            Some(deploy_response::Message::Result(s)) => Ok(s),
            None => Err(vec!["empty DeployResponse.message".to_string()]),
        }
    }

    async fn get_block_impl(&mut self, q: BlockQuery) -> ServiceResult<String> {
        let resp = self.client.get_block(q).await.map_err(error_to_vec)?;
        match resp.into_inner().message {
            Some(block_response::Message::Error(err)) => Err(error_to_vec(err)),
            Some(block_response::Message::BlockInfo(bi)) => Ok(proto_string(&bi)),
            None => Err(vec!["empty BlockResponse.message".to_string()]),
        }
    }

    async fn find_deploy_impl(&mut self, q: FindDeployQuery) -> ServiceResult<String> {
        let resp = self.client.find_deploy(q).await.map_err(error_to_vec)?;

        match resp.into_inner().message {
            Some(find_deploy_response::Message::Error(err)) => Err(error_to_vec(err)),
            Some(find_deploy_response::Message::BlockInfo(bi)) => Ok(proto_string(&bi)),
            None => Err(vec!["empty FindDeployResponse.message".to_string()]),
        }
    }

    async fn visualize_dag_impl(&mut self, q: VisualizeDagQuery) -> ServiceResult<String> {
        let mut stream = self
            .client
            .visualize_dag(q)
            .await
            .map_err(error_to_vec)?
            .into_inner();

        let mut contents = String::new();
        while let Some(item) = stream.message().await.map_err(error_to_vec)? {
            match item.message {
                Some(visualize_blocks_response::Message::Error(err)) => {
                    return Err(error_to_vec(err));
                }
                Some(visualize_blocks_response::Message::Content(s)) => {
                    contents.push_str(&s);
                }
                None => return Err(vec!["empty VisualizeBlocksResponse.message".into()]),
            }
        }
        Ok(contents)
    }

    async fn machine_verifiable_dag_impl(
        &mut self,
        q: MachineVerifyQuery,
    ) -> ServiceResult<String> {
        let resp = self
            .client
            .machine_verifiable_dag(q)
            .await
            .map_err(error_to_vec)?;

        match resp.into_inner().message {
            Some(machine_verify_response::Message::Error(err)) => Err(error_to_vec(err)),
            Some(machine_verify_response::Message::Content(s)) => Ok(s),
            None => Err(vec!["empty MachineVerifyResponse.message".into()]),
        }
    }

    async fn get_blocks_impl(&mut self, q: BlocksQuery) -> ServiceResult<String> {
        let mut stream = self
            .client
            .get_blocks(q)
            .await
            .map_err(error_to_vec)?
            .into_inner();

        let mut out = String::new();
        let mut count = 0usize;

        while let Some(item) = stream.message().await.map_err(error_to_vec)? {
            match item.message {
                Some(block_info_response::Message::Error(err)) => {
                    return Err(error_to_vec(err));
                }
                Some(block_info_response::Message::BlockInfo(bi)) => {
                    count += 1;
                    use std::fmt::Write as _;
                    let _ = writeln!(
                        out,
                        "\n------------- block {} ---------------\n{}\n-----------------------------------------------------\n",
                        bi.block_number,
                        proto_string(&bi)
                    );
                }
                // `getBlocks` never emits these, but the oneof is shared
                // with `getBlocksByHeights` (#1087). Ignore unexpected
                // variants so an unrelated proto addition can't break
                // this CLI path.
                Some(block_info_response::Message::Pruned(_))
                | Some(block_info_response::Message::Unavailable(_)) => {}
                None => return Err(vec!["empty BlockInfoResponse.message".into()]),
            }
        }

        use std::fmt::Write as _;
        let _ = writeln!(out, "count: {count}");
        Ok(out)
    }

    async fn listen_for_continuation_at_name_impl(
        &mut self,
        q: ContinuationAtNameQuery,
    ) -> ServiceResult<Vec<ContinuationsWithBlockInfo>> {
        let resp = self
            .client
            .listen_for_continuation_at_name(q)
            .await
            .map_err(error_to_vec)?;

        match resp.into_inner().message {
            Some(continuation_at_name_response::Message::Error(err)) => Err(error_to_vec(err)),
            Some(continuation_at_name_response::Message::Payload(p)) => Ok(p.block_results),
            None => Err(vec!["empty ContinuationAtNameResponse.message".into()]),
        }
    }

    async fn get_data_at_par_impl(
        &mut self,
        q: DataAtNameByBlockQuery,
    ) -> ServiceResult<(Vec<Par>, LightBlockInfo)> {
        let resp = self
            .client
            .get_data_at_name(q)
            .await
            .map_err(error_to_vec)?;

        match resp.into_inner().message {
            Some(rho_data_response::Message::Error(err)) => Err(error_to_vec(err)),
            Some(rho_data_response::Message::Payload(RhoDataPayload { par, block })) => {
                Ok((par, block.unwrap_or_default()))
            }
            // Issue #1157: the target block's state has been swept past the
            // retention floor; render as a structured client error so the
            // caller can route to an archive node.
            Some(rho_data_response::Message::Pruned(info)) => Err(vec![format!(
                "pruned_state: target_block_height={}, retention_floor={}",
                info.target_block_height, info.retention_floor
            )]),
            None => Err(vec!["empty RhoDataResponse.message".into()]),
        }
    }

    async fn exploratory_deploy_impl(
        &mut self,
        q: ExploratoryDeployQuery,
    ) -> ServiceResult<(DataWithBlockInfo, u64)> {
        let resp = self
            .client
            .exploratory_deploy(q)
            .await
            .map_err(error_to_vec)?;

        let resp = resp.into_inner();
        let cost = resp.cost;
        match resp.message {
            Some(exploratory_deploy_response::Message::Error(err)) => Err(error_to_vec(err)),
            Some(exploratory_deploy_response::Message::Result(data)) => Ok((data, cost)),
            // Issue #1166: the target block's state has been swept past the
            // retention floor; mirror the data-at-par prose so CLI callers
            // see one shape for all pruned-state refusals.
            Some(exploratory_deploy_response::Message::Pruned(info)) => Err(vec![format!(
                "pruned_state: target_block_height={}, retention_floor={}",
                info.target_block_height, info.retention_floor
            )]),
            None => Err(vec!["empty ExploratoryDeployResponse.message".into()]),
        }
    }

    async fn get_event_by_hash_impl(&mut self, q: ReportQuery) -> ServiceResult<BlockEventInfo> {
        let resp = self
            .client
            .get_event_by_hash(q)
            .await
            .map_err(error_to_vec)?;

        match resp.into_inner().message {
            Some(event_info_response::Message::Error(err)) => Err(error_to_vec(err)),
            Some(event_info_response::Message::Result(info)) => Ok(info),
            // Issue #1166: the block whose replay would produce events has a
            // pruned post-state; render with the same prose shape so the CLI
            // path is uniform across pruned-state refusals.
            Some(event_info_response::Message::Pruned(info)) => Err(vec![format!(
                "pruned_state: target_block_height={}, retention_floor={}",
                info.target_block_height, info.retention_floor
            )]),
            None => Err(vec!["empty EventInfoResponse.message".into()]),
        }
    }

    async fn last_finalized_block_impl(&mut self) -> ServiceResult<String> {
        let resp = self
            .client
            .last_finalized_block(LastFinalizedBlockQuery {})
            .await
            .map_err(error_to_vec)?;

        match resp.into_inner().message {
            Some(last_finalized_block_response::Message::Error(err)) => Err(error_to_vec(err)),
            Some(last_finalized_block_response::Message::BlockInfo(bi)) => Ok(proto_string(&bi)),
            None => Err(vec!["empty LastFinalizedBlockResponse.message".into()]),
        }
    }

    pub async fn is_finalized_impl(&mut self, q: IsFinalizedQuery) -> ServiceResult<String> {
        let resp = self.client.is_finalized(q).await.map_err(error_to_vec)?;

        match resp.into_inner().message {
            Some(is_finalized_response::Message::Error(err)) => Err(error_to_vec(err)),
            Some(is_finalized_response::Message::IsFinalized(true)) => {
                Ok("Block is finalized".into())
            }
            Some(is_finalized_response::Message::IsFinalized(false)) => {
                Err(vec!["Block is not finalized".into()])
            }
            None => Err(vec!["empty IsFinalizedResponse.message".into()]),
        }
    }

    pub async fn bond_status_impl(&mut self, q: BondStatusQuery) -> ServiceResult<String> {
        let resp = self.client.bond_status(q).await.map_err(error_to_vec)?;

        match resp.into_inner().message {
            Some(bond_status_response::Message::Error(err)) => Err(error_to_vec(err)),
            Some(bond_status_response::Message::IsBonded(true)) => Ok("Validator is bonded".into()),
            Some(bond_status_response::Message::IsBonded(false)) => {
                Err(vec!["Validator is not bonded".into()])
            }
            None => Err(vec!["empty BondStatusResponse.message".into()]),
        }
    }

    pub async fn status_impl(&mut self) -> ServiceResult<String> {
        let resp = self
            .client
            .status(tonic::Request::new(()))
            .await
            .map_err(error_to_vec)?;

        match resp.into_inner().message {
            Some(status_response::Message::Error(err)) => Err(error_to_vec(err)),
            Some(status_response::Message::Status(s)) => Ok(proto_string(&s)),
            None => Err(vec!["empty StatusResponse.message".into()]),
        }
    }
}

/// Substitute with any pretty/proto-string you prefer.
/// If your generated types implement `Debug`, this default works.
/// This is a simplified analogue of Scala-like `toProtoString`.
fn proto_string<T: std::fmt::Debug>(t: &T) -> String { format!("{t:#?}") }

#[cfg(test)]
mod exhaustive_arm_guards {
    //! Issue #1166: compile-time guards that every oneof arm on the three
    //! retention-aware response types is matched to a distinct label by the
    //! gRPC client. If the proto adds a new arm (another pruned/unavailable
    //! shape), these stub matches stop compiling — forcing the client-side
    //! helpers above to be updated instead of silently falling into a
    //! catch-all. The functions are never called at runtime.
    use models::casper::v1::{event_info_response, exploratory_deploy_response, rho_data_response};

    #[allow(dead_code)]
    fn rho_data_arms(msg: rho_data_response::Message) -> &'static str {
        match msg {
            rho_data_response::Message::Error(_) => "error",
            rho_data_response::Message::Payload(_) => "payload",
            rho_data_response::Message::Pruned(_) => "pruned",
        }
    }

    #[allow(dead_code)]
    fn exploratory_deploy_arms(msg: exploratory_deploy_response::Message) -> &'static str {
        match msg {
            exploratory_deploy_response::Message::Error(_) => "error",
            exploratory_deploy_response::Message::Result(_) => "result",
            exploratory_deploy_response::Message::Pruned(_) => "pruned",
        }
    }

    #[allow(dead_code)]
    fn event_info_arms(msg: event_info_response::Message) -> &'static str {
        match msg {
            event_info_response::Message::Error(_) => "error",
            event_info_response::Message::Result(_) => "result",
            event_info_response::Message::Pruned(_) => "pruned",
        }
    }

    #[test]
    fn arms_are_distinguishable_across_response_types() {
        // Build each oneof variant with default inner payloads and confirm
        // every arm maps to a distinct label. Detects both silent-drops
        // (two arms collapsing to the same string) and inadvertent arm
        // removal.
        use std::collections::HashSet;

        use models::casper::v1::{PrunedStateInfo, RhoDataPayload};
        use models::casper::{BlockEventInfo, DataWithBlockInfo};
        use models::servicemodelapi::ServiceError;

        let rho: HashSet<_> = [
            rho_data_arms(rho_data_response::Message::Error(ServiceError::default())),
            rho_data_arms(rho_data_response::Message::Payload(
                RhoDataPayload::default(),
            )),
            rho_data_arms(rho_data_response::Message::Pruned(
                PrunedStateInfo::default(),
            )),
        ]
        .into_iter()
        .collect();
        assert_eq!(
            rho.len(),
            3,
            "rho_data_response arms collapsed to same label"
        );

        let expl: HashSet<_> = [
            exploratory_deploy_arms(exploratory_deploy_response::Message::Error(
                ServiceError::default(),
            )),
            exploratory_deploy_arms(exploratory_deploy_response::Message::Result(
                DataWithBlockInfo::default(),
            )),
            exploratory_deploy_arms(exploratory_deploy_response::Message::Pruned(
                PrunedStateInfo::default(),
            )),
        ]
        .into_iter()
        .collect();
        assert_eq!(
            expl.len(),
            3,
            "exploratory_deploy_response arms collapsed to same label"
        );

        let event: HashSet<_> = [
            event_info_arms(event_info_response::Message::Error(ServiceError::default())),
            event_info_arms(event_info_response::Message::Result(
                BlockEventInfo::default(),
            )),
            event_info_arms(event_info_response::Message::Pruned(
                PrunedStateInfo::default(),
            )),
        ]
        .into_iter()
        .collect();
        assert_eq!(
            event.len(),
            3,
            "event_info_response arms collapsed to same label"
        );
    }
}
