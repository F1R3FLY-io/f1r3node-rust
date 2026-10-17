//! File I/O FIP static-provisioning replay-symmetry test.
//!
//! The ops-port chain (slices 1-9) ends here with the invariant that
//! justifies the whole thing: two validators booting from the same
//! operator-supplied HOCON config must produce byte-identical
//! Rholang source for the fs_generator deploy.  Byte-identical
//! source means byte-identical compiled bundles means byte-identical
//! genesis-block state means a joining validator's `validate_candidate`
//! deploy-diff passes.
//!
//! Covered pipeline (end-to-end at the Rust level, pre-Rholang-runtime):
//!
//!   HOCON config string
//!   -> `hocon::HoconLoader` resolve to `FileIoProvisioning`
//!   -> `provisioning_merge::merge_cli_into_config(cfg, [], [], [], [])`
//!      (ops-7c + ops-6e boot invariants)
//!   -> `provisioning_merge::project_bundle(&merged)`
//!      (ops-7a, routes through `BundleEntry::try_new`)
//!   -> `fs_genesis::format_bundle_for_rholang(&bundle)` on the
//!      casper side
//!
//! Boot the same HOCON string twice ("validator A" and "validator B"
//! in logical terms, same process in physical terms) and assert byte
//! equality at every stage.  HashMap iteration is randomized per
//! build, so without the ops-6c `flatten` sort + ops-7a projection
//! sort, the output would diverge across runs — this test is the
//! load-bearing cross-validator-boot invariant guard.
//!
//! Not covered (deferred to a future slice that wires in Rholang
//! runtime): compile the fs_generator deploy, run it on two fresh
//! RSpace instances, compare the resulting state roots.  The
//! byte-identical Rholang source emitted here is the deterministic
//! prerequisite for that compile-and-run step; if source diverges,
//! downstream compilation is guaranteed to diverge too.

use casper::rust::genesis::contracts::fs_genesis::format_bundle_for_rholang;
use node::rust::configuration::file_io_provisioning::FileIoProvisioning;
use node::rust::configuration::provisioning_merge::{merge_cli_into_config, project_bundle};

fn parse_hocon(text: &str) -> FileIoProvisioning {
    hocon::HoconLoader::new()
        .load_str(text)
        .expect("hocon load")
        .resolve()
        .expect("hocon resolve")
}

/// Boot "validator A" and "validator B" from the same HOCON config
/// string.  Each independently runs the full merge-validate-project
/// pipeline.  The merged provisioning, the projected bundle, and
/// the Rholang-emitted source must be byte-identical.
///
/// This is the replay-symmetry proof: a joining validator computing
/// its genesis against an operator-supplied config must land on the
/// same bytes a leader validator did, regardless of host-filesystem
/// state or HashMap iteration order.
#[test]
fn same_hocon_produces_byte_identical_bundle_across_boots() {
    let hocon = r#"
        oracle-static-files {
          "cfg/theme.json"    = "/etc/myapp/theme.json"
          "cfg/full.json"     = { path = "/etc/myapp/full.json", mode = "r+" }
        }
        oracle-static-dirs {
          "logs/"             = "/var/log/rnode"
        }
        consensus-static-files {
          "shared/state.json" = { path = "/srv/shared/state.json", mode = "r+" }
        }
        consensus-static-dirs {
          "shared/dir/"       = "/srv/shared/dir"
        }
    "#;

    let cfg_a = parse_hocon(hocon);
    let cfg_b = parse_hocon(hocon);

    let (merged_a, errors_a) =
        merge_cli_into_config(cfg_a, Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let (merged_b, errors_b) =
        merge_cli_into_config(cfg_b, Vec::new(), Vec::new(), Vec::new(), Vec::new());
    assert!(
        errors_a.is_empty(),
        "merge A unexpectedly errored: {errors_a:?}"
    );
    assert!(
        errors_b.is_empty(),
        "merge B unexpectedly errored: {errors_b:?}"
    );

    let bundle_a = project_bundle(&merged_a);
    let bundle_b = project_bundle(&merged_b);

    assert_eq!(
        bundle_a, bundle_b,
        "projected bundle must be byte-identical across validator boots"
    );

    let source_a = format_bundle_for_rholang(&bundle_a);
    let source_b = format_bundle_for_rholang(&bundle_b);

    assert_eq!(
        source_a, source_b,
        "format_bundle_for_rholang output must be byte-identical across validator boots"
    );
}

/// Three back-to-back boots from the same HOCON (within the same
/// process) must all land on the same bytes.  Catches any
/// HashMap-order leak that `same_hocon_produces_byte_identical_...`
/// might miss on a 2-run sample.  (A HashMap-order violation that
/// collides on run 1-vs-2 but surfaces on run 3 is unlikely but
/// pinned here as defense-in-depth.)
#[test]
fn three_consecutive_boots_land_on_identical_bytes() {
    let hocon = r#"
        oracle-static-files {
          "a" = "/x/a"
          "b" = "/x/b"
          "c" = "/x/c"
          "d" = "/x/d"
          "e" = "/x/e"
        }
        consensus-static-dirs {
          "f/" = "/x/f"
          "g/" = "/x/g"
          "h/" = "/x/h"
          "i/" = "/x/i"
          "j/" = "/x/j"
        }
    "#;

    let render = |cfg: FileIoProvisioning| -> String {
        let (merged, errors) =
            merge_cli_into_config(cfg, Vec::new(), Vec::new(), Vec::new(), Vec::new());
        assert!(errors.is_empty(), "merge errored: {errors:?}");
        format_bundle_for_rholang(&project_bundle(&merged))
    };
    let source_1 = render(parse_hocon(hocon));
    let source_2 = render(parse_hocon(hocon));
    let source_3 = render(parse_hocon(hocon));

    assert_eq!(source_1, source_2, "run 1 vs 2 must agree");
    assert_eq!(source_2, source_3, "run 2 vs 3 must agree");
}

/// An empty operator config (no `*-static-*` buckets) produces the
/// `{}` Rholang literal on every boot.  Pins the baseline shape
/// — a joining validator starting from `defaults.conf` with no
/// operator provisioning overlay lands on the same `{}` the leader
/// did.
#[test]
fn empty_config_produces_empty_rholang_map_consistently() {
    let hocon = r#""#;
    let cfg = parse_hocon(hocon);
    let (merged, errors) =
        merge_cli_into_config(cfg, Vec::new(), Vec::new(), Vec::new(), Vec::new());
    assert!(errors.is_empty(), "merge errored: {errors:?}");
    let source = format_bundle_for_rholang(&project_bundle(&merged));
    assert_eq!(source, "{}", "empty provisioning must render as `{{}}`");
}

/// HOCON surface accepts `--oracle-static-dir`-style ordering
/// variations (map keys in different declaration order within the
/// same HOCON text) and still produces the same bundle.  Operator
/// readability shouldn't affect validator-byte-identity.
#[test]
fn hocon_key_order_does_not_affect_bundle_bytes() {
    let hocon_order_1 = r#"
        oracle-static-files {
          "a" = "/x/a"
          "b" = "/x/b"
          "c" = "/x/c"
        }
    "#;
    let hocon_order_2 = r#"
        oracle-static-files {
          "c" = "/x/c"
          "a" = "/x/a"
          "b" = "/x/b"
        }
    "#;
    let hocon_order_3 = r#"
        oracle-static-files {
          "b" = "/x/b"
          "c" = "/x/c"
          "a" = "/x/a"
        }
    "#;

    let render = |hocon: &str| -> String {
        let cfg = parse_hocon(hocon);
        let (merged, errors) =
            merge_cli_into_config(cfg, Vec::new(), Vec::new(), Vec::new(), Vec::new());
        assert!(errors.is_empty(), "merge errored: {errors:?}");
        format_bundle_for_rholang(&project_bundle(&merged))
    };

    let s1 = render(hocon_order_1);
    let s2 = render(hocon_order_2);
    let s3 = render(hocon_order_3);

    assert_eq!(
        s1, s2,
        "HOCON order variation 1 vs 2 must land on same bytes"
    );
    assert_eq!(
        s2, s3,
        "HOCON order variation 2 vs 3 must land on same bytes"
    );
}

/// A config that would be rejected by boot validation (relative
/// path reaching the Genesis layer) returns `Err` with the
/// `NotAbsolute` diagnostic.  Pins that invalid provisioning
/// surfaces as validation errors, not silent empty bundles.
///
/// Operator-driven invariant: a validator refusing to proceed with
/// an invalid provisioning is strictly better than one silently
/// producing an empty-bundle genesis block that then diverges from
/// its peers.
/// A programmatically-constructed FileIoProvisioning with a
/// relative path bypasses the ops-6a HOCON serde validator, but
/// `merge_and_validate` runs the ops-6e boot validator which
/// catches it as `NotAbsolute`.  Pins that invalid provisioning
/// surfaces as a validation error, not a silent empty bundle.
///
/// Operator-driven invariant: a validator refusing to proceed with
/// an invalid provisioning is strictly better than one silently
/// producing an empty-bundle genesis block that then diverges from
/// its peers.
#[test]
fn invalid_provisioning_fails_merge_and_validate() {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use node::rust::configuration::file_io_provisioning::StaticFileEntry;
    use node::rust::configuration::provisioning_merge::merge_and_validate;

    let mut cfg = FileIoProvisioning {
        oracle_static_files: HashMap::new(),
        oracle_static_dirs: HashMap::new(),
        consensus_static_files: HashMap::new(),
        consensus_static_dirs: HashMap::new(),
    };
    cfg.oracle_static_files
        .insert("bad".into(), StaticFileEntry {
            path: PathBuf::from("relative/path"),
            mode: "r".into(),
        });

    let err = match merge_and_validate(cfg, Vec::new(), Vec::new(), Vec::new(), Vec::new()) {
        Err(e) => e,
        Ok(_) => panic!("relative path must fail boot validation"),
    };
    assert!(
        err.iter()
            .any(|e| format!("{e}").contains("not an absolute path")),
        "expected NotAbsolute diagnostic; got {err:?}"
    );
}
