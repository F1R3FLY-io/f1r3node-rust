# Extending the methodology

This chapter is the **operational checklist** for adding new search
targets, new properties, or new tools to the slashing methodology.
It complements the per-tool tutorials in [`tutorials/`](./tutorials/)
by addressing the **cross-cutting** concerns: where to put new
artifacts, how to wire them into CI, and how to enforce the
methodology's invariants.

Organization:

- [§1 — Adding a new property](#1--adding-a-new-property)
- [§2 — Adding a new technique to the stack](#2--adding-a-new-technique-to-the-stack)
- [§3 — Adding a new bug case study](#3--adding-a-new-bug-case-study)
- [§4 — Maintaining the bookkeeping invariants](#4--maintaining-the-bookkeeping-invariants)
- [§5 — Promoting a finding through the pipeline](#5--promoting-a-finding-through-the-pipeline)

---

## 1 · Adding a new property

A new property `φ` enters the methodology by **planning the stack
first**:

```
algorithm add_new_property(φ : Property):
    ▸ 1. State φ in mathematical English (one paragraph)
    ▸ 2. Choose the *primary* mechanized layer:
         | unbounded ⇒ Rocq         | bounded ⇒ TLA⁺           |
         | per-function ⇒ Kani      | byte-level ⇒ libFuzzer    |
    ▸ 3. Choose the *randomized* corroborating layer:
         | one-step ⇒ proptest      | stateful ⇒ Hypothesis    |
         | concurrent ⇒ Loom        | unstructured ⇒ libFuzzer  |
    ▸ 4. Choose the *differential / metamorphic* layer if applicable
    ▸ 5. Record the property in slashing-verification.md §<N>
    ▸ 6. Record the property in slashing-traceability.md (status: pending)
    ▸ 7. Implement each layer in turn (mechanized first; randomized
         second; differential / metamorphic third)
    ▸ 8. Verify the stack:
         - Print Assumptions returns "Closed under the global context"
         - TLC reports "No error has been found"
         - Kani returns "VERIFICATION:- SUCCESSFUL"
         - proptest/Hypothesis: 256+ cases pass
    ▸ 9. Update slashing-traceability.md to status `confirmed_fixed_bug`
         (if positive) or fix the source and re-run if negative
    ▸ 10. Update README cross-link tables
```

### 1.1 The "plan-the-stack" gate

A PR adding a new property is blocked at review until the stack
plan is filled in. The plan template is:

```markdown
## Property <name>

- Statement: <one paragraph>
- Mechanized layer: <Rocq | TLA+ | Kani>
- Randomized layer: <proptest | Hypothesis | libFuzzer | Loom>
- Diff/meta layer: <triple-bisim | metamorphic | none>
- Target stack depth: <3 | 4 | 5>
- Spec entry: slashing-verification.md §<N>
- Ledger entry: slashing-traceability.md row <ID>
```

---

## 2 · Adding a new technique to the stack

The methodology's stack is not closed. New tools may enter when:

1. They cover a property class **none of the existing tools cover
   well** (e.g. *“symbolic execution of `async` Rust”* — currently
   neither Kani nor Miri reaches this).
2. They have a **distinct trust base** (no overlap with existing
   tools' trust bases).
3. They have **a clear methodology slot** (i.e. they answer a
   specific cell in the
   [`02-glossary-and-notation.md §1`](./02-glossary-and-notation.md)
   `⟨shape, domain, observability, cost-budget⟩` matrix).

The addition procedure:

```
algorithm add_new_technique(tool : Tool):
    ▸ 1. Document tool's trust base
    ▸ 2. Document tool's domain (shape × cost matrix slot)
    ▸ 3. Write a chapter in formal-methods/, randomized-search/, or
         differential-and-metamorphic/ as appropriate
    ▸ 4. Update 01-philosophy.md §3 decision tree
    ▸ 5. Update 02-glossary-and-notation.md §4 tooling acronyms
    ▸ 6. Write a tutorial in tutorials/
    ▸ 7. Wire into scripts/ci/slashing-search-horizon.sh
    ▸ 8. Migrate at least one existing property to use the new tool
         (proof-of-concept)
```

---

## 3 · Adding a new bug case study

When a new bug is discovered:

```
algorithm add_new_case_study(bug : Bug):
    ▸ 1. Write the bug-fix manifest entry in design/09-bug-fixes-and-rationale.md
    ▸ 2. Write the case study at case-studies/NN-bug-NN-<slug>.md
    ▸ 3. Add the case-study row to case-studies/README.md
    ▸ 4. Verify the case study includes:
         - One-paragraph summary
         - Discovery technique with citation
         - Witness reproduction command
         - Classification trace
         - Evidence stack table
         - Lessons for the methodology (≥ 2)
    ▸ 5. Add the regression test pre_fix_bug_N.rs
    ▸ 6. Add the post-fix anchor uc_NN_*.rs
    ▸ 7. Add the Rocq theorem if normative
    ▸ 8. Update slashing-traceability.md
    ▸ 9. Update slashing-search-horizon.md if a new tool was used
```

---

## 4 · Maintaining the bookkeeping invariants

The methodology has six invariants that must hold at all times.
CI lints enforce them:

| Invariant                                                              | Enforced by                                                  |
|------------------------------------------------------------------------|--------------------------------------------------------------|
| Every Sage finding has a classification                                 | `corpus_generator.sage` schema validation                     |
| Every Rocq theorem closes under global context                          | `coqtop -batch -e 'Print Assumptions <name>.'` in CI         |
| Every TLA⁺ invariant has a TLC run with stable parameters               | `scripts/ci/check-tla-invariants.sh`                          |
| Every Kani harness verifies on every PR                                 | `scripts/ci/slashing-search-horizon.sh`                       |
| Every property has a `prop_t_*.rs` or `pre_fix_bug_*.rs` regression    | `casper/tests/slashing/mod.rs` registration                   |
| Every case study links to the bug-fix manifest entry                    | Markdown link check                                           |

A PR that breaks any invariant is blocked at CI; the methodology
treats CI failures as bookkeeping failures, not just code failures.

---

## 5 · Promoting a finding through the pipeline

The pipeline from witness to permanent artifact is documented in
[`pipeline/01-witness-to-source-rule.md`](./pipeline/01-witness-to-source-rule.md).
The condensed checklist:

```
[ ] 1. Witness reproduced deterministically (seed recorded)
[ ] 2. Threat-model class assigned (one of 6)
[ ] 3. Reproduction attempted on Rust production path
[ ] 4. Traceability status assigned (one of 8)
[ ] 5. Action taken per status:
        confirmed_current_bug          → fix + regression
        confirmed_fixed_bug            → keep regression
        not_reproduced_in_rust         → record only
        model_boundary                 → document
        projection_risk_guarded        → keep guard test
        assumption_counterexample      → strengthen theorem
        proof_or_model_strengthening   → promote to Rocq/TLA+
        needs_source_audit             → escalate
[ ] 6. Ledger entry appended to slashing-traceability.md
[ ] 7. Tool re-run confirms witness no longer surfaces (or surfaces
       with expected post-fix status)
[ ] 8. Persistent corpus updated (fuzz/corpus, hypothesis_persistent_corpus, …)
```

The checklist is the methodology's *standard operating procedure* for
every finding; deviation from it is a methodology defect that the
audit will catch.
