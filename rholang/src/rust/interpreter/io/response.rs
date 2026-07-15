//! Encoding helpers for the FIP `[true, ...result]` / `[false, code, msg]`
//! response tuple.
//!
//! Every user-facing file/dir agent method returns this shape per the
//! FIP §"Errors" section. Native primitives use it too so the agent
//! layer can forward the result unchanged; if a future refactor
//! pushes error translation into the Rholang layer, only these helpers
//! change.
//!
//! Error codes are the string constants listed in the FIP §"Standard
//! error codes". Kept as plain string constants here rather than an
//! `enum` because the Rholang side matches on the string form and it's
//! simpler to keep one source of truth.

use models::rhoapi::Par;

use crate::rust::interpreter::rho_type::{RhoBoolean, RhoList, RhoString};

pub const FSERR_UNSUPPORTED: &str = "FSERR_UNSUPPORTED";
pub const FSERR_NOT_FOUND: &str = "FSERR_NOT_FOUND";
pub const FSERR_EXISTS: &str = "FSERR_EXISTS";
pub const FSERR_PERM: &str = "FSERR_PERM";
pub const FSERR_IO: &str = "FSERR_IO";
pub const FSERR_BUSY: &str = "FSERR_BUSY";
pub const FSERR_REVOKED: &str = "FSERR_REVOKED";
pub const FSERR_QUOTA_EXCEEDED: &str = "FSERR_QUOTA_EXCEEDED";
pub const FSERR_CLOSED: &str = "FSERR_CLOSED";
pub const FSERR_QUARANTINE: &str = "FSERR_QUARANTINE";
pub const FSERR_CROSS_DEVICE: &str = "FSERR_CROSS_DEVICE";
pub const FSERR_BAD_ARG: &str = "FSERR_BAD_ARG";

/// Encode `[true, ...values]` as a Rholang list.
pub fn ok(values: Vec<Par>) -> Par {
    let mut list = Vec::with_capacity(1 + values.len());
    list.push(RhoBoolean::create_par(true));
    list.extend(values);
    RhoList::create_par(list)
}

/// Encode `[false, code, msg]` as a Rholang list.
pub fn err(code: &str, msg: impl Into<String>) -> Par {
    RhoList::create_par(vec![
        RhoBoolean::create_par(false),
        RhoString::create_par(code.to_string()),
        RhoString::create_par(msg.into()),
    ])
}

/// Translate a `std::io::Error` to a standard error tuple.
///
/// Maps `NotFound` / `AlreadyExists` / `PermissionDenied` to their
/// FIP-named codes; everything else is `FSERR_IO`. The FIP-mentioned
/// `FSERR_CROSS_DEVICE` (for atomic-rename failures) is emitted by
/// the rename handler directly since it needs to inspect the error
/// kind for `CrossesDevices`, which isn't in `io::ErrorKind` until
/// nightly.
pub fn from_io_error(e: std::io::Error) -> Par {
    let code = match e.kind() {
        std::io::ErrorKind::NotFound => FSERR_NOT_FOUND,
        std::io::ErrorKind::AlreadyExists => FSERR_EXISTS,
        std::io::ErrorKind::PermissionDenied => FSERR_PERM,
        _ => FSERR_IO,
    };
    err(code, e.to_string())
}

#[cfg(test)]
mod tests {
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::EList;

    use super::*;

    fn expect_list(par: &Par) -> &EList {
        let expr = par.exprs.first().expect("has expr");
        match expr.expr_instance.as_ref().expect("has instance") {
            ExprInstance::EListBody(list) => list,
            _ => panic!("expected EListBody"),
        }
    }

    #[test]
    fn ok_produces_true_prefixed_list() {
        use crate::rust::interpreter::rho_type::RhoNumber;
        let par = ok(vec![RhoNumber::create_par(42)]);
        let list = expect_list(&par);
        assert_eq!(list.ps.len(), 2);
    }

    #[test]
    fn err_produces_false_code_msg_triple() {
        let par = err(FSERR_NOT_FOUND, "no such file");
        let list = expect_list(&par);
        assert_eq!(list.ps.len(), 3);
    }

    #[test]
    fn from_io_error_maps_notfound() {
        let par = from_io_error(std::io::Error::new(std::io::ErrorKind::NotFound, "gone"));
        let list = expect_list(&par);
        let code = crate::rust::interpreter::rho_type::RhoString::unapply(&list.ps[1])
            .expect("code is string");
        assert_eq!(code, FSERR_NOT_FOUND);
    }
}
