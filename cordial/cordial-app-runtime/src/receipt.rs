use serde::{Deserialize, Serialize};

use crate::event::{AppEventId, AppId};

/// Result of processing a finalized app event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppReceipt {
    pub app_id: AppId,
    pub event_id: AppEventId,
    pub ordered_index: u64,
    pub status: AppReceiptStatus,
    pub state_root: Option<Vec<u8>>,
    pub message: Option<String>,
}

/// Application-level processing status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AppReceiptStatus {
    Applied,
    Rejected,
}
