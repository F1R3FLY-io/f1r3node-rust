// See casper/src/main/scala/coop/rchain/casper/state/instances/ProposerState.scala

use models::rust::casper::protocol::casper_message::BlockMessage;
use tokio::sync::oneshot;

use crate::rust::blocks::proposer::propose_result::ProposeResult;

#[derive(Debug)]
#[derive(Default)]
pub struct ProposerState {
    pub latest_propose_result: Option<(ProposeResult, Option<BlockMessage>)>,
    pub curr_propose_result: Option<oneshot::Receiver<(ProposeResult, Option<BlockMessage>)>>,
}
