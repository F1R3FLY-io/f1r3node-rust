// `stat` record building.
//
// Two shapes:
//
//   Oracular: {name, kind, size, mode, mtime, ctime, atime, owner, group}
//   Consensus: {name, kind, size, mode}                                    // host-transient omitted
//
// The Consensus shape is what folllowers replay against — the omitted
// fields are the ones that differ across hosts of the same file (owner
// name because of NSS, atime because of any read since the leader saw
// it, etc.).

use std::fs::Metadata;
use std::path::Path;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EMap, Expr, KeyValuePair, Par};
use shared::rust::BitSet;

use super::super::rho_type::{RhoNumber, RhoString};
use super::nss::{gid_to_name, uid_to_name};
use super::ConsensusMode;

/// Kinds we recognize in `stat` / `entries`.  Non-regular non-directory
/// entries fold into `"other"` — a `stat` call on them succeeds
/// (informational), but `open` on them fails with `FSERR_UNSUPPORTED`.
#[derive(Clone, Copy)]
pub enum Kind {
    File,
    Directory,
    Symlink,
    Other,
}

impl Kind {
    pub fn from_meta(m: &Metadata) -> Self {
        let ft = m.file_type();
        if ft.is_file() {
            Kind::File
        } else if ft.is_dir() {
            Kind::Directory
        } else if ft.is_symlink() {
            Kind::Symlink
        } else {
            Kind::Other
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::File => "file",
            Kind::Directory => "directory",
            Kind::Symlink => "symlink",
            Kind::Other => "other",
        }
    }
}

fn kv(k: &str, v: Par) -> KeyValuePair {
    KeyValuePair {
        key: Some(RhoString::create_par(k.to_string())),
        value: Some(v),
    }
}

/// Build a `stat` record for the entry at `path` given its (already
/// fetched) metadata.  `name` is the basename shown in the record —
/// callers pass either the last path component or the full logical name.
pub fn stat_record(name: &str, meta: &Metadata, mode_kind: ConsensusMode) -> Par {
    let mut pairs: Vec<KeyValuePair> = Vec::with_capacity(9);
    pairs.push(kv("name", RhoString::create_par(name.to_string())));
    pairs.push(kv(
        "kind",
        RhoString::create_par(Kind::from_meta(meta).as_str().to_string()),
    ));
    pairs.push(kv("size", RhoNumber::create_par(meta.len() as i64)));
    pairs.push(kv(
        "mode",
        RhoNumber::create_par(unix_mode_bits(meta) as i64),
    ));

    if mode_kind == ConsensusMode::Oracular {
        if let Some(mtime) = meta_time_ms(meta.modified()) {
            pairs.push(kv("mtime", RhoNumber::create_par(mtime)));
        }
        if let Some(ctime) = meta_time_ms(meta.created()) {
            pairs.push(kv("ctime", RhoNumber::create_par(ctime)));
        }
        if let Some(atime) = meta_time_ms(meta.accessed()) {
            pairs.push(kv("atime", RhoNumber::create_par(atime)));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if let Some(name) = uid_to_name(meta.uid()) {
                pairs.push(kv("owner", RhoString::create_par(name)));
            }
            if let Some(name) = gid_to_name(meta.gid()) {
                pairs.push(kv("group", RhoString::create_par(name)));
            }
        }
    }

    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EMapBody(EMap {
            kvs: pairs,
            locally_free: BitSet::default(),
            connective_used: false,
            remainder: None,
        })),
    }])
}

/// Build a stat-record for an entry whose metadata retrieval failed.  Used
/// by `entries` to make a per-entry error a row rather than aborting the
/// listing.
pub fn error_record(name: &str, err: &str) -> Par {
    let pairs = vec![
        kv("name", RhoString::create_par(name.to_string())),
        kv("error", RhoString::create_par(err.to_string())),
    ];
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EMapBody(EMap {
            kvs: pairs,
            locally_free: BitSet::default(),
            connective_used: false,
            remainder: None,
        })),
    }])
}

fn meta_time_ms(t: std::io::Result<std::time::SystemTime>) -> Option<i64> {
    t.ok()
        .and_then(|st| st.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
}

fn unix_mode_bits(meta: &Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // Mask to the low 12 bits (permission + setuid/setgid/sticky).
        meta.mode() & 0o7777
    }
    #[cfg(not(unix))]
    {
        // Windows fallback: read-only → 0o444, else 0o644.
        if meta.permissions().readonly() {
            0o444
        } else {
            0o644
        }
    }
}

// Cover the `_path` unused-in-consensus arm without a warning.
#[allow(dead_code)]
fn _path_holder(_: &Path) {}

// Suppress unused-import warnings on non-unix.
#[cfg(not(unix))]
#[allow(dead_code)]
fn _no_nss(_uid: u32, _gid: u32) {
    let _ = uid_to_name;
    let _ = gid_to_name;
}
