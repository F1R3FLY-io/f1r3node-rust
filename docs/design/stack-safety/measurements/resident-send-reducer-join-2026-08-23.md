# Resident Send projection and proof-preserving reducer Join — 2026-08-23

This record preserves the exploratory measurements and acceptance gates for
`f1r3node-rust-mettail@1afd4791ebe0611824c003e44bf8a25cfbed9275`. It supports
stack-safety report §5.35, PathMap engineering journal J-016, consensus register entry 069,
pgmcp bugs `resident-send-protobuf-region-projection` and
`stable-reducer-identity-preserve-locally-free-proof`, and pgmcp experiment 311.

## Notation and abbreviations

| term | definition |
|---|---|
| CBR | Consensus-Breaking Register: the historical stable identifier prefix retained by the living consensus-change register. |
| RMP2 | Rholang Match Path version 2: the versioned semantic component-record format used only by the reconstructible candidate index. |
| SHA | Secure Hash Algorithm: `SHA-256` identifies the 256-bit artifact digest used below. |
| LLVM | The compiler-infrastructure project's retained name, historically “Low Level Virtual Machine”; the measured Rust compiler uses its version 22.1.0 backend. |
| CoW | Copy on write: immutable PathMap roots share storage until mutation requires a private root. |
| RSS | Resident set size: the process memory currently resident in physical memory. |
| RPB3 | The version-3 authoritative ParBag key grammar. |
| EPM1 | The version-1 compact EPathMap snapshot format. |

## 1. Question and classifications

The production deploy gate was already native-stack safe, but the unchanged 120-second child
watchdog exposed two depth-dependent heap/time defects:

1. Could a descriptor-generated protobuf-region projection decode each immediate child of an
   admitted resident `Send` once, instead of rebuilding every recursively shorter nested `Par`
   suffix?
2. Could the reducer preserve the structural locally-free proof and the CoW
   `ParBag` root of a stable environment binding, instead of semantically decoding and importing
   that binding at the parent `Join`?

The result labels used below are:

- **MEASURED** — an observed counter, time, RSS, or test result;
- **DERIVED** — a consequence of source structure or algebra;
- **PRELIMINARY** — a diagnostic observation outside experiment 311's registered 30-sample
  statistical decision; and
- **ACCEPTANCE** — an unchanged executable project gate.

The single-observation ladders are sufficient to falsify the former approximately quadratic
mechanisms and to verify that the selected path reaches depth 131,072. They are not reported as
experiment 311's preregistered throughput verdict.

## 2. Immutable coordinates and environment

| item | value |
|---|---|
| control source | `253d1754535ea73fd734f51bc872076e144b5348` |
| selected source | `1afd4791ebe0611824c003e44bf8a25cfbed9275` |
| control `deploy_depth_ceiling` SHA-256 | `eca07dec938d231bc7cb0292fcf826db317b259b29c0dbbb68b9b758a52cf5e8` |
| selected `deploy_depth_ceiling` SHA-256 | `8f523ab544abd6a82243f434fbf48af89c0167439adda080af018d973c926e72` |
| operating system | Linux 7.1.9-arch1-2, x86-64 |
| processor availability | 32 logical processors; heavy runs started only after `mpstat` showed sufficient idle capacity |
| Rust | `rustc 1.95.0-nightly (6efa357bf 2026-02-08)`, LLVM 22.1.0 |
| Cargo | `cargo 1.95.0-nightly (fe2f314ae 2026-01-30)` |
| allocation profiler | Heaptrack 1.5.0 and `heaptrack_print` 1.5.0 |
| resource policy | systemd user scopes, bounded `MemoryHigh`/`MemoryMax`, `MemorySwapMax=0` |
| deploy worker stack | 2,097,152 bytes |
| per-child watchdog | unchanged 120 seconds |

The allocation captures used Heaptrack strictly headlessly:

```bash
heaptrack --record-only -o /tmp/<capture>.heaptrack.zst \
  target/release/deps/deploy_depth_ceiling-<hash> --ignored --exact deploy_child
heaptrack_print /tmp/<capture>.heaptrack.zst > /tmp/<capture>.txt
```

No graphical Heaptrack process ran. No run enlarged `RUST_MIN_STACK`, used `stacker`, enabled swap,
or lowered a traversal depth.

## 3. Diagnostic ladder

Times are elapsed wall seconds and RSS is maximum resident set size in KiB. Each cell is one
PRELIMINARY `/usr/bin/time -v` observation. The control and treatment were separate immutable
release binaries; cross-binary ratios are attribution evidence, not a confidence interval.

### 3.1 `plain_deploy`: generated nested-`Send` reconstruction

| depth | control time | control RSS | region-projection time | region-projection RSS |
|---:|---:|---:|---:|---:|
| 8,192 | 2.91 s | 64,144 | 0.06 s | 50,524 |
| 16,384 | 10.79 s | 105,188 | 0.13 s | 81,248 |
| 32,768 | 44.21 s | 191,644 | 0.26 s | 144,620 |
| 131,072 | not run: the preceding ladder predicts a watchdog failure | not run | 1.06 s | 484,776 |

The control's time grows approximately fourfold for each twofold depth increase, the signature of
$`\Theta(d^2)`$ repeated-suffix reconstruction. The treatment grows approximately twofold and
reaches the unchanged cap, consistent with $`\Theta(d)`$ work in emitted bytes. RSS grows roughly
linearly in both arms; the repaired classification is therefore a heap/time slope, not a native-
stack slope.

### 3.2 `env_get_deploy`: proof loss at reducer re-entry

The resident-Send treatment exposed a second independent defect. Before the neutral-Join repair,
the environment-bound case measured 2.59 s / 70,740 KiB at depth 8,192 and 9.94 s / 153,316 KiB at
depth 16,384. Its depth-131,072 probe remained live after 146.9 seconds and was interrupted; it had
already exceeded the unchanged watchdog budget.

| depth | proof-preserving Join time | proof-preserving Join RSS |
|---:|---:|---:|
| 8,192 | 0.13 s | 74,996 KiB |
| 16,384 | 0.25 s | 147,660 KiB |
| 32,768 | 0.48 s | 280,440 KiB |
| 131,072 | 2.03 s | 869,920 KiB |

Time and RSS now grow approximately in proportion to encoded input. This remaining $`\Theta(d)`$
heap is the explicit representation of a depth-$`d`$ value and its iterative construction, not a
native call chain or a retained series of suffixes.

## 4. Headless allocation attribution

| subject, depth 8,192 | allocation calls | dominant attribution |
|---|---:|---|
| `plain_deploy`, control | 53,716,424 | 52,712,929 calls through generated `Send` child attachment, `ParBag::insert_bytes`, and PathMap payload insertion |
| `env_get_deploy`, after Send projection but before Join repair | 53,747,932 | 52,639,873 calls through `computed_locally_free_par`, generated `Expr` decode, child attachment, and PathMap insertion |
| `env_get_deploy`, selected Join | 963,802 | ordinary per-level substitution scoring/construction; the repeated semantic decoder/import stack is absent from the dominant allocation sites |

The final allocation-call reduction relative to the immediate pre-Join binary is:

```math
1 - \frac{963{,}802}{53{,}747{,}932} = 0.982067\ldots,
```

or **98.21%** at displayed precision. This is a whole-process counter, not a peak-live-byte claim.
The selected profile's most-called site is `ParConstruction::score_i64`, not
`computed_locally_free_par` or `attach_par_exprs`.

## 5. Why the selected algorithm is exact

The repair has two independent algebraic boundaries.

**Algorithm 1 (Resident Send projection).** This algorithm crosses the admission boundary once and
retains the generated schema as the only field-role authority.

```pseudocode
ALGORITHM ResidentSendProjection(endpoint)
Input:  one invariant-checked canonical RPB3 Send endpoint
Output: the same ordinary Send value as the complete generated decoder

1. Read channel/data field tags from the generated Send match program.
2. Cut immediate Par children with the generated protobuf-region PDA.
3. Decode the compact scalar shell with deferred EPathMap validation.
4. Decode each retained child region once through the established top-level Par importer.
5. Preserve child order, optional-channel presence, metadata, unknown fields, and errors.
6. On a noncanonical topology or schema near miss, invoke the complete generated decoder.
```

The result is an ordinary generated `Send`; downstream evaluation receives no alternate type.

**Algorithm 2 (Resident neutral Join).** This algorithm applies the native Bag zero law while
preserving the metadata equations that are outside the Bag itself.

```pseudocode
ALGORITHM ResidentNeutralJoin(shell, child)
Input:  one expression-free shell and one completed resident child
Output: the historical child.parallel(shell) value

1. Require exactly one child and an empty component Bag in the shell.
2. Move the child's resident PathMap root; do not enumerate or reinsert its endpoints.
3. Set locally_free to union(child.locally_free, shell.locally_free), preserving wire length.
4. Set connective_used to child.connective_used OR shell.connective_used.
5. Carry the child's structural locally-free proof unchanged because the empty shell contributes
   no component fold.
6. Use the complete ParConstruction path for every near miss.
```

Every unmet precondition returns control to the complete `ParConstruction` path.

DERIVED: `Par::parallel` already defines the Bag sum, root locally-free union, and connective OR.
The selected Join applies its identity law only when the shell's authoritative component Bag is
empty. It does not special-case `EVar`, flatten a PathMap, enumerate a `Vec<Par>`, or modify PathMap.

## 6. Semantic, stack, and workspace gates

| gate | result |
|---|---|
| resident-Send projection release suite | 5 passed; presence/order/unknown metadata, complete-decoder fallback, malformed EPathMap, general child shape, and depth 4,096 on 256 KiB |
| substitution identity release suite | 3 passed; CoW root retained with/without sort, variable near miss substitutes, broader process-only reducer identity rejected |
| evaluator recursive differential release module | 15 passed; exact values, protobuf/bincode, errors, charge traces, fixed-stack drives, and arbitrary terms |
| exact production deploy-depth gate | 1 passed in 3.14 s; `plain_deploy=131072`, `env_get_deploy=131072`, 2 MiB worker, unchanged watchdog |
| exact full release workspace command | 4,495 passed, 53 skipped, 0 failed in 138.379 s |

The workspace command of record was:

```bash
systemd-run --user --scope \
  -p MemoryHigh=20G -p MemoryMax=24G -p MemorySwapMax=0 \
  env RUST_BACKTRACE=1 cargo nextest run --release --no-fail-fast --workspace
```

The skipped tests are the workspace's declared skip set; none is a failure or counted as a pass.
The run includes the previously failing MakeMint, PoS, scratch-directory stress, release-depth,
and nextest-listing surfaces.

## 7. Artifact integrity

The raw captures are volatile `/tmp` files. Their checksums make accidental substitution visible;
this committed record and pgmcp artifacts 380–381 retain the durable summaries and regeneration
coordinates.

| artifact | bytes | SHA-256 |
|---|---:|---|
| `campaign-4131-plain-deploy-8192-baseline.heaptrack.zst` | 803,518 | `2cd9a416292ca469544a945e561dd79b12579a6d37e35b56b995b825dc8f55f9` |
| `campaign-4131-env-get-8192-treatment-direct.heaptrack.zst` | 669,301 | `1f47abe0847e27f8d274c01b76464b7945ca0c463df2302ef0786553eef26ff7` |
| `campaign-4131-env-get-8192-proof-join-post.heaptrack.zst` | 154,684 | `5d99abecc6a6d05b3035305b97976ba46bdb803b714bea0b516b53f24a9a7427` |
| `campaign-4131-deploy-depth-gate-proof-join.log` | 506 | `b980e66a40703e3d36b8d623d393130ea79bb8e95bdb334e40db7af3760d1689` |
| `campaign-4131-release-focused-bundle.log` | 12,120 | `b083f0fb58f57ca443d9facfec6771d0a87ac1bec836f1bfbc34f589bccd8073` |
| `campaign-4131-evaluator-differential-release.log` | 18,567 | `077114495a4110e059ce59b1853d66869253e506587a0d988faa41fc2426beb7` |
| `campaign-4131-full-release-nextest-1afd4791e.log` | 597,023 | `03f6c1092ddcb14489b0fb0ddcb3bce35dc663dfdf99db154d33d28f8cf0cdbe` |

## 8. Threats to validity and retained work

- The ladders have one observation per depth. They establish mechanism and asymptotic-shape
  attribution because the control separates by orders of magnitude, but they do not satisfy
  experiment 311's registered 30-sample Welch decision.
- Wall time can be biased by frequency and concurrent activity. Runs began only with ample idle CPU;
  the allocation attribution and unchanged exact gate are independent checks.
- Heaptrack changes timing and memory. Its allocation-call comparison is used only for mechanism;
  unprofiled `/usr/bin/time -v` supplies the resource ladder.
- The deepest fixture is a canonical unary list. Generated-schema differentials, general child
  shapes, arbitrary evaluator terms, and the full workspace suite limit overfitting, but they are
  not an exhaustive proof over arbitrary malformed protobuf bytes.
- Rocq/Verus bridge work for the broader RMP2 event projection does not automatically prove this
  `Send` importer or reducer identity law. The executable recursive differentials are the current
  concrete authority; any future formal claim must name its exact runtime correspondence boundary.

The statistical throughput verdict remains pending in pgmcp experiment 311. The defect repair and
stack/semantic acceptance are complete without depending on that optional magnitude estimate.
