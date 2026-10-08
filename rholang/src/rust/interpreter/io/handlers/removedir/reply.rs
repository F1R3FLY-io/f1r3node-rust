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

// --- Reply-reading helpers (inverse of the builders above) ----

/// Read `nDeleted` from a removeDir reply Par.  Reads position 1
/// (success) or position 3 (failure) — both indices carry the
/// count uniformly across every removeDir code path (non-recursive,
/// recursive Oracular, recursive Consensus) post DD-RemoveDirReplyShape.
/// Returns `0` for any malformed / non-list reply so cost
/// accounting fails safe.
///
/// Supersedes the pre-shape-change branch-on-(recursive, cmode)
/// helpers that had to derive the count from the manifest
/// (Consensus recursive only) or hard-code it (non-recursive = 1,
/// Oracular recursive = 0).  Post-shape-change the reply itself
/// is the canonical count source on every code path — including
/// Oracular recursive, which now bills per-entry symmetrically
/// with Consensus recursive.
pub(super) fn extract_removedir_n_deleted(reply: &Par) -> u64 {
    let expr = match reply.exprs.first() {
        Some(e) => e,
        None => return 0,
    };
    let outer = match &expr.expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => return 0,
    };
    let ok_par = match outer.ps.first() {
        Some(p) => p,
        None => return 0,
    };
    let n_par = match RhoBoolean::unapply(ok_par) {
        Some(true) => match outer.ps.get(1) {
            Some(p) => p,
            None => return 0,
        },
        Some(false) => match outer.ps.get(3) {
            Some(p) => p,
            None => return 0,
        },
        None => return 0,
    };
    match RhoNumber::unapply(n_par) {
        Some(n) if n >= 0 => n as u64,
        _ => 0,
    }
}

/// Leader-path cost-supplement count — read from the fresh reply
/// Par.  Thin wrapper so the leader + follower cost-accounting
/// sites can share the same count-extraction logic without
/// slice-of-Par vs. Par-ref call-site mix-ups.
///
/// `_parsed` + `_cmode` are retained in the signature for
/// forward-compat with pre-shape-change branch logic that may
/// re-emerge if a future revision needs mode-aware costing.
pub(super) fn fs_remove_dir_supplement_count(
    _parsed: &Option<(String, String, bool)>,
    _cmode: ConsensusMode,
    reply: &Par,
) -> u64 {
    extract_removedir_n_deleted(reply)
}

/// Follower-path counterpart — read from `previous[0]`.  Same
/// helper; the split is preserved so callers can't accidentally
/// mix slice-of-Par with Par-ref call sites.
pub(super) fn fs_remove_dir_supplement_count_from_previous(
    _parsed: &Option<(String, String, bool)>,
    _cmode: ConsensusMode,
    previous: &[Par],
) -> u64 {
    match previous.first() {
        Some(reply) => extract_removedir_n_deleted(reply),
        None => 0,
    }
}

/// Parse the recursive-removeDir reply manifest into
/// `(PathBuf, RemoveKind)` tuples.
///
/// Reply shapes (Consensus recursive only; other paths have no
/// manifest and this returns an empty Vec):
///   * `[true, nDeleted, [[path, kind], ...]]` — success.
///   * `[false, code, msg, nDeletedBeforeError, [[path, kind], ...]]`
///     — partial success followed by failure; the inner list
///     contains only what was successfully deleted before the
///     failing entry.
///
/// Returns an empty Vec for any other shape (any 4-element
/// non-Consensus-recursive reply, or malformed input).
///
/// Kept available because the Consensus recursive reply ships
/// the manifest as an implementation-side channel; diagnostics
/// and future consumers can extract it without re-walking.  The
/// R5(b) follower reads its own manifest via
/// `collect_recursive_manifest` rather than consuming the
/// leader's, so this parser has no production caller today.
pub(super) fn extract_removedir_manifest(previous: &[Par]) -> Vec<(PathBuf, RemoveKind)> {
    let head = match previous.first() {
        Some(h) => h,
        None => return Vec::new(),
    };
    let expr = match head.exprs.first() {
        Some(e) => e,
        None => return Vec::new(),
    };
    let outer = match &expr.expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => return Vec::new(),
    };
    // Manifest lives at index 2 (success) or 4 (failure) post
    // DD-RemoveDirReplyShape.  Detect via the head bool.
    let ok_par = match outer.ps.first() {
        Some(p) => p,
        None => return Vec::new(),
    };
    let manifest_par = match RhoBoolean::unapply(ok_par) {
        Some(true) => match outer.ps.get(2) {
            Some(p) => p,
            None => return Vec::new(),
        },
        Some(false) => match outer.ps.get(4) {
            Some(p) => p,
            None => return Vec::new(),
        },
        None => return Vec::new(),
    };
    let manifest_expr = match manifest_par.exprs.first() {
        Some(e) => e,
        None => return Vec::new(),
    };
    let manifest_list = match &manifest_expr.expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => return Vec::new(),
    };
    let mut out = Vec::with_capacity(manifest_list.ps.len());
    for entry_par in &manifest_list.ps {
        let entry_expr = match entry_par.exprs.first() {
            Some(e) => e,
            None => continue,
        };
        let entry_list = match &entry_expr.expr_instance {
            Some(ExprInstance::EListBody(l)) => l,
            _ => continue,
        };
        let path_par = match entry_list.ps.first() {
            Some(p) => p,
            None => continue,
        };
        let kind_par = match entry_list.ps.get(1) {
            Some(p) => p,
            None => continue,
        };
        let path = match RhoString::unapply(path_par) {
            Some(s) => PathBuf::from(s),
            None => continue,
        };
        let kind = match RhoString::unapply(kind_par).as_deref() {
            Some("file") => RemoveKind::File,
            Some("dir") => RemoveKind::Dir,
            _ => continue,
        };
        out.push((path, kind));
    }
    out
}

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

    // --- Reply-reading helpers ---------------------------------

    #[test]
    fn extract_n_deleted_from_ok_with_count() {
        let par = ok_with_count(7);
        assert_eq!(extract_removedir_n_deleted(&par), 7);
    }

    #[test]
    fn extract_n_deleted_from_err_with_count() {
        let par = err_with_count(crate::rust::interpreter::io::errors::FSERR_IO, "err", 3);
        assert_eq!(extract_removedir_n_deleted(&par), 3);
    }

    #[test]
    fn extract_n_deleted_from_ok_recursive_manifest() {
        // LOAD-BEARING: Consensus recursive success carries the
        // count at position 1, same as non-recursive success.
        // Cost accounting MUST see the same number regardless of
        // code path.
        let deleted = vec![
            (PathBuf::from("a.bin"), RemoveKind::File),
            (PathBuf::from(""), RemoveKind::Dir),
        ];
        let par = ok_recursive_manifest(&deleted);
        assert_eq!(extract_removedir_n_deleted(&par), 2);
    }

    #[test]
    fn extract_n_deleted_from_err_with_manifest() {
        // LOAD-BEARING: Consensus recursive failure carries
        // nDeletedBeforeError at position 3, same as non-recursive
        // failure.
        let deleted = vec![(PathBuf::from("one.bin"), RemoveKind::File)];
        let par = err_with_manifest(
            crate::rust::interpreter::io::errors::FSERR_IO,
            "mid-walk failure",
            &deleted,
        );
        assert_eq!(extract_removedir_n_deleted(&par), 1);
    }

    #[test]
    fn extract_n_deleted_malformed_returns_zero() {
        // Cost accounting fails-safe: unparseable reply → 0 (don't
        // over-charge, don't under-charge based on garbage).
        let empty = Par::default();
        assert_eq!(extract_removedir_n_deleted(&empty), 0);
    }

    #[test]
    fn fs_remove_dir_supplement_count_reads_leader_reply() {
        let par = ok_with_count(5);
        assert_eq!(
            fs_remove_dir_supplement_count(&None, ConsensusMode::Consensus, &par),
            5
        );
    }

    #[test]
    fn fs_remove_dir_supplement_count_from_previous_reads_follower_slot() {
        let par = ok_with_count(5);
        let previous = vec![par];
        assert_eq!(
            fs_remove_dir_supplement_count_from_previous(
                &None,
                ConsensusMode::Consensus,
                &previous
            ),
            5
        );
    }

    #[test]
    fn fs_remove_dir_supplement_count_from_previous_empty_returns_zero() {
        let previous: Vec<Par> = Vec::new();
        assert_eq!(
            fs_remove_dir_supplement_count_from_previous(
                &None,
                ConsensusMode::Consensus,
                &previous
            ),
            0
        );
    }

    #[test]
    fn extract_manifest_from_ok_recursive() {
        let deleted = vec![
            (PathBuf::from("sub/leaf.bin"), RemoveKind::File),
            (PathBuf::from("sub"), RemoveKind::Dir),
        ];
        let par = ok_recursive_manifest(&deleted);
        let previous = vec![par];
        let parsed = extract_removedir_manifest(&previous);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0], (PathBuf::from("sub/leaf.bin"), RemoveKind::File));
        assert_eq!(parsed[1], (PathBuf::from("sub"), RemoveKind::Dir));
    }

    #[test]
    fn extract_manifest_from_err_with_manifest() {
        let deleted = vec![(PathBuf::from("done.bin"), RemoveKind::File)];
        let par = err_with_manifest(
            crate::rust::interpreter::io::errors::FSERR_IO,
            "err",
            &deleted,
        );
        let previous = vec![par];
        let parsed = extract_removedir_manifest(&previous);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0], (PathBuf::from("done.bin"), RemoveKind::File));
    }

    #[test]
    fn extract_manifest_from_non_recursive_shape_returns_empty() {
        // Non-recursive success is `[true, 1]` with no manifest.
        let par = ok_with_count(1);
        let previous = vec![par];
        let parsed = extract_removedir_manifest(&previous);
        assert!(parsed.is_empty());
    }

    #[test]
    fn extract_manifest_from_empty_previous_returns_empty() {
        let previous: Vec<Par> = Vec::new();
        let parsed = extract_removedir_manifest(&previous);
        assert!(parsed.is_empty());
    }

    #[test]
    fn extract_manifest_filters_malformed_entries() {
        // An entry whose kind string isn't "file" / "dir" is
        // dropped from the parse rather than erroring the whole
        // reply.  Defensive against future-schema additions a
        // joiner might not understand.
        let good = ok_recursive_manifest(&[(PathBuf::from("good.bin"), RemoveKind::File)]);
        // Hand-build a malformed inner entry [path, kind-with-unknown-string].
        let mut malformed = good.clone();
        if let Some(ExprInstance::EListBody(ref mut outer)) =
            malformed.exprs[0].expr_instance.as_mut()
        {
            if let Some(ExprInstance::EListBody(ref mut manifest)) =
                outer.ps[2].exprs[0].expr_instance.as_mut()
            {
                let entry_par = list_par_2(
                    RhoString::create_par("extra.bin".to_string()),
                    RhoString::create_par("fifo".to_string()),
                );
                manifest.ps.push(entry_par);
            }
        }
        let parsed = extract_removedir_manifest(&[malformed]);
        // Only the "good" entry parses; the "fifo"-kind one is
        // filtered.
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0], (PathBuf::from("good.bin"), RemoveKind::File));
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
