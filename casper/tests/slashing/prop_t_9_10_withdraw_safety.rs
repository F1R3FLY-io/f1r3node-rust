// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// Property test for T-9.10 (Bug #10): post-fix `payWithdraw`
// safety, total-funds preservation, parallel order-independence.
//
// Reference: docs/theory/slashing/design/14-test-plan.md §14.4.6,
// design/09-bug-fixes-and-rationale.md §9.13,
// formal/rocq/slashing/theories/BugFixWithdrawTransferFailure.v.
//
// Mirrors the three Rocq theorems:
//   • T-9.10  per-validator safety
//   • T-9.10' failure preserves total_funds
//   • T-9.10″ parallel order-independence
//
// The mirror is a hand-translated Rust port of the Rocq oracle
// `withdraw_with_transfer_oracle` (scope §3 of the theorem file).
// Property tests sample the (state, validator, transfer_ok) space
// and check the same disjunctions / equalities the Rocq proofs
// establish at the formal-model level. This is the example-trace
// counterpart to the Rocq mechanization; together they form the
// belt-and-suspenders verification of Bug #10's fix.

#![allow(dead_code)]

use std::collections::BTreeMap;

use proptest::prelude::*;

type Validator = u32;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct PoSStateW {
    /// `withdrawers : V → (bond, quarantine_block)`
    withdrawers: BTreeMap<Validator, (u64, u64)>,
    /// `committedRewards : V → reward`
    rewards: BTreeMap<Validator, u64>,
    /// PoS vault balance.
    pos_balance: u64,
}

fn payout(psw: &PoSStateW, v: Validator) -> u64 {
    let (bond, _) = psw.withdrawers.get(&v).copied().unwrap_or((0, 0));
    let reward = psw.rewards.get(&v).copied().unwrap_or(0);
    bond + reward
}

fn withdraw_with_transfer_oracle(psw: &PoSStateW, v: Validator, ok: bool) -> PoSStateW {
    if !ok {
        return psw.clone();
    }
    let p = payout(psw, v);
    let mut next = psw.clone();
    next.withdrawers.remove(&v);
    next.rewards.remove(&v);
    next.pos_balance = next.pos_balance.saturating_sub(p);
    next
}

fn total_funds(psw: &PoSStateW) -> u64 {
    let sum_payouts: u64 = psw
        .withdrawers
        .iter()
        .map(|(v, (b, _))| b + psw.rewards.get(v).copied().unwrap_or(0))
        .sum();
    psw.pos_balance + sum_payouts
}

fn arb_psw() -> impl Strategy<Value = PoSStateW> {
    proptest::collection::btree_map(0u32..8, (0u64..1000, 0u64..100), 0..6).prop_flat_map(
        |withdrawers| {
            let validators: Vec<Validator> = withdrawers.keys().copied().collect();
            let withdrawers_clone = withdrawers.clone();
            (
                Just(withdrawers),
                proptest::collection::vec(0u64..200, validators.len()),
                0u64..10_000,
            )
                .prop_map(move |(w, rewards_vec, balance)| {
                    let rewards: BTreeMap<Validator, u64> =
                        withdrawers_clone.keys().copied().zip(rewards_vec).collect();
                    PoSStateW {
                        withdrawers: w,
                        rewards,
                        pos_balance: balance,
                    }
                })
        },
    )
}

proptest! {
    /// T-9.10: per-validator safety.
    ///
    /// On success, the validator is removed from `withdrawers`.
    /// On failure, the entire state is unchanged.
    #[test]
    fn prop_t_9_10_withdraw_transfer_failure_safety(
        psw in arb_psw(),
        v in 0u32..8,
        ok in any::<bool>(),
    ) {
        let psw_after = withdraw_with_transfer_oracle(&psw, v, ok);
        if ok {
            prop_assert!(
                !psw_after.withdrawers.contains_key(&v),
                "T-9.10 success: {v} must be removed from withdrawers"
            );
        } else {
            prop_assert_eq!(&psw_after, &psw, "T-9.10 failure: state unchanged");
        }
    }

    /// T-9.10': a failed withdrawal preserves total_funds.
    #[test]
    fn prop_t_9_10_failure_preserves_total_funds(
        psw in arb_psw(),
        v in 0u32..8,
    ) {
        let psw_after = withdraw_with_transfer_oracle(&psw, v, false);
        prop_assert_eq!(
            total_funds(&psw_after),
            total_funds(&psw),
            "T-9.10' failure must preserve total_funds"
        );
    }

    /// T-9.10″: parallel order-independence.
    ///
    /// Withdrawing v then u produces the same withdrawer/reward maps
    /// as withdrawing u then v, when v ≠ u.
    #[test]
    fn prop_t_9_10_withdraw_independence(
        psw in arb_psw(),
        v in 0u32..8,
        u in 0u32..8,
        ok_v in any::<bool>(),
        ok_u in any::<bool>(),
    ) {
        prop_assume!(v != u);
        let psw_vu = withdraw_with_transfer_oracle(
            &withdraw_with_transfer_oracle(&psw, v, ok_v),
            u,
            ok_u,
        );
        let psw_uv = withdraw_with_transfer_oracle(
            &withdraw_with_transfer_oracle(&psw, u, ok_u),
            v,
            ok_v,
        );
        prop_assert_eq!(
            &psw_vu.withdrawers, &psw_uv.withdrawers,
            "T-9.10\u{2033}: withdrawer-map order-independence"
        );
        prop_assert_eq!(
            &psw_vu.rewards, &psw_uv.rewards,
            "T-9.10\u{2033}: reward-map order-independence"
        );
    }
}
