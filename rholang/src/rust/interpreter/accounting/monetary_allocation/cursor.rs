use std::num::NonZeroUsize;

use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MonetaryCursor {
    revision: i64,
    position: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MonetaryCursorTransition {
    scope: [u8; 32],
    expected: MonetaryCursor,
    next: MonetaryCursor,
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum MonetaryCursorError {
    #[error("monetary cursor revision must be nonnegative")]
    InvalidRevision,
    #[error("monetary cursor position is outside the payer cohort")]
    InvalidPosition,
    #[error("monetary cursor revision is exhausted")]
    RevisionExhausted,
    #[error("monetary cursor transition belongs to a different payer scope")]
    ScopeMismatch,
    #[error("monetary cursor transition does not match the current cursor")]
    StaleTransition,
    #[error("monetary cursor transition must advance exactly one revision")]
    InvalidSuccessor,
}

impl MonetaryCursor {
    pub const INITIAL: Self = Self {
        revision: 0,
        position: 0,
    };

    pub fn new(
        revision: i64,
        position: i64,
        payer_count: NonZeroUsize,
    ) -> Result<Self, MonetaryCursorError> {
        let cursor = Self { revision, position };
        cursor.validate(payer_count)?;
        Ok(cursor)
    }

    pub fn revision(self) -> i64 { self.revision }

    pub fn position(self) -> i64 { self.position }

    pub fn position_index(self, payer_count: NonZeroUsize) -> Result<usize, MonetaryCursorError> {
        self.validate(payer_count)?;
        usize::try_from(self.position).map_err(|_| MonetaryCursorError::InvalidPosition)
    }

    pub fn validate(self, payer_count: NonZeroUsize) -> Result<(), MonetaryCursorError> {
        if self.revision < 0 {
            return Err(MonetaryCursorError::InvalidRevision);
        }
        let position =
            usize::try_from(self.position).map_err(|_| MonetaryCursorError::InvalidPosition)?;
        if position >= payer_count.get() {
            return Err(MonetaryCursorError::InvalidPosition);
        }
        Ok(())
    }

    pub fn next_revision(self) -> Result<i64, MonetaryCursorError> {
        self.revision
            .checked_add(1)
            .ok_or(MonetaryCursorError::RevisionExhausted)
    }
}

impl MonetaryCursorTransition {
    pub fn new(
        scope: [u8; 32],
        expected: MonetaryCursor,
        next_position: i64,
        payer_count: NonZeroUsize,
    ) -> Result<Self, MonetaryCursorError> {
        expected.validate(payer_count)?;
        let next = MonetaryCursor::new(expected.next_revision()?, next_position, payer_count)?;
        Ok(Self {
            scope,
            expected,
            next,
        })
    }

    pub fn from_parts(
        scope: [u8; 32],
        expected: MonetaryCursor,
        next: MonetaryCursor,
        payer_count: NonZeroUsize,
    ) -> Result<Self, MonetaryCursorError> {
        let transition = Self::new(scope, expected, next.position, payer_count)?;
        if transition.next != next {
            return Err(MonetaryCursorError::InvalidSuccessor);
        }
        Ok(transition)
    }

    pub fn scope(&self) -> &[u8; 32] { &self.scope }

    pub fn expected(&self) -> MonetaryCursor { self.expected }

    pub fn next(&self) -> MonetaryCursor { self.next }

    pub fn checked_successor(
        &self,
        scope: &[u8; 32],
        current: MonetaryCursor,
        payer_count: NonZeroUsize,
    ) -> Result<MonetaryCursor, MonetaryCursorError> {
        current.validate(payer_count)?;
        Self::from_parts(self.scope, self.expected, self.next, payer_count)?;
        if self.scope != *scope {
            return Err(MonetaryCursorError::ScopeMismatch);
        }
        if self.expected != current {
            return Err(MonetaryCursorError::StaleTransition);
        }
        Ok(self.next)
    }
}

#[cfg(test)]
mod tests;
