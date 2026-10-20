// HTTP handler for `GET /files/{logical}[?offset=N&length=M]`.
//
// Opens the resolved host file with `O_RDONLY | O_NOFOLLOW`,
// takes an `F_RDLCK` on `[offset, offset + length)` via the local
// fcntl guard, `pread`s the bytes, and returns them as
// `application/octet-stream`.  Partial-at-EOF is signaled via a
// shorter response body + an `X-F1R3-Read-Length` header.
//
// Error shape:
//   * 404 — unknown logical name.
//   * 400 — bad `?sub_rel`, `?offset`, or `?length`; File entry
//     given a sub_rel; Dir entry missing sub_rel.
//   * 503 — Consensus-mode logical (sidecar can't resolve
//     `/@bundle/<X>`).
//   * 500 — I/O error during open/lock/read.
//
// # MAX_READ bytes
//
// Capped at 64 MiB per request — matches the node's
// `MAX_READ_BYTES`.  A hostile client can't exhaust the sidecar
// pool with giant requests.

use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use crate::fcntl::FcntlReadLockGuard;
use crate::index::{Entry, FileReadIndex, Lookup, SkipReason};

/// Server state shared across handler calls.
pub struct AppState {
    pub index: Arc<FileReadIndex>,
}

/// Max bytes a single read request may ask for.  Mirrors the
/// node's `MAX_READ_BYTES` (64 MiB) so a hostile client can't
/// request a sidecar read that would be rejected by the node's
/// coordinated write path.
pub const MAX_READ_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Deserialize)]
pub struct ReadQuery {
    #[serde(default)]
    pub offset: Option<u64>,
    #[serde(default)]
    pub length: Option<u64>,
    #[serde(default)]
    pub sub_rel: Option<String>,
}

/// Named header for the actual byte count served — distinct from
/// `Content-Length` so partial-at-EOF reads surface the real
/// delta (e.g., asked 1024 bytes, served 100 at EOF, body is 100
/// bytes, header is `X-F1R3-Read-Length: 100`).  `Content-Length`
/// matches the body length regardless; the header is for
/// client-side consistency checks.
pub const X_READ_LENGTH: &str = "X-F1R3-Read-Length";

pub async fn get_file(
    State(state): State<Arc<AppState>>,
    AxumPath(logical): AxumPath<String>,
    Query(q): Query<ReadQuery>,
) -> Response {
    let entry = match state.index.lookup(&logical) {
        Lookup::Found(e) => e,
        Lookup::Skipped(SkipReason::ConsensusMode) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                format!(
                    "logical name `{logical}` is a Consensus-mode entry; the \
                     sidecar can't resolve `/@bundle/<X>` to a per-validator \
                     on-disk path without the node's RootIdentityRegistry.  \
                     Use the node's deploy surface instead, or declare the \
                     entry as oracle-static-{{files,dirs}} if the data is \
                     operator-shared."
                ),
            )
                .into_response();
        }
        Lookup::NotFound => {
            return (
                StatusCode::NOT_FOUND,
                format!("unknown logical `{logical}`"),
            )
                .into_response();
        }
    };

    // Resolve the final host path.  File entries reject `?sub_rel`;
    // Dir entries require it.
    let host_path: PathBuf = match (entry, q.sub_rel.as_deref()) {
        (Entry::File { .. }, Some(_)) => {
            return (
                StatusCode::BAD_REQUEST,
                "File entries do not accept ?sub_rel".to_string(),
            )
                .into_response();
        }
        (Entry::File { path }, None) => path.clone(),
        (Entry::Dir { .. }, None) => {
            return (
                StatusCode::BAD_REQUEST,
                "Dir entries require ?sub_rel=<rel>".to_string(),
            )
                .into_response();
        }
        (Entry::Dir { path }, Some(rel)) => match safe_join(path, rel) {
            Ok(p) => p,
            Err(msg) => return (StatusCode::BAD_REQUEST, msg).into_response(),
        },
    };

    let offset = q.offset.unwrap_or(0);
    let length = q.length.unwrap_or(0);
    if length > MAX_READ_BYTES {
        return (
            StatusCode::BAD_REQUEST,
            format!("length {length} exceeds MAX_READ_BYTES ({MAX_READ_BYTES})"),
        )
            .into_response();
    }

    match read_bytes(&host_path, offset, length) {
        Ok(bytes) => build_ok_response(bytes),
        Err(ReadError::IoOpen(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("open failed: {e}"),
        )
            .into_response(),
        Err(ReadError::Lock(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("fcntl F_RDLCK failed: {e}"),
        )
            .into_response(),
        Err(ReadError::Pread(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("pread failed: {e}"),
        )
            .into_response(),
    }
}

#[derive(Debug)]
enum ReadError {
    IoOpen(std::io::Error),
    Lock(std::io::Error),
    Pread(std::io::Error),
}

/// Open the file with `O_RDONLY | O_NOFOLLOW`, take an F_RDLCK,
/// pread the bytes.  `length == 0` returns an empty response.
///
/// `O_NOFOLLOW` on the open prevents a symlinked logical path
/// from redirecting to a different file post-config-load.  (The
/// operator declares an absolute path in HOCON; a mid-path
/// component swap can still redirect, but that's a host-boundary
/// hazard the operator owns.)
fn read_bytes(path: &Path, offset: u64, length: u64) -> Result<Vec<u8>, ReadError> {
    use std::os::unix::fs::OpenOptionsExt;

    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(ReadError::IoOpen)?;

    let fd = file.as_raw_fd();

    let _lock = FcntlReadLockGuard::acquire(fd, offset, length).map_err(ReadError::Lock)?;

    // Allocate the full requested buffer; truncate to the actual
    // pread return (handles EOF-partial cleanly).
    let mut buf = vec![0u8; length as usize];
    if length == 0 {
        return Ok(buf);
    }
    // SAFETY: `fd` is live for the duration of this call (owned
    // by `file` which stays in scope through the Ok return).
    // `buf` is a live Vec<u8> of `length` bytes.  pread writes up
    // to `length` bytes starting at the given offset and returns
    // the count written (or -1 on error).  Lock guard keeps
    // F_RDLCK held through the syscall.
    let got = unsafe {
        libc::pread(
            fd,
            buf.as_mut_ptr() as *mut _,
            length as usize,
            offset as i64,
        )
    };
    if got < 0 {
        return Err(ReadError::Pread(std::io::Error::last_os_error()));
    }
    buf.truncate(got as usize);
    Ok(buf)
}

fn build_ok_response(bytes: Vec<u8>) -> Response {
    let actual_len = bytes.len();
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    headers.insert(X_READ_LENGTH, HeaderValue::from(actual_len));
    (StatusCode::OK, headers, Body::from(bytes)).into_response()
}

/// Component-wise join of `root` and `rel`, rejecting any `..` /
/// absolute / non-UTF-8 component.  Prevents a client-supplied
/// `?sub_rel=../etc/passwd` from escaping the operator's declared
/// Dir root.
fn safe_join(root: &Path, rel: &str) -> Result<PathBuf, String> {
    use std::path::Component;
    let rel_path = Path::new(rel);
    let mut out = root.to_path_buf();
    for comp in rel_path.components() {
        match comp {
            Component::Normal(c) => {
                let s = c
                    .to_str()
                    .ok_or_else(|| "sub_rel contains non-UTF-8 component".to_string())?;
                out.push(s);
            }
            Component::CurDir => {}
            Component::ParentDir => return Err("sub_rel contains `..`".into()),
            Component::RootDir | Component::Prefix(_) => {
                return Err("sub_rel must be relative".into());
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_join_rejects_parent_component() {
        let err = safe_join(Path::new("/srv/share"), "../etc/passwd").unwrap_err();
        assert!(err.contains(".."));
    }

    #[test]
    fn safe_join_rejects_absolute_rel() {
        let err = safe_join(Path::new("/srv/share"), "/etc/passwd").unwrap_err();
        assert!(err.contains("relative"));
    }

    #[test]
    fn safe_join_accepts_normal_components() {
        let out = safe_join(Path::new("/srv/share"), "sub/file.txt").unwrap();
        assert_eq!(out, Path::new("/srv/share/sub/file.txt"));
    }

    #[test]
    fn safe_join_normalizes_cur_dir() {
        let out = safe_join(Path::new("/srv/share"), "./file.txt").unwrap();
        assert_eq!(out, Path::new("/srv/share/file.txt"));
    }
}
