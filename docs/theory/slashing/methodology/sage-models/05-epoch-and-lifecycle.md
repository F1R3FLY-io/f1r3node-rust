# 05 · Epoch & lifecycle models

## 1 · Family motivation

A validator's lifecycle spans **bonding** → **active** → (possibly)
**slashed** → **unbonded** → **withdrawn** → (possibly) **rebonded**.
Each transition has a precondition; each precondition has an
adversarial corner case. The epoch dimension adds a temporal axis:
the same validator key may be active in epoch `k` and unbonded in
epoch `k + 1`; evidence from epoch `k − 1` may be stale by epoch
`k + 1`. This family searches the epoch × lifecycle space for
boundary cases.

## 2 · Models in this family

| Model                                                                                                | Searches                                                                                    |
|------------------------------------------------------------------------------------------------------|---------------------------------------------------------------------------------------------|
| [`epoch_lifecycle_model.sage`](../../../../../formal/sage/slashing/epoch_lifecycle_model.sage)       | Current-epoch / current-validator filtering for stale and fresh evidence                    |
| [`epoch_churn_attack_model.sage`](../../../../../formal/sage/slashing/epoch_churn_attack_model.sage) | Stale evidence, validator rejoin/rebond identity, pending-slash carryover policy boundaries |

## 3 · Representative witness

```json
{
  "kind": "epoch_churn_witness",
  "scenario": "rebond_same_key_after_slash",
  "epoch_length": 10,
  "trace": [
    {"epoch": 0, "op": "bond",       "v": "v0", "key": "k0", "stake": 100},
    {"epoch": 0, "op": "equivocate", "v": "v0", "seq": 3},
    {"epoch": 0, "op": "slash",      "v": "v0"},
    {"epoch": 0, "op": "unbond",     "v": "v0"},
    {"epoch": 1, "op": "withdraw",   "v": "v0"},
    {"epoch": 2, "op": "rebond",     "v": "v0", "key": "k0", "stake": 100},
    {"epoch": 2, "op": "submit_stale_evidence", "evidence_epoch": 0, "v": "v0"}
  ],
  "stale_evidence_rejected": true,
  "covered_by_bug": "Bug #13 (same-key rebond stale-evidence)",
  "covered_by_theorem": "T-9.11 (rebond rejects stale evidence)"
}
```

Reading: the scenario walks the entire lifecycle of one validator
key: bond → equivocate → slash → unbond → withdraw → rebond at the
same key → adversary attempts to submit stale evidence from before
the rebond. The post-fix Rust path **rejects** the stale evidence
(`stale_evidence_rejected = true`), as proven by Theorem T-9.11 and
exercised by Bug #13's regression.

## 4 · Promotion targets

| Witness shape                                 | Rocq theorem                        | TLA⁺ model                                                 | Rust regression                                               |
|-----------------------------------------------|-------------------------------------|------------------------------------------------------------|---------------------------------------------------------------|
| Stale evidence after rebond                   | T-9.11                              | `AuthorizedSlashFlow.tla` `Inv_RebondRejectsStaleEvidence` | `epoch_evidence_rollover.rs`, `rebonded_identity_boundary.rs` |
| Current-epoch filter on slash candidates      | T-9.8                               | `AuthorizedSlashFlow.tla` `Inv_SlashOnlyIfAuthorized`      | `prop_t_9_8_unbonded_proposer.rs`, `prop_t_auth_check.rs`     |
| Pending-slash carryover across epoch boundary | (informal; documented in design/06) | (model-checked finite)                                     | `stale_evidence_filtered.rs`                                  |

## 5 · Related findings

In [`formal/sage/slashing/FINDINGS.md`](../../../../../formal/sage/slashing/FINDINGS.md):

- **#13** Epoch / current-validator filtering separates stale from
  fresh evidence.
- **#22** (extension) Rebond scenarios surface stale-evidence corner
  case fixed by Bug #13.

## 6 · Methodology note

The epoch dimension is the **temporal** axis of the strategy space.
Most adversarial attacks exploit a *temporal asymmetry* — evidence
becomes stale, validators rotate, proposer slots arrive at
particular times. This family is the primary search engine for
temporal-asymmetry bugs.
