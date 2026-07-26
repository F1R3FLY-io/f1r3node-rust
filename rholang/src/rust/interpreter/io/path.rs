// Path quarantine.
//
// Every path-taking native takes `(rootCanon, rel)` and canonicalizes
// `root.join(rel)` under the following rules:
//
//   - The resolved path must be a descendant of `rootCanon`.
//   - No component of the relative path may traverse a symlink.
//   - The relative path must be non-empty and not resolve to `rootCanon`
//     itself (`.`-only after collapse).
//
// Rejection returns `QuarantineError` which the handler layer translates
// to `FSERR_QUARANTINE`.

use std::path::{Component, Path, PathBuf};

#[derive(Debug, PartialEq, Eq)]
pub enum QuarantineError {
    Empty,
    RootSelf,
    EscapesRoot,
    SymlinkComponent,
    IoError(String),
}

/// Canonicalize `root.join(rel)` and verify the result is a strict
/// descendant of `root`.
///
/// `root` must already be canonicalized (that is the static-provisioning
/// invariant — the config-loader canonicalizes each root at boot).
pub fn canonicalize_and_quarantine(root: &Path, rel: &str) -> Result<PathBuf, QuarantineError> {
    if rel.is_empty() {
        return Err(QuarantineError::Empty);
    }

    let rel_path = Path::new(rel);

    // Reject absolute rels immediately — they would trivially escape.
    if rel_path.is_absolute() {
        return Err(QuarantineError::EscapesRoot);
    }

    // Component-by-component walk: reject `..` that would climb past
    // root, and check each intermediate path for symlink status before
    // descending further.
    let mut acc = root.to_path_buf();
    let mut nonempty = false;
    for comp in rel_path.components() {
        match comp {
            Component::CurDir => continue,
            Component::ParentDir => return Err(QuarantineError::EscapesRoot),
            Component::Normal(name) => {
                nonempty = true;
                acc.push(name);
                // Only inspect existing intermediates — a create-new open
                // legitimately targets a non-existent leaf.
                if let Ok(meta) = acc.symlink_metadata() {
                    if meta.file_type().is_symlink() {
                        return Err(QuarantineError::SymlinkComponent);
                    }
                }
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(QuarantineError::EscapesRoot);
            }
        }
    }

    if !nonempty {
        return Err(QuarantineError::RootSelf);
    }

    // Final defensive check: resolve the parent (which must exist) and
    // confirm descendancy.  We use the parent because the leaf may be
    // create-new.
    if let Some(parent) = acc.parent() {
        match parent.canonicalize() {
            Ok(canon_parent) => {
                if !canon_parent.starts_with(root) {
                    return Err(QuarantineError::EscapesRoot);
                }
            }
            Err(e) => {
                // Missing intermediate is legitimate (create-new); other
                // errors surface as I/O.
                if e.kind() != std::io::ErrorKind::NotFound {
                    return Err(QuarantineError::IoError(e.to_string()));
                }
            }
        }
    }

    Ok(acc)
}

/// Translate a `QuarantineError` to the (code, message) pair for the
/// `[false, code, msg]` reply shape.
pub fn quarantine_err_reply(e: &QuarantineError) -> (&'static str, String) {
    use super::errors::{FSERR_BAD_ARG, FSERR_IO, FSERR_QUARANTINE};
    match e {
        QuarantineError::Empty => (FSERR_BAD_ARG, "empty relative path".into()),
        QuarantineError::RootSelf => (FSERR_BAD_ARG, "path resolves to root itself".into()),
        QuarantineError::EscapesRoot => (FSERR_QUARANTINE, "path escapes root".into()),
        QuarantineError::SymlinkComponent => {
            (FSERR_QUARANTINE, "symlink in path components".into())
        }
        QuarantineError::IoError(m) => (FSERR_IO, m.clone()),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn rejects_empty() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        assert_eq!(
            canonicalize_and_quarantine(&root, ""),
            Err(QuarantineError::Empty)
        );
    }

    #[test]
    fn rejects_parent_traversal() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        assert_eq!(
            canonicalize_and_quarantine(&root, "../escape.txt"),
            Err(QuarantineError::EscapesRoot)
        );
    }

    #[test]
    fn rejects_absolute() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        assert_eq!(
            canonicalize_and_quarantine(&root, "/etc/passwd"),
            Err(QuarantineError::EscapesRoot)
        );
    }

    #[test]
    fn allows_simple_descendant() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let file = root.join("sub/nested.txt");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, b"hi").unwrap();
        let resolved = canonicalize_and_quarantine(&root, "sub/nested.txt").unwrap();
        assert_eq!(resolved, file);
    }

    #[test]
    fn rejects_root_self_after_collapse() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        assert_eq!(
            canonicalize_and_quarantine(&root, "."),
            Err(QuarantineError::RootSelf)
        );
    }
}
