//! The errors of the Reified RSpaces reducer hooks.
//!
//! P2 added the first four variants. P4 adds the reducer-level codes of the P4 design C.10
//! and of readiness review §4. The space-level codes are in `spaces::errors::SpaceError`.

// Disabled: only the replaced variant used `Marker` (A19, `SpaceError::RuntimeUnavailable`).
// use models::rust::reified::encoding::{Marker, ReifiedDecodeError};
use models::rust::reified::encoding::ReifiedDecodeError;

use crate::rust::interpreter::errors::InterpreterError;

/// A Reified construct that the reducer rejects. The deploy fails closed with
/// `InterpreterError::ReduceError("reified:<Code>: …")`.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ReifiedReduceError {
    #[error("{0}")]
    Malformed(ReifiedDecodeError),
    // Disabled: P4 raises `SpaceError::RuntimeUnavailable` instead (A19), so no code builds it.
    // #[error("the {marker} construct needs the Reified RSpaces runtime, which is not available")]
    // RuntimeUnavailable { marker: Marker },
    #[error("a send to this channel takes no params")]
    ParamsUnsupported,
    #[error("a receive from this channel takes no query")]
    QuerySpecUnsupported,
    /// A persistent send to a factory channel `[0xE0, 0x00..=0x20]` (A21).
    #[error("a persistent send to a space factory aborts")]
    PersistentFactoryCall,
    /// A persistent send to a space handle `H(sid)` or `h(sid)` (A21).
    #[error("a persistent send to a space handle aborts")]
    PersistentHandleCall,
    /// A persistent send to another op channel that takes calls (readiness review §4).
    #[error("a persistent send to a Reified system channel aborts")]
    PersistentOpCall,
    /// A space that a user agent implements (P8).
    #[error("spaces that user agents implement are not available")]
    AgentSpacesUnavailable,
    /// A join whose channels route to more than one space, or to a space and the default
    /// space.
    #[error("a join reads from more than one space")]
    CrossSpaceJoin,
    // Disabled by decision 29: a join whose channels all route to one native space runs in the
    // join engine of P5c (`spaces::join`), so no route builds this variant.
    // /// A join on a collection of an ordered discipline (A3, joins arrive in P5).
    // #[error("a join on an ordered collection is not supported")]
    // OrderedJoinUnsupported,
    /// The empty `Par` as a space reference.
    #[error("the space reference is empty")]
    EmptySpaceRef,
    /// A space handle under a bundle without the write flag.
    #[error("the space handle is not writable")]
    HandleNotWritable,
    /// A Reified id that is not a space handle, as a space reference.
    #[error("the value is not a space reference")]
    NotASpaceRef,
}

impl ReifiedReduceError {
    /// The stable code of the error, the `<Code>` of `reified:<Code>:`.
    pub fn code(&self) -> &'static str {
        match self {
            ReifiedReduceError::Malformed(_) => "Malformed",
            // Disabled with its variant: P4 raises `SpaceError::RuntimeUnavailable` instead (A19).
            // ReifiedReduceError::RuntimeUnavailable { .. } => "RuntimeUnavailable",
            ReifiedReduceError::ParamsUnsupported => "ParamsUnsupported",
            ReifiedReduceError::QuerySpecUnsupported => "QuerySpecUnsupported",
            ReifiedReduceError::PersistentFactoryCall => "PersistentFactoryCall",
            ReifiedReduceError::PersistentHandleCall => "PersistentHandleCall",
            ReifiedReduceError::PersistentOpCall => "PersistentOpCall",
            ReifiedReduceError::AgentSpacesUnavailable => "AgentSpacesUnavailable",
            ReifiedReduceError::CrossSpaceJoin => "CrossSpaceJoin",
            // Disabled by decision 29 with its variant (P5c).
            // ReifiedReduceError::OrderedJoinUnsupported => "OrderedJoinUnsupported",
            ReifiedReduceError::EmptySpaceRef => "EmptySpaceRef",
            ReifiedReduceError::HandleNotWritable => "HandleNotWritable",
            ReifiedReduceError::NotASpaceRef => "NotASpaceRef",
        }
    }
}

impl From<ReifiedReduceError> for InterpreterError {
    fn from(error: ReifiedReduceError) -> Self {
        InterpreterError::ReduceError(format!("reified:{}: {}", error.code(), error))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::rust::interpreter::spaces::errors::SpaceError;

    #[test]
    fn reducer_codes_are_distinct_and_disjoint_from_the_space_codes() {
        let errors = [
            ReifiedReduceError::Malformed(ReifiedDecodeError::NotSoleNew),
            // Disabled with its variant: P4 raises `SpaceError::RuntimeUnavailable` instead (A19).
            // ReifiedReduceError::RuntimeUnavailable {
            //     marker: Marker::Use,
            // },
            ReifiedReduceError::ParamsUnsupported,
            ReifiedReduceError::QuerySpecUnsupported,
            ReifiedReduceError::PersistentFactoryCall,
            ReifiedReduceError::PersistentHandleCall,
            ReifiedReduceError::PersistentOpCall,
            ReifiedReduceError::AgentSpacesUnavailable,
            ReifiedReduceError::CrossSpaceJoin,
            // Disabled by decision 29 with its variant (P5c).
            // ReifiedReduceError::OrderedJoinUnsupported,
            ReifiedReduceError::EmptySpaceRef,
            ReifiedReduceError::HandleNotWritable,
            ReifiedReduceError::NotASpaceRef,
        ];
        let codes: BTreeSet<&str> = errors.iter().map(ReifiedReduceError::code).collect();
        assert_eq!(codes.len(), errors.len());
        let space_codes: BTreeSet<&str> = [
            SpaceError::SpaceAlreadyExists,
            SpaceError::PositionalParamsUnsupported,
            SpaceError::DanglingHandle,
            SpaceError::DanglingName,
            SpaceError::CounterOverflow,
            // Retired by decision 47.
            // SpaceError::SpacesUnsupportedInNativeExecution,
        ]
        .iter()
        .map(SpaceError::code)
        .collect();
        assert!(codes.is_disjoint(&space_codes));
        // The one shared code: an agent rejects a receive query with the code of the hook, so a
        // program sees one code wherever the query is rejected (decision 22, P5 design v3 S1a).
        assert_eq!(
            SpaceError::QuerySpecUnsupported.code(),
            ReifiedReduceError::QuerySpecUnsupported.code()
        );
        for error in errors {
            let code = error.code();
            assert_eq!(
                InterpreterError::from(error.clone()),
                InterpreterError::ReduceError(format!("reified:{code}: {error}"))
            );
        }
    }
}
