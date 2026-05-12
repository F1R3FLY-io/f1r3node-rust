# Case study #4 — PoS transfer-failure FIXME

## 1 · Summary

Pre-fix, the Rholang `PoS` contract's slash function transferred the
slashed bond to the coop vault via
`posVault!("transfer", coopMultiVaultAddr, valBond, posAuthKey, *transferDoneCh)`.
If the transfer failed, the continuation `for (_ <- transferDoneCh)` never
fired and the slash deploy hung indefinitely. The validator stayed in
`SlashPending`; replay failed to converge. Post-fix, an alternate
continuation listens for an error signal and returns
`(false, "transfer failed")` deterministically.

## 2 · Discovery technique

**Primary**: FIXME audit pass. The pre-fix code carried the literal
comment *“FIXME handle transfer failing case”* at `PoS.rhox:469`.

**Corroborating**: Rocq mechanization of the slash transition under
a transfer oracle (`BugFixTransferFailure.v`). The Rocq theorem
T-9.4 proves the post-fix transition terminates in finite time
regardless of the transfer outcome.

## 3 · Witness reproduction

```
cargo test -p casper --test mod -- slashing::pre_fix_bug_4
```

The fixture
[`casper/tests/slashing/pre_fix_bug_4.rs`](../../../../../casper/tests/slashing/pre_fix_bug_4.rs)
encodes a transfer-failure scenario; pre-fix the slash deploy hangs;
post-fix it returns `false` deterministically with the validator's
bond unchanged.

## 4 · Classification trace

```
threat_class       = permitted_bug_fix (FIXME annotation = known defect)
ledger_status      = confirmed_fixed_bug
action             = Keep pre_fix_bug_4.rs + post-fix anchor uc_13_transfer_failure.rs
```

## 5 · Evidence stack

| Layer            | Artifact                                                                                        |
|------------------|-------------------------------------------------------------------------------------------------|
| Rocq theorem     | `t_9_4_transfer_failure_safety` (`BugFixTransferFailure.v:40`)                                  |
| Rust regression  | `pre_fix_bug_4.rs`, `prop_t_9_4_transfer_failure.rs`, `uc_13_transfer_failure.rs`               |
| Rholang fix      | `casper/src/main/resources/PoS.rhox` (transfer failure continuation)                            |
| Bug-fix manifest | [`../../design/09-bug-fixes-and-rationale.md §9.5`](../../design/09-bug-fixes-and-rationale.md) |

**Stack depth: 4** (Rocq + Rust regression + Rholang + design).

## 6 · Lessons for the methodology

1. **FIXME comments are first-class evidence**. A FIXME with no
   ticket is a known bug; the methodology mandates a FIXME audit
   pass as the second step of any port (after the TODO audit).
2. **Rholang code requires its own Rocq mechanization**. The slash
   transition is implemented in Rholang; the Rocq theorem
   T-9.4 proves the transition's safety under a transfer oracle
   that abstracts the failure mode. Without the oracle, the proof
   would be over the *production transfer's exact failure surface*
   and would be untestable.
3. **Termination is a slashing-correctness property, not just a
   liveness one**. A slash deploy that hangs causes the validator's
   stake to remain at risk forever; this is observable from the
   protocol level and is therefore a soundness concern, not just a
   QoS concern.
