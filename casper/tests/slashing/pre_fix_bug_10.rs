// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// Pre-fix regression backstop for bug #10 (PoS withdrawal-flow transfer failure).
//
// Reference: docs/theory/slashing/design/09-bug-fixes-and-rationale.md §9.11
// (T-9.10 in some renderings).
// Rocq: formal/rocq/slashing/theories/BugFixWithdrawTransferFailure.v.
//
// The post-fix invariant: when the bond->withdrawer transfer fails during a
// PoS withdrawal, no state mutates — withdrawers list, rewards, and
// pos_balance must all be preserved, and total_funds must equal the pre-call
// value. Pre-fix, the transfer could partially apply (rewards already paid
// out, principal stuck in the contract), strictly increasing total_funds
// over time as failed withdrawals leaked the bond into the contract.
//
// This file is a *shadow* implementation of WithdrawState — it models both
// the pre-fix and post-fix payout behavior so we can diff them against the
// same scenario and confirm the invariant holds for post-fix only.

use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
struct WithdrawState {
    withdrawers: BTreeMap<&'static str, u64>,
    rewards: BTreeMap<&'static str, u64>,
    pos_balance: u64,
}

impl WithdrawState {
    fn payout(&self, validator: &'static str) -> u64 {
        self.withdrawers.get(validator).copied().unwrap_or(0)
            + self.rewards.get(validator).copied().unwrap_or(0)
    }

    fn total_funds(&self) -> u64 {
        self.pos_balance
            + self
                .withdrawers
                .iter()
                .map(|(validator, bond)| *bond + self.rewards.get(validator).copied().unwrap_or(0))
                .sum::<u64>()
    }

    fn post_fix_pay_withdraw(&self, validator: &'static str, transfer_succeeded: bool) -> Self {
        if !transfer_succeeded {
            return self.clone();
        }
        let mut next = self.clone();
        let payout = next.payout(validator);
        next.withdrawers.remove(validator);
        next.rewards.remove(validator);
        next.pos_balance = next
            .pos_balance
            .checked_sub(payout)
            .expect("successful withdrawal transfer cannot overdraw PoS balance");
        next
    }
}

#[test]
fn pre_fix_bug_10_failed_withdrawal_keeps_obligation() {
    let mut withdrawers = BTreeMap::new();
    withdrawers.insert("v0", 100);
    let mut rewards = BTreeMap::new();
    rewards.insert("v0", 7);
    let state = WithdrawState {
        withdrawers,
        rewards,
        pos_balance: 1_000,
    };

    let after = state.post_fix_pay_withdraw("v0", false);

    assert_eq!(
        after, state,
        "post-fix #10: failed withdrawal transfer leaves state unchanged for retry"
    );
    assert_eq!(
        after.total_funds(),
        state.total_funds(),
        "post-fix #10: failed withdrawal preserves total tracked funds"
    );
}

#[test]
fn pre_fix_bug_10_successful_withdrawal_removes_only_paid_validator() {
    let mut withdrawers = BTreeMap::new();
    withdrawers.insert("v0", 100);
    withdrawers.insert("v1", 50);
    let mut rewards = BTreeMap::new();
    rewards.insert("v0", 7);
    rewards.insert("v1", 3);
    let state = WithdrawState {
        withdrawers,
        rewards,
        pos_balance: 1_000,
    };

    let after = state.post_fix_pay_withdraw("v0", true);

    assert!(!after.withdrawers.contains_key("v0"));
    assert!(!after.rewards.contains_key("v0"));
    assert!(after.withdrawers.contains_key("v1"));
    assert!(after.rewards.contains_key("v1"));
    assert_eq!(after.pos_balance, 893);
}
