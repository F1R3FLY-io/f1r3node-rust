//! Per-test `$TMPDIR` scoping — a `mktemp -d` equivalent that cleans up on
//! drop and also redirects the process's `TMPDIR` env var at the fresh dir
//! for the guard's lifetime, so any code the test exercises that calls
//! `std::env::temp_dir()` lands under this test's private tree.
//!
//! Safe under `cargo nextest`, which runs each test in its own subprocess
//! (one `TMPDIR` reader for the guard's lifetime). Under `cargo test`'s
//! thread-per-test model the `TMPDIR` mutation races with concurrent tests
//! in the same binary — reach for plain `tempfile::tempdir()` there.

use std::ffi::OsString;
use std::path::Path;

use tempfile::TempDir;

pub struct ScopedTmpdir {
    dir: Option<TempDir>,
    previous: Option<OsString>,
}

impl ScopedTmpdir {
    pub fn new() -> Self {
        let dir = TempDir::new().expect("create scoped tmpdir");
        let previous = std::env::var_os("TMPDIR");
        // SAFETY: Under nextest's subprocess-per-test model, this test is the
        // sole reader of `TMPDIR` for the guard's lifetime. The guard restores
        // the prior value on drop before any later test observes it.
        unsafe {
            std::env::set_var("TMPDIR", dir.path());
        }
        Self {
            dir: Some(dir),
            previous,
        }
    }

    pub fn path(&self) -> &Path {
        self.dir
            .as_ref()
            .expect("ScopedTmpdir dir is populated until drop")
            .path()
    }
}

impl Default for ScopedTmpdir {
    fn default() -> Self { Self::new() }
}

impl Drop for ScopedTmpdir {
    fn drop(&mut self) {
        unsafe {
            match self.previous.take() {
                Some(v) => std::env::set_var("TMPDIR", v),
                None => std::env::remove_var("TMPDIR"),
            }
        }
        // TempDir drops here, removing the directory and everything under it.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scoped_tmpdir_redirects_env_temp_dir_and_cleans_up_on_drop() {
        let outside = std::env::temp_dir();
        let path_copy;
        {
            let scope = ScopedTmpdir::new();
            path_copy = scope.path().to_path_buf();
            assert_eq!(std::env::temp_dir(), path_copy);
            assert!(path_copy.exists());
            assert_ne!(path_copy, outside);
        }
        assert!(!path_copy.exists(), "tmpdir should be removed on drop");
        assert_eq!(std::env::temp_dir(), outside);
    }
}
