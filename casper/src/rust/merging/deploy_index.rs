// See casper/src/main/scala/coop/rchain/casper/merging/DeployIndex.scala

use models::rust::casper::protocol::casper_message::Event;
use rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex;

#[derive(Debug, Clone, Eq, PartialEq, Hash, PartialOrd)]
pub struct DeployIndex {
    pub deploy_id: prost::bytes::Bytes,
    pub cost: u64,
    pub event_log_index: EventLogIndex,
}

impl DeployIndex {
    // Rejection-option selection weighs branches by cost; system deploys
    // carry none.
    pub const SYS_SLASH_DEPLOY_COST: u64 = 0;
    pub const SYS_CLOSE_BLOCK_DEPLOY_COST: u64 = 0;
    pub const SYS_EMPTY_DEPLOY_COST: u64 = 0;

    // Trailing byte of a system deploy's 33-byte id, defined from the
    // system-deploy markers `is_system_deploy_id` recognizes.
    pub const SYS_SLASH_DEPLOY_ID: &'static [u8] = &[crate::rust::system_deploy::SLASH_MARKER];
    pub const SYS_CLOSE_BLOCK_DEPLOY_ID: &'static [u8] =
        &[crate::rust::system_deploy::CLOSE_BLOCK_MARKER];
    pub const SYS_EMPTY_DEPLOY_ID: &'static [u8] = &[crate::rust::system_deploy::HEARTBEAT_MARKER];

    pub fn new(
        sig: prost::bytes::Bytes,
        cost: u64,
        events: Vec<Event>,
        create_event_log_index: impl Fn(Vec<Event>) -> EventLogIndex,
    ) -> Self {
        let event_log_index = create_event_log_index(events);

        Self {
            deploy_id: sig,
            cost,
            event_log_index,
        }
    }
}
