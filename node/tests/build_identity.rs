#[path = "../build_identity.rs"]
mod build_identity;

use std::io;
#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
#[cfg(windows)]
use std::os::windows::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::{ExitStatus, Output};
use std::sync::atomic::{AtomicU64, Ordering};

fn git_output(code: i32, stdout: &[u8]) -> io::Result<Output> {
    Ok(Output {
        status: ExitStatus::from_raw((code << 8) as _),
        stdout: stdout.to_vec(),
        stderr: Vec::new(),
    })
}

#[test]
fn explicit_identity_takes_precedence_without_running_git() {
    let identity = build_identity::resolve_build_id(Some("  v0.4.46-42-g4d8d9d79  "), || {
        panic!("Git must not run when the build identity is explicit")
    })
    .unwrap();
    assert_eq!(identity, "v0.4.46-42-g4d8d9d79");
}

#[test]
fn explicit_identity_trims_surrounding_whitespace_before_validation() {
    for explicit in ["v1\n", "\tv1\r\n", "\u{2003}v1\u{2003}"] {
        let identity = build_identity::resolve_build_id(Some(explicit), || {
            panic!("Git must not run when the trimmed identity is explicit")
        })
        .unwrap();
        assert_eq!(identity, "v1");
    }
}

#[test]
fn explicit_identity_preserves_internal_spaces() {
    let identity = build_identity::resolve_build_id(Some("  source build 42  "), || {
        panic!("Git must not run when the build identity is explicit")
    })
    .unwrap();
    assert_eq!(identity, "source build 42");
}

#[test]
fn missing_or_blank_identity_uses_git() {
    for explicit in [None, Some(""), Some("   ")] {
        let identity =
            build_identity::resolve_build_id(explicit, || git_output(0, b"v0.4.46-42-g4d8d9d79\n"))
                .unwrap();
        assert_eq!(identity, "v0.4.46-42-g4d8d9d79");
    }
}

#[test]
fn failed_git_cannot_supply_an_identity() {
    for stdout in [b"".as_slice(), b"misleading-identity\n".as_slice()] {
        let identity = build_identity::resolve_build_id(None, || git_output(128, stdout)).unwrap();
        assert_eq!(identity, "unknown");
    }
}

#[test]
fn missing_git_uses_unknown() {
    let identity = build_identity::resolve_build_id(None, || {
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "git is unavailable",
        ))
    })
    .unwrap();
    assert_eq!(identity, "unknown");
}

#[test]
fn empty_or_invalid_git_output_uses_unknown() {
    for stdout in [b"".as_slice(), b" \n".as_slice(), b"\xff".as_slice()] {
        let identity = build_identity::resolve_build_id(None, || git_output(0, stdout)).unwrap();
        assert_eq!(identity, "unknown");
    }
}

#[test]
fn explicit_control_characters_cannot_emit_cargo_directives() {
    for explicit in [
        "build\ncargo:rustc-env=INJECTED=value",
        "build\rvalue",
        "build\tvalue",
        "build\0value",
    ] {
        assert!(build_identity::resolve_build_id(Some(explicit), || {
            panic!("An invalid explicit identity must fail before Git runs")
        })
        .is_err());
    }
}

#[test]
fn git_control_characters_cannot_emit_cargo_directives() {
    let identity = build_identity::resolve_build_id(None, || {
        git_output(0, b"build\ncargo:rustc-env=INJECTED=value\n")
    })
    .unwrap();
    assert_eq!(identity, "unknown");
}

struct MetadataFixture {
    root: PathBuf,
}

impl MetadataFixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "f1r3node-metadata-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(root.join("node")).unwrap();
        for name in ["refs/heads", "refs/tags", "refs/remotes/origin"] {
            std::fs::create_dir_all(root.join(".git").join(name)).unwrap();
        }
        std::fs::write(root.join(".git/HEAD"), "ref: refs/heads/dev\n").unwrap();
        Self { root }
    }

    fn paths(&self, branch: Option<&str>, absolute: bool) -> Vec<PathBuf> {
        build_identity::git_metadata_paths(&self.root.join("node"), |args| match args {
            ["symbolic-ref", "-q", "HEAD"] => match branch {
                Some(branch) => git_output(0, branch.as_bytes()),
                None => git_output(1, b""),
            },
            ["rev-parse", "--git-path", name] => {
                let path = if absolute {
                    self.root.join(".git").join(name)
                } else {
                    PathBuf::from("../.git").join(name)
                };
                git_output(0, path.to_str().unwrap().as_bytes())
            }
            _ => panic!("Unexpected Git arguments: {args:?}"),
        })
    }
}

impl Drop for MetadataFixture {
    fn drop(&mut self) { std::fs::remove_dir_all(&self.root).unwrap(); }
}

#[test]
fn metadata_tracks_head_current_branch_tags_and_packed_refs() {
    let fixture = MetadataFixture::new();
    std::fs::write(fixture.root.join(".git/refs/heads/dev"), "commit\n").unwrap();
    std::fs::write(fixture.root.join(".git/packed-refs"), "packed\n").unwrap();
    for absolute in [false, true] {
        let paths = fixture.paths(Some("refs/heads/dev"), absolute);
        let actual: Vec<_> = paths
            .iter()
            .map(|path| path.canonicalize().unwrap())
            .collect();
        let expected: Vec<_> = ["HEAD", "refs/tags", "packed-refs", "refs/heads/dev"]
            .iter()
            .map(|name| fixture.root.join(".git").join(name).canonicalize().unwrap())
            .collect();
        assert_eq!(actual, expected);
    }
}

#[test]
fn packed_branch_tracks_nearest_existing_local_branch_directory() {
    let fixture = MetadataFixture::new();
    std::fs::write(fixture.root.join(".git/packed-refs"), "packed\n").unwrap();
    let heads = fixture.root.join(".git/refs/heads");
    assert!(fixture
        .paths(Some("refs/heads/fix/428"), true)
        .contains(&heads));
    let nested = heads.join("fix");
    std::fs::create_dir(&nested).unwrap();
    assert!(fixture
        .paths(Some("refs/heads/fix/428"), true)
        .contains(&nested));
    let branch = nested.join("428");
    std::fs::write(&branch, "new commit\n").unwrap();
    let paths = fixture.paths(Some("refs/heads/fix/428"), true);
    assert!(paths.contains(&branch));
    assert!(!paths.contains(&heads));
    assert!(!paths.contains(&nested));
}

#[test]
fn detached_head_does_not_track_branch_or_remote_refs() {
    let fixture = MetadataFixture::new();
    let paths = fixture.paths(None, true);
    assert_eq!(paths, vec![
        fixture.root.join(".git/HEAD"),
        fixture.root.join(".git/refs/tags")
    ]);
}

#[test]
fn missing_tag_directory_is_tracked_without_tracking_all_refs() {
    let fixture = MetadataFixture::new();
    let tags = fixture.root.join(".git/refs/tags");
    std::fs::remove_dir(&tags).unwrap();
    assert!(fixture.paths(None, true).contains(&tags));
}

#[test]
fn unavailable_git_does_not_add_metadata_paths() {
    let paths = build_identity::git_metadata_paths(std::path::Path::new("node"), |_| {
        git_output(128, b"not a repository")
    });
    assert!(paths.is_empty());
}
