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
