use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crypto::rust::hash::blake2b256::Blake2b256;

use super::snapshot_chunk::{snapshot_merkle_root, EMPTY_SNAPSHOT_ROOT};

pub const MANIFEST_FILE: &str = "BULK.manifest";
pub const ROOT_FILE: &str = "BULK.root";
pub const CONTROL_DIR: &str = ".bulk";
pub const STAGING_DIR: &str = "staging";
pub const RETIRED_DIR: &str = "retired";
pub const EMPTY_ROOT: [u8; 32] = EMPTY_SNAPSHOT_ROOT;
pub const MAX_SEGMENT_LEN: usize = 128;
pub const MAX_NAMESPACE_DEPTH: usize = 8;

const ENTRY_DOMAIN: &[u8] = b"f1r3fly/bulk-io/tree-entry/v1";
const HASH_BUF: usize = 1 << 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeEntry {
    pub rel: String,
    pub len: u64,
    pub hash: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BulkTreeError {
    #[error("io error at {path}: {message}")]
    Io { path: String, message: String },
    #[error("malformed manifest line {line}: {message}")]
    Malformed { line: usize, message: String },
    #[error("missing or unreadable root file")]
    MissingRootFile,
    #[error("result root mismatch: expected {expected}, found {actual}")]
    RootMismatch { expected: String, actual: String },
    #[error("base root mismatch: expected {expected}, found {actual}")]
    BaseMismatch { expected: String, actual: String },
    #[error("tree listing does not match manifest: {0}")]
    ListingMismatch(String),
    #[error("invalid name: {0}")]
    InvalidName(String),
    #[error("symbolic link not allowed: {0}")]
    Symlink(String),
}

impl BulkTreeError {
    fn io(path: &Path, e: std::io::Error) -> Self {
        BulkTreeError::Io {
            path: path.display().to_string(),
            message: e.to_string(),
        }
    }
}

pub fn blake2b(bytes: &[u8]) -> [u8; 32] {
    let h = Blake2b256::hash(bytes.to_vec());
    let mut out = [0u8; 32];
    out.copy_from_slice(&h);
    out
}

pub fn hex_root(root: &[u8; 32]) -> String { hex::encode(root) }

pub fn parse_hex_root(s: &str) -> Option<[u8; 32]> {
    let s = s.trim();
    if s.len() != 64
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return None;
    }
    let v = hex::decode(s).ok()?;
    let mut out = [0u8; 32];
    out.copy_from_slice(&v);
    Some(out)
}

pub fn entry_hash(e: &TreeEntry) -> [u8; 32] {
    let rel = e.rel.as_bytes();
    let mut buf = Vec::with_capacity(ENTRY_DOMAIN.len() + 4 + rel.len() + 8 + 32);
    buf.extend_from_slice(ENTRY_DOMAIN);
    buf.extend_from_slice(&(rel.len() as u32).to_be_bytes());
    buf.extend_from_slice(rel);
    buf.extend_from_slice(&e.len.to_be_bytes());
    buf.extend_from_slice(&e.hash);
    blake2b(&buf)
}

pub fn tree_root(entries: &[TreeEntry]) -> [u8; 32] {
    let leaves: Vec<[u8; 32]> = entries.iter().map(entry_hash).collect();
    snapshot_merkle_root(&leaves)
}

fn segment_ok(seg: &str) -> bool {
    !seg.is_empty()
        && seg.len() <= MAX_SEGMENT_LEN
        && seg != "."
        && seg != ".."
        && !seg.starts_with('.')
        && seg
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.')
}

pub fn validate_rel(rel: &str) -> Result<(), BulkTreeError> {
    if rel.is_empty() || rel.starts_with('/') || rel.ends_with('/') {
        return Err(BulkTreeError::InvalidName(rel.to_string()));
    }
    if rel == MANIFEST_FILE || rel == ROOT_FILE {
        return Err(BulkTreeError::InvalidName(rel.to_string()));
    }
    if rel.split('/').all(segment_ok) {
        Ok(())
    } else {
        Err(BulkTreeError::InvalidName(rel.to_string()))
    }
}

pub fn validate_namespace(ns: &str) -> Result<(), BulkTreeError> {
    validate_rel(ns)?;
    if ns.split('/').count() > MAX_NAMESPACE_DEPTH {
        return Err(BulkTreeError::InvalidName(ns.to_string()));
    }
    Ok(())
}

pub fn validate_stage_id(id: &str) -> Result<(), BulkTreeError> {
    if parse_hex_root(id).is_some() {
        Ok(())
    } else {
        Err(BulkTreeError::InvalidName(id.to_string()))
    }
}

pub fn render_manifest(entries: &[TreeEntry]) -> String {
    let mut s = String::new();
    for e in entries {
        s.push_str(&hex::encode(e.hash));
        s.push('\t');
        s.push_str(&e.len.to_string());
        s.push('\t');
        s.push_str(&e.rel);
        s.push('\n');
    }
    s
}

pub fn parse_manifest(text: &str) -> Result<Vec<TreeEntry>, BulkTreeError> {
    let mut out: Vec<TreeEntry> = Vec::new();
    if !text.is_empty() && !text.ends_with('\n') {
        return Err(BulkTreeError::Malformed {
            line: text.lines().count(),
            message: "missing trailing newline".into(),
        });
    }
    for (i, line) in text.lines().enumerate() {
        let n = i + 1;
        let mut parts = line.splitn(3, '\t');
        let (h, l, r) = match (parts.next(), parts.next(), parts.next()) {
            (Some(h), Some(l), Some(r)) => (h, l, r),
            _ => {
                return Err(BulkTreeError::Malformed {
                    line: n,
                    message: "expected three tab-separated fields".into(),
                })
            }
        };
        let hash = parse_hex_root(h).ok_or_else(|| BulkTreeError::Malformed {
            line: n,
            message: "bad hash".into(),
        })?;
        if l.is_empty() || l.starts_with('+') || (l.len() > 1 && l.starts_with('0')) {
            return Err(BulkTreeError::Malformed {
                line: n,
                message: "bad length".into(),
            });
        }
        let len: u64 = l.parse().map_err(|_| BulkTreeError::Malformed {
            line: n,
            message: "bad length".into(),
        })?;
        validate_rel(r).map_err(|_| BulkTreeError::Malformed {
            line: n,
            message: "bad path".into(),
        })?;
        if let Some(prev) = out.last() {
            if prev.rel.as_bytes() >= r.as_bytes() {
                return Err(BulkTreeError::Malformed {
                    line: n,
                    message: "paths not strictly ascending".into(),
                });
            }
        }
        out.push(TreeEntry {
            rel: r.to_string(),
            len,
            hash,
        });
    }
    Ok(out)
}

fn no_symlink(p: &Path) -> Result<bool, BulkTreeError> {
    match fs::symlink_metadata(p) {
        Ok(m) if m.file_type().is_symlink() => Err(BulkTreeError::Symlink(p.display().to_string())),
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(BulkTreeError::io(p, e)),
    }
}

pub fn exists_no_symlink(p: &Path) -> Result<bool, BulkTreeError> { no_symlink(p) }

pub fn read_manifest(dir: &Path) -> Result<Vec<TreeEntry>, BulkTreeError> {
    let p = dir.join(MANIFEST_FILE);
    if !no_symlink(&p)? {
        return Err(BulkTreeError::Io {
            path: p.display().to_string(),
            message: "manifest not found".into(),
        });
    }
    let text = fs::read_to_string(&p).map_err(|e| BulkTreeError::io(&p, e))?;
    parse_manifest(&text)
}

pub fn read_sealed_root(dir: &Path) -> Result<[u8; 32], BulkTreeError> {
    let entries = read_manifest(dir)?;
    let computed = tree_root(&entries);
    let p = dir.join(ROOT_FILE);
    if !no_symlink(&p)? {
        return Err(BulkTreeError::MissingRootFile);
    }
    let text = fs::read_to_string(&p).map_err(|_| BulkTreeError::MissingRootFile)?;
    let stated = parse_hex_root(&text).ok_or(BulkTreeError::MissingRootFile)?;
    if stated != computed {
        return Err(BulkTreeError::RootMismatch {
            expected: hex_root(&stated),
            actual: hex_root(&computed),
        });
    }
    Ok(computed)
}

pub fn hash_file(p: &Path) -> Result<(u64, [u8; 32]), BulkTreeError> {
    let mut f = File::open(p).map_err(|e| BulkTreeError::io(p, e))?;
    let mut all = Vec::new();
    let mut buf = vec![0u8; HASH_BUF];
    loop {
        let n = f.read(&mut buf).map_err(|e| BulkTreeError::io(p, e))?;
        if n == 0 {
            break;
        }
        all.extend_from_slice(&buf[..n]);
    }
    Ok((all.len() as u64, blake2b(&all)))
}

fn walk(
    base: &Path,
    dir: &Path,
    prefix: &str,
    out: &mut Vec<TreeEntry>,
) -> Result<(), BulkTreeError> {
    let rd = fs::read_dir(dir).map_err(|e| BulkTreeError::io(dir, e))?;
    let mut names = Vec::new();
    for ent in rd {
        let ent = ent.map_err(|e| BulkTreeError::io(dir, e))?;
        let name = ent
            .file_name()
            .into_string()
            .map_err(|n| BulkTreeError::InvalidName(format!("{}", PathBuf::from(n).display())))?;
        names.push(name);
    }
    names.sort();
    for name in names {
        if prefix.is_empty() && (name == MANIFEST_FILE || name == ROOT_FILE) {
            continue;
        }
        let rel = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        validate_rel(&rel)?;
        let p = dir.join(&name);
        let m = fs::symlink_metadata(&p).map_err(|e| BulkTreeError::io(&p, e))?;
        if m.file_type().is_symlink() {
            return Err(BulkTreeError::Symlink(p.display().to_string()));
        }
        if m.is_dir() {
            walk(base, &p, &rel, out)?;
        } else if m.is_file() {
            let (len, hash) = hash_file(&p)?;
            out.push(TreeEntry { rel, len, hash });
        } else {
            return Err(BulkTreeError::InvalidName(rel));
        }
    }
    Ok(())
}

pub fn scan_tree(dir: &Path) -> Result<Vec<TreeEntry>, BulkTreeError> {
    let mut out = Vec::new();
    walk(dir, dir, "", &mut out)?;
    out.sort_by(|a, b| a.rel.as_bytes().cmp(b.rel.as_bytes()));
    Ok(out)
}

fn write_synced(p: &Path, bytes: &[u8]) -> Result<(), BulkTreeError> {
    let tmp = p.with_extension("tmp");
    let mut f = File::create(&tmp).map_err(|e| BulkTreeError::io(&tmp, e))?;
    f.write_all(bytes).map_err(|e| BulkTreeError::io(&tmp, e))?;
    f.sync_all().map_err(|e| BulkTreeError::io(&tmp, e))?;
    fs::rename(&tmp, p).map_err(|e| BulkTreeError::io(p, e))
}

pub fn seal_tree(dir: &Path) -> Result<[u8; 32], BulkTreeError> {
    let entries = scan_tree(dir)?;
    let root = tree_root(&entries);
    write_synced(
        &dir.join(MANIFEST_FILE),
        render_manifest(&entries).as_bytes(),
    )?;
    write_synced(
        &dir.join(ROOT_FILE),
        format!("{}\n", hex_root(&root)).as_bytes(),
    )?;
    sync_dir(dir)?;
    Ok(root)
}

pub fn verify_tree(dir: &Path) -> Result<[u8; 32], BulkTreeError> {
    let root = read_sealed_root(dir)?;
    let stated = read_manifest(dir)?;
    let actual = scan_tree(dir)?;
    if stated != actual {
        let first = stated
            .iter()
            .zip(actual.iter())
            .find(|(a, b)| a != b)
            .map(|(a, _)| a.rel.clone())
            .unwrap_or_else(|| format!("entry count {} vs {}", stated.len(), actual.len()));
        return Err(BulkTreeError::ListingMismatch(first));
    }
    Ok(root)
}

pub fn sync_dir(dir: &Path) -> Result<(), BulkTreeError> {
    let f = File::open(dir).map_err(|e| BulkTreeError::io(dir, e))?;
    f.sync_all().map_err(|e| BulkTreeError::io(dir, e))
}

pub fn staging_path(import_root: &Path, stage_id: &str) -> PathBuf {
    import_root
        .join(CONTROL_DIR)
        .join(STAGING_DIR)
        .join(stage_id)
}

pub fn retired_path(import_root: &Path, stage_id: &str) -> PathBuf {
    import_root
        .join(CONTROL_DIR)
        .join(RETIRED_DIR)
        .join(stage_id)
}

pub fn current_root(target: &Path) -> Result<[u8; 32], BulkTreeError> {
    if no_symlink(target)? {
        read_sealed_root(target)
    } else {
        Ok(EMPTY_ROOT)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwapOutcome {
    Applied,
    AlreadyApplied,
}

pub fn swap_in(
    staging: &Path,
    target: &Path,
    retired: &Path,
    expected: &[u8; 32],
    base: &[u8; 32],
) -> Result<SwapOutcome, BulkTreeError> {
    let staged_present = no_symlink(staging)?;
    let target_present = no_symlink(target)?;
    if !staged_present {
        if target_present && read_sealed_root(target)? == *expected {
            return Ok(SwapOutcome::AlreadyApplied);
        }
        return Err(BulkTreeError::Io {
            path: staging.display().to_string(),
            message: "staged tree not present".into(),
        });
    }
    let staged = read_sealed_root(staging)?;
    if staged != *expected {
        return Err(BulkTreeError::RootMismatch {
            expected: hex_root(expected),
            actual: hex_root(&staged),
        });
    }
    let current = if target_present {
        read_sealed_root(target)?
    } else {
        EMPTY_ROOT
    };
    if current != *base {
        return Err(BulkTreeError::BaseMismatch {
            expected: hex_root(base),
            actual: hex_root(&current),
        });
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| BulkTreeError::io(parent, e))?;
    }
    if target_present {
        if no_symlink(retired)? {
            return Err(BulkTreeError::Io {
                path: retired.display().to_string(),
                message: "retired path already exists".into(),
            });
        }
        if let Some(parent) = retired.parent() {
            fs::create_dir_all(parent).map_err(|e| BulkTreeError::io(parent, e))?;
        }
        fs::rename(target, retired).map_err(|e| BulkTreeError::io(target, e))?;
    }
    fs::rename(staging, target).map_err(|e| BulkTreeError::io(staging, e))?;
    if let Some(parent) = target.parent() {
        sync_dir(parent)?;
    }
    if let Some(parent) = staging.parent() {
        sync_dir(parent)?;
    }
    Ok(SwapOutcome::Applied)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(p: &Path, b: &[u8]) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b).unwrap();
    }

    #[test]
    fn empty_tree_root_is_sentinel() {
        assert_eq!(tree_root(&[]), EMPTY_ROOT);
    }

    #[test]
    fn manifest_round_trip_and_ordering() {
        let a = TreeEntry {
            rel: "a/x".into(),
            len: 3,
            hash: blake2b(b"abc"),
        };
        let b = TreeEntry {
            rel: "b".into(),
            len: 0,
            hash: blake2b(b""),
        };
        let text = render_manifest(&[a.clone(), b.clone()]);
        assert_eq!(parse_manifest(&text).unwrap(), vec![a.clone(), b.clone()]);
        let swapped = render_manifest(&[b, a]);
        assert!(parse_manifest(&swapped).is_err());
    }

    #[test]
    fn manifest_rejects_bad_fields() {
        let h = hex::encode([1u8; 32]);
        assert!(parse_manifest(&format!("{h}\t01\tx\n")).is_err());
        assert!(parse_manifest(&format!("{h}\t1\t../x\n")).is_err());
        assert!(parse_manifest(&format!("{h}\t1\t.hidden\n")).is_err());
        assert!(parse_manifest(&format!("{h}\t1\tx")).is_err());
        assert!(parse_manifest(&format!(
            "{}\t1\tx\n",
            hex::encode([0xab; 32]).to_uppercase()
        ))
        .is_err());
        assert!(parse_manifest(&format!("{h}\t1\tx\n")).is_ok());
    }

    #[test]
    fn root_changes_with_any_field() {
        let e = TreeEntry {
            rel: "x".into(),
            len: 1,
            hash: [7u8; 32],
        };
        let base = tree_root(std::slice::from_ref(&e));
        let mut e2 = e.clone();
        e2.len = 2;
        assert_ne!(base, tree_root(&[e2]));
        let mut e3 = e.clone();
        e3.rel = "y".into();
        assert_ne!(base, tree_root(&[e3]));
        let mut e4 = e;
        e4.hash = [8u8; 32];
        assert_ne!(base, tree_root(&[e4]));
    }

    #[test]
    fn seal_then_verify_and_detect_tamper() {
        let d = tempfile::tempdir().unwrap();
        write(&d.path().join("ab/cd/part-00000.rec"), b"hello");
        write(&d.path().join("PROVENANCE"), b"p");
        let root = seal_tree(d.path()).unwrap();
        assert_eq!(verify_tree(d.path()).unwrap(), root);
        assert_eq!(read_sealed_root(d.path()).unwrap(), root);
        fs::write(d.path().join("ab/cd/part-00000.rec"), b"HELLO").unwrap();
        assert!(matches!(
            verify_tree(d.path()),
            Err(BulkTreeError::ListingMismatch(_))
        ));
        assert_eq!(read_sealed_root(d.path()).unwrap(), root);
    }

    #[test]
    fn swap_in_first_import_then_delta_then_idempotent() {
        let d = tempfile::tempdir().unwrap();
        let import = d.path();
        let s1 = "1".repeat(64);
        let s2 = "2".repeat(64);
        let st1 = staging_path(import, &s1);
        write(&st1.join("f"), b"one");
        let r1 = seal_tree(&st1).unwrap();
        let target = import.join("osm/planet");
        assert_eq!(
            swap_in(&st1, &target, &retired_path(import, &s1), &r1, &EMPTY_ROOT).unwrap(),
            SwapOutcome::Applied
        );
        assert_eq!(read_sealed_root(&target).unwrap(), r1);
        assert_eq!(
            swap_in(&st1, &target, &retired_path(import, &s1), &r1, &EMPTY_ROOT).unwrap(),
            SwapOutcome::AlreadyApplied
        );
        let st2 = staging_path(import, &s2);
        write(&st2.join("f"), b"two");
        let r2 = seal_tree(&st2).unwrap();
        assert!(matches!(
            swap_in(&st2, &target, &retired_path(import, &s2), &r2, &EMPTY_ROOT),
            Err(BulkTreeError::BaseMismatch { .. })
        ));
        assert_eq!(
            swap_in(&st2, &target, &retired_path(import, &s2), &r2, &r1).unwrap(),
            SwapOutcome::Applied
        );
        assert_eq!(read_sealed_root(&target).unwrap(), r2);
        assert_eq!(read_sealed_root(&retired_path(import, &s2)).unwrap(), r1);
    }

    #[test]
    fn swap_in_rejects_wrong_expected_root() {
        let d = tempfile::tempdir().unwrap();
        let s = "3".repeat(64);
        let st = staging_path(d.path(), &s);
        write(&st.join("f"), b"x");
        seal_tree(&st).unwrap();
        let r = swap_in(
            &st,
            &d.path().join("ns"),
            &retired_path(d.path(), &s),
            &[9u8; 32],
            &EMPTY_ROOT,
        );
        assert!(matches!(r, Err(BulkTreeError::RootMismatch { .. })));
        assert!(st.exists());
    }

    #[test]
    fn scan_rejects_symlinks() {
        let d = tempfile::tempdir().unwrap();
        write(&d.path().join("real"), b"x");
        std::os::unix::fs::symlink(d.path().join("real"), d.path().join("link")).unwrap();
        assert!(matches!(
            scan_tree(d.path()),
            Err(BulkTreeError::Symlink(_))
        ));
    }

    #[test]
    fn names_and_ids_validate() {
        assert!(validate_namespace("osm/planet").is_ok());
        assert!(validate_namespace("osm/../x").is_err());
        assert!(validate_namespace(".bulk/x").is_err());
        assert!(validate_namespace("a/b/c/d/e/f/g/h/i").is_err());
        assert!(validate_stage_id(&"a".repeat(64)).is_ok());
        assert!(validate_stage_id(&"A".repeat(64)).is_err());
        assert!(validate_stage_id("abc").is_err());
    }
}
