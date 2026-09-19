# Carrier Index Profile

## Scope

This controlled-transcript profile implements TASK-017-10 and supports CLAIM-CASPER-SOAK-008. It does not run a node or prove carrier-index equivalence.

Live, post-merge, experimental-policy, and typed-identity requests remain blocked. A synthetic qualification cannot qualify a node interface.

## Inputs and generation

The `casper-carrier-index` binary provides `identity`, `run`, and `models` commands.

Each request binds exact manifest bytes, compiled sources, the executable, candidate identities, a seed, and three input artifacts.

The artifacts contain configuration, paired members, and expected results. Each qualified capability binds its provider, revisions, profile identity, executable, and identity domain.

Each pair has an index member and a reference member. Both members use identical candidate, DAG, deploy signature, scan window, availability, watermark, and retention inputs.

The raw identity is a user deploy signature. It is not a block signature, validator signature, or substitute for a typed envelope.

Generation retains each requested path and emits deterministic workload and fault requests. Blocked generation emits neither workloads nor fault requests.

## Collection and classification

The collector validates artifact hashes, transport identities, producer sequences, context, and observation deadlines. Repeated identical events count once.

Contradictory copies invalidate evidence, including copies that change deadlines or predecessor links. Quarantine cannot hide these contradictions within one event identity.

A snapshot records actual path engagement, carrier results, fallback reason, probe count, and ancestor-body-read count. Counts use canonical unsigned decimal strings.

Missing measurements retain null values and reasons. A valid observed zero remains distinct from missing data.

Fallback to reference traversal does not establish index engagement. The report withholds attributed work counters when the requested path was not observed.

Differential results require both paths and complete measurements. Independent mismatches against pinned expectations remain recorded when other measurements are absent or malformed.

Carrier sets retain block hashes and valid, invalid, or approved status. Set order does not matter. Duplicate block identities invalidate a result.

Fault receipts must identify the requested target, action, trigger, and dependencies. A requested fault or successful method return cannot establish observed fault coverage.

Restart requires a predecessor link, prior exit, and successor readiness. Restart and read-failure cases require a fault schedule for both members.

Snapshots must follow acknowledged faults. Dependent acknowledgments require increasing sequences from one producer and a shared clock.

Invalid evidence takes verdict precedence over product failure and incomplete coverage. Known product failures remain in the report regardless of verdict precedence.

Reports retain exact input bytes, raw observations, rejected sources, fault receipts, and source identities. Every synthetic report records zero node launches and a nonpassing soak.

## Bounds and assumptions

Each executable invocation handles two members, at most eight faults, 64 transport records, and 64 carrier blocks per result.

Each input file is limited to one MiB. Heights and counters use canonical unsigned 64-bit decimal strings. These are parser bounds, not node limits.

The filesystem helpers assume cooperating writers and the inherited filesystem contract. This profile does not prove durability or containment against a hostile local process.

The profile compares pinned outcomes. It does not calculate node reference scans, verify signatures, or infer which carriers should satisfy consensus rules.

## Finite model

The model explores two scenarios with three observation steps each. Each scenario abstracts one index/reference pair.

Matched inputs, actual engagement, and complete counters are Boolean abstractions. Observed zero represents a known measurement, not a proven work bound.

| Property | Executable fixture |
| --- | --- |
| `PathEngagementObserved` | `carrier_path_unobserved` requires an incomplete result and unknown attributed work. |
| `CarrierInputsMatched` | `carrier_window_mismatch` rejects mismatched paired inputs before collection. |
| `MissingCountersUnknown` | `carrier_counter_missing` preserves missing counts as null instead of zero. |

The clean configuration checks all three properties. Each unsafe configuration must produce TLC exit 12 with its registered property and a counterexample trace.

The abstraction assumes valid auxiliary fields and no independent product failure. Executable fixtures check additional evidence, restart, fault, and failure-retention cases.

This finite model does not establish unbounded behavior or node correctness. Construction is not applicable.

## Verification

Set `TLA_TOOLS_JAR` to the pinned TLC 1.7.4 JAR. Run this command with a new output directory:

```bash
bash scripts/casper-soak/check-carrier-index.sh target/carrier-index-check
```

The runner requires SHA-256 `936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88`. Set `SOAK_CARRIER_JAVA` to select Java.

The runner checks 23 Rust tests, an exact inventory of 88 cases and 91 invocations, and four model controls.

The runner retains failed and interrupted outcomes. A successful check requires the completed summary and exit record, not only some passing fixture records.

Binding acceptance, hosted workflow verification, evidence publication, and the proposed workflow tag remain separate review obligations. Passing local checks does not complete the task.
