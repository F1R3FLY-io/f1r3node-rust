//! End-to-end smoke test for the `ScopedTmpdir` SIGTERM cleanup path.
//!
//! Forks: the child creates a `ScopedTmpdir`, writes its path to a shared
//! file, and sleeps. The parent reads the path, sends `SIGTERM`, waits for
//! the child to exit, and asserts the directory is gone.

use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use shared::rust::test_tmpdir::ScopedTmpdir;

#[test]
fn sigterm_in_child_removes_the_scoped_tmpdir() {
    // A shared file in the parent's tmpdir lets the child publish its
    // tempdir path. Parent polls until it appears.
    let outside = std::env::temp_dir();
    let handoff = outside.join(format!("f1r3node-scoped-sigterm-test-{}.txt", std::process::id()));
    let _ = fs::remove_file(&handoff);

    let child = unsafe { libc::fork() };
    assert!(child >= 0, "fork failed");

    if child == 0 {
        // Child
        let scope = ScopedTmpdir::new();
        fs::write(&handoff, scope.path().to_string_lossy().as_bytes()).expect("write handoff");
        // Give the parent a generous window to SIGTERM us. If it doesn't,
        // the test's own timeout will reap us.
        thread::sleep(Duration::from_secs(30));
        // If we get here, the parent failed to signal. Keep the TempDir
        // alive so the cleanup-on-drop path still runs, then exit.
        drop(scope);
        std::process::exit(0);
    }

    // Parent: wait for the child to publish its path.
    let child_path = {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Ok(s) = fs::read_to_string(&handoff) {
                if !s.trim().is_empty() {
                    break PathBuf::from(s.trim());
                }
            }
            if Instant::now() >= deadline {
                // Clean up child before failing.
                unsafe { libc::kill(child, libc::SIGKILL) };
                let _ = fs::remove_file(&handoff);
                panic!("child did not publish its tmpdir path in time");
            }
            thread::sleep(Duration::from_millis(50));
        }
    };

    assert!(child_path.exists(), "child's tmpdir exists before SIGTERM");

    // Send SIGTERM and reap the child.
    assert_eq!(
        unsafe { libc::kill(child, libc::SIGTERM) },
        0,
        "SIGTERM dispatch failed"
    );

    // waitpid with a bounded loop so a stuck child cannot hang the test.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut status: libc::c_int = 0;
    loop {
        let r = unsafe { libc::waitpid(child, &mut status, libc::WNOHANG) };
        if r == child {
            break;
        }
        if Instant::now() >= deadline {
            unsafe { libc::kill(child, libc::SIGKILL) };
            unsafe { libc::waitpid(child, &mut status, 0) };
            let _ = fs::remove_file(&handoff);
            panic!("child did not exit within 10s of SIGTERM");
        }
        thread::sleep(Duration::from_millis(50));
    }

    let _ = fs::remove_file(&handoff);

    assert!(
        !child_path.exists(),
        "child's tmpdir should be removed by the SIGTERM cleanup; still at {}",
        child_path.display()
    );
}
