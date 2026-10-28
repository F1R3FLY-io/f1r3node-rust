//! End-to-end integration tests for the file-read sidecar.
//!
//! Spin up the sidecar on an ephemeral port, use an async
//! `reqwest::Client` to issue GET requests, and verify the
//! response body + status + headers.

use std::io::Write;
use std::sync::Arc;

use file_read_service::config::load_from_path;
use file_read_service::index::FileReadIndex;
use file_read_service::server::serve;

/// Start the sidecar in-process on 127.0.0.1:0 (ephemeral port).
/// Returns `(base_url, _handle)` — holding `_handle` keeps the
/// server task alive for the test.
async fn start_server(
    config_path: &std::path::Path,
) -> (String, tokio::task::JoinHandle<std::io::Result<()>>) {
    let cfg = load_from_path(config_path).expect("load config");
    let index = Arc::new(FileReadIndex::build(&cfg));
    let (local, handle) = serve("127.0.0.1:0".parse().unwrap(), index)
        .await
        .expect("start server");
    (format!("http://{local}"), handle)
}

fn write_file(path: &std::path::Path, bytes: &[u8]) {
    let mut f = std::fs::File::create(path).unwrap();
    f.write_all(bytes).unwrap();
}

/// Golden path: GET /files/{logical} on an operator-declared file
/// returns the full file contents + 200 + Content-Type.
#[tokio::test]
async fn get_oracle_static_file_returns_full_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let seed_path = dir.path().join("boot.log");
    write_file(&seed_path, b"hello from the sidecar");

    let config_path = dir.path().join("config.conf");
    let config_body = format!(
        r#"
        oracle-static-files = {{
          boot_log = "{}"
        }}
        "#,
        seed_path.display()
    );
    write_file(&config_path, config_body.as_bytes());

    let (base, _handle) = start_server(&config_path).await;
    let resp = reqwest::get(format!("{base}/files/boot_log?length=1024"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let actual_bytes = resp.bytes().await.unwrap();
    assert_eq!(actual_bytes.as_ref(), b"hello from the sidecar");
}

/// Unknown logical name → 404 with a readable error message
/// naming the logical.
#[tokio::test]
async fn unknown_logical_returns_404() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.conf");
    write_file(&config_path, b"{}");

    let (base, _handle) = start_server(&config_path).await;
    let resp = reqwest::get(format!("{base}/files/ghost")).await.unwrap();
    assert_eq!(resp.status(), 404);
    let body = resp.text().await.unwrap();
    assert!(
        body.contains("ghost"),
        "404 body must name the missing logical; got {body}"
    );
}

/// Consensus-static-FILE logical → 503.  File entries pass through
/// `fs_bundle_staging` so the sidecar can't resolve them from HOCON.
#[tokio::test]
async fn consensus_static_file_returns_503() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.conf");
    write_file(
        &config_path,
        br#"
        consensus-static-files = {
          state = "/var/state"
        }
        "#,
    );

    let (base, _handle) = start_server(&config_path).await;
    let resp = reqwest::get(format!("{base}/files/state")).await.unwrap();
    assert_eq!(resp.status(), 503);
    let body = resp.text().await.unwrap();
    assert!(
        body.contains("consensus-static-FILE") && body.contains("fs_bundle_staging"),
        "503 body must explain the file-staging skip; got {body}"
    );
}

/// Consensus-static-DIRS resolves the same as oracle-static-dirs.
/// The node writes WAL-replicated bytes directly into `entry.path`,
/// so the sidecar can serve them from the operator-declared path.
#[tokio::test]
async fn consensus_static_dir_with_sub_rel_reads_subfile() {
    let dir = tempfile::tempdir().unwrap();
    let bundle = dir.path().join("bundle");
    std::fs::create_dir(&bundle).unwrap();
    let sub_file = bundle.join("note.txt");
    write_file(&sub_file, b"wal-replicated bytes");

    let config_path = dir.path().join("config.conf");
    let config_body = format!(
        r#"
        consensus-static-dirs = {{
          root = {{ path = "{}", mode = "r+" }}
        }}
        "#,
        bundle.display()
    );
    write_file(&config_path, config_body.as_bytes());

    let (base, _handle) = start_server(&config_path).await;
    let resp = reqwest::get(format!("{base}/files/root?sub_rel=note.txt&length=1024"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let actual_bytes = resp.bytes().await.unwrap();
    assert_eq!(actual_bytes.as_ref(), b"wal-replicated bytes");
}

/// Dir entry requires `?sub_rel`.  Without it, 400.
#[tokio::test]
async fn dir_entry_without_sub_rel_returns_400() {
    let dir = tempfile::tempdir().unwrap();
    let share_dir = dir.path().join("share");
    std::fs::create_dir(&share_dir).unwrap();

    let config_path = dir.path().join("config.conf");
    let config_body = format!(
        r#"
        oracle-static-dirs = {{
          share = "{}"
        }}
        "#,
        share_dir.display()
    );
    write_file(&config_path, config_body.as_bytes());

    let (base, _handle) = start_server(&config_path).await;
    let resp = reqwest::get(format!("{base}/files/share")).await.unwrap();
    assert_eq!(resp.status(), 400);
    let body = resp.text().await.unwrap();
    assert!(body.contains("sub_rel"));
}

/// Dir entry with `?sub_rel` reads the file under the dir.
#[tokio::test]
async fn dir_entry_with_sub_rel_reads_subfile() {
    let dir = tempfile::tempdir().unwrap();
    let share_dir = dir.path().join("share");
    std::fs::create_dir(&share_dir).unwrap();
    let sub_file = share_dir.join("hello.txt");
    write_file(&sub_file, b"nested bytes");

    let config_path = dir.path().join("config.conf");
    let config_body = format!(
        r#"
        oracle-static-dirs = {{
          share = "{}"
        }}
        "#,
        share_dir.display()
    );
    write_file(&config_path, config_body.as_bytes());

    let (base, _handle) = start_server(&config_path).await;
    let resp = reqwest::get(format!("{base}/files/share?sub_rel=hello.txt&length=1024"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let actual_bytes = resp.bytes().await.unwrap();
    assert_eq!(actual_bytes.as_ref(), b"nested bytes");
}

/// Dir entry with `?sub_rel=../escape` → 400 with path-escape
/// error.  Path-traversal defense.
#[tokio::test]
async fn dir_entry_rejects_path_escape() {
    let dir = tempfile::tempdir().unwrap();
    let share_dir = dir.path().join("share");
    std::fs::create_dir(&share_dir).unwrap();

    let config_path = dir.path().join("config.conf");
    let config_body = format!(
        r#"
        oracle-static-dirs = {{
          share = "{}"
        }}
        "#,
        share_dir.display()
    );
    write_file(&config_path, config_body.as_bytes());

    let (base, _handle) = start_server(&config_path).await;
    let resp = reqwest::get(format!("{base}/files/share?sub_rel=../etc/passwd"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
}

/// `length` beyond MAX_READ_BYTES → 400.  Quota defense.
#[tokio::test]
async fn oversized_length_returns_400() {
    let dir = tempfile::tempdir().unwrap();
    let seed_path = dir.path().join("boot.log");
    write_file(&seed_path, b"small file");

    let config_path = dir.path().join("config.conf");
    let config_body = format!(
        r#"
        oracle-static-files = {{
          boot_log = "{}"
        }}
        "#,
        seed_path.display()
    );
    write_file(&config_path, config_body.as_bytes());

    let (base, _handle) = start_server(&config_path).await;
    // 128 MiB > the 64 MiB MAX_READ_BYTES cap.
    let resp = reqwest::get(format!(
        "{base}/files/boot_log?length={}",
        128 * 1024 * 1024_u64
    ))
    .await
    .unwrap();
    assert_eq!(resp.status(), 400);
}

/// Offset + length within file bounds returns the requested
/// slice; the X-F1R3-Read-Length header matches the body length.
#[tokio::test]
async fn offset_and_length_slice_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let seed_path = dir.path().join("boot.log");
    write_file(&seed_path, b"0123456789abcdef");

    let config_path = dir.path().join("config.conf");
    let config_body = format!(
        r#"
        oracle-static-files = {{
          boot_log = "{}"
        }}
        "#,
        seed_path.display()
    );
    write_file(&config_path, config_body.as_bytes());

    let (base, _handle) = start_server(&config_path).await;
    let resp = reqwest::get(format!("{base}/files/boot_log?offset=4&length=6"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let header = resp
        .headers()
        .get("X-F1R3-Read-Length")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let body = resp.bytes().await.unwrap();
    assert_eq!(body.as_ref(), b"456789");
    assert_eq!(header.as_deref(), Some("6"));
}

/// Offset past EOF returns 0 bytes with a 200 (POSIX pread
/// semantics).
#[tokio::test]
async fn offset_past_eof_returns_empty_body() {
    let dir = tempfile::tempdir().unwrap();
    let seed_path = dir.path().join("boot.log");
    write_file(&seed_path, b"short");

    let config_path = dir.path().join("config.conf");
    let config_body = format!(
        r#"
        oracle-static-files = {{
          boot_log = "{}"
        }}
        "#,
        seed_path.display()
    );
    write_file(&config_path, config_body.as_bytes());

    let (base, _handle) = start_server(&config_path).await;
    let resp = reqwest::get(format!("{base}/files/boot_log?offset=1000&length=100"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = resp.bytes().await.unwrap();
    assert_eq!(body.len(), 0);
}
