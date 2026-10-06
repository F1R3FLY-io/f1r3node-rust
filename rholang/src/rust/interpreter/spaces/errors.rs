//! The errors of the native spaces (P4 design C.10) and their mapping from the substrate
//! (readiness review S8).
//!
//! A [`SpaceError`] aborts the deploy with `InterpreterError::ReduceError("reified:<Code>: …")`.
//! A host error passes through unchanged, so `OutOfPhlogistons` and `HostWorkRejected` keep
//! their class. The reducer-level codes are in `reified::errors::ReifiedReduceError`. A protocol
//! error of a handle call is a typed reply with a [`SpaceReplyCode`], not an abort. Message
//! text is not consensus state.

use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::space_store_handle::SpaceTxnError;
use rspace_plus_plus::rspace::trace::event::COMM;

use crate::rust::interpreter::errors::InterpreterError;

/// A failure of a space operation.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum SpaceError {
    /// The factory found a config record on `C(sid)`.
    #[error("a space with this id already exists")]
    SpaceAlreadyExists,
    /// A factory constructor that is not the canonical sequence `[qualifier?][FREE?][PARAMS?]`.
    #[error("the constructor argument is not accepted: {0}")]
    BadConstructorArgument(String),
    /// A named param that the schema of the channel does not hold.
    #[error("{key:?} is not a param of this channel{}", suggestion_text(.suggestion))]
    UnknownParam {
        key: String,
        suggestion: Option<String>,
    },
    /// Positional params on a factory, a handle or a space channel.
    #[error("this channel takes no positional params")]
    PositionalParamsUnsupported,
    /// A qualifier that this phase does not support (`temp` and `seq` arrive in P7).
    #[error("the {0} qualifier is not available")]
    QualifierUnavailable(String),
    /// A theory that this phase does not support (P7).
    #[error("the theory {0} is not available")]
    TheoryUnavailable(String),
    /// A config record whose outer store and inner collection this phase does not support.
    #[error("the {outer}:{inner} space family is not available")]
    FamilyUnavailable { outer: String, inner: String },
    /// A space operation in a runtime without spaces.
    #[error("the Reified RSpaces runtime is not available")]
    RuntimeUnavailable,
    // Replaced by decision 46: native play now fails closed as native replay does, so one code
    // names both, `SpacesUnsupportedInNativeExecution`.
    // /// A space operation in the native replay environment (I-13).
    // #[error("native replay does not support Reified RSpaces")]
    // SpacesUnsupportedInNativeReplay,
    /// A space operation in native execution, native play or native replay (I-13 as amended by
    /// decision 46). It fails before any space work, so play and replay fail at one point.
    #[error("native execution does not support Reified RSpaces")]
    SpacesUnsupportedInNativeExecution,
    /// A handle whose space has no config record.
    #[error("the space of this handle does not exist")]
    DanglingHandle,
    /// A name whose space has no config record.
    #[error("the space of this name does not exist")]
    DanglingName,
    /// The `ord` counter of a collection reached `i64::MAX`.
    #[error("a space counter reached its maximum")]
    CounterOverflow,
    /// A store or a park on a full collection: a cell holds one record and one waiter (F15).
    /// The check runs before the staging, so the operation stages nothing.
    #[error("the cell is occupied")]
    CellOccupied,
    /// A named param whose value the schema of its channel does not accept, for example a
    /// priority outside `0..priorities` (P5b).
    #[error("the value of the param {key:?} is not accepted")]
    InvalidParam { key: String },
    /// A named param that the schema of a factory requires, for example the `size` of an Array
    /// space (F7).
    #[error("the param {key:?} is required")]
    MissingParam { key: String },
    /// A fixed Array space that has no free index name (P5d).
    #[error("the space has no free name")]
    OutOfNames,
    /// A channel of an Array or Vector space that is not an index name of that space (F8).
    #[error("the channel is not an index name of this space")]
    NotAnIndexName,
    /// A path element of a PathMap space that holds a PathMap or a zipper (P5e).
    #[error("the path holds a PathMap or a zipper")]
    UnsupportedPathElement,
    /// A path of a PathMap space that is deeper than its depth limit (F9).
    #[error("the path is deeper than the depth limit of a PathMap space")]
    PathTooDeep,
    /// A receive query on a collection whose discipline takes no query params. The hook of a
    /// default-space receive reports the same code (decision 22), so a program sees one code.
    #[error("a receive from this channel takes no query")]
    QuerySpecUnsupported,
    /// A space record or the staged view that the agent reads is not well formed.
    #[error("a space record is inconsistent: {0}")]
    Inconsistent(&'static str),
    /// The computed COMM is not among the recorded bindings of its trigger event.
    #[error("replay divergence: the COMM {0:?} is not among the recorded bindings")]
    ReplayDivergence(Box<COMM>),
    /// Every other substrate failure (readiness review S8).
    #[error("{0}")]
    Substrate(SpaceTxnError),
    /// A host or observer error. It converts with `InterpreterError::from(RSpaceError)`.
    #[error("{0}")]
    Host(RSpaceError),
}

fn suggestion_text(suggestion: &Option<String>) -> String {
    match suggestion {
        Some(suggestion) => format!(", did you mean {suggestion:?}?"),
        None => String::new(),
    }
}

impl SpaceError {
    /// The stable code of the error, the `<Code>` of `reified:<Code>:`. A host error has the
    /// code `Host`, but it converts without a code.
    pub fn code(&self) -> &'static str {
        match self {
            SpaceError::SpaceAlreadyExists => "SpaceAlreadyExists",
            SpaceError::BadConstructorArgument(_) => "BadConstructorArgument",
            SpaceError::UnknownParam { .. } => "UnknownParam",
            SpaceError::PositionalParamsUnsupported => "PositionalParamsUnsupported",
            SpaceError::QualifierUnavailable(_) => "QualifierUnavailable",
            SpaceError::TheoryUnavailable(_) => "TheoryUnavailable",
            SpaceError::FamilyUnavailable { .. } => "FamilyUnavailable",
            SpaceError::RuntimeUnavailable => "RuntimeUnavailable",
            SpaceError::SpacesUnsupportedInNativeExecution => "SpacesUnsupportedInNativeExecution",
            SpaceError::DanglingHandle => "DanglingHandle",
            SpaceError::DanglingName => "DanglingName",
            SpaceError::CounterOverflow => "CounterOverflow",
            SpaceError::CellOccupied => "CellOccupied",
            SpaceError::InvalidParam { .. } => "InvalidParam",
            SpaceError::MissingParam { .. } => "MissingParam",
            SpaceError::OutOfNames => "OutOfNames",
            SpaceError::NotAnIndexName => "NotAnIndexName",
            SpaceError::UnsupportedPathElement => "UnsupportedPathElement",
            SpaceError::PathTooDeep => "PathTooDeep",
            SpaceError::QuerySpecUnsupported => "QuerySpecUnsupported",
            SpaceError::Inconsistent(_) => "Inconsistent",
            SpaceError::ReplayDivergence(_) => "ReplayDivergence",
            SpaceError::Substrate(_) => "Substrate",
            SpaceError::Host(_) => "Host",
        }
    }
}

/// Readiness review S8: a host error stays a host error, a replay divergence keeps its own
/// code, and every other variant is `Substrate`. This includes `UnlockedChannel`,
/// `NestedTransaction`, `ParentOrderReused` and the `i32` produce counter `CounterOverflow`.
impl From<SpaceTxnError> for SpaceError {
    fn from(error: SpaceTxnError) -> Self {
        match error {
            SpaceTxnError::Host(error) => SpaceError::Host(error),
            SpaceTxnError::ReplayDivergence(comm) => SpaceError::ReplayDivergence(comm),
            other => SpaceError::Substrate(other),
        }
    }
}

impl From<RSpaceError> for SpaceError {
    fn from(error: RSpaceError) -> Self { SpaceError::Host(error) }
}

impl From<SpaceError> for InterpreterError {
    fn from(error: SpaceError) -> Self {
        match error {
            SpaceError::Host(error) => InterpreterError::from(error),
            other => InterpreterError::ReduceError(format!("reified:{}: {}", other.code(), other)),
        }
    }
}

/// The tag of a typed error reply `("rho:space:error", code, detail)` (P4 design C.8, A23).
pub const SPACE_ERROR_REPLY_TAG: &str = "rho:space:error";

/// The code of a typed error reply. A handle call or a `getSpaceAgent` call replies with it
/// and does not abort.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SpaceReplyCode {
    MalformedMessage,
    UnknownMethod,
    BadArgument,
    InvalidArrow,
    ForeignMatcher,
    UnknownParam,
    DanglingHandle,
    NotANativeSpace,
    NotASpaceRef,
    EmptySpaceRef,
    HandleNotWritable,
}

impl SpaceReplyCode {
    /// Every reply code.
    pub const ALL: [SpaceReplyCode; 11] = [
        SpaceReplyCode::MalformedMessage,
        SpaceReplyCode::UnknownMethod,
        SpaceReplyCode::BadArgument,
        SpaceReplyCode::InvalidArrow,
        SpaceReplyCode::ForeignMatcher,
        SpaceReplyCode::UnknownParam,
        SpaceReplyCode::DanglingHandle,
        SpaceReplyCode::NotANativeSpace,
        SpaceReplyCode::NotASpaceRef,
        SpaceReplyCode::EmptySpaceRef,
        SpaceReplyCode::HandleNotWritable,
    ];

    /// The code string of the reply.
    pub const fn code(self) -> &'static str {
        match self {
            SpaceReplyCode::MalformedMessage => "MalformedMessage",
            SpaceReplyCode::UnknownMethod => "UnknownMethod",
            SpaceReplyCode::BadArgument => "BadArgument",
            SpaceReplyCode::InvalidArrow => "InvalidArrow",
            SpaceReplyCode::ForeignMatcher => "ForeignMatcher",
            SpaceReplyCode::UnknownParam => "UnknownParam",
            SpaceReplyCode::DanglingHandle => "DanglingHandle",
            SpaceReplyCode::NotANativeSpace => "NotANativeSpace",
            SpaceReplyCode::NotASpaceRef => "NotASpaceRef",
            SpaceReplyCode::EmptySpaceRef => "EmptySpaceRef",
            SpaceReplyCode::HandleNotWritable => "HandleNotWritable",
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    fn every_space_error() -> Vec<SpaceError> {
        vec![
            SpaceError::SpaceAlreadyExists,
            SpaceError::BadConstructorArgument("a list".to_string()),
            SpaceError::UnknownParam {
                key: "priorty".to_string(),
                suggestion: Some("priority".to_string()),
            },
            SpaceError::PositionalParamsUnsupported,
            SpaceError::QualifierUnavailable("temp".to_string()),
            SpaceError::TheoryUnavailable("Nat".to_string()),
            SpaceError::FamilyUnavailable {
                outer: "hashmap".to_string(),
                inner: "bag".to_string(),
            },
            SpaceError::RuntimeUnavailable,
            SpaceError::SpacesUnsupportedInNativeExecution,
            SpaceError::DanglingHandle,
            SpaceError::DanglingName,
            SpaceError::CounterOverflow,
            SpaceError::CellOccupied,
            SpaceError::InvalidParam {
                key: "priority".to_string(),
            },
            SpaceError::MissingParam {
                key: "size".to_string(),
            },
            SpaceError::OutOfNames,
            SpaceError::NotAnIndexName,
            SpaceError::UnsupportedPathElement,
            SpaceError::PathTooDeep,
            SpaceError::QuerySpecUnsupported,
            SpaceError::Inconsistent("two counter records"),
            SpaceError::ReplayDivergence(Box::new(COMM {
                consume: Default::default(),
                produces: Vec::new(),
                peeks: BTreeSet::new(),
                times_repeated: Default::default(),
            })),
            SpaceError::Substrate(SpaceTxnError::NestedTransaction),
        ]
    }

    #[test]
    fn space_errors_abort_with_their_reified_code() {
        let errors = every_space_error();
        let codes: BTreeSet<&str> = errors.iter().map(SpaceError::code).collect();
        assert_eq!(codes.len(), errors.len());
        for error in errors {
            let code = error.code();
            match InterpreterError::from(error) {
                InterpreterError::ReduceError(message) => {
                    assert!(
                        message.starts_with(&format!("reified:{code}: ")),
                        "{message}"
                    );
                }
                other => panic!("{code} converted to {other:?}"),
            }
        }
        let unknown = InterpreterError::from(SpaceError::UnknownParam {
            key: "priorty".to_string(),
            suggestion: Some("priority".to_string()),
        });
        assert_eq!(
            unknown,
            InterpreterError::ReduceError(
                "reified:UnknownParam: \"priorty\" is not a param of this channel, did you mean \
                 \"priority\"?"
                    .to_string()
            )
        );
    }

    #[test]
    fn host_errors_pass_through_with_their_class() {
        assert_eq!(
            InterpreterError::from(SpaceError::Host(RSpaceError::OutOfPhlogistons)),
            InterpreterError::OutOfPhlogistonsError
        );
        assert_eq!(
            InterpreterError::from(SpaceError::Host(RSpaceError::HostWorkRejected)),
            InterpreterError::HostWorkRejected
        );
        assert_eq!(
            InterpreterError::from(SpaceError::from(RSpaceError::BugFoundError("b".into()))),
            InterpreterError::RSpaceError(RSpaceError::BugFoundError("b".into()))
        );
    }

    #[test]
    fn substrate_errors_map_as_the_readiness_review_s8_states() {
        assert_eq!(
            SpaceError::from(SpaceTxnError::Host(RSpaceError::OutOfPhlogistons)),
            SpaceError::Host(RSpaceError::OutOfPhlogistons)
        );
        let comm = Box::new(COMM {
            consume: Default::default(),
            produces: Vec::new(),
            peeks: BTreeSet::new(),
            times_repeated: Default::default(),
        });
        assert_eq!(
            SpaceError::from(SpaceTxnError::ReplayDivergence(comm.clone())),
            SpaceError::ReplayDivergence(comm)
        );
        for error in [
            SpaceTxnError::InvalidOperation("lock keys must not be empty"),
            SpaceTxnError::NotInView("a candidate"),
            SpaceTxnError::InstalledChannel,
            SpaceTxnError::StoreSwapped,
            SpaceTxnError::NativeReplayDirectiveUnsupported,
            SpaceTxnError::Inconsistent("staged data is not loaded"),
            SpaceTxnError::EventCoverage {
                op: 0,
                rule: "removal",
            },
            SpaceTxnError::SweepCrossesDurable,
            SpaceTxnError::SweepTouchesPersisted,
            SpaceTxnError::Finished,
            SpaceTxnError::NonCanonicalChannelOrder,
            SpaceTxnError::UnlockedChannel,
            SpaceTxnError::ParentOrderReused,
            SpaceTxnError::CounterOverflow,
            SpaceTxnError::NestedTransaction,
        ] {
            assert_eq!(
                SpaceError::from(error.clone()),
                SpaceError::Substrate(error)
            );
        }
    }

    #[test]
    fn reply_codes_are_distinct() {
        let codes: BTreeSet<&str> = SpaceReplyCode::ALL.iter().map(|code| code.code()).collect();
        assert_eq!(codes.len(), SpaceReplyCode::ALL.len());
        assert_eq!(SPACE_ERROR_REPLY_TAG, "rho:space:error");
    }
}
