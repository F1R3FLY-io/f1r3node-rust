//! Phase 6 review B-P6-2 fix: end-to-end genesis-through-runtime test
//! for the FsGenesis deploy.  Verifies (a) `fs_generator` executes at
//! genesis without error, (b) the resulting Fs cap is registered at
//! the URI derived from `FS_GENERATOR_PK`, (c) a user deploy can
//! resolve that URI and invoke `openFile` — receiving the expected
//! `FSERR_UNSUPPORTED` under the empty-bundle MVP.
//!
//! Mirrors `stack_spec.rs` / `list_ops_spec.rs` structure but
//! generates the `.rho` test source inline via `format!()` because
//! the target URI is a Blake2b256 hash of `FS_GENERATOR_PUB_KEY` and
//! isn't stable at .rho-file-authoring time.
//!
//! Complements the parse/normalize/signature/shape checks already in
//! `standard_deploys_spec.rs` — those cover deploy construction; this
//! covers deploy EXECUTION at genesis + user-scope lookup.

use std::collections::HashMap;

use casper::rust::genesis::contracts::{fs_genesis, standard_deploys};
use rholang::rust::build::compile_rholang_source::CompiledRholangSource;

use crate::genesis::contracts::GENESIS_TEST_TIMEOUT;
use crate::helper::rho_spec::RhoSpec;

#[tokio::test]
async fn fs_generator_spec() {
    // Derive the registry URI where fs_generator published the Fs cap.
    let fs_uri = fs_genesis::fs_genesis_uri(&standard_deploys::FS_GENERATOR_PUB_KEY);

    // Compose the RhoSpec test source: look up the Fs cap, attempt an
    // openFile on a non-existent logical name (empty-bundle MVP →
    // FSERR_UNSUPPORTED), assert the reply shape.
    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_lookup_returns_fs_cap,
  test_open_file_on_empty_bundle_yields_unsupported,
  test_stdout_returns_working_cap
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [
        ("fs cap is registered and resolvable", *test_lookup_returns_fs_cap),
        ("openFile on empty bundle yields FSERR_UNSUPPORTED",
         *test_open_file_on_empty_bundle_yields_unsupported),
        ("stdout returns a working Stdout cap",
         *test_stdout_returns_working_cap)
      ])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{

    // Test 1: the registry lookup returned a non-Nil value.  Trivially
    // true if fsCh received anything (RhoSpec harness fails on missing
    // register lookup).  Verify by asserting a String pattern equality
    // — we use a trivial always-true assertion because RhoSpec asserts
    // predicates, and the fact that `for(@(_, fs) &lt;- fsCh)` fired means
    // the URI resolved.
    contract test_lookup_returns_fs_cap(rhoSpec, _, ackCh) = {{
      rhoSpec!("assert", (true, "==", true),
        "Fs URI resolves to a cap (fsCh fired)", *ackCh)
    }} |

    // Test 2: openFile on a logical name not in the (empty) bundle
    // must return [false, "FSERR_UNSUPPORTED", ...].  Uses the agent-
    // dispatch `!?` sugar to avoid manual return-channel plumbing.
    contract test_open_file_on_empty_bundle_yields_unsupported(rhoSpec, _, ackCh) = {{
      for(@reply <- @fs!?("openFile", "nonexistent.txt", {{}})) {{
        match reply {{
          [false, "FSERR_UNSUPPORTED", _msg] => {{
            rhoSpec!("assert", (true, "==", true),
              "openFile on empty bundle → FSERR_UNSUPPORTED", *ackCh)
          }}
          _ => {{
            rhoSpec!("assert", (reply, "==", "FSERR_UNSUPPORTED tuple"),
              "openFile on empty bundle → FSERR_UNSUPPORTED", *ackCh)
          }}
        }}
      }}
    }} |

    // Test 3: stdout() must return [true, cap].  Under shared-Fs MVP
    // this cap wraps fd 1 for all deploys; per-principal delegation
    // is a future powerbox slice.
    contract test_stdout_returns_working_cap(rhoSpec, _, ackCh) = {{
      for(@reply <- @fs!?("stdout")) {{
        match reply {{
          [true, _cap] => {{
            rhoSpec!("assert", (true, "==", true),
              "fs.stdout() returns [true, cap]", *ackCh)
          }}
          _ => {{
            rhoSpec!("assert", (reply, "==", "[true, cap] shape"),
              "fs.stdout() returns [true, cap]", *ackCh)
          }}
        }}
      }}
    }}
  }}
}}
"#
    );

    let compiled =
        CompiledRholangSource::new(test_source, HashMap::new(), "FsGeneratorSpec".to_string())
            .expect("Failed to compile FsGenerator E2E test source");

    let spec = RhoSpec::new(compiled, vec![], GENESIS_TEST_TIMEOUT);
    spec.run_tests()
        .await
        .expect("FsGenerator E2E spec tests failed");
}
