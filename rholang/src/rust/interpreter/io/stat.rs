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

/// Kinds we recognize in `stat` / `entries`.  Non-regular non-
/// directory entries fold into `"other"` — a `stat` call on them
/// succeeds (informational), but `open` on them fails with
/// `FSERR_UNSUPPORTED`.
///
/// **M-07 (2026-09-04):** the enum has no `Symlink` variant.
/// Symlinks are excluded at boot by `Fs::validate_root_tree`'s
/// `is_symlink()` walk (§Static provisioning in the FIP), and the
/// spec treats their absence as an invariant.  A symlink that
/// somehow appears post-boot (e.g., under Oracular with an
/// external mutator ignoring the spec) folds through the `Other`
/// arm in `from_meta` below.  There is no code path that emits
/// `"symlink"` on the wire.
#[derive(Clone, Copy)]
pub enum Kind {
    File,
    Directory,
    Other,
}

impl Kind {
    pub fn from_meta(m: &Metadata) -> Self {
        let ft = m.file_type();
        if ft.is_file() {
            Kind::File
        } else if ft.is_dir() {
            Kind::Directory
        } else {
            // M-07 (2026-09-04): symlinks fold to Other.  See the
            // `Kind` docstring above — the enum has no Symlink
            // variant by design; the boot-time walk enforces
            // absence, and this arm is defense-in-depth for
            // Oracular's external-mutator threat model.
            Kind::Other
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::File => "file",
            Kind::Directory => "directory",
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
    let kind = Kind::from_meta(meta);
    pairs.push(kv("kind", RhoString::create_par(kind.as_str().to_string())));
    // M-15 (2026-09-04): spec §Dir > Listing declares `size: u64,
    // bytes for regular files; omitted otherwise`.  Only push the
    // `size` field when the kind is `File`.  For directories,
    // `Metadata::len()` returns platform-specific values (~4096 on
    // ext4/apfs); pushing it would let a Rholang caller doing
    // `if entry.get("size") != Nil { ... regular-file logic ... }`
    // incorrectly branch into regular-file logic on a Dir record.
    // Consensus-observable — the WAL entry's payload_ref hash rolls
    // for every fs_stat / fs_entries call on a non-regular entry.
    // Hard-fork-free per the f1r3node_no_running_network invariant.
    if matches!(kind, Kind::File) {
        pairs.push(kv("size", RhoNumber::create_par(meta.len() as i64)));
    }
    // H-26-F1 review fix: under Consensus, mask to permission bits only
    // (`& 0o0777`) — drop setuid/setgid/sticky (`& 0o7000`).  Those
    // high bits can vary across validator hosts (umask, install(1),
    // overlayfs) and would otherwise cause silent divergence on
    // otherwise-identical file content.  Oracular keeps the full 12
    // bits for host-level ergonomics.
    let mode_mask = match mode_kind {
        ConsensusMode::Consensus => 0o0777,
        ConsensusMode::Oracular => 0o7777,
    };
    pairs.push(kv(
        "mode",
        RhoNumber::create_par((unix_mode_bits(meta) & mode_mask) as i64),
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
    // L-26-F1 review fix: `as_millis()` returns u128; a naive `as i64`
    // wraps at ~292 million years, but a far-future stat call would
    // still silently produce a nonsense negative timestamp on wrap.
    // Saturate at `i64::MAX` instead so any out-of-range timestamp is
    // at least monotonic.  Only reached under Oracular (times are
    // omitted under Consensus per H-26-F1); no consensus impact.
    t.ok()
        .and_then(|st| st.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
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

// ---------------------------------------------------------------------
// Slice 26 review-fix tests (H-26-F1, ST-26-13).
// ---------------------------------------------------------------------
#[cfg(test)]
#[cfg(unix)]
mod stat_record_tests {
    use models::rhoapi::expr::ExprInstance;

    use super::*;

    fn expect_map_keys(par: &Par) -> std::collections::BTreeSet<String> {
        let expr = par
            .exprs
            .first()
            .expect("stat_record must produce Par with expr");
        let map = match &expr.expr_instance {
            Some(ExprInstance::EMapBody(m)) => m,
            other => panic!("expected EMap, got {other:?}"),
        };
        map.kvs
            .iter()
            .filter_map(|kv| {
                let key_par = kv.key.as_ref()?;
                let key_expr = key_par.exprs.first()?;
                match &key_expr.expr_instance {
                    Some(ExprInstance::GString(s)) => Some(s.clone()),
                    _ => None,
                }
            })
            .collect()
    }

    fn expect_mode_bits(par: &Par) -> i64 {
        let expr = par.exprs.first().expect("stat_record must produce Par");
        let map = match &expr.expr_instance {
            Some(ExprInstance::EMapBody(m)) => m,
            _ => panic!(),
        };
        for kv in &map.kvs {
            let key = kv.key.as_ref().unwrap();
            let key_str = match &key.exprs.first().unwrap().expr_instance {
                Some(ExprInstance::GString(s)) => s.as_str(),
                _ => continue,
            };
            if key_str == "mode" {
                let val = kv.value.as_ref().unwrap();
                let val_expr = val.exprs.first().unwrap();
                if let Some(ExprInstance::GInt(v)) = &val_expr.expr_instance {
                    return *v;
                }
            }
        }
        panic!("mode key not found");
    }

    // Use a real file (via tempfile) to get a real Metadata.  Set
    // setuid/setgid/sticky bits via chmod so the mask test has
    // something to strip.
    fn make_meta_with_mode(bits: u32) -> std::fs::Metadata {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f");
        std::fs::write(&path, b"x").unwrap();
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_mode(bits);
        std::fs::set_permissions(&path, perms).unwrap();
        // Leak the tempdir so metadata stays valid for the test lifetime.
        std::mem::forget(dir);
        std::fs::metadata(&path).unwrap()
    }

    // H-26-F1: under Consensus, setuid/setgid/sticky bits must be
    // stripped — those bits can vary across validator hosts (umask,
    // install(1), overlayfs) and would silently fork the network.
    #[test]
    fn consensus_mode_strips_setuid_setgid_sticky_bits() {
        // 0o4755 = setuid + rwxr-xr-x.  Consensus should mask to 0o755.
        let meta = make_meta_with_mode(0o4755);
        let rec = stat_record("f", &meta, ConsensusMode::Consensus);
        let mode = expect_mode_bits(&rec);
        assert_eq!(
            mode & 0o7000,
            0,
            "consensus must strip setuid/setgid/sticky; got {mode:o}"
        );
        assert_eq!(
            mode & 0o0777,
            0o0755,
            "consensus must preserve perm bits; got {mode:o}"
        );
    }

    // Under Oracular, host-level ergonomics are preserved — setuid
    // etc. DO show up.
    #[test]
    fn oracular_mode_preserves_setuid_setgid_sticky_bits() {
        let meta = make_meta_with_mode(0o4755);
        let rec = stat_record("f", &meta, ConsensusMode::Oracular);
        let mode = expect_mode_bits(&rec);
        assert_eq!(
            mode & 0o7000,
            0o4000,
            "oracular must preserve setuid; got {mode:o}"
        );
    }

    // MT-26-12 shard: pin the field-omission behavior at the unit
    // level.  Consensus record must have exactly {name, kind, size,
    // mode}; Oracular adds mtime/ctime/atime/owner/group.
    #[test]
    fn consensus_mode_omits_host_transient_fields() {
        let meta = make_meta_with_mode(0o0644);
        let rec = stat_record("f", &meta, ConsensusMode::Consensus);
        let keys = expect_map_keys(&rec);
        for k in ["mtime", "ctime", "atime", "owner", "group"] {
            assert!(
                !keys.contains(k),
                "consensus record leaked host-transient key `{k}`; got {keys:?}"
            );
        }
        for k in ["name", "kind", "size", "mode"] {
            assert!(
                keys.contains(k),
                "consensus record missing `{k}`; got {keys:?}"
            );
        }
    }

    /// M-15 (2026-09-04): spec §Dir > Listing declares
    /// `size: u64, bytes for regular files; omitted otherwise`.
    /// Directory records must NOT carry the `size` field —
    /// `Metadata::len()` returns platform-specific values (~4096
    /// on ext4/apfs) that would leak host state and let a Rholang
    /// caller writing `if entry.get("size") != Nil { … regular
    /// file logic … }` incorrectly branch on a Dir record.
    #[test]
    fn directory_record_omits_size_field() {
        let dir = tempfile::tempdir().unwrap();
        let dir_meta = std::fs::metadata(dir.path()).unwrap();
        for mode in [ConsensusMode::Consensus, ConsensusMode::Oracular] {
            let rec = stat_record("d", &dir_meta, mode);
            let keys = expect_map_keys(&rec);
            assert!(
                !keys.contains("size"),
                "M-15: directory record must omit `size` (spec §Dir > \
                 Listing: `bytes for regular files; omitted otherwise`); \
                 got {keys:?} under {mode:?}"
            );
        }
    }

    /// M-15 (2026-09-04) review follow-up (Gap 4): freeze the
    /// Consensus-mode directory record key set at exactly `{name,
    /// kind, mode}`.  Under Consensus, host-transient fields
    /// (`mtime`/`ctime`/`atime`/`owner`/`group`) are stripped and
    /// `size` is omitted for non-regular entries — so a directory
    /// record has three keys and only three keys.  A regression
    /// that added a new field (e.g. re-adding `size`, or leaking a
    /// per-host field like `dev`) would flip this assertion long
    /// before it reached Phase-5 verify at runtime.
    #[test]
    fn consensus_directory_record_key_set_is_frozen() {
        use std::collections::BTreeSet;
        let dir = tempfile::tempdir().unwrap();
        let dir_meta = std::fs::metadata(dir.path()).unwrap();
        let rec = stat_record("d", &dir_meta, ConsensusMode::Consensus);
        let keys: BTreeSet<String> = expect_map_keys(&rec);
        let expected: BTreeSet<String> = ["name", "kind", "mode"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            keys, expected,
            "M-15: Consensus dir record key set must be exactly \
             {{name, kind, mode}} — a regression adding a new key (or \
             re-adding `size`) is a Consensus surface change; catch it \
             here before the Phase-5 verify surface at runtime.  Prior \
             frozen shape: {{name, kind, mode}}; got {keys:?}"
        );
    }

    /// M-15 (2026-09-04) review follow-up (Gap 1): `Kind::Other`
    /// records (FIFOs, sockets, char/block devices) must also omit
    /// `size`.  Spec §Dir > Listing says `size` is present only for
    /// regular files; every non-regular kind — Directory and Other —
    /// must omit.  A regression that changed the gate from
    /// `matches!(kind, Kind::File)` to `matches!(kind, Kind::File |
    /// Kind::Other)` would still pass `directory_record_omits_size_
    /// field` (Directory case unchanged) but would leak size on
    /// Other; this pin catches that.
    #[test]
    fn other_kind_record_omits_size_field() {
        use std::os::unix::fs::FileTypeExt;
        let dir = tempfile::tempdir().unwrap();
        let fifo_path = dir.path().join("f.fifo");
        // Create a FIFO — the classic non-regular non-directory
        // entry.  `Metadata::len()` returns 0 for FIFOs; we don't
        // want that 0 leaking through the wire either.
        let fifo_c = std::ffi::CString::new(fifo_path.to_str().unwrap()).unwrap();
        // SAFETY: `fifo_c` is a locally-owned CString outliving this
        // block; its `.as_ptr()` returns a NUL-terminated
        // `*const c_char` valid for the call.  `libc::mkfifo` does
        // not retain the pointer.
        let rc = unsafe { libc::mkfifo(fifo_c.as_ptr(), 0o644) };
        assert_eq!(
            rc,
            0,
            "mkfifo failed; errno = {:?}",
            std::io::Error::last_os_error()
        );
        let meta = std::fs::symlink_metadata(&fifo_path).unwrap();
        assert!(
            meta.file_type().is_fifo(),
            "test setup: expected FIFO, got {:?}",
            meta.file_type()
        );
        // The FIFO folds through Kind::Other (not File, not Directory,
        // not the retired Symlink), and its stat record must omit size.
        for mode in [ConsensusMode::Consensus, ConsensusMode::Oracular] {
            let rec = stat_record("f.fifo", &meta, mode);
            let keys = expect_map_keys(&rec);
            assert!(
                !keys.contains("size"),
                "M-15: Kind::Other record (FIFO here) must omit `size`; \
                 got {keys:?} under {mode:?}"
            );
        }
    }

    // L-P7-1 (Phase 7 whole-review): pin the `kind` bundle to the
    // exact string values downstream Rho code branches on
    // (`file` / `directory` / `symlink` / `other`).  A rename
    // (`file` → `regularFile`, etc.) would silently break Dir.rho's
    // openFile stat-verify without any type error at the boundary.
    fn expect_kind_str(par: &Par) -> String {
        let expr = par.exprs.first().expect("stat_record must produce Par");
        let map = match &expr.expr_instance {
            Some(ExprInstance::EMapBody(m)) => m,
            _ => panic!(),
        };
        for kv in &map.kvs {
            let key = kv.key.as_ref().unwrap();
            let key_str = match &key.exprs.first().unwrap().expr_instance {
                Some(ExprInstance::GString(s)) => s.as_str(),
                _ => continue,
            };
            if key_str == "kind" {
                let val = kv.value.as_ref().unwrap();
                let val_expr = val.exprs.first().unwrap();
                if let Some(ExprInstance::GString(s)) = &val_expr.expr_instance {
                    return s.clone();
                }
            }
        }
        panic!("kind key not found");
    }

    #[test]
    fn stat_record_kind_bundle_pins_wire_strings() {
        // File.
        let file_meta = make_meta_with_mode(0o0644);
        let rec = stat_record("f", &file_meta, ConsensusMode::Consensus);
        assert_eq!(expect_kind_str(&rec), "file");

        // Directory.
        let dir = tempfile::tempdir().unwrap();
        let dir_meta = std::fs::metadata(dir.path()).unwrap();
        let rec = stat_record("d", &dir_meta, ConsensusMode::Consensus);
        assert_eq!(expect_kind_str(&rec), "directory");

        // M-07 (2026-09-04): symlinks fold to "other" instead of
        // producing a dedicated "symlink" wire string.  Symlinks
        // are excluded at boot by `Fs::validate_root_tree`; this
        // arm is defense-in-depth for Oracular's external-mutator
        // threat model.  A regression that reintroduces the
        // Symlink variant would surface here as "symlink" != "other".
        let sym_dir = tempfile::tempdir().unwrap();
        let target = sym_dir.path().join("target");
        std::fs::write(&target, b"x").unwrap();
        let link = sym_dir.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let sym_meta = std::fs::symlink_metadata(&link).unwrap();
        let rec = stat_record("link", &sym_meta, ConsensusMode::Consensus);
        assert_eq!(
            expect_kind_str(&rec),
            "other",
            "M-07: symlinks must fold to \"other\" (Kind enum has no \
             Symlink variant); a regression reintroducing the variant \
             would surface here as \"symlink\""
        );
    }

    #[test]
    fn oracular_mode_includes_host_transient_fields() {
        let meta = make_meta_with_mode(0o0644);
        let rec = stat_record("f", &meta, ConsensusMode::Oracular);
        let keys = expect_map_keys(&rec);
        // mtime is always available on Unix; atime and ctime may be
        // suppressed by mount options (`noatime`, `nodiratime`) so
        // only assert on `mtime`.
        assert!(
            keys.contains("mtime"),
            "oracular record missing mtime; got {keys:?}"
        );
    }
}
