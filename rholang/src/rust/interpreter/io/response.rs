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

pub fn ok_bool(b: bool) -> Par { list_par(vec![bool_par(true), bool_par(b)]) }

pub fn ok_bytes(bytes: Vec<u8>) -> Par {
    list_par(vec![bool_par(true), RhoByteArray::create_par(bytes)])
}

pub fn ok_string(s: String) -> Par { list_par(vec![bool_par(true), RhoString::create_par(s)]) }

pub fn ok_par(p: Par) -> Par { list_par(vec![bool_par(true), p]) }

pub fn ok_list(items: Vec<Par>) -> Par { list_par(vec![bool_par(true), list_par(items)]) }

pub fn err(code: &str, msg: impl Into<String>) -> Par {
    list_par(vec![
        bool_par(false),
        RhoString::create_par(code.to_string()),
        RhoString::create_par(msg.into()),
    ])
}

/// Streaming-backing slice (2026-08-25): end-of-stream terminator
/// for `entriesStreamNext`.  Deliberately 2-element (`[false, "EOS"]`)
/// so callers can distinguish normal termination from a genuine
/// error (`[false, code, msg]` = 3-element).  `"EOS"` is not an
/// `FSERR_*` code — EOS is expected control flow, not a failure.
pub fn err_eos() -> Par {
    list_par(vec![
        bool_par(false),
        RhoString::create_par("EOS".to_string()),
    ])
}

/// C-R1 review fix: extract the head-`true` + second-element-u64 shape
/// from a cached `previous` reply, treating the second element as a
/// **non-negative quantity**.  Returns `Some(n)` if the Par is
/// `[true, n_int]` with `n_int >= 0`; `None` otherwise (error reply,
/// wrong shape, or negative).
///
/// # Semantic scope (2026-08-30 review-follow-up)
///
/// This helper is for reply values that ARE semantically non-negative
/// quantities:
/// - `fs_write` / `fs_write_at` reply `n` (bytes written by the
///   syscall; libc `write()` returns `ssize_t` with negatives
///   flagged as errors upstream at `handlers.rs:1663`, so a
///   well-formed reply always has `n >= 0` bounded above by
///   `MAX_WRITE_BYTES < i64::MAX`).
/// - `fs_seek` reply `new_pos` (POSIX file offset; non-negative in
///   any valid file).
///
/// A well-formed reply always fits in a positive i64 for these
/// callers, so the reject-negative guard is a defense-in-depth
/// safety net (a byzantine `previous` with `n < 0` would otherwise
/// reinterpret to a huge unsigned value and corrupt downstream
/// state — e.g., `position.saturating_add(n)` would saturate to
/// `u64::MAX`).  RSpace's rig-verification (`observe_produce`) makes
/// byzantine injection impractical in practice, but the guard costs
/// nothing and preserves the "well-formed reply or None" contract.
///
/// # Fd extraction uses [`extract_ok_fd`] instead
///
/// A prior version of this helper was ALSO used for fd extraction
/// on `fs_open`'s replay branch (see `handlers.rs:936`).  Fds are
/// seeded from state-hash entropy via `seed_next_fd_from_state_hash`
/// and commonly exceed `i64::MAX` — Rholang's `GInt` wraps those
/// bits to negative `i64`, and this helper's reject-negative guard
/// would then bail, causing the follower to silently skip its
/// shadow install (PB-M-14 canary flakiness root cause,
/// 2026-08-29).  The fix at commit `02b4c2efe` briefly changed
/// this helper to bit-preserve-reinterpret, but that weakened the
/// safety net for the fs_write/fs_seek callers.  The 2026-08-30
/// review-follow-up split the two semantics: fd sites now call
/// [`extract_ok_fd`] (bit reinterpret), quantity sites call
/// `extract_ok_u64` (reject negative).
pub fn extract_ok_u64(previous: &[Par]) -> Option<u64> {
    let head = previous.first()?;
    let expr = head.exprs.first()?;
    let list = match expr.expr_instance.as_ref()? {
        ExprInstance::EListBody(l) => l,
        _ => return None,
    };
    // Shape: [true_par, u64_par].
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

/// DD-7b-2 (a) Option 2 review-follow-up (2026-08-30): fd-specific
/// extraction from a cached `[true, fd]` reply — bit-preserving
/// reinterpret of any i64 payload to a u64 fd.
///
/// # Why a separate helper from [`extract_ok_u64`]
///
/// Fds are seeded from state-hash entropy via
/// `seed_next_fd_from_state_hash` (44 bits of entropy in the upper
/// `u64` range).  For state hashes whose top byte's high bit is set,
/// the derived watermark is `>= 2^63`, so allocated fds commonly
/// exceed `i64::MAX`.  Rholang's `GInt(fd as i64)` storage wraps
/// those bits to a large negative `i64`; a well-formed reply's fd
/// slot must be reinterpreted bitwise to recover the original
/// unsigned fd number.
///
/// The mutating fs handlers already parse their fd argument via
/// `RhoNumber::unapply(fd_par).map(|n| n as u64)` — bit-preserving
/// reinterpret (e.g., `handlers.rs::fs_write` line 1452).  This
/// helper matches that shape for fs_open's / entriesStream_open's
/// replay-branch shadow-install path so both sides derive
/// bit-identical fd values regardless of sign.
///
/// # Call sites
///
/// - `handlers.rs::fs_open` replay branch — reconstruct the leader's
///   fd for shadow-handle `insert_at`.
/// - `handlers.rs::fs_entries_stream_open` replay branch — mirror
///   for the dir-handle table.
///
/// A regression to reject-negative here would re-open the PB-M-14
/// canary flake (documented at
/// `extract_ok_fd_matches_observed_failing_fd`).
pub fn extract_ok_fd(previous: &[Par]) -> Option<u64> {
    let head = previous.first()?;
    let expr = head.exprs.first()?;
    let list = match expr.expr_instance.as_ref()? {
        ExprInstance::EListBody(l) => l,
        _ => return None,
    };
    // Shape: [true_par, fd_par].
    let ok_par = list.ps.first()?;
    if RhoBoolean::unapply(ok_par) != Some(true) {
        return None;
    }
    let val_par = list.ps.get(1)?;
    let n = RhoNumber::unapply(val_par)?;
    // Bit-preserving reinterpret — matches `handlers.rs::fs_write`'s
    // `fd as u64` parse.  See the docstring for the state-hash
    // entropy rationale.
    Some(n as u64)
}

/// H-6 fix (2026-08-06): extract the string FSERR code from an
/// error reply of shape `[false, "FSERR_...", "msg"]`.  Returns
/// `None` if the reply is not an error (head is `true`), the
/// shape doesn't match, or the code slot is missing / non-string.
/// Both leader (fresh syscall reply) and follower (cached
/// `previous` reply) use this to derive an identical failure
/// code for `finalize_failure_journal`, keeping WAL entries
/// byte-identical across the leader/follower split.
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

/// Slice 32 (PB-M-14 read-hash) counterpart to `extract_ok_u64`:
/// extract the bytes payload from a cached `[true, ByteArray]`
/// reply.  Used by `fs_read` / `fs_read_at`'s `is_replay = true`
/// branch to re-hash the leader's returned bytes and append a
/// matching Read/ReadAt WAL entry — keeping leader/follower WALs
/// byte-identical without re-issuing the syscall on the follower.
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

/// Slice 9b-iv follow-up: extract the length of the inner list from
/// a cached `[true, [x, y, z, ...]]` reply.  Returns `Some(n)` where
/// `n` is the number of items in the inner list; `None` on error
/// replies (`[false, ...]`), on `[true, non_list]` shapes, or on
/// any non-list-of-list reply.
///
/// The entries-family handlers (`fs_entries`,
/// eventually `fs_entries_stream`) use this on their `is_replay = true`
/// branch to recover the leader-supplied entry count from `previous`
/// and match the leader's per-entry supplement charge —
/// keeping the D3 canonical event log byte-identical across the
/// leader/follower split without re-executing the syscall.
///
/// Total; never panics.  A hostile or out-of-band `previous` shape
/// yields `None` rather than a partial extraction, and the caller
/// treats `None` as "charge 0 per-entry supplement" — which matches
/// the leader path's charge on error replies (also 0), preserving
/// parity even when the reply is malformed.
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
    //! Slice 9b-iv follow-up: parity of `extract_ok_list_len` across
    //! the reply shapes fs_entries emits — the extraction MUST agree
    //! with the leader's actual `ok_list(rows)` construction on
    //! success paths and cleanly yield `None` (→ zero supplement
    //! charge) on every error path, keeping leader/replay per-entry
    //! charges byte-identical.
    //!
    //! Regression scenarios each test defends against:
    //!   * ok_list(N) length drift: extraction miscounts N.
    //!   * Empty error reply matches a success shape by accident.
    //!   * `[true, non_list]` reply (never produced by real handlers,
    //!     but a hostile follower cache would fall through cleanly
    //!     rather than panic).
    //!   * Empty `previous` slice (should not happen in practice but
    //!     the fallback is `None` → 0-supplement, matching leader on
    //!     malformed reply).
    use super::*;

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
        let reply = err("FSERR_QUOTA_EXCEEDED", "entries exceeds MAX_ENTRIES");
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
        // ok_int is `[true, GInt]` — the second slot is a scalar,
        // not an EList.  Real handlers never emit this shape for
        // entries-family, but a hostile follower cache could;
        // fall-through to None is the safe default.
        let reply = ok_int(42);
        assert_eq!(
            extract_ok_list_len(std::slice::from_ref(&reply)),
            None,
            "extract_ok_list_len must reject `[true, non-list]` shapes with \
             None rather than panic or synthesize a bogus count.  Total \
             extraction is required — any panic here would crash a follower \
             validator on hostile input."
        );
    }

    #[test]
    fn extract_ok_list_len_returns_none_on_empty_previous() {
        // An empty `previous` slice should never happen for a produced
        // reply, but the safe fallback is None so the caller charges
        // 0 supplement.
        let previous: [Par; 0] = [];
        assert_eq!(
            extract_ok_list_len(&previous),
            None,
            "extract_ok_list_len must not panic on an empty previous slice; \
             the None fallback yields a 0-supplement charge which is \
             conservative and matches leader on any malformed reply."
        );
    }

    #[test]
    fn extract_ok_list_len_returns_none_on_bare_ok() {
        // `ok_bare()` is `[true]` (no payload).  Fine as a success
        // reply for `close` / `flush` / `remove_file`; not a
        // valid entries reply, so extraction returns None.
        let reply = ok_bare();
        assert_eq!(
            extract_ok_list_len(std::slice::from_ref(&reply)),
            None,
            "extract_ok_list_len must return None on `[true]` (bare success \
             with no payload) — a bare ok reply is not an entries reply, so \
             the None → 0-supplement fallback is correct."
        );
    }

    // ---------------------------------------------------------------
    // 2026-08-30 review-follow-up split: `extract_ok_u64` (reject
    // negative — for quantities) vs. `extract_ok_fd` (bit-preserving
    // reinterpret — for fds).  Prior to the split, a single
    // `extract_ok_u64` served both cases; commit `02b4c2efe`
    // temporarily broadened it to reinterpret in order to fix the
    // PB-M-14 canary flake (see below), but that weakened the
    // safety net for the quantity callers.  The follow-up split
    // preserves both semantics:
    //
    // * Fd sites (`fs_open`, `fs_entries_stream_open` replay
    //   branches) call `extract_ok_fd` — fds seeded from state-hash
    //   entropy commonly exceed `i64::MAX` and must be reinterpreted.
    // * Quantity sites (`fs_write` / `fs_write_at` bytes-written
    //   parsing, `fs_seek` new-position parsing) call
    //   `extract_ok_u64` — the reject-negative guard is defense-in-
    //   depth against a byzantine `previous` producing garbage
    //   downstream state.
    //
    // The tests below cover both helpers.
    // ---------------------------------------------------------------

    // --- extract_ok_fd (bit-preserving reinterpret) ----------------

    /// PB-M-14 canary root cause (2026-08-29): fds seeded from
    /// state-hash entropy commonly land above `i64::MAX`; Rholang
    /// stores them as `GInt(fd as i64)`, which wraps to a large
    /// negative i64.  `extract_ok_fd` must bit-preserve-reinterpret
    /// to recover the original u64 fd.  A regression to
    /// reject-negative here re-opens the PB-M-14 canary flake.
    #[test]
    fn extract_ok_fd_accepts_negative_i64_as_u64_reinterpret() {
        let neg = i64::MIN;
        let expected = neg as u64;
        let reply = ok_int(neg);
        assert_eq!(
            extract_ok_fd(std::slice::from_ref(&reply)),
            Some(expected),
            "extract_ok_fd must bit-preserve-reinterpret negative i64 fds \
             (upper-u64-range fds seeded from state-hash entropy commonly \
             land here).  A regression to `if n < 0 {{ None }}` re-opens \
             the PB-M-14 canary flake — the follower's fs_open replay \
             branch would silently skip insert_at for exactly those \
             state-hashes whose top byte's high bit is set."
        );
    }

    /// The specific fd value observed in the failing PB-M-14 trace.
    /// GInt(-3992895570068897791) reinterprets to u64
    /// 14453848503640653825 — the same fd `fs_write`'s
    /// `journal_write` was looking up.  Pins the exact scenario.
    #[test]
    fn extract_ok_fd_matches_observed_failing_fd() {
        let observed_neg: i64 = -3992895570068897791;
        let expected_u64: u64 = 14453848503640653825;
        assert_eq!(observed_neg as u64, expected_u64, "i64→u64 bit reinterpret sanity");
        let reply = ok_int(observed_neg);
        assert_eq!(
            extract_ok_fd(std::slice::from_ref(&reply)),
            Some(expected_u64),
            "extract_ok_fd must return the exact fd that a mutating fs \
             handler will look up in the fd table.  The 2026-08-29 \
             PB-M-14 canary trace showed this specific pair — fixing \
             the extraction is what eliminates that flake mode."
        );
    }

    /// Error replies still bail cleanly on both helpers.
    #[test]
    fn extract_ok_fd_still_rejects_error_replies() {
        let reply = err("FSERR_BAD_ARG", "invalid path");
        assert_eq!(
            extract_ok_fd(std::slice::from_ref(&reply)),
            None,
            "extract_ok_fd must return None on `[false, code, msg]` \
             shapes — the head-`true` guard bails before the reinterpret."
        );
    }

    /// Positive fds round-trip identically through both helpers.
    #[test]
    fn extract_ok_fd_accepts_positive_fds() {
        for fd in [1u64, 100, 1_000_000, i64::MAX as u64] {
            let reply = ok_int(fd as i64);
            assert_eq!(
                extract_ok_fd(std::slice::from_ref(&reply)),
                Some(fd),
                "positive fd {fd} must round-trip through extract_ok_fd"
            );
        }
    }

    // --- extract_ok_u64 (reject-negative — for quantities) --------

    /// Quantity extraction restored to reject-negative semantics
    /// (2026-08-30 review-follow-up).  Legit fs_write / fs_seek
    /// replies always have the quantity `>= 0`; a byzantine
    /// negative would otherwise reinterpret to a huge unsigned and
    /// corrupt `position.saturating_add(n)` downstream (saturate to
    /// `u64::MAX`).  RSpace rig-verification makes this
    /// exploitation-impractical, but the guard costs nothing.
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
                 and corrupt downstream state (position saturation etc.).  \
                 Use `extract_ok_fd` for fd extraction where negatives are \
                 the wrapped-bit-pattern of legit upper-u64-range fds."
            );
        }
    }

    /// Error replies bail cleanly on the quantity path too.
    #[test]
    fn extract_ok_u64_rejects_error_replies() {
        let reply = err("FSERR_QUOTA_EXCEEDED", "over cap");
        assert_eq!(
            extract_ok_u64(std::slice::from_ref(&reply)),
            None,
            "extract_ok_u64 must return None on `[false, code, msg]` shapes."
        );
    }

    /// Non-negative quantities round-trip through `extract_ok_u64`.
    #[test]
    fn extract_ok_u64_accepts_nonneg_quantities() {
        for n in [0u64, 1, 100, 1_000_000, i64::MAX as u64] {
            let reply = ok_int(n as i64);
            assert_eq!(
                extract_ok_u64(std::slice::from_ref(&reply)),
                Some(n),
                "non-negative quantity {n} must round-trip through extract_ok_u64"
            );
        }
    }

    // --- Parity between the two helpers ---------------------------

    /// The two helpers agree on `[true, n]` for every non-negative
    /// `n` that fits in `i64`.  They diverge ONLY on negative `n`:
    /// `extract_ok_fd` reinterprets, `extract_ok_u64` returns None.
    /// A refactor that broke this parity would surface here — a
    /// deploy that opened an fd in the low range (`n <= i64::MAX`)
    /// would silently see different extraction results depending on
    /// which helper the call site was migrated to.
    #[test]
    fn extract_ok_fd_and_extract_ok_u64_agree_on_nonneg_values() {
        for n in [0u64, 1, 100, 1_000_000, (i64::MAX as u64) - 1, i64::MAX as u64] {
            let reply = ok_int(n as i64);
            let fd = extract_ok_fd(std::slice::from_ref(&reply));
            let u = extract_ok_u64(std::slice::from_ref(&reply));
            assert_eq!(
                fd, u,
                "extract_ok_fd and extract_ok_u64 must agree on \
                 non-negative n={n}; they diverge only on negative i64."
            );
            assert_eq!(fd, Some(n));
        }
    }
}
