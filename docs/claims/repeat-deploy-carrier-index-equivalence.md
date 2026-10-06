# Repeat-Deploy Carrier-Index Equivalence Claim

```yaml
claim_id: CLAIM-FINALITY-002
artifacts:
  - casper/src/rust/validate.rs
  - block-storage/src/rust/dag/carrier_index.rs
  - block-storage/src/rust/dag/block_dag_key_value_storage.rs
status: pending
adapter: agentic
mechanization: formal/tlaplus/carrier_index/CarrierIndex.tla
references:
  - https://github.com/F1R3FLY-io/f1r3node-rust/issues/24
  - https://github.com/F1R3FLY-io/f1r3node-rust/actions/runs/33707959088
  - docs/casper/design/cbc-repair-plan.md
  - docs/casper/CONSENSUS_PHILOSOPHY.md
```

## Context

`Validate::repeat_deploy` rejects a signature carried by a parent-scope ancestor inside the expiration window.

The carrier index can remove absent signatures from that scan. The index must not change the reference verdict.

Run 33707959088 used the PR #382 revision and still had six completed finalization failures.

The worst iteration measured 171.5 ms in `repeat_deploy`, 752.6 ms in `merge_call`, and 965 ms in replay.

This claim constrains the carrier fast path. It does not claim that the carrier index resolves issue #24.

## Reference predicate

```text
repeat(parents, sig, start) :=
  exists block in parent_scope(parents) :
    block.height >= start AND sig in block.body.deploys
```

The reference path returns `BlockNotHeld` when required ancestry or a required body is unavailable.

## Claim statements

### C1 — Complete carrier recording

Every DAG-visible block at or above watermark `W` has one carrier entry for each signature in `block.body.deploys`.

A carrier write must finish before the corresponding block becomes DAG-visible.

A failed carrier write must prevent DAG visibility. A completed carrier write followed by a failed DAG write is safe.

Redelivery must not create a second entry for the same signature and block hash.

### C2 — Sound absence proof

The fast path can use index absence only when `W <= max(start, 0)`.

Under that gate, an absent index row proves that the reference predicate is false for that signature.

An index hit is not a repeat verdict. The hit routes to the reference scope and window check.

### C3 — Read-failure refusal

A watermark read failure or carrier-row read failure gives no absence proof.

After either failure, validation must run the reference scan or return its typed unavailable-history result.

The implementation must not convert unreadable index state into acceptance.

### C4 — Retention safety

Pruning can remove only entries below every future scan start.

Pruning must preserve C2 for all later validation calls.

A stale or repeated prune request must not remove a required carrier.

### C5 — Verdict equivalence

For each generated DAG, candidate block, expiration window, and storage-availability pattern:

```text
indexed_verdict = reference_scan_verdict
```

The comparison includes valid, invalid, and approved carriers. It also includes forks, missing bodies, and missing ancestors.

### C6 — Absence-path work bound

For `S` distinct candidate signatures after retry exemptions:

```text
carrier_row_reads <= S
ancestor_metadata_visits = 0
ancestor_body_reads = 0
```

The bound applies after watermark engagement when every candidate signature is absent.

A hit can use the reference scan until a scope-aware replacement has a separate discharged claim.

### C7 — Diagnostic isolation

Counters, timers, and forced-path controls are node-local test and diagnostic inputs.

They must not change block validity, admission, parent selection, merge output, or finality.

## Formal model

`formal/tlaplus/carrier_index/CarrierIndex.tla` models carrier writes, DAG publication, crashes, read failures, and pruning.

The post-fix configuration requires index-first publication and fallback after read failure.

Two negative controls permit DAG-first publication or treat a read failure as absence. Each control must violate absence soundness.

The baseline configuration completed 222 distinct states with no error. It models carrier safety that production already has.

The DAG-first control violated `IndexCompleteForWindow`. The read-failure control violated `AbsenceProofSound`.

These results do not verify the residual issue #24 repair. The model proves the abstract carrier state machine only.

## Production bridge

The deterministic bridge has four layers:

1. Run a `proptest` operation trace against `CarrierIndex` and an independent reference map.
2. Generate bounded DAGs and compare forced-index results with forced-reference-scan results.
3. Fuzz insert, crash, restart, prune, read-failure, and validation sequences against the same oracle.
4. Run the fixed soak profile and record carrier, merge, replay, and finalization measurements.

Property tests and fuzzing are not proof authority. They connect the model to production code and search larger bounded input spaces.

`block-storage/tests/carrier_index_property_test.rs` now runs 512 generated operation traces against an independent reference map.

The test covers record idempotence, write-once watermark behavior, strided pruning, and absence results. This passing test is baseline evidence.

The first residual-repair property must target the measured work bound and fail current production for the expected reason. The complete validation differential remains open.

## Required generated cases

The DAG generator must vary these inputs:

- linear and multi-parent ancestry
- valid, invalid, and approved carriers
- a carrier inside and outside parent scope
- heights below, at, and above the expiration start
- watermark below, at, and above the scan start
- fresh signatures, repeated signatures, and repeated signatures on another fork
- missing ancestor metadata and missing block bodies
- index-row and watermark read failures
- crash after carrier write but before DAG publication
- pruning before and after the scan start advances
- insertion and parent-order permutations

The generator must retain failing seeds and minimize each failure.

## Performance and soak gate

Telemetry must report these counts for each validation:

- watermark gate engaged
- watermark gate not ready
- index absence
- index hit
- index read failure
- fallback scan
- ancestor metadata visited
- ancestor bodies read

The report must separate merge and replay sub-stage work.

A candidate repair must show which measured bound changed. Total stage time alone does not prove fast-path engagement.

Do not discharge this claim until forced-path differential tests pass and completed soaks have no finalization-limit failures.
