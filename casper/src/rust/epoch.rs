//! Epoch type — a typed wrapper around the integer epoch number used by
//! the slashing authorization predicate (T-9.8, T-9.14) and the
//! validator-bond lifecycle.
//!
//! P4-3 (slashing audit): the protocol previously passed epoch numbers as
//! bare `i64`/`i32` everywhere, with the meaning ("epoch number" vs "block
//! number" vs "block height") inferred from the parameter name. This file
//! introduces a one-field newtype `Epoch(i64)` that is used wherever the
//! slashing-authorization code reasons about epochs, so that:
//!
//! * accidental mixing (e.g. passing a block height where an epoch
//!   number is expected) becomes a type error;
//! * arithmetic on epochs is checked-by-default (`Epoch::checked_add`
//!   surfaces overflow as `None`);
//! * external code that reaches the protobuf boundary (`SystemDeployData::Slash`)
//!   still uses bare `i64`, with explicit conversion at the boundary.
//!
//! The type derives `Copy + Eq + Ord + Hash`, mirroring the operations the
//! audit's call sites depend on (comparison, map keys, hash-set membership).

use std::fmt;

use serde::{Deserialize, Serialize};

/// Phase 9.5 (R-5): added `Default` (for `BTreeMap::Entry::or_default()`),
/// `Serialize` / `Deserialize` (for any future state snapshots or wire
/// formats), and `From<i32>` (shard config carries `epoch_length: i32`).
///
/// Inner field is private: anyone outside this module must construct an
/// `Epoch` via `Epoch::new(...)` / `From<i64>` / `From<i32>` and project
/// back via `.get()`. The `pub i64` of the prior design defeated the
/// newtype's value proposition — any caller could write `Epoch(some_height)`
/// without a compiler warning, silently mixing epoch numbers and block
/// heights. The private field forces the type system to enforce the
/// epoch-vs-height distinction.
#[derive(
    Copy,
    Clone,
    Debug,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize
)]
pub struct Epoch(i64);

impl Epoch {
    /// Construct an epoch from a raw integer at a protobuf boundary.
    pub const fn new(value: i64) -> Self {
        Epoch(value)
    }

    /// Project back to the raw integer for protobuf serialization or
    /// comparisons with non-Epoch values.
    pub const fn get(self) -> i64 {
        self.0
    }

    /// Checked addition. Returns `None` on overflow (a hostile or
    /// malformed input could conceivably reach `i64::MAX` after enough
    /// epochs, even if it would take centuries of block production).
    pub const fn checked_add(self, rhs: i64) -> Option<Self> {
        match self.0.checked_add(rhs) {
            Some(v) => Some(Epoch(v)),
            None => None,
        }
    }

    /// Checked subtraction. Returns `None` on underflow (e.g. asking for
    /// the predecessor of epoch 0 or beyond `i64::MIN`). Mirror of
    /// `checked_add` for the docstring's "checked-by-default" claim.
    pub const fn checked_sub(self, rhs: i64) -> Option<Self> {
        match self.0.checked_sub(rhs) {
            Some(v) => Some(Epoch(v)),
            None => None,
        }
    }

    /// Checked multiplication. Returns `None` on overflow. Used in
    /// boundary arithmetic that converts between epoch numbers and
    /// block numbers (the epoch-to-block-number multiplication of the
    /// epoch by `epoch_length`).
    pub const fn checked_mul(self, rhs: i64) -> Option<Self> {
        match self.0.checked_mul(rhs) {
            Some(v) => Some(Epoch(v)),
            None => None,
        }
    }
}

impl fmt::Display for Epoch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<i64> for Epoch {
    fn from(value: i64) -> Self {
        Epoch(value)
    }
}

impl From<i32> for Epoch {
    fn from(value: i32) -> Self {
        Epoch(i64::from(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_round_trip() {
        let e = Epoch::new(42);
        assert_eq!(e.get(), 42);
        assert_eq!(format!("{e}"), "42");
    }

    #[test]
    fn epoch_checked_add_overflow() {
        let max_epoch = Epoch::new(i64::MAX);
        assert!(max_epoch.checked_add(1).is_none());
        let near_max = Epoch::new(i64::MAX - 1);
        assert_eq!(near_max.checked_add(1), Some(Epoch::new(i64::MAX)));
    }

    #[test]
    fn epoch_ordering() {
        assert!(Epoch::new(1) < Epoch::new(2));
        assert!(Epoch::new(5) > Epoch::new(3));
    }

    #[test]
    fn epoch_from_i64() {
        let e: Epoch = 7_i64.into();
        assert_eq!(e, Epoch::new(7));
    }

    #[test]
    fn epoch_default_is_zero() {
        let e: Epoch = Default::default();
        assert_eq!(e, Epoch::new(0));
    }

    #[test]
    fn epoch_from_i32_widens_to_i64() {
        let e: Epoch = 13_i32.into();
        assert_eq!(e, Epoch::new(13));
    }

    #[test]
    fn epoch_serde_roundtrip() {
        let original = Epoch::new(0xDEAD_BEEF);
        let json = serde_json::to_string(&original).expect("serialize");
        let back: Epoch = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, original);
    }
}
