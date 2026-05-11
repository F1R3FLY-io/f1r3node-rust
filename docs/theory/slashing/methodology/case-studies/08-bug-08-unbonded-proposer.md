# Case study #8 — `prepare_slashing_deploys` did not check proposer is bonded

## 1 · Summary

Pre-fix, `block_creator.rs:298-306` allowed an *unbonded* proposer
to construct slash deploys. An attacker who briefly bonded a
validator to enter the proposer rotation, equivocated, was slashed,
and then attempted to propose again could feed an unbonded
proposer into `prepare_slashing_deploys`; the proposer would
return a syntactically-valid but semantically-invalid SlashDeploy.
Post-fix, the proposer-bond is checked early and the function
returns an empty deploy list for unbonded proposers.

## 2 · Discovery technique

**Primary**: Hypothesis `hypothesis_multi_epoch_state_machine.rs`
produced action sequences ending in the unbonded-proposer state;
the witness was shrunk to a 6-action sequence revealing the
unchecked precondition.

**Corroborating**: Rocq mechanization of the proposer's bond
predicate in `BugFixUnbondedProposer.v` proved that the post-fix
check is *necessary* — an unconditional `prepare_slashing_deploys`
admits inconsistent slash deploys under any model where validators
can unbond.

## 3 · Witness reproduction

```
cargo test -p casper --test mod -- slashing::pre_fix_bug_8
```

The fixture
[`casper/tests/slashing/pre_fix_bug_8.rs`](../../../../../casper/tests/slashing/pre_fix_bug_8.rs)
encodes the canonical bond → equivocate → slash → propose-while-
unbonded sequence; pre-fix the function returns a non-empty deploy
list; post-fix it returns empty.

## 4 · Classification trace

```
threat_class       = permitted_bug_fix
ledger_status      = confirmed_fixed_bug
action             = Keep pre_fix_bug_8.rs + post-fix anchor
                     prop_t_9_8_unbonded_proposer.rs
```

## 5 · Evidence stack

| Layer            | Artifact                                                                                          |
|------------------|---------------------------------------------------------------------------------------------------|
| Rocq theorem     | T-9.8 (`BugFixUnbondedProposer.v`)                                                                 |
| Hypothesis       | `hypothesis_multi_epoch_state_machine.rs`                                                          |
| Rust regression  | `pre_fix_bug_8.rs`, `prop_t_9_8_unbonded_proposer.rs`, `uc_22_unbonded_proposer.rs`                |
| Kani harness     | `received_authorization_requires_positive_bond_on_bounded_domain`                                  |
| Bug-fix manifest | [`../../design/09-bug-fixes-and-rationale.md §9.9`](../../design/09-bug-fixes-and-rationale.md)  |

**Stack depth: 5** (Rocq + Hypothesis + Rust + Kani + design).

## 6 · Lessons for the methodology

1. **Multi-epoch state matters**. The bug is invisible in a
   single-epoch test because the validator never unbonds. Multi-
   epoch Hypothesis tests are the canonical detector for this
   pattern.
2. **"Trust internal" is a defensive code smell**. The pre-fix
   `prepare_slashing_deploys` trusted its caller to only invoke it
   for bonded proposers. The methodology prefers *explicit early
   returns* over caller-trust contracts.
3. **Kani extends the Rocq theorem to the function-level domain**.
   T-9.8 is unbounded; the Kani harness proves the *function*
   satisfies it on the bounded bond-vector domain — operational
   evidence the Rocq theorem cannot give without a Rust extraction.
