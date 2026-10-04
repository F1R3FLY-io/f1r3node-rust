//! Generic application runtime interface over Cordial Miners ordered output.
//!
//! This crate is intentionally app-neutral. It defines the vocabulary and
//! boundaries that future applications use to consume finalized Cordial order
//! without depending directly on blocklace, tau, gRPC, or f1r3node internals.

pub mod error;
pub mod event;
pub mod receipt;
pub mod runtime;

pub use error::AppError;
pub use event::{AppCursor, AppEvent, AppEventId, AppId, AppSnapshot};
pub use receipt::{AppReceipt, AppReceiptStatus};
pub use runtime::{AppRuntime, CordialApp};
