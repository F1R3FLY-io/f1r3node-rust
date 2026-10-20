# TASK-019-4 Rust verification package

The verification tools now use Rust. This package replaces the Python tools and preserves the previous package as historical evidence.
Named maintainer acceptance and claim discharge remain pending.

| Check | Result |
| --- | --- |
| Positive bounded models | Both pass. |
| Negative model controls | All 16 produce the expected invariant violation and transition trace. |
| Isolated native Rust suites | All 65 tests pass. One peer helper remains ignored during direct invocation. |
| Generation rejection regression | Disabling rejection fails the expected assertion. The corrected source passes. |
| Strict Clippy | All targets pass for shared, block-storage, and node. |
| Source verification | All 4,141 archived files match the isolated volume. |
| Property bindings | All 23 properties map to passing tests. The checker verifies 1,130 current build inputs. |
| Gate regression | Classification, routing, and registration checks pass with 87 registered controls. |
| Checker regressions | All eight Rust checker tests pass. |
| Strict status audit | Exit 4. Eight mandatory records await acceptance. |

The [report](report.json) records the results, executable identities, retained failures, and evidence hashes.
The [source manifest](sources.sha256) identifies all 75 current claim, source, model, and verification inputs.
The [model scope](../../../../formal/tlaplus/node_observation/README.md) explains the bounds and trusted assumptions.
The [strict audit](strict-audit.json) records the pending gate result.

The source base is `4aa93d11cf7a4c318074975dd4266208575c8aca`.
The package also includes uncommitted test and verification changes. The base revision alone does not identify the reviewed inputs.

| Identity | SHA-256 |
| --- | --- |
| Source manifest | `a3454592619cf7c3792a64720f3f57766d9e7b8f30a46cc8dcf178917afb21f2` |
| Report | `dc7acd04b4da21aaac99b8a19d0bd56193a85a24a914e9bb09f0334892c6300c` |

## Evidence limits

The bindings connect reviewed source behavior, bounded invariants, and executable tests. They do not establish a machine-checked refinement of Rust.
Hash checks identify tested bytes. They do not establish semantic correctness by themselves.

The transaction-open race has model evidence and source inspection, but no deterministic executable injection.
The shutdown regression checks source ordering. It does not execute a complete production node shutdown.

The tests ran natively in a Linux container on ARM64. The interface executable contains 18,050,808 bytes and requires no stripping step.
These results do not qualify a production node, candidate image, live adapter, campaign, or soak.
The earlier correction and failure packages remain unchanged.

## Local reproduction

The retained node build inputs and logs are under `target/task-019-4-verification/`.
The Rust tool results are under `target/task-019-4-rust-verification/`.
The report records the tool images, isolated build scripts, source archive, compiler, and resource limits.
The build used a fresh project target and imported no host project objects.

Run the model checks from the repository root:

```sh
cargo test --locked --manifest-path scripts/node-observation/Cargo.toml
cargo clippy --locked --manifest-path scripts/node-observation/Cargo.toml --all-targets -- -D warnings
cargo run --locked --manifest-path scripts/node-observation/Cargo.toml -- models \
  --jar /path/to/tla2tools.jar \
  --output /path/to/new-model-output
```

Check the source bindings with the same tool executable:

```sh
cargo run --locked --manifest-path scripts/node-observation/Cargo.toml -- bindings \
  --isolated-manifest target/task-019-4-verification/isolated-inputs.sha256 \
  --rust-log target/task-019-4-verification/rebuild-final.log \
  --models /path/to/new-model-output/report.json \
  --output /path/to/new-binding-report.json
```

The Rust tool rechecks the retained node test results and source inputs. The node source did not change during this tooling correction.
The earlier 65-test run remains the node execution evidence. This correction does not claim a new node rebuild.
The report identifies archives of the previous tool sources and evidence records.

## Prepared acceptance request

This request is prepared for PR #447. It has not been posted.

Please review `CLAIM-CASPER-NODE-OBSERVATION-001` and `CLAIM-CASPER-NODE-OBSERVATION-002` against the exact package and source manifest identified above.
Please record acceptance or specific unresolved findings under an eligible maintainer identity.
The proposed reviewer is `jltatbeach`. Other eligible identities are `spreston8`, `dylon`, `metaweta`, and `jeffrey-l-turner`.

The [task acceptance rule](../../../ToDos.md) states: "Acceptance is recorded by a named maintainer. Passing tests alone do not discharge a claim."
Acceptance must name the reviewed revision, both claims, and this evidence package.
After the changes are committed, verify that the committed inputs match this manifest before recording the accepted revision.
Record the maintainer identity and acceptance reference, then update the claim records and rerun the strict gate.
