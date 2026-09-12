# Native launch admission handoff

## Result

The selected native admission cycle has matched production and formal RED/GREEN evidence.
The native-query fixture and the native controller-loss fixture remain byte-identical to commit `dd1043c70addf40d436500216a89e157080938d1`.
The final integrated runtime snapshot matches the current eight runtime files at the handoff check.
The integration owner must recheck those identities before packaging or combined verification.

The raw evidence root is `[EVIDENCE_ROOT]`.
The pointer is `/tmp/soak-d2-native-admission-current-path`.
No commit or push was made by this session.
Other actors changed the index during this cycle.

## Candidate

| File | Final SHA-256 |
| --- | --- |
| `scripts/bench/soak-containment.py` | `35399004d7ac8b9eab14db2fd9f9d1514bd46fbd8993ce530ef3ea94ac69d0c6` |
| `scripts/bench/run-soak-contained.sh` | `6c62dcc2184d54112acc55112d58f74bda2eaf39ddff57b8040f27a03c5b2210` |
| B46 `scripts/run-merge-recovery-soak.sh` | `08e0fa0c07cb8cf4ec3b326052e65de31421b335b9ad7e5ef08dc8499cb69ce7` |
| Unchanged `scripts/bench/test-soak-native-admission.py` | `0780357e1ef2bfd5a0bf250aed9fa558ccaf615679c4221c62110f03965a17c8` |

`frozen-files.sha256` identifies the committed baseline.
`final-source.sha256` identifies the final launcher with the committed B45 driver.
`final-integrated-source.sha256` identifies the final launcher with the frozen B46 driver.
`formal-input.sha256` identifies the seven finite model and configuration files.
The model note is `formal/tlaplus/soak_disk/NativeLaunchAdmission.md`.

## Production records

| Stage | Exit | Classification |
| --- | --- | --- |
| `red` | 1 | The query stalls, the launcher refuses with exit 2, but one iteration was admitted. |
| `green` | 0 | The first corrected launcher prevents admission with the unchanged fixture. |
| `native-regression` | 0 | Positive B45 admission and native controller-loss regression. |
| `reviewed-green` | 0 | The integer-parsing refactor preserves query refusal. |
| `reviewed-native-regression` | 0 | The refactored launcher preserves the B45 native regression. |
| `integrated-green` | 0 | The first B46 integration prevents admission during the query fault. |
| `integrated-native-regression` | 2 | Setup failure before admission because mode 0711 prevents B46 directory reads. |
| `final-green` | 0 | The final launcher and committed B45 driver prevent admission. |
| `final-native-regression` | 0 | The final launcher and committed B45 driver pass the native regression. |
| `final-integrated-green` | 0 | The final launcher and frozen B46 driver prevent admission. |
| `final-integrated-native-regression` | 0 | The final launcher and frozen B46 driver pass the native regression. |

The final launcher uses mode 0755 for the control directory.
The environment, gate identity, and release files remain private to root.
`integrated-permission-diagnostic.txt` confirms the earlier directory read failure against the actual control directory.
This integration failure is not the early-admission behavioral RED.

Each successful native regression confirms both controller deaths and owned-writer termination before fixture cleanup.
Each successful native regression preserves the unrelated writer and two normal recovery summaries with counters `[1,1,0,0]`.
The query observations preserve launcher refusal status 2 and unrelated-writer progress before cleanup.

## Formal records

- `formal-red.txt`: exit 12, exact `UnavailableQueryPreventsNativeAdmission` violation, three distinct states.
- `MC_NativeLaunchAdmission.txt`: exit 0, three distinct states.
- `MC_NativeLaunchAdmission_available.txt`: exit 0, four distinct states.

No shared `/tmp/tlc-*` output was used.
These finite results do not establish an implementation theorem or a wall-clock bound.
Construction is not applicable, and no correctness claim is discharged.

## Retrieval and termination

`native-admission-results.tar.gz` contains 524 selected regular members from 11 invocations and their control records.
Its SHA-256 is `f66bb3afe8b6e626519e40d4b33ac643790ff5063a1fe69a60bacb13d52683e9`.
Its size is 412,882 bytes, and its expanded payload is 1,383,365 bytes.
Five special entries were excluded, and no private environment file was present in the selected input at retrieval.
The archive is not a complete filesystem image.

`verify-retrieval.rb` verifies the archive identity, member identities, observed exits, fault observations, and recovery summaries.
`retrieval-audit.txt` records the successful audit.
`runner-results/` contains the validated extraction.

The runner is `ci-eph-f1r3node-rust-amd64-d2-20260912-032924-b21b6e`.
The identity-checked termination operation completed after successful retrieval and audit.
`termination.txt` and `runner/runner-final/state.txt` record the observed `TERMINATED` state.
The earlier provisioning record describes the pre-test state only.

## Checks and remaining work

Python, embedded gate, and shell syntax checks pass.
The ordinary-user fixture and gate refuse work, and invalid UID text returns launcher status 2.
The final five-path diagnostic recheck confirms all five paths clean after an earlier inconclusive probe.
The four initial integer-call findings were reviewed false positives, followed by an explicitly handled parsing refactor and fresh runtime checks.
The scoped STE Check and whitespace check pass.
Human STE Review and maintainer review remain pending.

The integration owner retains the TDD plan, shared gate registrations, combined verification, evidence packaging, and inventory updates.
Register the exact native control only with its matching positive configurations and retained source identities.
Keep combined reruns digest-only and preserve all prior execution identities.
Do not register the original direct-launch B44 model as an accepted control.

The root launcher requires a trusted administrative startup context.
Adversarial release access, every metadata failure, failed stops, and observer-loss windows still need separate fault evidence.
Private Docker containment, pending creation, creation fencing, durable publication, aggregate deadlines, and D3 reserve bounds remain incomplete.
Normal workflow integration, hosted enforcement, claim ratification, and acceptance remain pending.
The workload, finalization semantics, 45-second wait, and harness pins remain unchanged by this cycle.
