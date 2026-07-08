# 12 · Theorem-assumption counterexamples

## 1 · Family motivation

A Rocq theorem `∀ x ∈ X. P(x) ⇒ Q(x)` is only useful if `P` is
*load-bearing* — i.e. removing `P` would allow some `x` to violate
`Q`. If `P` is **superfluous**, the theorem is weaker than it
needs to be; if `P` is **necessary**, the theorem cannot be
strengthened. This family searches the assumption space for
counterexamples that distinguish the two cases.

## 2 · The model

| Model                                                                                                                    | Searches                                                                                |
|--------------------------------------------------------------------------------------------------------------------------|-----------------------------------------------------------------------------------------|
| [`theorem_assumption_counterexamples.sage`](../../../../../formal/sage/slashing/theorem_assumption_counterexamples.sage) | Minimal witnesses for what happens when key Rocq / TLA⁺ theorem assumptions are removed |

## 3 · Representative witness

```json
{
  "kind": "assumption_counterexample_witness",
  "target_theorem": "T-12 (BFT quorum preservation)",
  "removed_assumption": "f < n/3",
  "n": 4,
  "fault_budget": 2,
  "equivocators": [0, 1],
  "neglect_edges": [[2, 0], [3, 1]],
  "closure": [0, 1, 2, 3],
  "active_after": 0,
  "quorum_required": 3,
  "quorum_violated_after": true,
  "violation_severity": "complete loss of active validators"
}
```

Reading: removing the `f < n/3` assumption from T-12 allows
`f = 2 = n/2` direct equivocators; the resulting closure slashes
*every* validator, violating quorum. This corroborates that
`f < n/3` is **necessary** for T-12.

## 4 · Promotion targets

| Witness target              | Theorem precondition asserted                                           | Rust regression                         |
|-----------------------------|-------------------------------------------------------------------------|-----------------------------------------|
| T-11 (closure depth)        | `n ≥ 1`                                                                 | `theorem_assumption_counterexamples.rs` |
| T-12 (BFT quorum)           | `f < n/3`                                                               | (same)                                  |
| T-9.10 (withdraw safety)    | `posVault.transfer` may fail                                            | (same)                                  |
| T-9.11 (detector totality)  | Missing pointers are non-contributing, not fatal                        | (same)                                  |
| T-9.8 (slash authorization) | Current epoch ∧ matching evidence epoch ∧ positive bond ∧ invalid block | (same)                                  |

## 5 · Related findings

In [`formal/sage/slashing/FINDINGS.md`](../../../../../formal/sage/slashing/FINDINGS.md):

- **#18–#22** Assumption counterexamples for various theorems; each
  records the minimal removed-assumption witness.

## 6 · Methodology note

This family is the methodology's defense against **theorem
weakening** during refactors. A future contributor who proposes
weakening a theorem's precondition can be directed to the
counterexample fixture; the existence of the counterexample is the
*proof* that the precondition is load-bearing.

This is a pattern the methodology borrows from the proof-engineering
literature [Pol10]: every load-bearing hypothesis should have an
associated witness that demonstrates its necessity.
