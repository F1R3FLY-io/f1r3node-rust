use serde::{Deserialize, Serialize};

/// Stable application identifier used to route finalized events.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct AppId(pub String);

/// Stable application event identifier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct AppEventId(pub String);

/// Finalized event envelope delivered to an application.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppEvent {
    pub event_id: AppEventId,
    pub app_id: AppId,
    pub event_type: String,
    pub payload: Vec<u8>,
    pub submitter: Vec<u8>,
    pub ordered_index: u64,
    pub block_hash: Vec<u8>,
    pub deploy_signature: Option<Vec<u8>>,
    pub finalized_anchor: Vec<u8>,
}

/// Runtime cursor for replay and duplicate protection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppCursor {
    pub finalized_anchor: Vec<u8>,
    pub ordered_index: u64,
}

/// Opaque app-state snapshot exposed by an application state machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppSnapshot {
    pub app_id: AppId,
    pub cursor: Option<AppCursor>,
    pub state_root: Option<Vec<u8>>,
    pub payload: Vec<u8>,
}
