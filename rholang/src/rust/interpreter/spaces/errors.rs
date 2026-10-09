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

use super::namespace::SpaceId;
use crate::rust::interpreter::errors::InterpreterError;
use crate::rust::interpreter::tensor::num::NumError;

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
    // Changed by P7 step 2 (design v3 QT-5): `temp` and `seq` are available, and only legacy
    // execution refuses `seq`, because the Seq rules need the job order of native execution.
    // /// A qualifier that this phase does not support (`temp` and `seq` arrive in P7).
    // #[error("the {0} qualifier is not available")]
    // QualifierUnavailable(String),
    /// A qualifier that the execution model of the deploy cannot run: legacy execution refuses
    /// `seq`, because its rules need the deterministic job order of native execution (QT-5).
    #[error("the {0} qualifier needs native execution")]
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
    // Retired by decision 47: native play and native replay run spaces. A runtime that cannot
    // run a space job, a detached runtime or a native replay runtime that is not bound yet,
    // fails with `RuntimeUnavailable`.
    // /// A space operation in native execution, native play or native replay (I-13 as amended
    // /// by decision 46). It fails before any space work, so play and replay fail at one point.
    // #[error("native execution does not support Reified RSpaces")]
    // SpacesUnsupportedInNativeExecution,
    /// A handle whose space has no config record, or an id of an ephemeral space that an ended
    /// deploy created (P7, DG-1). The error names the missing space id (DG-3).
    #[error("the space {} of this handle does not exist", hex::encode(.space))]
    DanglingHandle { space: SpaceId },
    /// A name whose space has no config record, or a name of an ephemeral space that an ended
    /// deploy created (P7, DG-1). The error names the missing space id (DG-3).
    #[error("the space {} of this name does not exist", hex::encode(.space))]
    DanglingName { space: SpaceId },
    /// A send whose data holds a name of a `seq` space of the evaluation, at any depth (P7,
    /// SQ-2). The error names the space id.
    #[error("a name of the seq space {} is sent", hex::encode(.space))]
    SeqSent { space: SpaceId },
    /// A use of a channel of a `seq` space whose last use is not a causal ancestor of the
    /// using process (P7, SQ-3). The error names the space id.
    #[error("a channel of the seq space {} is used by a concurrent process", hex::encode(.space))]
    SeqConcurrent { space: SpaceId },
    /// The `ord` counter of a collection reached `i64::MAX`.
    #[error("a space counter reached its maximum")]
    CounterOverflow,
    /// A store or a park on a full collection: a cell holds one record and one waiter (F15).
    /// The check runs before the staging, so the operation stages nothing.
    #[error("the cell is occupied")]
    CellOccupied,
    /// A store on a collection that already holds the most records of its space (P6): the
    /// `capacity` of a VectorDB space bounds the records of each collection. The check runs
    /// before the staging, so the operation stages nothing.
    #[error("the collection holds the most records of its space")]
    CapacityExceeded,
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
    /// A cosine query whose vector is zero, which has no direction (P6, design v3 4.1). A zero
    /// record vector is not an error: that record is not eligible.
    #[error("the query vector of a cosine query is zero")]
    ZeroQueryVector,
    /// A vector whose length is not the dimension of its VectorDB space (P6).
    #[error("the vector has {found} elements, and the space takes {expected}")]
    DimensionMismatch { expected: usize, found: usize },
    /// A vector element that is NaN or an infinity (P6).
    #[error("a vector element is not finite")]
    NonFiniteValue,
    /// A delivered value beyond the finite range of `f64` (P6, decision 66).
    #[error("a delivered value is beyond the finite range of f64")]
    ValueOutOfRange,
    /// A query param that a query without a vector cannot take: `threshold`, `radius`,
    /// `metric`, `scores` or `ids` (P6, design v3 4.4).
    #[error("the query param {key:?} needs a query vector")]
    QueryNeedsVector { key: String },
    /// A `radius` on a similarity metric (P6).
    #[error("a radius bounds a distance, and the metric of the query is a similarity")]
    RadiusNeedsDistance,
    /// A `threshold` on a distance metric (P6).
    #[error("a threshold bounds a similarity, and the metric of the query is a distance")]
    ThresholdNeedsSimilarity,
    /// A query with both a `threshold` and a `radius` (P6, decision 56).
    #[error("a query takes a threshold or a radius, not both")]
    ConflictingParams,
    /// A receive with two or more binds whose bind on a VectorDB collection is in list mode: a
    /// list is one value of one bind (P6, decision 66).
    #[error("a list-mode query is the query of a receive with one bind")]
    ListModeInJoin,
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

impl SpaceError {
    /// The space error of a failure of the numeric core (P6) on the value of the param `key`:
    /// a value that is not a number or not a vector is an invalid value of its param.
    pub fn of_number(error: NumError, key: &str) -> SpaceError {
        match error {
            NumError::NotANumber | NumError::NotAVector | NumError::MalformedNumber => {
                SpaceError::InvalidParam {
                    key: key.to_string(),
                }
            }
            NumError::NonFiniteValue => SpaceError::NonFiniteValue,
            NumError::DimensionMismatch { expected, found } => {
                SpaceError::DimensionMismatch { expected, found }
            }
            NumError::ValueOutOfRange => SpaceError::ValueOutOfRange,
            NumError::MixedScores => {
                SpaceError::Inconsistent("two scores of different metrics meet")
            }
            NumError::HostWork(error) => SpaceError::Host(error),
        }
    }
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
            // Retired by decision 47 with its variant.
            // SpaceError::SpacesUnsupportedInNativeExecution => "SpacesUnsupportedInNativeExecution",
            SpaceError::DanglingHandle { .. } => "DanglingHandle",
            SpaceError::DanglingName { .. } => "DanglingName",
            SpaceError::SeqSent { .. } => "SeqSent",
            SpaceError::SeqConcurrent { .. } => "SeqConcurrent",
            SpaceError::CounterOverflow => "CounterOverflow",
            SpaceError::CellOccupied => "CellOccupied",
            SpaceError::CapacityExceeded => "CapacityExceeded",
            SpaceError::InvalidParam { .. } => "InvalidParam",
            SpaceError::MissingParam { .. } => "MissingParam",
            SpaceError::OutOfNames => "OutOfNames",
            SpaceError::NotAnIndexName => "NotAnIndexName",
            SpaceError::UnsupportedPathElement => "UnsupportedPathElement",
            SpaceError::PathTooDeep => "PathTooDeep",
            SpaceError::QuerySpecUnsupported => "QuerySpecUnsupported",
            SpaceError::ZeroQueryVector => "ZeroQueryVector",
            SpaceError::DimensionMismatch { .. } => "DimensionMismatch",
            SpaceError::NonFiniteValue => "NonFiniteValue",
            SpaceError::ValueOutOfRange => "ValueOutOfRange",
            SpaceError::QueryNeedsVector { .. } => "QueryNeedsVector",
            SpaceError::RadiusNeedsDistance => "RadiusNeedsDistance",
            SpaceError::ThresholdNeedsSimilarity => "ThresholdNeedsSimilarity",
            SpaceError::ConflictingParams => "ConflictingParams",
            SpaceError::ListModeInJoin => "ListModeInJoin",
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
            SpaceError::QualifierUnavailable("seq".to_string()),
            SpaceError::TheoryUnavailable("Nat".to_string()),
            SpaceError::FamilyUnavailable {
                outer: "hashmap".to_string(),
                inner: "bag".to_string(),
            },
            SpaceError::RuntimeUnavailable,
            // Retired by decision 47.
            // SpaceError::SpacesUnsupportedInNativeExecution,
            SpaceError::DanglingHandle { space: [1; 32] },
            SpaceError::DanglingName { space: [2; 32] },
            SpaceError::SeqSent { space: [3; 32] },
            SpaceError::SeqConcurrent { space: [4; 32] },
            SpaceError::CounterOverflow,
            SpaceError::CellOccupied,
            SpaceError::CapacityExceeded,
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
            SpaceError::ZeroQueryVector,
            SpaceError::DimensionMismatch {
                expected: 3,
                found: 2,
            },
            SpaceError::NonFiniteValue,
            SpaceError::ValueOutOfRange,
            SpaceError::QueryNeedsVector {
                key: "threshold".to_string(),
            },
            SpaceError::RadiusNeedsDistance,
            SpaceError::ThresholdNeedsSimilarity,
            SpaceError::ConflictingParams,
            SpaceError::ListModeInJoin,
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
        // DG-3: a dangling handle or name names the missing space id in lowercase hex.
        for (error, noun) in [
            (SpaceError::DanglingHandle { space: [0xab; 32] }, "handle"),
            (SpaceError::DanglingName { space: [0xab; 32] }, "name"),
        ] {
            let code = error.code();
            assert_eq!(
                InterpreterError::from(error),
                InterpreterError::ReduceError(format!(
                    "reified:{code}: the space {} of this {noun} does not exist",
                    "ab".repeat(32)
                ))
            );
        }
        // SQ-2 and SQ-3: a Seq violation names the seq space in lowercase hex.
        assert_eq!(
            InterpreterError::from(SpaceError::SeqSent { space: [0xcd; 32] }),
            InterpreterError::ReduceError(format!(
                "reified:SeqSent: a name of the seq space {} is sent",
                "cd".repeat(32)
            ))
        );
        assert_eq!(
            InterpreterError::from(SpaceError::SeqConcurrent { space: [0xcd; 32] }),
            InterpreterError::ReduceError(format!(
                "reified:SeqConcurrent: a channel of the seq space {} is used by a concurrent \
                 process",
                "cd".repeat(32)
            ))
        );
        // QT-5: legacy execution refuses `seq`.
        assert_eq!(
            InterpreterError::from(SpaceError::QualifierUnavailable("seq".to_string())),
            InterpreterError::ReduceError(
                "reified:QualifierUnavailable: the seq qualifier needs native execution".to_string()
            )
        );
    }

    #[test]
    fn space_errors_of_the_numeric_core_keep_their_class() {
        assert_eq!(
            SpaceError::of_number(NumError::NotAVector, "embedding"),
            SpaceError::InvalidParam {
                key: "embedding".to_string()
            }
        );
        assert_eq!(
            SpaceError::of_number(NumError::NotANumber, "q"),
            SpaceError::InvalidParam {
                key: "q".to_string()
            }
        );
        assert_eq!(
            SpaceError::of_number(NumError::MalformedNumber, "threshold"),
            SpaceError::InvalidParam {
                key: "threshold".to_string()
            }
        );
        assert_eq!(
            SpaceError::of_number(NumError::NonFiniteValue, "q"),
            SpaceError::NonFiniteValue
        );
        assert_eq!(
            SpaceError::of_number(
                NumError::DimensionMismatch {
                    expected: 4,
                    found: 3
                },
                "q"
            ),
            SpaceError::DimensionMismatch {
                expected: 4,
                found: 3
            }
        );
        assert_eq!(
            SpaceError::of_number(NumError::ValueOutOfRange, "q"),
            SpaceError::ValueOutOfRange
        );
        assert!(matches!(
            SpaceError::of_number(NumError::MixedScores, "q"),
            SpaceError::Inconsistent(_)
        ));
        assert_eq!(
            SpaceError::of_number(NumError::HostWork(RSpaceError::HostWorkRejected), "q"),
            SpaceError::Host(RSpaceError::HostWorkRejected)
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
