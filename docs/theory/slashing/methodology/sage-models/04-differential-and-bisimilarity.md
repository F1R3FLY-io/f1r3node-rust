# 04 · Differential & bisimilarity models

## 1 · Family motivation

The slashing port is a Rust reimplementation of a Scala original;
divergence between the two is the strongest single source of bug
candidates the methodology has. This family runs the same inputs
through both implementations and a Rocq-derived oracle, and emits
witnesses for every disagreement. The pedagogical framework lives
in
[`../differential-and-metamorphic/01-differential-rust-vs-scala.md`](../differential-and-metamorphic/01-differential-rust-vs-scala.md);
this chapter documents the two Sage models.

## 2 · Models in this family

| Model                                                                                                              | Searches                                                                                                        |
|--------------------------------------------------------------------------------------------------------------------|-----------------------------------------------------------------------------------------------------------------|
| [`differential_bisimilarity_model.sage`](../../../../../formal/sage/slashing/differential_bisimilarity_model.sage) | Small-state Rust vs. Scala vs. Rocq-oracle divergence search; classifies each divergence under the threat-model |
| [`differential_trace_generator.sage`](../../../../../formal/sage/slashing/differential_trace_generator.sage)       | Generates JSON traces that drive both implementations through the same action sequence                          |

The two models cooperate: the generator produces traces; the
bisimilarity model runs the traces and classifies divergences.

## 3 · Representative witness

```json
{
  "kind": "differential_divergence",
  "scenario_id": "diff-027",
  "n": 3,
  "trace": [
    {"op": "bond", "v": "v0", "stake": 10},
    {"op": "bond", "v": "v1", "stake": 10},
    {"op": "equivocate", "v": "v0", "seq": 0},
    {"op": "dispatch", "hash": "h_0a"}
  ],
  "observations": {
    "rust":   {"status": "IgnorableEquivocation", "record_count": 1},
    "scala":  {"status": "Valid",                  "record_count": 0},
    "oracle": {"status": "IgnorableEquivocation", "record_count": 1}
  },
  "divergence_class": "permitted_bug_fix",
  "cited_bug": "Bug #1 (IgnorableEquivocation non-slashable)"
}
```

Reading: the trace exercises the canonical scenario where Bug #1
manifests in Scala but not in Rust. Two of three implementations
agree (Rust and Rocq oracle); Scala dissents. The divergence is
classified `permitted_bug_fix` and cites the bug-fix manifest entry.

## 4 · Promotion targets

| Witness class        | Action                                                        |
|----------------------|---------------------------------------------------------------|
| `bisimilar`          | Record only                                                   |
| `permitted_bug_fix`  | Cite bug-fix manifest entry; add post-fix regression          |
| `candidate_boundary` | Document precondition in `slashing-specification.md`          |
| `projection_risk`    | Add guard test                                                |
| `unexpected`         | **Halt and investigate** — must be reclassified before commit |

The Rocq theorem behind this family is the headline bisimilarity
statement T-15a/b (`main_bisimilarity_theorem`); the Sage model is
the *executable witness search* for that theorem on small bounds.

## 5 · Related findings

In [`formal/sage/slashing/FINDINGS.md`](../../../../../formal/sage/slashing/FINDINGS.md):

- **#9** Differential bisimilarity found no `unexpected` divergence
  in the small searched state space.
- **#16–#22** Specific permitted-bug-fix divergences corresponding
  to Bugs #1, #3, #4, #5, #6, #7, #8.

## 6 · Methodology note

This family is what makes the bisimilarity theorem **operationally
checkable**. The Rocq theorem is mathematically correct under its
definitions; the Sage model corroborates that those definitions
remain in contact with both running implementations. Without the
Sage corroboration, the Rocq theorem would risk becoming a
*statement about the model* rather than *a statement about the
system*.
