# Cordial source import

Upstream: <https://github.com/iCog-Labs-Dev/cordial-f1r3node>

Source branch: `dev`

Source revision: `45de8b8ac85238f9b26563ed525c301c6e03a36d`

## Imported modules

| Upstream path | Local path |
| --- | --- |
| `crates/cordial-miners-core` | `cordial/cordial-miners-core` |
| `crates/cordial-app-runtime` | `cordial/cordial-app-runtime` |

The import retains native source, tests, benchmarks, documentation, and agent instructions.
The import excludes the prototype node factory and its stub adapters.
Upstream documentation describes the source repository, not completed integration in this node.
Relative links in retained upstream documentation can require the source repository.

No license file or package license declaration exists in these source directories at the recorded revision.
This import does not assign a license to upstream code.
Maintainers must confirm redistribution terms before publishing the imported code.

## Local changes

- Register the core and application runtime as target workspace members.
- Set package version `0.1.0` and edition `2024` explicitly in each imported manifest.
- Preserve Cordial's `rand` 0.8 dependency instead of inheriting the target's `rand` 0.9 dependency.
- Set the upstream `sha2` and `ed25519-dalek` requirements explicitly.
- Enable Serde derive explicitly instead of relying on the source workspace.
- Use the target workspace's pinned Rust toolchain and lockfile.
- Use default Rust 2024 formatting under `cordial/` to retain upstream source formatting.

`upstream-files.json` records original Git blob IDs for all imported files.
The initial import changed manifests only. The subsequent approved native corrections are
recorded by exact SHA-256 and reason in `local-patches.json`; the original blob inventory is retained.
See [Native admission v2](ADMISSION-V2.md) for the incompatible signed-content format,
mandatory received-block checks, tests, and remaining production gates.

Three native files are intentionally patched: `crypto.rs`, `consensus/validation.rs`, and
`consensus/mod.rs`. Approval, finality, tau ordering, and PoR implementations are unchanged.
One upstream test helper in `tests/test_dissemination.rs` now explicitly disables signature
checking for its synthetic one-byte identities and unsigned blocks. Those structural fixtures
already disabled hashing; they previously depended on the empty-signature bypass. Production
received admission has no such flags and is covered by real-signed regressions.

Two additional imported files now contain executable doctest fixtures: `consensus/dissemination.rs` and `consensus/evidence.rs`.
Their algorithms are unchanged. All three previously ignored core doctests now execute.
The local `cordial-consensus` crate is new integration code, not imported upstream code.
See [the approved integration profile](INTEGRATION-PROFILE.md) for its scope and remaining production gates.

## Verification

Run these commands from the target repository:

```bash
rtk proxy python3 scripts/check_cordial_import.py
rtk cargo test --locked --release -p cordial-miners-core
rtk cargo test --locked --release -p cordial-app-runtime
```

The import does not enable Cordial in the production node.
Native test success does not prove process-level networking, execution, or recovery.

The added `check_integration_readiness` example checks three native admission regressions.
This example is a separate readiness gate, not an upstream test.

```bash
rtk cargo run --locked --release -p cordial-miners-core --example check_integration_readiness
```

## Initial import baseline, 2026-10-02

| Check | Result |
| --- | --- |
| Source inventory | 143 unchanged files and three adapted manifests |
| Workspace resolution and native dependency separation | Passed |
| Formatting for all three imported crates | Passed |
| Core tests | 440 passed, three upstream doctests ignored |
| PoR tests | 171 passed |
| Application runtime tests | Nine passed |
| Production admission readiness | Failed on all three checks |

The readiness example uses real Secp256k1 signatures for its valid control blocks.
It reproduces these results in the pinned implementation:

1. `ValidationConfig::strict()` permits a missing signature.
2. Two equal predecessor sets can produce different content hashes when distinct identities share a content hash.
3. Two concurrent blocks pass strict validation against the same initial view. Receiving one makes the other fail `NotCordial`.

Three additional executions reproduced all three results.
These failures are not caused by the workspace manifest changes.
At that baseline, the source hash inventory confirmed that the relevant native implementation was unchanged.

The user subsequently approved versioned native corrections. The receive-time regression now
uses four signed initial blocks and a valid previous-round quorum. The original single-root
fixture reproduced receiver-local tip dependence but did not satisfy the new quorum rule.

Do not use native test success as permission to enable network admission. The v2 domain changes
signed bytes for every object, not only equal-hash predecessor cases. No production selector
or chain-authenticated wire protocol is enabled by these corrections.

## Corrected native verification, 2026-10-02

| Check | Result |
| --- | --- |
| Provenance | 139 unchanged imported files, four recorded patches, three adapted manifests |
| Core suite | 449 passed, zero failed, three existing ignored doctests |
| PoR suite | 171 passed |
| Application runtime suite | Nine passed |
| Trace-enabled admission regressions | Nine passed |
| Admission readiness example | Passed: all three defect indicators are false |
| Native formatting and tracked diff whitespace | Passed |

The three native suites total 629 passing tests. The trace run repeats the nine added
admission regressions under a different feature configuration; it is not nine additional
distinct tests.

The first core run exposed ten synthetic dissemination-fixture failures after mandatory
signature verification was restored. The fixture-only configuration correction is recorded
above. The next full run passed those tests but could not bind sockets in 20 peer/node tests.
The final full run passed with local-network permission. No failing run was erased.

Evidence is retained in
[`cordial-v2-20261002`](../../consensus-network-test.dzTsey/evidence/cordial-v2-20261002/):
`results.json` records the initial run; `core-retry.json` records the sandbox-limited retry;
`core-network-approved.json` and `core-network-approved.log` record the complete green core run.
PoR, application, readiness, and trace logs are alongside them.

The Part A release-node binary is unchanged. Its latest official network run remains
122 passed and two resource-guard errors, not full network acceptance. See
[the Part A report](../docs/plans/consensus-runtime-part-a.md).

These results complete this native regression check, not Part B or production qualification.

## Fixed-profile integration verification, 2026-10-02

The user approved fixed membership and execution after native tau commitment for the initial integration.
The new `cordial-consensus` crate implements the authenticated packet, durable store, native output, and ingress lifecycle portions.
Rholang execution and production node registration remain incomplete.

| Check | Result |
| --- | --- |
| Provenance | 137 unchanged imported files, six recorded patches, three adapted manifests |
| Core suite, including executable doctests | 452 passed, zero ignored |
| PoR suite | 171 passed |
| Application runtime suite | Nine passed |
| New chain, store, ordering, and lifecycle tests | 20 passed |
| Shared runtime lifecycle tests | 15 passed |
| Combined suite | 667 passed, zero failed, zero ignored |

These counts do not include repeated focused test runs.
The consensus API crate compiles but contains no independent tests.

The first combined run stopped when the sandbox denied socket creation in 13 native node tests.
The full rerun passed after local-network permission was granted.
All logs remain in [`cordial-profile-20261002`](../../consensus-network-test.dzTsey/evidence/cordial-profile-20261002/).
`suites-final.log` records another complete passing run after the pre-queue packet-limit correction.
`results.json` records the command, counts, revisions, binary hash, and remaining gates.
The CI matrix now includes all four Cordial crates and checks imported source provenance.
The CI configuration is local and has not run on a remote service.

The latest official Casper network result remains 122 passed and two resource-guard errors.
The host still lacks sufficient available memory for a safe rerun of those load scenarios.
No limits were reduced and no unrelated applications were stopped.

## PoR removal and committed execution verification, 2026-10-02

This run excludes PoR. It verifies the active import and the new local execution code.

| Check | Result |
| --- | --- |
| Provenance | 99 unchanged imported files, six recorded patches, two adapted manifests |
| Core suite, including doctests | 452 passed |
| Application runtime | Nine passed |
| Chain, store, output, journal, and lifecycle | 23 passed |
| Real Rholang execution and failure boundaries | Three passed |
| Shared runtime | 15 passed |
| Focused Casper lifecycle regression | Six passed |
| Total distinct tests | 508 passed, zero failed, zero ignored |

The new journal persists committed execution receipts, state roots, deploy identities, and the next execution index.
The Rholang bridge executes committed output during runtime recovery and after packet admission.
The real VM test compares receipts after RSpace reopen, reverse-order ingress, and runtime recovery.
Separate tests check missing RSpace state and an incorrect execution chain, including an idle executor.
The final journal test exhausts the real LMDB map during receipt persistence.
It verifies rollback of the receipt, deploy index, and execution cursor, including after reopen.
The Casper regression includes real deploy, proposal, finalization, and recovery.

The first native run encountered 13 sandbox socket denials. The complete rerun passed with local-network permission.
Logs remain in [`cordial-no-por-execution-20261002`](../../consensus-network-test.dzTsey/evidence/cordial-no-por-execution-20261002/).
`native-suites.log` preserves the denied run. `native-suites-network-approved.log` and `rholang.log` record the passing suites.
`execution-journal-final.log` records the additional map-exhaustion test and repeats the two earlier journal tests.
`casper-regression.log` records all six passing Casper tests. `results.json` lists commands and counts without counting repeated tests twice.

The CI matrix excludes PoR and includes `cordial-rholang`. This local CI change has not run on a remote service.
Production node selection, transport, proposal, application APIs, and full Cordial network acceptance remain incomplete.
See [the integration profile](INTEGRATION-PROFILE.md) for execution limits and remaining acceptance requirements.

## Updating the source

PoR was removed from this integration at the user's request after the verification runs recorded above.
Its 39 imported files, workspace entry, lockfile package, CI entry, and active inventory entries are removed.
The remaining inventory has 99 unchanged files, six recorded patches, and two adapted manifests.
The earlier PoR test counts remain historical evidence, not current integration coverage.
Reintroduce PoR only after the upstream work is ready and a separate integration is approved.
Native equivocation evidence and fixed validator weights remain part of Cordial consensus.

Select a new upstream revision explicitly.
Review the upstream changes against the revision above.
Preserve and review local adaptations.
Update the inventory and this document.
Run native tests and both protocol acceptance suites before enabling an updated adapter.
