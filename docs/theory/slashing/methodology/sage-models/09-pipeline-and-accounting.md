# 09 · Pipeline & accounting models

## 1 · Family motivation

The slashing pipeline has clear *accounting* invariants: bonds sum
to a known total; slashed bonds equal the vault credit; the record
set is well-formed regardless of insertion order; the slash effect
is independent of batch execution order. This family checks each
accounting invariant exactly on small configurations.

## 2 · Models in this family

| Model                                                                                                    | Searches                                                                                |
|----------------------------------------------------------------------------------------------------------|-----------------------------------------------------------------------------------------|
| [`pipeline_effect_model.sage`](../../../../../formal/sage/slashing/pipeline_effect_model.sage)           | Bond accounting, idempotence, zeroing of slashed bonds, preservation of unslashed bonds |
| [`record_normalization_model.sage`](../../../../../formal/sage/slashing/record_normalization_model.sage) | Record meaning modulo insertion order and duplicate hashes                              |
| [`slash_order_model.sage`](../../../../../formal/sage/slashing/slash_order_model.sage)                   | Batch slash execution permutations: final bonds, slashed set, vault balance             |
| [`validator_boundary_model.sage`](../../../../../formal/sage/slashing/validator_boundary_model.sage)     | Current-validator filtering vs. unfiltered evidence-domain projection                   |

## 3 · Representative witnesses

### 3.1 Pipeline-effect witness

```json
{
  "kind": "pipeline_effect_witness",
  "n": 4,
  "initial_bonds": [10, 7, 11, 13],
  "slash_set": [0, 2],
  "final_bonds": [0, 7, 0, 13],
  "vault_delta": 21,
  "non_negative_bonds": true,
  "slashed_zeroed": true,
  "unslashed_preserved": true
}
```

Reading: initial bond vector `[10, 7, 11, 13]`; slashing validators
0 and 2 zeros their bonds and credits the vault by `10 + 11 = 21`.
Validators 1 and 3 are unchanged. All three accounting invariants
hold.

### 3.2 Slash-order witness

```json
{
  "kind": "slash_order_witness",
  "n": 4,
  "initial_bonds": [5, 7, 11, 13],
  "slash_orders_tested": 24,
  "final_bonds_unique": [[0, 0, 0, 0]],
  "vault_unique": [36],
  "slashed_set_unique": [[0, 1, 2, 3]],
  "order_independent": true
}
```

Reading: 4! = 24 slash orders all produce the same final bonds
vector, the same vault balance, and the same slashed set. Slash
order is observationally independent in this configuration.

## 4 · Promotion targets

| Witness shape                    | Rocq theorem                       | TLA⁺ model                                        | Rust regression                                                   |
|----------------------------------|------------------------------------|---------------------------------------------------|-------------------------------------------------------------------|
| Bond accounting                  | T-7 (`slash_zeros_bond`)           | `SlashFlow.tla` `Inv_BondAccounting`              | `prop_t_7_slash_zeros_bond.rs`                                    |
| Slash idempotence                | T-Idem                             | (model-checked)                                   | `prop_t_idem_slash_idempotence.rs`, `slash_idempotent_example.rs` |
| Record meaning under permutation | T-9.11 (permutation invariance)    | (model-checked)                                   | `prop_t_9_11_detector_permutation_invariance.rs`                  |
| Record uniqueness                | T-4                                | `EquivocationDetector.tla` `Inv_RecordUniqueness` | `prop_t_4_record_uniqueness.rs`                                   |
| Slash order independence         | (informal; covered by composition) | (model-checked)                                   | `prop_t_5_record_monotonicity.rs`                                 |
| Parent-bond authorization        | T-9.13′                            | `AuthorizedSlashFlow.tla`                         | `slash_authorization_regressions`                                 |
| Rejected-slash reissue           | T-9.13″                            | `AuthorizedSlashFlow.tla`, `SlashFlow.tla`        | `slash_recovery_spec`, `rejected_slash`                           |

## 5 · Related findings

In [`formal/sage/slashing/FINDINGS.md`](../../../../../formal/sage/slashing/FINDINGS.md):

- **#5** Validator-set boundary filtering candidate divergence
  (covered by current-validator filter).
- **#12** Batch slash order is observationally independent.
- **#17** Record normalization modulo insertion order.

## 6 · Methodology note

The pipeline-and-accounting family is the methodology's
**algebraic-property** layer. Bond accounting is *additive*; slash
order is *commutative*; record insertion is *idempotent*. These are
classical algebraic properties, and the family encodes them as exact
Sage computations that round-trip through the production code path.
