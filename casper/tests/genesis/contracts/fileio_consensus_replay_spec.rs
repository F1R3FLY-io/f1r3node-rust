//! Consensus-mode host-transient field-omission regressions.
//!
//! The plan's `fileio_consensus_replay_spec.rs` (line 775) has two
//! layers:
//!   1. **Field omission**: under `ConsensusMode::Consensus`, `stat`
//!      and `entries` replies omit host-transient fields (`mtime`,
//!      `atime`, `ctime`, `owner`, `group`) so leader and follower
//!      compute byte-identical records regardless of local host state.
//!   2. **Replay round-trip**: leader captures reply → follower replays
//!      → byte-identical.
//!
//! This spec covers layer 1 only — the mode-routing through Fs →
//! Dir constructor → `cmodeP` cell → native `fs_stat` (slice 26
//! plumbing).  Layer 2 is deferred until the leader/follower two-
//! runtime harness lands; the field omission is what makes byte-
//! identity POSSIBLE, so pinning it here is the load-bearing
//! regression guard.
//!
//! Complements the ~15 `resolve_cmode` unit tests + 4 `stat_record`
//! unit tests in `handlers.rs` by exercising the full pipeline
//! (BundleConsensusMode → format_bundle_for_rholang → Fs.openDir →
//! openDirImpl → Dir constructor → cmodeP cell → Dir.stat →
//! fs_stat native → stat_record with Consensus arm) end-to-end.

use std::collections::HashMap;

use casper::rust::genesis::contracts::fs_genesis::{
    BundleConsensusMode, BundleEntry, BundleEntryKind,
};
use casper::rust::genesis::contracts::{fs_genesis, standard_deploys};
use rholang::rust::build::compile_rholang_source::CompiledRholangSource;

use crate::genesis::contracts::GENESIS_TEST_TIMEOUT;
use crate::helper::rho_spec::RhoSpec;
use crate::util::genesis_builder::GenesisBuilder;

/// Bundle a Dir with a child file, in the given consensus mode.
/// Returns tempdir (keep alive), params, fs_uri.
fn bundle_dir_with_child(
    consensus_mode: BundleConsensusMode,
) -> (
    tempfile::TempDir,
    crate::util::genesis_builder::GenesisParameters,
    String,
) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(dir.path()).expect("canonicalize");
    // Seed a child file so Dir.stat("child.txt") has something to stat.
    std::fs::write(root.join("child.txt"), b"child").expect("seed child");

    let entry = BundleEntry::try_new(
        "shareddir".to_string(),
        root,
        BundleEntryKind::Dir,
        "r".to_string(),
        consensus_mode,
    )
    .expect("bundle entry construction");

    let mut params = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    params.2.fs_bundle = vec![entry];

    let fs_uri = fs_genesis::fs_genesis_uri(&standard_deploys::FS_GENERATOR_PUB_KEY);
    (dir, params, fs_uri)
}

/// Under Consensus, `Dir.stat("child.txt")` returns a record whose
/// `contains` reports FALSE for every host-transient key (mtime,
/// atime, ctime, owner, group).  Consensus record must be stable
/// under host state (leader/follower produce byte-identical replies).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stat_omits_host_transient_fields_under_consensus() {
    let (_dir, params, fs_uri) = bundle_dir_with_child(BundleConsensusMode::Consensus);

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_consensus_stat_omits_host_fields
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("Consensus stat omits mtime/atime/ctime/owner/group",
        *test_consensus_stat_omits_host_fields)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract test_consensus_stat_omits_host_fields(rhoSpec, _, ackCh) = {{
      for(@[true, dir] <- @fs!?("openDir", "shareddir", {{}})) {{
        for(@[true, rec] <- @dir!?("stat", "child.txt")) {{
          match [
            rec.contains("name"),   rec.contains("kind"),
            rec.contains("size"),   rec.contains("mode"),
            rec.contains("mtime"),  rec.contains("atime"),
            rec.contains("ctime"),  rec.contains("owner"),
            rec.contains("group")
          ] {{
            [true, true, true, true, false, false, false, false, false] => {{
              rhoSpec!("assert", (true, "==", true),
                "Consensus stat: 4 always-fields present, 5 host-transients absent",
                *ackCh)
            }}
            _ => {{
              rhoSpec!("assert", (rec, "==",
                "record with only [name, kind, size, mode]"),
                "Consensus stat: 4 always-fields present, 5 host-transients absent",
                *ackCh)
            }}
          }}
        }}
      }}
    }}
  }}
}}
"#
    );

    let compiled = CompiledRholangSource::new(
        test_source,
        HashMap::new(),
        "ConsensusStatOmissionSpec".to_string(),
    )
    .expect("compile consensus_stat_omission spec");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests()
        .await
        .expect("consensus_stat_omission spec failed");
}

/// Under Oracular, the same stat call INCLUDES the host-transient
/// fields.  This complementary test proves the mode routing is
/// actually branching on the bundle's declared consensus_mode — not
/// silently defaulting to one or the other.  If the field-omission
/// logic ever regresses to "always omit" or "always include", one
/// of the two tests fails.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stat_includes_host_transient_fields_under_oracular() {
    let (_dir, params, fs_uri) = bundle_dir_with_child(BundleConsensusMode::Oracular);

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_oracular_stat_includes_host_fields
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("Oracular stat includes mtime/atime/ctime/owner/group",
        *test_oracular_stat_includes_host_fields)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract test_oracular_stat_includes_host_fields(rhoSpec, _, ackCh) = {{
      for(@[true, dir] <- @fs!?("openDir", "shareddir", {{}})) {{
        for(@[true, rec] <- @dir!?("stat", "child.txt")) {{
          match [
            rec.contains("name"),   rec.contains("kind"),
            rec.contains("size"),   rec.contains("mode"),
            rec.contains("mtime"),  rec.contains("atime"),
            rec.contains("ctime"),  rec.contains("owner"),
            rec.contains("group")
          ] {{
            [true, true, true, true, true, true, true, true, true] => {{
              rhoSpec!("assert", (true, "==", true),
                "Oracular stat: all 9 fields present", *ackCh)
            }}
            _ => {{
              rhoSpec!("assert", (rec, "==",
                "record with [name, kind, size, mode, mtime, atime, ctime, owner, group]"),
                "Oracular stat: all 9 fields present", *ackCh)
            }}
          }}
        }}
      }}
    }}
  }}
}}
"#
    );

    let compiled = CompiledRholangSource::new(
        test_source,
        HashMap::new(),
        "OracularStatOmissionSpec".to_string(),
    )
    .expect("compile oracular_stat_omission spec");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests()
        .await
        .expect("oracular_stat_omission spec failed");
}
