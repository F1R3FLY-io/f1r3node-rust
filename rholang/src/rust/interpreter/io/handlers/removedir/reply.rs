// DD-RemoveDirReplyShape reply builders for `fs_remove_dir`.
//
// All items are `#[allow(dead_code)]`-permitted until the
// handler-side slice lands and makes them reachable.  Tests
// cover the shape-building logic in the meantime.
#![allow(dead_code)]

// The unified removeDir reply shape (2026-09-03) carries an
// `nDeleted` count at position 1 (success) or position 3
// (failure).  Four reply shapes exist across the recursive /
// non-recursive × success / failure matrix:
//
//   * `ok_with_count(n)` — `[true, n]` — non-recursive success +
//     Oracular recursive success.
//   * `err_with_count(code, msg, n)` — `[false, code, msg, n]` —
//     non-recursive failure + Oracular recursive failure.
//   * `ok_recursive_manifest(deleted)` —
//     `[true, nDeleted, [[path, kind], ...]]` — Consensus
//     recursive success (manifest at position 2 is the
//     implementation-side R5(b) follower re-execution channel;
//     Dir.rho unwraps to `[true, nDeleted]` at the Rholang
//     boundary).
//   * `err_with_manifest(code, msg, deleted)` —
//     `[false, code, msg, nDeleted, [[path, kind], ...]]` —
//     Consensus recursive failure.
//
// Plus `early_err_for_remove_dir(recursive, cmode, code, msg)` —
// the pre-walk early-failure picker that chooses between the
// 4-element and 5-element failure shapes based on
// (recursive, cmode) without needing a partial manifest.
//
// All reply builders are module-private (`pub(super)`): the
// handler-side `fs_remove_dir` method is the only legitimate
// caller; isolating them here reduces the risk of a sibling
// handler accidentally picking the wrong shape.

use std::path::PathBuf;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, Expr, Par};

use crate::rust::interpreter::io::errors::{fserr_to_code, io_err_code, FserrCode};
use crate::rust::interpreter::io::handlers::helpers::RemoveKind;
use crate::rust::interpreter::io::ConsensusMode;
use crate::rust::interpreter::rho_type::{RhoBoolean, RhoNumber, RhoString};

/// Unified removeDir reply shape: success reply carrying
/// `nDeleted` at position 1.  Used for non-recursive success
/// (`[true, 1]`) and Oracular recursive success (`[true, n]`).
pub(super) fn ok_with_count(n_deleted: u64) -> Par {
    list_par_2(bool_par_true(), RhoNumber::create_par(n_deleted as i64))
}

/// Unified removeDir reply shape: failure reply carrying
/// `nDeletedBeforeError` at position 3.  Used for non-recursive
/// failure (`n = 0`) and Oracular recursive failure
/// (`n = count-before-error`).
pub(super) fn err_with_count(code: FserrCode, msg: impl Into<String>, n_deleted: u64) -> Par {
    let items = vec![
        bool_par_false(),
        RhoString::create_par(code.as_str().to_string()),
        RhoString::create_par(msg.into()),
        RhoNumber::create_par(n_deleted as i64),
    ];
    list_par_from(items)
}

/// DD-RemoveDirReplyShape: success reply for a recursive
/// removeDir on a Consensus cap.  Shape:
/// `[true, nDeleted, [[path, kind], ...]]`.  `nDeleted` sits at
/// position 1 (uniform with all other removeDir success shapes);
/// the manifest at position 2 is the implementation-side channel
/// for R5(b) follower re-execution.  Dir.rho unwraps to
/// `[true, nDeleted]` at the Rholang boundary.
pub(super) fn ok_recursive_manifest(deleted: &[(PathBuf, RemoveKind)]) -> Par {
    let inner: Vec<Par> = deleted
        .iter()
        .map(|(path, kind)| {
            let path_s = path.to_string_lossy().into_owned();
            list_par_2(
                RhoString::create_par(path_s),
                RhoString::create_par(kind.as_wire().to_string()),
            )
        })
        .collect();
    let items = vec![
        bool_par_true(),
        RhoNumber::create_par(deleted.len() as i64),
        list_par_from(inner),
    ];
    list_par_from(items)
}

/// DD-RemoveDirReplyShape: early-failure reply picker for
/// `fs_remove_dir`.  Non-recursive OR Oracular → 4-element
/// `[false, code, msg, 0]`.  Consensus recursive → 5-element
/// `[false, code, msg, 0, []]` (empty manifest, no deletions
/// before this early error).  Used at pre-walk failure sites in
/// both the leader and follower spawn_blocking closures where
/// the walk didn't get far enough to have a partial manifest to
/// report.
pub(super) fn early_err_for_remove_dir(
    recursive: bool,
    cmode: ConsensusMode,
    code: FserrCode,
    msg: impl Into<String>,
) -> Par {
    if recursive && cmode == ConsensusMode::Consensus {
        err_with_manifest(code, msg, &[])
    } else {
        err_with_count(code, msg, 0)
    }
}

/// DD-RemoveDirReplyShape: failure reply for a recursive
/// removeDir on a Consensus cap.  Shape:
/// `[false, code, msg, nDeletedBeforeError, [[path, kind], ...]]`.
/// `nDeletedBeforeError` at position 3; manifest at position 4.
/// Dir.rho unwraps to `[false, code, msg, nDeletedBeforeError]`
/// at the Rholang boundary.
pub(super) fn err_with_manifest(
    code: FserrCode,
    msg: impl Into<String>,
    deleted: &[(PathBuf, RemoveKind)],
) -> Par {
    let inner: Vec<Par> = deleted
        .iter()
        .map(|(path, kind)| {
            let path_s = path.to_string_lossy().into_owned();
            list_par_2(
                RhoString::create_par(path_s),
                RhoString::create_par(kind.as_wire().to_string()),
            )
        })
        .collect();
    let items = vec![
        bool_par_false(),
        RhoString::create_par(code.as_str().to_string()),
        RhoString::create_par(msg.into()),
        RhoNumber::create_par(deleted.len() as i64),
        list_par_from(inner),
    ];
    list_par_from(items)
}

/// Compose `io_err_code(e) → fserr_to_code(...)` for the WAL
/// `WalOutcome::Failure { code }` slot.  Consumed by the handler
/// path when it needs the numeric FSERR code without a string
/// round-trip.
#[allow(dead_code)]
pub(super) fn io_err_code_u32(e: &std::io::Error) -> u32 { fserr_to_code(io_err_code(e).as_str()) }

// --- Internal Par builders ------------------------------------

fn list_par_from(items: Vec<Par>) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EListBody(EList {
            ps: items,
            locally_free: shared::rust::BitSet::default(),
            connective_used: false,
            remainder: None,
        })),
    }])
}

fn list_par_2(a: Par, b: Par) -> Par { list_par_from(vec![a, b]) }

fn bool_par_true() -> Par { Par::default().with_exprs(vec![RhoBoolean::create_expr(true)]) }

fn bool_par_false() -> Par { Par::default().with_exprs(vec![RhoBoolean::create_expr(false)]) }

// --- RemoveKind::as_wire extension ----------------------------

/// Trait extension carrying the wire-string projection for
/// [`RemoveKind`].  Lives alongside the removedir reply builders
/// because they're the only consumers today — the manifest's
/// per-entry kind strings (`"file"` / `"dir"`) are a
/// DD-RemoveDirReplyShape convention, not a general [`RemoveKind`]
/// API.
///
/// Implemented as an extension trait rather than an inherent impl
/// so the sibling `handlers::helpers::unlink` module's definition
/// of [`RemoveKind`] stays unchanged.  Extension-trait pattern
/// keeps the family-local projection out of the public struct's
/// API surface.
pub(super) trait RemoveKindAsWire {
    fn as_wire(&self) -> &'static str;
}

impl RemoveKindAsWire for RemoveKind {
    fn as_wire(&self) -> &'static str {
        match self {
            RemoveKind::File => "file",
            RemoveKind::Dir => "dir",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remove_kind_as_wire_values_pin() {
        // LOAD-BEARING: the wire strings "file" / "dir" are a
        // DD-RemoveDirReplyShape convention; a future rename
        // would break Dir.rho's manifest-field unpack.
        assert_eq!(RemoveKind::File.as_wire(), "file");
        assert_eq!(RemoveKind::Dir.as_wire(), "dir");
    }

    #[test]
    fn ok_with_count_shape() {
        let par = ok_with_count(7);
        let items = extract_list(&par);
        assert_eq!(items.len(), 2);
        assert_eq!(extract_bool(&items[0]), Some(true));
        assert_eq!(extract_int(&items[1]), Some(7));
    }

    #[test]
    fn err_with_count_shape() {
        let par = err_with_count(
            crate::rust::interpreter::io::errors::FSERR_IO,
            "disk read failed",
            3,
        );
        let items = extract_list(&par);
        assert_eq!(items.len(), 4);
        assert_eq!(extract_bool(&items[0]), Some(false));
        assert_eq!(extract_string(&items[1]), Some("FSERR_IO".to_string()));
        assert_eq!(
            extract_string(&items[2]),
            Some("disk read failed".to_string())
        );
        assert_eq!(extract_int(&items[3]), Some(3));
    }

    #[test]
    fn ok_recursive_manifest_shape() {
        let deleted = vec![
            (PathBuf::from("/a/b/leaf.bin"), RemoveKind::File),
            (PathBuf::from("/a/b"), RemoveKind::Dir),
        ];
        let par = ok_recursive_manifest(&deleted);
        let items = extract_list(&par);
        assert_eq!(items.len(), 3);
        assert_eq!(extract_bool(&items[0]), Some(true));
        assert_eq!(extract_int(&items[1]), Some(2));
        let manifest = extract_list(&items[2]);
        assert_eq!(manifest.len(), 2);
        let first = extract_list(&manifest[0]);
        assert_eq!(extract_string(&first[0]), Some("/a/b/leaf.bin".to_string()));
        assert_eq!(extract_string(&first[1]), Some("file".to_string()));
        let second = extract_list(&manifest[1]);
        assert_eq!(extract_string(&second[0]), Some("/a/b".to_string()));
        assert_eq!(extract_string(&second[1]), Some("dir".to_string()));
    }

    #[test]
    fn err_with_manifest_shape() {
        let deleted = vec![(PathBuf::from("/a/leaf.bin"), RemoveKind::File)];
        let par = err_with_manifest(
            crate::rust::interpreter::io::errors::FSERR_IO,
            "mid-walk failure",
            &deleted,
        );
        let items = extract_list(&par);
        assert_eq!(items.len(), 5);
        assert_eq!(extract_bool(&items[0]), Some(false));
        assert_eq!(extract_string(&items[1]), Some("FSERR_IO".to_string()));
        assert_eq!(
            extract_string(&items[2]),
            Some("mid-walk failure".to_string())
        );
        assert_eq!(extract_int(&items[3]), Some(1));
        let manifest = extract_list(&items[4]);
        assert_eq!(manifest.len(), 1);
    }

    #[test]
    fn early_err_picker_oracular_recursive_uses_4_element_shape() {
        let par = early_err_for_remove_dir(
            true,
            ConsensusMode::Oracular,
            crate::rust::interpreter::io::errors::FSERR_IO,
            "early fail",
        );
        assert_eq!(extract_list(&par).len(), 4);
    }

    #[test]
    fn early_err_picker_non_recursive_uses_4_element_shape() {
        let par = early_err_for_remove_dir(
            false,
            ConsensusMode::Consensus,
            crate::rust::interpreter::io::errors::FSERR_IO,
            "early fail",
        );
        assert_eq!(extract_list(&par).len(), 4);
    }

    #[test]
    fn early_err_picker_consensus_recursive_uses_5_element_shape() {
        // LOAD-BEARING: Consensus recursive pre-walk failures must
        // emit the 5-element shape even with an empty manifest, so
        // Dir.rho's shape unwrap doesn't fire a type error.
        let par = early_err_for_remove_dir(
            true,
            ConsensusMode::Consensus,
            crate::rust::interpreter::io::errors::FSERR_IO,
            "early fail",
        );
        let items = extract_list(&par);
        assert_eq!(items.len(), 5);
        // Position 3 is nDeletedBeforeError (0 at pre-walk).
        assert_eq!(extract_int(&items[3]), Some(0));
        // Position 4 is the (empty) manifest.
        assert!(extract_list(&items[4]).is_empty());
    }

    // --- test-local extractors (shape-driven assertions) ------

    fn extract_list(par: &Par) -> Vec<Par> {
        for expr in &par.exprs {
            if let Some(ExprInstance::EListBody(el)) = &expr.expr_instance {
                return el.ps.clone();
            }
        }
        panic!("Par is not an EList: {par:?}");
    }

    fn extract_bool(par: &Par) -> Option<bool> {
        for expr in &par.exprs {
            if let Some(ExprInstance::GBool(b)) = &expr.expr_instance {
                return Some(*b);
            }
        }
        None
    }

    fn extract_int(par: &Par) -> Option<i64> {
        for expr in &par.exprs {
            if let Some(ExprInstance::GInt(n)) = &expr.expr_instance {
                return Some(*n);
            }
        }
        None
    }

    fn extract_string(par: &Par) -> Option<String> {
        for expr in &par.exprs {
            if let Some(ExprInstance::GString(s)) = &expr.expr_instance {
                return Some(s.clone());
            }
        }
        None
    }
}
