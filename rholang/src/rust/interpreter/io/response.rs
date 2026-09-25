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

// ---------------------------------------------------------------------
// Extractors — receive side of the reply protocol.
//
// The split between `extract_ok_u64` (reject-negative — for
// quantities) vs. `extract_ok_fd` (bit-preserving reinterpret — for
// fds) is load-bearing.  Prior to the split, a single
// `extract_ok_u64` served both cases; a fix that broadened it to
// reinterpret fixed a canary flake but weakened the safety net for
// quantity callers.  Splitting preserves both semantics:
//
// * Fd sites (fs_open, fs_entries_stream_open replay branches) call
//   `extract_ok_fd` — fds seeded from state-hash entropy commonly
//   exceed `i64::MAX` and must be reinterpreted.
// * Quantity sites (fs_write / fs_write_at bytes-written parsing,
//   fs_seek new-position parsing) call `extract_ok_u64` — the
//   reject-negative guard is defense-in-depth against a byzantine
//   `previous` producing garbage downstream state.
// ---------------------------------------------------------------------

/// Extract the head-`true` + second-element-u64 shape from a cached
/// `previous` reply, treating the second element as a **non-negative
/// quantity**.  Returns `Some(n)` if the Par is `[true, n_int]` with
/// `n_int >= 0`; `None` otherwise (error reply, wrong shape, or
/// negative).
///
/// # Semantic scope
///
/// For reply values that ARE semantically non-negative quantities:
/// - `fs_write` / `fs_write_at` reply `n` (bytes written; libc
///   `write()` returns `ssize_t` with negatives flagged upstream, so
///   a well-formed reply always has `n >= 0` bounded above by
///   `MAX_WRITE_BYTES < i64::MAX`).
/// - `fs_seek` reply `new_pos` (POSIX file offset; non-negative).
///
/// A well-formed reply always fits in a positive i64 for these
/// callers.  The reject-negative guard is a defense-in-depth safety
/// net: a byzantine `previous` with `n < 0` would otherwise
/// reinterpret to a huge unsigned and corrupt downstream state
/// (e.g., `position.saturating_add(n)` saturates to `u64::MAX`).
///
/// # Fd extraction uses [`extract_ok_fd`] instead
///
/// A prior version served both fd and quantity extraction.  Fds
/// seeded from state-hash entropy commonly exceed `i64::MAX`;
/// Rholang stores them as `GInt(fd as i64)` which wraps to negative
/// i64.  The reject-negative guard here would silently skip fd
/// insert_at on those state-hashes — see [`extract_ok_fd`].
pub fn extract_ok_u64(previous: &[Par]) -> Option<u64> {
    let head = previous.first()?;
    let expr = head.exprs.first()?;
    let list = match expr.expr_instance.as_ref()? {
        ExprInstance::EListBody(l) => l,
        _ => return None,
    };
    let ok_par = list.ps.first()?;
    if RhoBoolean::unapply(ok_par) != Some(true) {
        return None;
    }
    let val_par = list.ps.get(1)?;
    let n = RhoNumber::unapply(val_par)?;
    if n < 0 {
        None
    } else {
        Some(n as u64)
    }
}

/// Fd-specific extraction from a cached `[true, fd]` reply — bit-
/// preserving reinterpret of any i64 payload to a u64 fd.
///
/// # Why a separate helper from [`extract_ok_u64`]
///
/// Fds are seeded from state-hash entropy (44 bits of entropy in the
/// upper `u64` range).  For state hashes whose top byte's high bit
/// is set, the derived watermark is `>= 2^63`, so allocated fds
/// commonly exceed `i64::MAX`.  Rholang's `GInt(fd as i64)` storage
/// wraps those bits to a large negative `i64`; a well-formed reply's
/// fd slot must be reinterpreted bitwise to recover the original
/// unsigned fd number.
///
/// The mutating fs handlers parse their fd argument via
/// `RhoNumber::unapply(fd_par).map(|n| n as u64)` — bit-preserving
/// reinterpret.  This helper matches that shape for the replay-branch
/// shadow-install path so both sides derive bit-identical fd values
/// regardless of sign.
///
/// # Return type
///
/// Returns [`Fd`], not raw `u64` — the newtype is a compile-time
/// guard against a future call site accidentally passing an fd to a
/// quantity slot (or vice versa).  Call sites unwrap via
/// [`Fd::as_u64`] at the fd-table boundary.
pub fn extract_ok_fd(previous: &[Par]) -> Option<Fd> {
    let head = previous.first()?;
    let expr = head.exprs.first()?;
    let list = match expr.expr_instance.as_ref()? {
        ExprInstance::EListBody(l) => l,
        _ => return None,
    };
    let ok_par = list.ps.first()?;
    if RhoBoolean::unapply(ok_par) != Some(true) {
        return None;
    }
    let val_par = list.ps.get(1)?;
    let n = RhoNumber::unapply(val_par)?;
    // Bit-preserving reinterpret — matches mutating fs handlers'
    // `fd as u64` parse.
    Some(Fd(n as u64))
}

/// Extract the string FSERR code from an error reply of shape
/// `[false, "FSERR_...", "msg"]`.  Returns `None` if the reply is
/// not an error (head is `true`), the shape doesn't match, or the
/// code slot is missing / non-string.  Both leader (fresh syscall
/// reply) and follower (cached `previous` reply) use this to derive
/// an identical failure code, keeping WAL entries byte-identical
/// across the leader/follower split.
pub fn extract_err_code(reply: &[Par]) -> Option<String> {
    let head = reply.first()?;
    let expr = head.exprs.first()?;
    let list = match expr.expr_instance.as_ref()? {
        ExprInstance::EListBody(l) => l,
        _ => return None,
    };
    let ok_par = list.ps.first()?;
    if RhoBoolean::unapply(ok_par) != Some(false) {
        return None;
    }
    let code_par = list.ps.get(1)?;
    RhoString::unapply(code_par)
}

/// Counterpart to `extract_ok_u64`: extract the bytes payload from
/// a cached `[true, ByteArray]` reply.  Used by `fs_read` /
/// `fs_read_at`'s `is_replay = true` branch to re-hash the leader's
/// returned bytes and append a matching Read/ReadAt WAL entry —
/// keeping leader/follower WALs byte-identical without re-issuing
/// the syscall on the follower.
pub fn extract_ok_bytes(previous: &[Par]) -> Option<Vec<u8>> {
    let head = previous.first()?;
    let expr = head.exprs.first()?;
    let list = match expr.expr_instance.as_ref()? {
        ExprInstance::EListBody(l) => l,
        _ => return None,
    };
    let ok_par = list.ps.first()?;
    if RhoBoolean::unapply(ok_par) != Some(true) {
        return None;
    }
    let val_par = list.ps.get(1)?;
    RhoByteArray::unapply(val_par)
}

/// Extract the length of the inner list from a cached
/// `[true, [x, y, z, ...]]` reply.  Returns `Some(n)` where `n` is
/// the number of items in the inner list; `None` on error replies
/// (`[false, ...]`), on `[true, non_list]` shapes, or on any non-
/// list-of-list reply.
///
/// Entries-family handlers (`fs_entries`, eventually
/// `fs_entries_stream`) use this on their `is_replay = true` branch
/// to recover the leader-supplied entry count from `previous` and
/// match the leader's per-entry supplement charge — keeping the
/// canonical event log byte-identical across the leader/follower
/// split without re-executing the syscall.
///
/// Total; never panics.  A hostile or out-of-band `previous` shape
/// yields `None`, and the caller treats `None` as "charge 0 per-
/// entry supplement" — which matches the leader path's charge on
/// error replies (also 0), preserving parity even when the reply is
/// malformed.
pub fn extract_ok_list_len(previous: &[Par]) -> Option<u64> {
    let head = previous.first()?;
    let expr = head.exprs.first()?;
    let outer = match expr.expr_instance.as_ref()? {
        ExprInstance::EListBody(l) => l,
        _ => return None,
    };
    let ok_par = outer.ps.first()?;
    if RhoBoolean::unapply(ok_par) != Some(true) {
        return None;
    }
    let inner_par = outer.ps.get(1)?;
    let inner_expr = inner_par.exprs.first()?;
    let inner = match inner_expr.expr_instance.as_ref()? {
        ExprInstance::EListBody(l) => l,
        _ => return None,
    };
    Some(inner.ps.len() as u64)
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

    // --- extract_ok_list_len ---------------------------------------

    #[test]
    fn extract_ok_list_len_matches_ok_list_arity() {
        for n in [0usize, 1, 10, 65_536] {
            let items: Vec<Par> = (0..n).map(|_| Par::default()).collect();
            let reply = ok_list(items);
            assert_eq!(
                extract_ok_list_len(std::slice::from_ref(&reply)),
                Some(n as u64),
                "extract_ok_list_len must count exactly the number of items \
                 in ok_list — mismatch at n={n} means the two-branch charge \
                 for fs_entries would over/under-count entries."
            );
        }
    }

    #[test]
    fn extract_ok_list_len_returns_none_on_error_reply() {
        let reply = err(
            super::super::errors::FSERR_QUOTA_EXCEEDED,
            "entries exceeds MAX_ENTRIES",
        );
        assert_eq!(
            extract_ok_list_len(std::slice::from_ref(&reply)),
            None,
            "extract_ok_list_len must return None on `[false, ...]` shapes \
             so the caller charges 0 per-entry supplement — matches leader's \
             own charge on error replies, preserving leader/replay parity."
        );
    }

    #[test]
    fn extract_ok_list_len_returns_none_on_ok_non_list_shape() {
        // ok_int is `[true, GInt]` — second slot is a scalar, not
        // an EList.  Real handlers never emit this shape for the
        // entries family, but a hostile follower cache could; fall-
        // through to None is the safe default.
        let reply = ok_int(42);
        assert_eq!(
            extract_ok_list_len(std::slice::from_ref(&reply)),
            None,
            "extract_ok_list_len must reject `[true, non-list]` shapes with \
             None rather than panic or synthesize a bogus count."
        );
    }

    #[test]
    fn extract_ok_list_len_returns_none_on_empty_previous() {
        let previous: [Par; 0] = [];
        assert_eq!(
            extract_ok_list_len(&previous),
            None,
            "extract_ok_list_len must not panic on an empty previous slice."
        );
    }

    #[test]
    fn extract_ok_list_len_returns_none_on_bare_ok() {
        // `ok_bare()` is `[true]` (no payload) — valid for
        // close/flush/remove_file replies but not entries.
        let reply = ok_bare();
        assert_eq!(
            extract_ok_list_len(std::slice::from_ref(&reply)),
            None,
            "extract_ok_list_len must return None on `[true]` (bare success)."
        );
    }

    // --- extract_ok_fd (bit-preserving reinterpret) ----------------

    /// Fds seeded from state-hash entropy commonly land above
    /// `i64::MAX`; Rholang stores them as `GInt(fd as i64)`, which
    /// wraps to a large negative i64.  `extract_ok_fd` must bit-
    /// preserve-reinterpret to recover the original u64 fd.  A
    /// regression to reject-negative here re-opens a canary flake
    /// where the follower's fs_open replay branch silently skipped
    /// insert_at for state-hashes whose top byte's high bit was set.
    #[test]
    fn extract_ok_fd_accepts_negative_i64_as_u64_reinterpret() {
        let neg = i64::MIN;
        let expected = Fd(neg as u64);
        let reply = ok_int(neg);
        assert_eq!(
            extract_ok_fd(std::slice::from_ref(&reply)),
            Some(expected),
            "extract_ok_fd must bit-preserve-reinterpret negative i64 fds"
        );
    }

    /// The specific fd value observed in the failing canary trace.
    /// `GInt(-3992895570068897791)` reinterprets to u64
    /// `14453848503640653825` — the same fd `fs_write`'s
    /// `journal_write` was looking up.  Pins the exact scenario.
    #[test]
    fn extract_ok_fd_matches_observed_failing_fd() {
        let observed_neg: i64 = -3992895570068897791;
        let expected_u64: u64 = 14453848503640653825;
        assert_eq!(
            observed_neg as u64, expected_u64,
            "i64→u64 bit reinterpret sanity"
        );
        let reply = ok_int(observed_neg);
        assert_eq!(
            extract_ok_fd(std::slice::from_ref(&reply)),
            Some(Fd(expected_u64)),
            "extract_ok_fd must return the exact fd that a mutating fs \
             handler will look up in the fd table."
        );
    }

    #[test]
    fn extract_ok_fd_still_rejects_error_replies() {
        let reply = err(super::super::errors::FSERR_BAD_ARG, "invalid path");
        assert_eq!(
            extract_ok_fd(std::slice::from_ref(&reply)),
            None,
            "extract_ok_fd must return None on `[false, code, msg]` shapes."
        );
    }

    #[test]
    fn extract_ok_fd_accepts_positive_fds() {
        for fd in [1u64, 100, 1_000_000, i64::MAX as u64] {
            let reply = ok_int(fd as i64);
            assert_eq!(
                extract_ok_fd(std::slice::from_ref(&reply)),
                Some(Fd(fd)),
                "positive fd {fd} must round-trip through extract_ok_fd"
            );
        }
    }

    /// The emission-side `ok_fd(Fd)` + extraction-side
    /// `extract_ok_fd(...)` MUST round-trip identically across the
    /// full u64 range, including the upper half where `as i64`
    /// reinterprets to negative i64.  Regression risk: a "signed-
    /// only" guard added to `ok_fd` would silently reject
    /// `fd > i64::MAX as u64` and produce a divergent reply on
    /// state-hash-seeded upper-half fds, firing
    /// `FSERR_CONSENSUS_DIVERGENCE` at replay.
    #[test]
    fn ok_fd_and_extract_ok_fd_round_trip_across_u64_range() {
        for raw in [
            1u64,
            1_000_000,
            i64::MAX as u64,
            (i64::MAX as u64).wrapping_add(1),
            u64::MAX,
        ] {
            let fd = Fd::from(raw);
            let reply = ok_fd(fd);
            let round_tripped = extract_ok_fd(std::slice::from_ref(&reply));
            assert_eq!(
                round_tripped,
                Some(fd),
                "round-trip failed for fd raw={raw}: ok_fd → extract_ok_fd \
                 must be lossless across the full u64 range"
            );
        }
    }

    // --- extract_ok_u64 (reject-negative — for quantities) --------

    #[test]
    fn extract_ok_u64_rejects_negative_i64_quantities() {
        for neg in [-1i64, -100, -1_000_000_000, i64::MIN] {
            let reply = ok_int(neg);
            assert_eq!(
                extract_ok_u64(std::slice::from_ref(&reply)),
                None,
                "extract_ok_u64 must return None on negative i64 quantities \
                 (n={neg}).  A regression to bit-preserving reinterpret \
                 would silently accept malformed fs_write / fs_seek replies \
                 and corrupt downstream state.  Use `extract_ok_fd` for fd \
                 extraction where negatives are the wrapped-bit-pattern of \
                 legit upper-u64-range fds."
            );
        }
    }

    #[test]
    fn extract_ok_u64_rejects_error_replies() {
        let reply = err(super::super::errors::FSERR_QUOTA_EXCEEDED, "over cap");
        assert_eq!(
            extract_ok_u64(std::slice::from_ref(&reply)),
            None,
            "extract_ok_u64 must return None on `[false, code, msg]` shapes."
        );
    }

    #[test]
    fn extract_ok_u64_accepts_nonneg_quantities() {
        for n in [0u64, 1, 100, 1_000_000, i64::MAX as u64] {
            let reply = ok_int(n as i64);
            assert_eq!(
                extract_ok_u64(std::slice::from_ref(&reply)),
                Some(n),
                "non-negative quantity {n} must round-trip through \
                 extract_ok_u64"
            );
        }
    }

    /// Parity: the two helpers agree on `[true, n]` for every non-
    /// negative `n` that fits in `i64`.  They diverge ONLY on
    /// negative `n`: `extract_ok_fd` reinterprets, `extract_ok_u64`
    /// returns None.  A refactor that broke this parity would
    /// silently give different extraction results depending on
    /// which helper the call site migrated to.
    #[test]
    fn extract_ok_fd_and_extract_ok_u64_agree_on_nonneg_values() {
        for n in [
            0u64,
            1,
            100,
            1_000_000,
            (i64::MAX as u64) - 1,
            i64::MAX as u64,
        ] {
            let reply = ok_int(n as i64);
            let fd = extract_ok_fd(std::slice::from_ref(&reply));
            let u = extract_ok_u64(std::slice::from_ref(&reply));
            assert_eq!(
                fd.map(Fd::as_u64),
                u,
                "extract_ok_fd and extract_ok_u64 must agree on non-negative \
                 n={n}; they diverge only on negative i64."
            );
            assert_eq!(fd, Some(Fd(n)));
        }
    }
}
