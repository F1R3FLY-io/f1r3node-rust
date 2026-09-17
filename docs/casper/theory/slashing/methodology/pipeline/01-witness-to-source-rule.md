# 01 · The witness-to-source rule

> *“Extraordinary claims require extraordinary evidence.”* — Carl
> Sagan, *Cosmos*, 1980 [Sag80].

This chapter is the operational form of the witness rule from
[`../01-philosophy.md §4`](../01-philosophy.md). It defines the
**single rule** that converts a machine-generated witness into a
classified outcome — and the **three forbidden short-cuts** that
the methodology refuses.

Organization:

- [§1 — The rule, in three sentences](#1--the-rule-in-three-sentences)
- [§2 — The pipeline in literate pseudocode](#2--the-pipeline-in-literate-pseudocode)
- [§3 — The three forbidden short-cuts](#3--the-three-forbidden-short-cuts)
- [§4 — Example traces — the rule in action](#4--example-traces--the-rule-in-action)
- [§5 — Pitfalls](#5--pitfalls)
- [§6 — Related work](#6--related-work)

---

## 1 · The rule, in three sentences

> **Witness rule**:
>
> 1. A generated witness is **not** a Rust vulnerability unless it
>    is reproduced on the production Rust path *or* contradicts a
>    production-path invariant.
> 2. Every witness must be classified under both the threat-model
>    vocabulary
>    ([`../../slashing-threat-model.md §4`](../../slashing-threat-model.md))
>    *and* the traceability ledger
>    ([`../../slashing-traceability.md`](../../slashing-traceability.md)).
> 3. A finding that has not been classified under both may not
>    motivate a source-code change.

The rule is symmetric: it prevents both *over-claiming* (calling a
model artifact a Rust bug) and *under-claiming* (silently dismissing
a Rust-reproducible witness as a model artifact).

---

## 2 · The pipeline in literate pseudocode

```
algorithm witness_to_action(w : Witness, tool : Tool) → Action:
    (* §1 — record the witness *)
    let kind     ← witness_kind(w)
    let metadata ← extract_metadata(w, tool)
    log(witness_log, (w, tool, kind, metadata, timestamp))

    (* §2 — classify under threat-model vocabulary *)
    let threat_class ← classify_under_threat_model(
        w,
        candidates = {bisimilar, permitted_bug_fix, candidate_boundary,
                      projection_risk, assumption_counterexample, unexpected}
    )
    if threat_class = unexpected:
        ▸ halt — every witness must be reclassified out of unexpected

    (* §3 — trace into the production Rust path *)
    let reproduction ← attempt_rust_reproduction(w)
    match reproduction:
        Reproduces                 → ledger_status ← confirmed_current_bug
        Reproduces_only_pre_fix    → ledger_status ← confirmed_fixed_bug
        Rejected_by_production     → ledger_status ← not_reproduced_in_rust
        Guarded_by_production_check→ ledger_status ← projection_risk_guarded
        Requires_theorem_precond   → ledger_status ← assumption_counterexample
        Suggests_stronger_theorem  → ledger_status ← proof_or_model_strengthening
        Inconclusive               → ledger_status ← needs_source_audit

    (* §4 — record in the traceability ledger *)
    let ledger_entry ← LedgerEntry(
        witness = w, tool = tool, threat_class = threat_class,
        ledger_status = ledger_status, timestamp = now()
    )
    append ledger_entry to ../../slashing-traceability.md

    (* §5 — derive the required action from the status *)
    match ledger_status:
        confirmed_current_bug       → return Action::FixSourceAndAddRegression(w)
        confirmed_fixed_bug         → return Action::KeepPreFixRegression(w)
        not_reproduced_in_rust      → return Action::RecordOnly(w)
        model_boundary              → return Action::DocumentBoundary(w)
        projection_risk_guarded     → return Action::KeepGuardTest(w)
        assumption_counterexample   → return Action::StrengthenTheoremPrecondition(w)
        proof_or_model_strengthening→ return Action::PromoteToMechanizedArtifact(w)
        needs_source_audit          → return Action::EscalateAudit(w)

    (* §6 — execute the action *)
    execute(action)

    (* §7 — close the loop *)
    re-run the tool that produced w; verify the witness no longer
    appears (for current_bug) or appears with expected status (for
    other classes); record outcome
```

### 2.1 Why a literate algorithm, not a flowchart

The pipeline has eight branches and several feedback loops; a flowchart
would obscure the *invariant* — *every witness yields exactly one
action* — and the *strict ordering* — classification before
action. The literate form makes both explicit and is auditable
line-by-line.

---

## 3 · The three forbidden short-cuts

The methodology refuses three short-cuts. Each appears at least once
as a temptation during a real bug investigation and each was
explicitly rejected.

### 3.1 Forbidden short-cut #1: "the witness is obviously a bug"

A witness that *looks* like a clear Rust bug may not reproduce on
the production path. The methodology requires the reproduction step
*even when reproduction seems certain*; the cost is small and the
information value when reproduction *fails* is high.

Example: Sage finding #5 (validator-set boundary filtering) looked
like a clear Rust bug (the filtered current-validator model slashes
`[]` where the unfiltered projection slashes `[0]`). The witness
did not reproduce on the production path because the Rust code
filters validators by current epoch before the comparison. The
short-cut would have produced a wasted source change.

### 3.2 Forbidden short-cut #2: "the witness is obviously a model artifact"

A witness that *looks* like a model artifact may actually reproduce
in production. The methodology requires the reproduction step
*even when the witness uses model-only constructs*; the cost is
small and a false dismissal hides a real bug.

Example: Sage finding #8 (bounded-arithmetic overflow) looked like a
model boundary because Sage uses unbounded integers. The witness did
reproduce — the production `i32::MAX + 1` overflow was real, and the
fix introduced `checked_add` saturating to `None`. The short-cut
would have left Bug #15 in production.

### 3.3 Forbidden short-cut #3: "the classification is provisional"

A witness left in the `unexpected` or `needs_source_audit` status
forever is effectively unclassified. The methodology requires every
witness to **eventually** reach one of the seven *terminal* statuses
(every status except `unexpected`).

The methodology enforces this with a CI gate: a pull request that
adds a `confirmed_current_bug` entry must include the fix or a
linked issue; a pull request that adds a `needs_source_audit` entry
must include the audit assignment.

---

## 4 · Example traces — the rule in action

### 4.1 Trace A — confirmed_current_bug (Bug #11)

The Hypothesis search `hypothesis_assumption_minimization.py` emits
a 3-validator DAG where validator `v2`'s justifications point at a
non-existent latest message. The Rust detector returns `Valid`
because its BFS traversal silently returns `∅` on missing pointers,
causing the equivocation to escape detection.

Classification:

```
threat_class       = candidate_boundary?  no — the implementation
                                                projection differs from
                                                the model on a real
                                                input
                   = projection_risk?     yes
ledger_status      = confirmed_current_bug
action             = FixSourceAndAddRegression
fix                = make detector's BFS treat missing pointers as
                     non-contributing rather than as a fatal exit
regression test    = casper/tests/slashing/prop_t_9_11_detector_totality.rs
Rocq theorem       = T-9.11 (detector totality)
TLA+ invariant     = Inv_FixedDetectorTotal
                     in formal/tlaplus/slashing/EquivocationDetector.tla
```

The witness flows through every stage of the pipeline and lands in
**four** complementary artifacts (Rust fix, regression test, Rocq
theorem, TLA⁺ invariant).

### 4.2 Trace B — not_reproduced_in_rust (Sage finding #3)

The Sage script `weighted_closure_model.sage` emits a 3-validator
configuration with stake vector `[0, 2, 2]`, where the zero-stake
validator is treated as a direct offender, causing the closure to
include a stake-2 validator. The witness suggests a real attack
where zero-stake offenders are slashable.

Classification:

```
threat_class       = candidate_boundary
ledger_status      = not_reproduced_in_rust
                     (the production Rust path rejects zero-stake
                      bonding at the bond_validator step; no zero-
                      stake validator can become a direct offender)
action             = RecordOnly
documentation      = mark this as a model boundary in FINDINGS.md;
                     no source change
```

The witness does **not** become a source change; it remains in the
findings ledger as a documented model boundary. This protects the
ledger from an out-of-scope action.

### 4.3 Trace C — assumption_counterexample (theorem_assumption_counterexamples.sage)

The Sage script `theorem_assumption_counterexamples.sage` generates
witnesses for what happens when a theorem's preconditions are
removed. For example, T-11's `f < n/3` precondition: removing it
generates a witness where the closure exceeds `n − 1`, demonstrating
the BFT bound is necessary.

Classification:

```
threat_class       = assumption_counterexample
ledger_status      = assumption_counterexample
action             = StrengthenTheoremPrecondition
                     (the precondition was already there; the
                      witness corroborates that it is load-bearing)
documentation      = add a note to the Rocq theorem citing the
                     witness as evidence that the precondition cannot
                     be weakened
regression test    = casper/tests/slashing/theorem_assumption_counterexamples.rs
```

The witness has no Rust source-change effect, but it produces
*permanent evidence* in the ledger and the Rust regression that
prevents a future weakening of the theorem.

---

## 5 · Pitfalls

### 5.1 Pitfall: skipping the classification step

The most common failure mode is to skip directly from witness to
fix. The fix may be correct, but the classification is the audit
trail; without it the same kind of witness next time will not be
recognizable as already-classified.

**Mitigation**: the methodology requires every commit message that
fixes a bug to include the classification (the threat-class and the
ledger-status). A commit without the classification fails review.

### 5.2 Pitfall: classifying without reproducing

A witness can be classified `not_reproduced_in_rust` by visual
inspection ("this looks like a model artifact") instead of an actual
reproduction attempt. This is the dual of [§3.1](#31-forbidden-short-cut-1-the-witness-is-obviously-a-bug).

**Mitigation**: every `not_reproduced_in_rust` classification must
cite a Rust test that *attempts* to reproduce the witness and
confirms the production path's behavior; the citation goes in the
ledger entry.

### 5.3 Pitfall: classifying the wrong witness

A long Sage search may produce many witnesses; classifying *one* of
them and assuming the classification applies to *all* of them is a
fallacy. Each witness has its own configuration and must be classified
independently.

**Mitigation**: the methodology assigns a unique finding number to
every witness; the ledger entry references the finding number.

### 5.4 Pitfall: re-running the tool without verifying the witness is gone

After a fix, the methodology requires the original tool that
produced the witness to be re-run, with the witness verified to no
longer appear. Skipping this step leaves a fixed-in-spirit bug that
could re-emerge.

**Mitigation**: every `pre_fix_bug_N.rs` regression test is paired
with a *post-fix* assertion that exercises the same code path and
expects the fix's behavior; CI runs both, ensuring the bug stays
fixed.

---

## 6 · Related work

- **Bug life-cycle models in software engineering**: see Anvik *et
  al.* [Anv05] for the canonical bug-triage workflow that inspired
  this pipeline's eight-status design.
- **Triage of fuzzing results**: McNally *et al.* [McN12] on
  crash-deduplication and triage at scale.
- **Promotion ladders in distributed-systems verification**: the
  TLA⁺-then-Rust pattern is documented by Newcombe *et al.* [New14]
  at AWS.

DOIs in [`../references.md`](../references.md).

---

## 7 · Next chapter

[`02-classification-taxonomy.md`](./02-classification-taxonomy.md)
— the full eight-status / six-class vocabulary and the decision tree
that maps witnesses to statuses.
