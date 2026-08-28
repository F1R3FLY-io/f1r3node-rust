use std::fmt;

use comm::rust::errors::CommError;
use models::rust::block_hash::BlockHash;
use models::rust::casper::pretty_printer::PrettyPrinter;
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
    /// A walk needed a block this node does not hold. It is a statement about
    /// this node's history, never about the block being judged: a node whose
    /// history is truncated below its sync anchor legitimately lacks blocks its
    /// peers have. Carried as a variant rather than a message so the block
    /// processor can request the named block and retry, instead of folding it
    /// into the storage-failure class that becomes a slashable verdict.
    BlockNotHeld(BlockHash),
    /// The floor derivation found finalized candidates that are mutually
    /// incompatible (same-height certified siblings with no containment and
    /// no re-merge). Under a BFT threshold (θ ≥ 0) this is impossible
    /// without a protocol breach and stays a loud error; under a negative
    /// threshold "finalized" is bare majority agreement per snapshot, so the
    /// live clock absorbs it as an expected transient hold
    /// (`FloorOfView::IncompatibilityHold`). Typed so that regime split is a
    /// match, not a string search. Carries the full preformatted detail.
    IncompatibleFinalizedFork(String),
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
            CasperError::BlockNotHeld(hash) => write!(
                f,
                "block not held by this node: {} — its history does not reach that block",
                PrettyPrinter::build_string_bytes(hash)
            ),
            // The detail is self-describing ("finalized-floor safety
            // violation: ... — incompatible finalized fork"), and harness
            // forbidden-log patterns key on that text — print it verbatim.
            CasperError::IncompatibleFinalizedFork(detail) => write!(f, "{}", detail),
            CasperError::Other(error) => write!(f, "Other error: {}", error),
        }
    }
}

impl CasperError {
    /// True for errors that state an ABSENCE on this node — a block or a state
    /// root it does not hold — rather than a fault or a fact about a block.
    /// These must reach `BlockError::from_validation_error` typed: any path
    /// that stringifies one launders "I don't have it" into a slashable
    /// verdict against whoever proposed the block being judged.
    pub fn is_availability(&self) -> bool {
        use rholang::rust::interpreter::errors::InterpreterError;
        use rspace_plus_plus::rspace::errors::{HistoryError, RSpaceError, RootError};

        matches!(
            self,
            CasperError::BlockNotHeld(_)
                | CasperError::InterpreterError(InterpreterError::RSpaceError(
                    RSpaceError::HistoryError(HistoryError::RootError(RootError::RootNotFound(_))),
                ))
        )
    }
}

impl From<SlashAuthError> for CasperError {
    fn from(error: SlashAuthError) -> Self { CasperError::SlashAuth(error) }
}

impl From<InterpreterError> for CasperError {
    fn from(error: InterpreterError) -> Self { CasperError::InterpreterError(error) }
}

/// A block the DAG does not hold arrives here as a store error, because the DAG
/// primitives the clique oracle reads through are storage APIs. It is not a
/// storage failure, and the two must not share a class: the storage class
/// becomes `InvalidTransaction`, which is slashable. Collapsing it into
/// `BlockNotHeld` at the boundary means every walk under `floor.rs` — the
/// oracle's weight maps, main-chain membership, self-justification chains —
/// gets the deferral the floor walk already had, without a match arm at each
/// caller.
impl From<KvStoreError> for CasperError {
    fn from(error: KvStoreError) -> Self {
        match error {
            KvStoreError::MissingBlock { hash, .. } => CasperError::BlockNotHeld(hash),
            other => CasperError::KvStoreError(other),
        }
    }
}

impl From<ReplayFailure> for CasperError {
    fn from(error: ReplayFailure) -> Self { CasperError::ReplayFailure(error) }
}

impl From<CommError> for CasperError {
    fn from(error: CommError) -> Self { CasperError::CommError(error) }
}

/// Conversion from un-typed `String` errors. Used by `?` propagation
/// from APIs that return `Result<_, String>` (e.g.
/// `EventPublisher::publish`). The string is wrapped in
/// `CasperError::RuntimeError` — semantically the same as the explicit
/// `.map_err(|e| CasperError::RuntimeError(e.to_string()))?` pattern it
/// replaces, but without the per-site boilerplate.
impl From<String> for CasperError {
    fn from(error: String) -> Self { CasperError::RuntimeError(error) }
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
