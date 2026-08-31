# RSpace channel-prefix publication experiment — 2026-08-31

## Question

Why did the exact 20,000-bind RSpace gate stop before candidate matching, and can channel loading
retain one global `PathMap<Multiplicity>` without paying for all previously loaded channels on each
`set_channel` call?

## Mechanism hypothesis

The predecessor cloned the complete `RSpaceDataStore`, cleared one channel on the clone, inserted
the replacement, and assigned the clone back. The guarded CoW root made the initial clone cheap, but
publishing it destroyed the previous complete root. Repeating that sequence while constructing
channels 1 through $`n`$ made channel $`i`$ pay for topology accumulated through $`i-1`$.

The treatment prepares a private channel-relative `Bag`, obtains one strictly prefix-rooted guarded
write cursor, removes only the target prefix when it was already loaded, and streams the prepared
relative endpoints into that prefix. Catalog publication and loaded-channel accounting occur only
after preparation succeeds. The cursor cannot address siblings or expose a raw `PathMap`, detached
subtree, graft, or whole-store replacement capability.

## Protocol

- Production treatment: `prefix-local`, 1,000–16,000 distinct singleton channels.
- Frozen causal control: `whole-store-clone`, 64–1,024 channels, using the predecessor's complete
  store clone/mutate/publish sequence against the current value semantics.
- Five trials per arm and size, pinned to CPU 2 under a systemd user scope with an 8 GiB resident
  hard cap and disabled swap.
- Timing excludes final store destruction; every observation validates exact distinct cardinality,
  total cardinality, and loaded-channel count.
- The exact release 20,000-bind gate was separately profiled before and after on CPU 2 with AMD
  uProf, a 4 GiB resident hard cap, and disabled swap.
- The successor was separately recorded with Heaptrack and analyzed in report-only mode. No GUI was
  opened.

The raw causal matrix is
[`rspace-channel-prefix-publication-2026-08-31.tsv`](rspace-channel-prefix-publication-2026-08-31.tsv).

## Results

| arm | channel ladder | median elapsed time | fitted log-log exponent |
|---|---:|---:|---:|
| prefix-local | 1,000 / 2,000 / 4,000 / 8,000 / 16,000 | 36.076 / 73.064 / 145.383 / 294.479 / 580.205 ms | 1.00258 |
| whole-store-clone | 64 / 128 / 256 / 512 / 1,024 | 87.452 / 346.858 / 1,391.681 / 5,541.807 / 21,846.408 ms | 1.99273 |

Every observation preserved the exact singleton distinct/cardinality/loaded triple. The treatment
therefore removes the measured quadratic construction mechanism and exhibits an approximately
linear channel-loading curve over the registered ladder.

The exact release gate passes at 20,000 binds in 7.52 s with 151,408 KiB peak RSS and zero swap.
Before the repair, AMD uProf attributed 99.90% inclusive CPU time to `set_channel`,
`InMemHotStoreState::from_snapshot`, and guarded `AnchorRoot::drop`. In the successor, `set_channel`
and `from_snapshot` are absent from the 100 hottest functions (the report's 0.14% sampling floor).

Report-only Heaptrack recorded 33,318,622 allocation calls, 376,232 temporary allocations, and 69
reported leaks for the complete successor process. The prefix-local publication appears only as
linear construction work. The single final guarded Bag teardown remains prominent: PathMap
line-list split/drop paths account for 23,208,116 allocation calls and final-coordinate AMD uProf
measures `AnchorRoot::drop` at 0.69% self / 63.90% inclusive sampled CPU time. This is a distinct,
once-per-root teardown cost exposed
after removing the per-channel repetition; it is not evidence that the old quadratic publication
remains.

## Equivalence and failure atomicity

The focused executable suite covers randomized replacement against an independently rebuilt Bag,
empty loaded channels, target/sibling isolation, target-only catalog invalidation, old immutable
catalog snapshots, prepared-iterator failure, cloned-store isolation, unique root reuse, and a
source gate forbidding `self.clone()` in `set_channel`. Exact cardinalities and target contents agree.

The Rocq transaction model proves prefix confinement, exact target replacement, non-target
preservation, validation-before-publication, loaded-marker uniqueness/count bounds, target-only
cache invalidation, old-snapshot validity, cardinality formulas, and legacy success-result
equivalence. Its exact catalog contains 27 invariants and 20 bound causal mutants covering global
replacement, sibling mutation, early publication, global cache clearing, duplicate loaded counts,
and metric drift. Main and mutant `coqc` plus independent `coqchk -silent` pass under a 768 MiB,
zero-swap cap. The exhaustive formal-to-executable mapping is
[`../../pathmap/rspace-channel-prefix-replacement-invariant-catalog-2026-08-31.md`](../../pathmap/rspace-channel-prefix-replacement-invariant-catalog-2026-08-31.md).

## Artifact identity and limitations

- Raw TSV SHA-256: `eef41c4b8528abd062e2d2d955e6821381bcbd5127518c09a428e4c9d4f8aeb3`.
- Baseline AMD uProf CSV SHA-256:
  `7e5c35cd8b0f143e391c992d0e8900b9e984f730cafe6336e2754462e33b29e2`.
- Final-code successor AMD uProf CSV SHA-256:
  `a99fb2966da779be0be57fcd4acf6e85b45bf89dd5a11ed84156c4801a0638aa`.
- Successor Heaptrack recording SHA-256:
  `44cf88802d8c4fe9f93d43bcba515aeb01182b4da9d336215084c2382d1f4105`.
- Profiled release executable SHA-256:
  `6f88023be6425f221ee794bb354e6d58b4e444784332a9f2dce94f62cb26248c`.

The arms use different channel ladders because the frozen causal control reaches 21.5 s by 1,024
channels. This experiment establishes the mechanism and asymptotic separation; it is not a claim
that the 1,024-channel control time can be divided into a hardware-independent speedup at 16,000.
AMD uProf is sampling-based, and absence from its top-100 table means below the displayed sampling
floor, not zero cycles. Heaptrack's process-wide counts include setup, candidate traversal, and
final teardown. The exposed final-root teardown is a separate open optimization and must not be
silently folded into this accepted channel-publication result.
