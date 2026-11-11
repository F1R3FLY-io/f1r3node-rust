use std::io;
use std::path::{Path, PathBuf};
use std::process::Output;

pub fn resolve_build_id(
    explicit: Option<&str>,
    git: impl FnOnce() -> io::Result<Output>,
) -> Result<String, &'static str> {
    if let Some(value) = explicit {
        let identity = value.trim();
        if !identity.is_empty() {
            if identity.chars().any(char::is_control) {
                return Err("F1R3NODE_BUILD_ID must not contain control characters");
            }
            return Ok(identity.to_owned());
        }
    }

    Ok(git_value(git()).unwrap_or_else(|| "unknown".to_owned()))
}

fn git_value(output: io::Result<Output>) -> Option<String> {
    output
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty() && !value.chars().any(char::is_control))
}

pub fn git_metadata_paths(
    manifest_dir: &Path,
    mut git: impl FnMut(&[&str]) -> io::Result<Output>,
) -> Vec<PathBuf> {
    let branch = git_value(git(&["symbolic-ref", "-q", "HEAD"]));
    let mut names = vec!["HEAD", "refs/tags", "packed-refs"];
    if let Some(branch) = branch.as_deref() {
        names.push(branch);
    }
    let mut paths = Vec::new();
    for name in names {
        let Some(value) = git_value(git(&["rev-parse", "--git-path", name])) else {
            continue;
        };
        let mut path = manifest_dir.join(value);
        if Some(name) == branch.as_deref() && !path.exists() {
            let Some(heads) = git_value(git(&["rev-parse", "--git-path", "refs/heads"])) else {
                continue;
            };
            let heads = manifest_dir.join(heads);
            while path.starts_with(&heads) && path != heads && !path.exists() {
                path.pop();
            }
            if !path.starts_with(&heads) {
                continue;
            }
        }
        if (name != "packed-refs" || path.exists()) && !paths.contains(&path) {
            paths.push(path);
        }
    }

    paths
}
