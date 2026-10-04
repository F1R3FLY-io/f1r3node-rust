use thiserror::Error;

use crate::event::{AppEventId, AppId};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AppError {
    #[error("unknown app id: {app_id:?}")]
    UnknownApp { app_id: AppId },

    #[error("duplicate app event: {event_id:?}")]
    DuplicateEvent { event_id: AppEventId },

    #[error("app validation rejected event: {message}")]
    ValidationRejected { message: String },

    #[error("app apply failed: {message}")]
    ApplyFailed { message: String },
}
