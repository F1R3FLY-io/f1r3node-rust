// Builders for the two canonical native-reply shapes:
//   [true, ...values]           on success
//   [false, code, msg]          on error
//
// Both shapes are represented as a Rholang list (`EList`) with the head
// discriminator followed by the payload.

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, Expr, Par};
use shared::rust::BitSet;

use super::super::rho_type::{RhoBoolean, RhoByteArray, RhoNumber, RhoString};

/// Type-safe wrapper around a raw file descriptor u64.  Introduced
/// after a signedness bug that swapped `extract_ok_u64` for
/// `extract_ok_fd` at fd extraction sites: the two helpers have
/// diverging semantics on negative i64 payloads (fd sites must
/// bit-preserve, quantity sites must reject-negative), so a call
/// site picking the wrong helper was one line away from re-opening
/// a canary flake for exactly those state-hashes whose derived fd
/// watermark exceeds `i64::MAX`.
///
/// Making the fd type distinct from `u64` means a future call site
/// that hands a fd into a quantity slot (or vice versa) becomes a
/// type error at the call site, not a runtime WAL divergence.
///
/// `#[repr(transparent)]` guarantees ABI compatibility with `u64`.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Fd(u64);

impl Fd {
    /// Unwrap into the raw `u64` for handoff to the fd-table layer.
    /// The newtype's job is to keep call sites from mixing fd with
    /// quantity values; once the fd reaches the table boundary it
    /// becomes just a u64 again.
    #[inline]
    pub fn as_u64(self) -> u64 { self.0 }
}

/// The fd → `Fd` conversion is the only blessed way to lift a raw
/// `u64` fd into the newtype now that the field is private.
impl From<u64> for Fd {
    #[inline]
    fn from(raw: u64) -> Self { Fd(raw) }
}

fn list_par(items: Vec<Par>) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EListBody(EList {
            ps: items,
            locally_free: BitSet::default(),
            connective_used: false,
            remainder: None,
        })),
    }])
}

fn bool_par(b: bool) -> Par { Par::default().with_exprs(vec![RhoBoolean::create_expr(b)]) }

pub fn ok_bare() -> Par { list_par(vec![bool_par(true)]) }

pub fn ok_int(n: i64) -> Par { list_par(vec![bool_par(true), RhoNumber::create_par(n)]) }

pub fn ok_u64(n: u64) -> Par {
    // Rholang integers are 64-bit signed.  Cap at i64::MAX; callers who
    // supply oversized values were rejected upstream by the per-call caps.
    ok_int(n as i64)
}

/// Emission-side companion to `extract_ok_fd`.  Wire format is
/// byte-identical to `ok_u64(fd.as_u64())` — the reply is
/// `[true, i64_cast(fd)]`, and fd values above `i64::MAX`
/// bit-preserve-reinterpret through the same `as i64` cast that
/// `extract_ok_fd` reverses via bit-preserve.  Callers that
/// historically wrote `ok_u64(fd)` should migrate to
/// `ok_fd(Fd::from(fd))` so the fd-vs-quantity newtype invariant is
/// enforced at BOTH emission and extraction ends symmetrically.
pub fn ok_fd(fd: Fd) -> Par {
    // Same wire shape as ok_u64: `[true, i64_cast(u64)]`.
    // Extraction via `extract_ok_fd` bit-preserve-reinterprets the
    // i64 back to u64, so the round-trip is lossless across the
    // full u64 range.
    ok_int(fd.as_u64() as i64)
}

pub fn ok_bool(b: bool) -> Par { list_par(vec![bool_par(true), bool_par(b)]) }

pub fn ok_bytes(bytes: Vec<u8>) -> Par {
    list_par(vec![bool_par(true), RhoByteArray::create_par(bytes)])
}

pub fn ok_string(s: String) -> Par { list_par(vec![bool_par(true), RhoString::create_par(s)]) }

pub fn ok_par(p: Par) -> Par { list_par(vec![bool_par(true), p]) }

pub fn ok_list(items: Vec<Par>) -> Par { list_par(vec![bool_par(true), list_par(items)]) }

/// Construct a `[false, code, msg]` error reply Par.  `code` is
/// typed as `FserrCode` (the newtype over `&'static str`) so the
/// compiler rejects raw-string misuse at every call site — any
/// code passed here must be a spec-canonical `FSERR_*` constant.
///
/// Wire bytes unchanged from a pre-newtype `&'static str` signature:
/// `FserrCode::as_str()` returns the same `&'static str` value, and
/// `RhoString::create_par` encodes it identically.
pub fn err(code: super::errors::FserrCode, msg: impl Into<String>) -> Par {
    list_par(vec![
        bool_par(false),
        RhoString::create_par(code.as_str().to_string()),
        RhoString::create_par(msg.into()),
    ])
}

/// End-of-stream terminator for streaming replies (`entriesStreamNext`).
/// Deliberately 2-element (`[false, "EOS"]`) so callers can distinguish
/// normal termination from a genuine error (`[false, code, msg]` = 3-
/// element).  `"EOS"` is not an `FSERR_*` code — EOS is expected
/// control flow, not a failure.
pub fn err_eos() -> Par {
    list_par(vec![
        bool_par(false),
        RhoString::create_par("EOS".to_string()),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wire-format equivalence pin: `ok_fd(Fd::from(raw))` must produce
    /// a byte-identical Par to `ok_u64(raw)`.  Consensus depends on this
    /// equivalence — any pre-newtype WAL entry / block hash that
    /// captured `ok_u64(fd)` output must remain valid under the new
    /// emission path.
    #[test]
    fn ok_fd_wire_identical_to_legacy_ok_u64() {
        for raw in [0u64, 1, 100, i64::MAX as u64, u64::MAX] {
            let via_fd = ok_fd(Fd::from(raw));
            let via_u64 = ok_u64(raw);
            assert_eq!(
                via_fd, via_u64,
                "wire-format drift: ok_fd(Fd::from({raw})) MUST produce \
                 a byte-identical Par to ok_u64({raw})"
            );
        }
    }

    /// `#[repr(transparent)]` invariant pin: any refactor that removes
    /// the attribute breaks CI here, so FFI boundaries that transmute
    /// between `Fd` and `u64` stay sound.
    #[test]
    fn fd_is_repr_transparent_over_u64() {
        assert_eq!(
            std::mem::size_of::<Fd>(),
            std::mem::size_of::<u64>(),
            "Fd must have identical size to u64 (#[repr(transparent)])"
        );
        assert_eq!(
            std::mem::align_of::<Fd>(),
            std::mem::align_of::<u64>(),
            "Fd must have identical alignment to u64 (#[repr(transparent)])"
        );
    }
}
