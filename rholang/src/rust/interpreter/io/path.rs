//! Path canonicalization + quarantine helper.
//!
//! Every user-facing file/dir operation goes through a Rholang
//! `Dir` agent, and every `Dir` agent has a canonicalized absolute
//! `root` path it was given by the powerbox. Before dispatching a
//! request that names a path (or a relative name), the agent has
//! to check that the resolved path is *inside* its root. That
//! check is:
//!
//! 1. Join `root` with the caller-supplied `rel`.
//! 2. Fully canonicalize the joined path -- meaning follow every
//!    symlink so an attacker who plants `evil -> /etc/passwd`
//!    can't escape by naming `evil`.
//! 3. Compare the canonical form's prefix against `root`. If it
//!    doesn't start with `root`, the request escapes.
//!
//! The wrinkle is step 2: `std::fs::canonicalize` requires the
//! whole path to exist. That's fine for stat/read/remove, but
//! `open("w")` and `rename(to)` name a *destination* that doesn't
//! exist yet. For that case we walk up until we find an ancestor
//! that does exist, canonicalize that, and then re-join the
//! non-existent tail. The tail's `..` components are resolved
//! lexically at that point; if any of them tries to climb above
//! the canonicalized ancestor, we reject.
//!
//! This is *not* a substitute for opening dir fds and using
//! `openat`/`renameat` (which would close the TOCTOU window
//! between quarantine check and syscall). It's the FIP's chosen
//! trade-off: portable, cheap, and safe against the offline
//! symlink-planted-in-tree threat. TOCTOU races against an
//! attacker with concurrent write access to the tree are out of
//! scope for the FIP; the powerbox is expected to hand out
//! agents only for trees the deployer controls.
//!
//! Rejection produces `FSERR_QUARANTINE`; a missing path
//! upstream of the tail (the root itself doesn't exist)
//! produces `FSERR_NOT_FOUND`; other host errors produce
//! `FSERR_IO`.

use std::path::{Component, Path, PathBuf};

use super::response::{FSERR_BAD_ARG, FSERR_IO, FSERR_NOT_FOUND, FSERR_QUARANTINE};

/// Outcome of `canonicalize_and_quarantine`.
///
/// The three error variants map 1:1 to the FIP error codes so the
/// caller can just forward `code()` and `message()` into
/// `response::err`.
#[derive(Debug)]
pub enum QuarantineError {
    /// The relative path escaped the root.
    Escapes(String),
    /// A path component (or the root) did not exist.
    NotFound(String),
    /// Bad relative-path input (e.g., contained a null byte).
    BadArg(String),
    /// Any other host error during canonicalization.
    Io(std::io::Error),
}

impl QuarantineError {
    pub fn code(&self) -> &'static str {
        match self {
            QuarantineError::Escapes(_) => FSERR_QUARANTINE,
            QuarantineError::NotFound(_) => FSERR_NOT_FOUND,
            QuarantineError::BadArg(_) => FSERR_BAD_ARG,
            QuarantineError::Io(_) => FSERR_IO,
        }
    }

    pub fn message(&self) -> String {
        match self {
            QuarantineError::Escapes(m) => m.clone(),
            QuarantineError::NotFound(m) => m.clone(),
            QuarantineError::BadArg(m) => m.clone(),
            QuarantineError::Io(e) => e.to_string(),
        }
    }
}

/// Canonicalize `root.join(rel)` and confirm the result is
/// underneath `root`.
///
/// `root` must already be a canonicalized absolute path (the
/// caller -- the powerbox at boot -- is responsible for that).
/// `rel` is the caller-supplied relative path; it may name an
/// existing entry or a fresh child to be created. Leading `/`
/// on `rel` is treated as relative-to-root rather than absolute,
/// matching the FIP §"With the config file" powerbox conventions.
pub fn canonicalize_and_quarantine(root: &Path, rel: &str) -> Result<PathBuf, QuarantineError> {
    if rel.contains('\0') {
        return Err(QuarantineError::BadArg(
            "rel path contains a null byte".to_string(),
        ));
    }

    // Drop any leading `/` so the caller can't smuggle absolute
    // paths past the join. We also strip a leading `./` for
    // hygiene.
    let stripped: &str = rel.trim_start_matches('/').trim_start_matches("./");

    // Empty or `.`-only tails would resolve to `root` itself,
    // which lets `removeDir("", true)` (or `removeDir(".", true)`,
    // `rename("", ...)`, etc.) target the sandbox root -- a
    // caller who legitimately holds only a `Dir` handle could
    // wipe the entire sandbox. Reject at the quarantine layer
    // so every path-taking native inherits the check (rather
    // than requiring each agent method to remember). A caller
    // that actually wants to operate on the root uses a
    // root-scoped method (`entries()`, future `stat()`-on-self)
    // instead.
    if stripped.is_empty()
        || Path::new(stripped)
            .components()
            .all(|c| matches!(c, Component::CurDir))
    {
        return Err(QuarantineError::BadArg(
            "rel path resolves to the root itself; use root-scoped methods instead".to_string(),
        ));
    }

    let joined = root.join(stripped);

    // Fast path: fully-existing target.
    if let Ok(canonical) = std::fs::canonicalize(&joined) {
        return check_prefix(root, &canonical);
    }

    // Slow path: some suffix of the target doesn't exist yet.
    // Walk up until we find an existing ancestor, canonicalize
    // it, then resolve the remaining tail lexically.
    let (existing, tail) = split_at_existing(&joined)?;
    let canonical_existing = std::fs::canonicalize(&existing).map_err(QuarantineError::Io)?;
    let resolved = resolve_lexical(&canonical_existing, &tail)?;
    check_prefix(root, &resolved)
}

fn check_prefix(root: &Path, candidate: &Path) -> Result<PathBuf, QuarantineError> {
    if candidate.starts_with(root) {
        Ok(candidate.to_path_buf())
    } else {
        Err(QuarantineError::Escapes(format!(
            "path {candidate:?} escapes root {root:?}"
        )))
    }
}

/// Split `p` into `(deepest_existing_ancestor, non_existent_tail)`.
/// The tail is empty when `p` itself exists (unreachable given the
/// caller already tried that, but harmless as a fallback).
fn split_at_existing(p: &Path) -> Result<(PathBuf, PathBuf), QuarantineError> {
    let mut base: PathBuf = p.to_path_buf();
    let mut tail: PathBuf = PathBuf::new();
    loop {
        if base.exists() {
            return Ok((base, tail));
        }
        let Some(name) = base.file_name().map(|s| s.to_owned()) else {
            // We climbed all the way to `/` (or empty) without
            // finding an existing ancestor -- the root itself is
            // missing.
            return Err(QuarantineError::NotFound(format!(
                "no existing ancestor for {p:?}"
            )));
        };
        // Prepend `name` to `tail`.
        let mut new_tail = PathBuf::from(name);
        new_tail.push(&tail);
        tail = new_tail;
        if !base.pop() {
            return Err(QuarantineError::NotFound(format!(
                "no existing ancestor for {p:?}"
            )));
        }
    }
}

/// Resolve a `tail` of not-yet-existing components against an
/// already-canonicalized `base`, honoring `..` lexically. Rejects
/// any `..` that would climb above `base`.
fn resolve_lexical(base: &Path, tail: &Path) -> Result<PathBuf, QuarantineError> {
    let mut cur = base.to_path_buf();
    for comp in tail.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                if cur.as_path() == base {
                    return Err(QuarantineError::Escapes(format!(
                        "'..' from canonical base {base:?} would escape"
                    )));
                }
                if !cur.pop() {
                    return Err(QuarantineError::Escapes(format!(
                        "'..' from {cur:?} could not pop"
                    )));
                }
            }
            Component::Normal(name) => cur.push(name),
            Component::Prefix(_) | Component::RootDir => {
                // Shouldn't appear in a stripped relative tail; be
                // defensive.
                return Err(QuarantineError::BadArg(format!(
                    "unexpected absolute component in relative tail: {comp:?}"
                )));
            }
        }
    }
    Ok(cur)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create `root/{a, sub/b}` and return canonical `root` plus
    /// its `PathBuf`.
    fn scratch_tree() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        std::fs::write(root.join("a"), b"").unwrap();
        std::fs::create_dir(root.join("sub")).unwrap();
        std::fs::write(root.join("sub/b"), b"").unwrap();
        (dir, root)
    }

    #[test]
    fn simple_relative_path_succeeds() {
        let (_g, root) = scratch_tree();
        let out = canonicalize_and_quarantine(&root, "a").unwrap();
        assert_eq!(out, root.join("a"));
    }

    #[test]
    fn nested_relative_path_succeeds() {
        let (_g, root) = scratch_tree();
        let out = canonicalize_and_quarantine(&root, "sub/b").unwrap();
        assert_eq!(out, root.join("sub/b"));
    }

    #[test]
    fn leading_slash_is_relative_to_root_not_absolute() {
        let (_g, root) = scratch_tree();
        let out = canonicalize_and_quarantine(&root, "/a").unwrap();
        assert_eq!(out, root.join("a"));
    }

    #[test]
    fn parent_dir_escape_is_rejected() {
        let (_g, root) = scratch_tree();
        let err = canonicalize_and_quarantine(&root, "../etc/passwd").unwrap_err();
        assert_eq!(err.code(), FSERR_QUARANTINE);
    }

    #[test]
    fn parent_dir_that_stays_inside_root_is_ok() {
        let (_g, root) = scratch_tree();
        let out = canonicalize_and_quarantine(&root, "sub/../a").unwrap();
        assert_eq!(out, root.join("a"));
    }

    #[test]
    fn creating_a_new_file_inside_root_works() {
        let (_g, root) = scratch_tree();
        let out = canonicalize_and_quarantine(&root, "sub/new.txt").unwrap();
        assert_eq!(out, root.join("sub/new.txt"));
    }

    #[test]
    fn creating_a_new_file_that_climbs_out_is_rejected() {
        let (_g, root) = scratch_tree();
        let err = canonicalize_and_quarantine(&root, "sub/../../out.txt").unwrap_err();
        assert_eq!(err.code(), FSERR_QUARANTINE);
    }

    #[test]
    #[cfg(unix)]
    fn symlink_that_points_outside_root_is_rejected() {
        use std::os::unix::fs::symlink;
        let (_g, root) = scratch_tree();
        let escape_target = tempfile::tempdir().unwrap();
        symlink(escape_target.path(), root.join("evil")).unwrap();
        let err = canonicalize_and_quarantine(&root, "evil").unwrap_err();
        assert_eq!(err.code(), FSERR_QUARANTINE);
    }

    #[test]
    fn null_byte_in_rel_is_bad_arg() {
        let (_g, root) = scratch_tree();
        let err = canonicalize_and_quarantine(&root, "a\0b").unwrap_err();
        assert_eq!(err.code(), FSERR_BAD_ARG);
    }

    /// Empty relpath must not resolve to the root itself, or
    /// `removeDir("", true)` wipes the sandbox.
    #[test]
    fn empty_rel_is_bad_arg() {
        let (_g, root) = scratch_tree();
        let err = canonicalize_and_quarantine(&root, "").unwrap_err();
        assert_eq!(err.code(), FSERR_BAD_ARG);
    }

    /// `"."`, `"./"`, and any all-`CurDir`-components tail must
    /// also be rejected -- they'd equally resolve to the root.
    #[test]
    fn dot_only_rel_is_bad_arg() {
        let (_g, root) = scratch_tree();
        for rel in [".", "./", "./.", "././.", "././"] {
            let err = canonicalize_and_quarantine(&root, rel).unwrap_err();
            assert_eq!(
                err.code(),
                FSERR_BAD_ARG,
                "expected FSERR_BAD_ARG for rel {rel:?}"
            );
        }
    }

    /// Leading-slash-only forms (`/`, `//`) are stripped to
    /// empty and must be rejected too.
    #[test]
    fn slash_only_rel_is_bad_arg() {
        let (_g, root) = scratch_tree();
        for rel in ["/", "//", "///"] {
            let err = canonicalize_and_quarantine(&root, rel).unwrap_err();
            assert_eq!(
                err.code(),
                FSERR_BAD_ARG,
                "expected FSERR_BAD_ARG for rel {rel:?}"
            );
        }
    }

    /// Leading slash on a real name still works (stripped, then
    /// treated as relative-to-root). Regression guard so the
    /// empty-tail fix doesn't over-reject legitimate paths.
    #[test]
    fn leading_slash_on_real_name_still_works() {
        let (_g, root) = scratch_tree();
        let out = canonicalize_and_quarantine(&root, "/a").unwrap();
        assert_eq!(out, root.join("a"));
    }

    /// `./name` (leading `./` stripped) still works.
    #[test]
    fn leading_dot_slash_on_real_name_still_works() {
        let (_g, root) = scratch_tree();
        let out = canonicalize_and_quarantine(&root, "./a").unwrap();
        assert_eq!(out, root.join("a"));
    }
}
