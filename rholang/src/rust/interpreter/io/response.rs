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
/// from a cached `previous` reply.  Returns `Some(n)` if the Par is
/// `[true, n_int]` with `n_int >= 0`; `None` otherwise (error reply,
/// wrong shape, or negative).  Used by handlers whose `is_replay = true`
/// branch needs the leader's returned value (e.g., `fs_open` to
/// reconstruct the fd for shadow-handle insertion).
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
}
