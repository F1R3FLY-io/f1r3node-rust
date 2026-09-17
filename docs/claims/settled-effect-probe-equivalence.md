# Claim: settled-effect probe reference semantics and batched-walk equivalence

```yaml
claim_id: CLAIM-FINALITY-001
artifacts:
  - casper/src/rust/finality/deploy_lifecycle.rs    # effect_in_state_of reference walk
  - casper/src/rust/util/rholang/interpreter_util.rs # sig_settled_in_base / sig_settled_in_floor probe sites
  - casper/src/rust/merging/dag_merger.rs            # per-merge probe consumer (dedup + settled-content protection)
status: mechanized
adapter: agentic
mechanization: formal/rocq/finalized_floor/theories/SettledEffectProbe.v  # scripts/check-finalized-floor-ALL.sh
references:
  - https://github.com/F1R3FLY-io/f1r3node-rust/issues/24   # sustained finalization lag
  - GitHub Actions run 33099406770                          # attribution telemetry (2026-08-27)
  - docs/casper/theory/finalized-floor/finalized-floor-specification.md
```

## Context

The settled-effect probe decides whether a deploy sig's effect is already
committed on a state lineage. `dag_merger::merge` calls it from two sites.
`sig_settled_in_base` protects the merge against re-applying content the
base already holds. `sig_settled_in_floor` protects settled chains from
rejection (#341). Both answers are consensus content: they shape the
rejected-deploy records that peers check with `InvalidRejectedDeploy`.

Soak run 33099406770 (master `af1e5209d`) attributed 92% of the 2.16s
average `merge_call` cost to these probes. Each probe re-walks the same
lineage segment, at about 30 probes per merge, with one full block-body
load per step. The walk depth is `tip − (floor − deploy_lifespan)`, so
finalization lag deepens the walk, which slows blocks, which deepens the
lag further. The planned remediation batches the walk and memoizes
segments. This claim pins the semantics that any such optimization must
preserve exactly.

## Specification (reference semantics)

`effect_in_state_of(store, h, sig, min_height)` answers TRUE iff some
block `B` on the state lineage of `h`, with `block_number(B) >=
min_height`, applied the sig:

```text
applied(B, sig) :=
     (exists pd in B.body.deploys : pd.deploy.sig = sig AND NOT pd.is_failed)
  OR (sig in B.body.applied_from_scope)

settled(h, sig, min_height) :=
  exists B on state_lineage(h) : block_number(B) >= min_height AND applied(B, sig)
```

The state lineage steps as follows. Each rule is exact:

1. The recorded `merge_base` is the next block when it is non-empty.
2. A single-parent block steps to its sole header parent.
3. Genesis ends the lineage with answer FALSE.
4. A multi-parent block without a recorded `merge_base` is malformed. The
   walk returns an error. It never guesses a lineage.
5. An absent block body is `BlockNotHeld` (deferral). It is never an
   answer. Two nodes with different block availability must not derive
   different verdicts from the same parents.

The probe sites fix the arguments:

- `sig_settled_in_base(sig)` = `settled(main_parent, sig, floor_number −
  deploy_lifespan)`.
- `sig_settled_in_floor(sig)` = TRUE iff `settled(f.hash, sig,
  f.block_number − deploy_lifespan)` for some settled floor `f` in the
  block's frozen justification snapshot.

## Claim statements

**C1 — Reference purity and cross-view determinism.** The predicate is a
pure function of consensus data: the recorded lineage, the block bodies on
it, and the height bound. The proposer and every validator derive
identical answers for identical arguments.

**C2 — Batched-walk equivalence.** An implementation that walks the
segment once, collects every applied sig into a set, and answers each
probe by membership is extensionally equal to the reference walk.
Mechanized: `walk_collect_equiv`.

**C3 — Segment composition and memoization soundness.** The walk
distributes over segment concatenation. A segment known FALSE for a sig
can be skipped (`checked_below`), and a TRUE answer survives any
extension above the memoized segment. Mechanized:
`walk_segment_composition`, `walk_memo_false_stable`, `walk_true_stable`.

**C4 — Answer stability across merges.** Consecutive merges share almost
the whole lineage. Cross-merge reuse of segment answers is sound exactly
because of C3, provided the cached segment is keyed on the lineage blocks
it covered and the bound it was computed under.

## Seam premises (documented, not proven)

- **Walk-bound soundness.** A scope-live sig's validity window was open at
  its execution, so no block below `floor_number − deploy_lifespan` holds
  its effect. This premise comes from the deploy-validity rules, not from
  this claim's mechanization.
- **Availability deferral.** `BlockNotHeld` propagates as an error and
  defers the block. The mechanization models only complete segments. The
  batched implementation (`settled_sigs_of_lineage`) strengthens this
  fail-closed WITHIN one segment: it always covers the full segment, so a
  gap below an applied sig refuses the whole answer where the per-sig
  reference walk can answer TRUE without reaching the gap. A deferral
  where the reference sometimes answered is the safe direction, and the
  divergence is pinned by `batched_walk_is_fail_closed_on_a_gapped_segment`.
  ACROSS floors the reference short-circuit is preserved:
  `FloorSettledProbe` scans floors in order and builds each floor's set
  lazily, so a floor after the answering one is never read and its
  unavailability cannot poison the probe (pinned by
  `floor_probe_short_circuit_skips_unavailable_later_floors`).
- **Terminal never-flip.** The deploy-lifecycle store enforces that a
  terminal record never flips (`put_deploy_terminal_if_absent`). C4's TRUE
  stability aligns with it but does not prove the store property.

## Mechanization mapping

| Model (`SettledEffectProbe.v`) | Rust (`deploy_lifecycle.rs`) |
|---|---|
| `lineage_block` (list of sigs) | non-failed `body.deploys` sigs ∪ `body.applied_from_scope` |
| `segment` (tip-first list) | merge-base lineage from tip down to the walk bound |
| `walk seg sig` | `effect_in_state_of` per-sig loop |
| `collect seg` | `settled_sigs_of_lineage` (one walk, every sig) |
| per-floor short-circuit scan | `FloorSettledProbe::settled` (in-order lazy per-floor sets) |
| `walk_memo_false_stable` premise | `checked_below` early stop (`effect_in_state_of_above`) |

The per-block `LineageStep` cache stores content-addressed per-block
facts, never answers, so cache hits stay inside C2's equivalence. A hit
is additionally revalidated against the CALLER's store with a raw
key-existence check, so the walk remains a function of the supplied
store: a block that store does not hold is `BlockNotHeld` even when
another store in the same process cached it.

The gate `scripts/check-finalized-floor-ALL.sh` builds the theory and
asserts all four probe theorems axiom-free alongside the domain
capstones.

## Discharge plan for the remediation

The optimization on `fix/one-walk-merge-per-block-sig-nodeserialize`
must, with the status of each item recorded here:

1. Keep the reference walk as the specification oracle. **Done** —
   `effect_in_state_of` is untouched; the batched form is the separate
   `settled_sigs_of_lineage`.
2. Add a property test that compares the batched implementation against
   the reference walk on generated lineages, including failed deploys,
   `applied_from_scope` entries, bound truncation, and segment splits.
   **Done** — `batched_walk_matches_the_reference_walk_on_generated_lineages`
   plus `batched_walk_is_fail_closed_on_a_gapped_segment` in
   `deploy_lifecycle.rs`.
3. Record the evidence in `docs/cbc-evidence/` for the touched
   `cbc=mandatory` artifacts and cite this claim id. **Done** —
   `casper-src-rust-finality-deploy-lifecycle-rs.md` and the appended
   record in `casper-src-rust-util-rholang-interpreter-util-rs.md`.
4. Not change any rejected-record output: `InvalidRejectedDeploy`
   equality against unfixed peers is the end-to-end acceptance check.
   **Open** — rides the casper merge/validation suites now and the next
   soak preflight for the end-to-end run.
