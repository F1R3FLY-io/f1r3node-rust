use std::fmt;

use comm::rust::errors::CommError;
use rholang::rust::interpreter::errors::InterpreterError;
use rspace_plus_plus::rspace::errors::HistoryError;
use shared::rust::store::key_value_store::KvStoreError;

use super::slashing_authorization::SlashAuthError;
use super::util::rholang::replay_failure::ReplayFailure;
use super::util::rholang::system_deploy_user_error::SystemDeployPlatformFailure;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CasperError {
    InterpreterError(InterpreterError),
    KvStoreError(KvStoreError),
    RuntimeError(String),
    SystemRuntimeError(SystemDeployPlatformFailure),
    SigningError(String),
    ReplayFailure(ReplayFailure),
    CommError(CommError),
    HistoryError(HistoryError),
    StreamError(String),
    LockError(String),
    /// Phase 9 (R-2): typed `Slash`-deploy authorization failure. Carries
    /// the [`SlashAuthError`] variant so callers in
    /// `engine::multi_parent_casper::validation_dispatcher` can `match` on the structured
    /// reason instead of grepping a stringified error.
    SlashAuth(SlashAuthError),
    /// Multi-signature deploy pre-charge failed for a specific cosigner.
    /// Carries the signer's index in the canonical cosigner list, the
    /// pk in hex form, and the underlying error message. Surfaced at the
    /// runtime fan-out layer (`runtime.rs:402-490`); triggers a
    /// `revert_to_soft_checkpoint` to roll back any preceding successful
    /// cosigner pre-charges atomically.
    InsufficientPhloByCosigner {
        signer_index: usize,
        pk_hex: String,
        message: String,
    },
    /// Runtime-layer detection of a duplicate cosigner in the fan-out loop.
    /// Unreachable if `Cosigned::from_signed_data`'s no-duplicate invariant
    /// holds (the envelope rejects duplicate `pk`s at construction); surfaced
    /// here for debuggability if a future code path bypasses that invariant.
    /// The PoS contract also rejects double-charge defensively.
    DuplicateCosignerCharge {
        pk_hex: String,
    },
    Other(String),
}

impl fmt::Display for CasperError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CasperError::InterpreterError(error) => write!(f, "Interpreter error: {}", error),
            CasperError::KvStoreError(error) => write!(f, "KvStore error: {}", error),
            CasperError::RuntimeError(error) => write!(f, "Runtime error: {}", error),
            CasperError::SystemRuntimeError(error) => write!(f, "System runtime error: {}", error),
            CasperError::SigningError(error) => write!(f, "Signing error: {}", error),
            CasperError::ReplayFailure(error) => write!(f, "Replay failure: {}", error),
            CasperError::CommError(error) => write!(f, "Comm error: {}", error),
            CasperError::HistoryError(error) => write!(f, "History error: {}", error),
            CasperError::StreamError(error) => write!(f, "Stream error: {}", error),
            CasperError::LockError(error) => write!(f, "Lock error: {}", error),
            CasperError::SlashAuth(error) => write!(f, "Slash authorization error: {}", error),
            CasperError::InsufficientPhloByCosigner {
                signer_index,
                pk_hex,
                message,
            } => write!(
                f,
                "Insufficient phlo by cosigner at index {} (pk={}): {}",
                signer_index, pk_hex, message
            ),
            CasperError::DuplicateCosignerCharge { pk_hex } => write!(
                f,
                "Duplicate cosigner charge attempted for pk={} \
                 (Cosigned envelope dedup invariant violated)",
                pk_hex
            ),
            CasperError::Other(error) => write!(f, "Other error: {}", error),
        }
    }
}

impl From<SlashAuthError> for CasperError {
    fn from(error: SlashAuthError) -> Self {
        CasperError::SlashAuth(error)
    }
}

impl From<InterpreterError> for CasperError {
    fn from(error: InterpreterError) -> Self {
        CasperError::InterpreterError(error)
    }
}

impl From<KvStoreError> for CasperError {
    fn from(error: KvStoreError) -> Self {
        CasperError::KvStoreError(error)
    }
}

impl From<ReplayFailure> for CasperError {
    fn from(error: ReplayFailure) -> Self {
        CasperError::ReplayFailure(error)
    }
}

impl From<CommError> for CasperError {
    fn from(error: CommError) -> Self {
        CasperError::CommError(error)
    }
}

/// Conversion from un-typed `String` errors. Used by `?` propagation
/// from APIs that return `Result<_, String>` (e.g.
/// `EventPublisher::publish`). The string is wrapped in
/// `CasperError::RuntimeError` — semantically the same as the explicit
/// `.map_err(|e| CasperError::RuntimeError(e.to_string()))?` pattern it
/// replaces, but without the per-site boilerplate.
impl From<String> for CasperError {
    fn from(error: String) -> Self {
        CasperError::RuntimeError(error)
    }
}

/// Conversion from `std::time::SystemTimeError`. Wraps the underlying
/// error message into `CasperError::RuntimeError`. Used by `?`
/// propagation in `construct_deploy::source_deploy_now` and
/// `source_deploy_now_full` — both compute deploy timestamps via
/// `SystemTime::now().duration_since(UNIX_EPOCH)?` which can fail on a
/// pre-epoch system clock.
impl From<std::time::SystemTimeError> for CasperError {
    fn from(error: std::time::SystemTimeError) -> Self {
        CasperError::RuntimeError(format!("System time error: {}", error))
    }
}
