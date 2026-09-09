use std::collections::BTreeSet;
use std::mem::size_of;

use proptest::prelude::*;
use proptest::test_runner::Config as ProptestConfig;

const VALIDATOR_COUNT: usize = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Validator {
    balance: i64,
    active: bool,
    halted: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct EpochState {
    validators: [Validator; VALIDATOR_COUNT],
    rewards: i64,
    withdrawals: i64,
    active_generation: u64,
    authorized_issuance: i64,
    minted_through_epoch: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct HistoricalState {
    validators: [Validator; VALIDATOR_COUNT],
    rewards: i64,
    withdrawals: i64,
    active_generation: u64,
    authorized_issuance: i64,
    receipts: BTreeSet<(usize, i64)>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CloseError {
    FrontierGap,
    Arithmetic,
}

fn eligible(validator: Validator) -> bool { validator.active && !validator.halted }

fn prepared_from(pre_state: &EpochState) -> EpochState {
    let mut prepared = pre_state.clone();
    prepared.rewards += 1;
    prepared.withdrawals += 1;
    prepared.active_generation += 1;
    prepared
}

fn frontier_accepts_new_epoch(frontier: i64, epoch: i64) -> bool {
    (frontier == -1 && matches!(epoch, 0 | 1)) || (frontier >= 0 && epoch == frontier + 1)
}

fn implementation_close(
    pre_state: &EpochState,
    prepared_state: &EpochState,
    epoch: i64,
    amount: i64,
    order: [usize; VALIDATOR_COUNT],
) -> Result<EpochState, CloseError> {
    if epoch <= pre_state.minted_through_epoch {
        return Ok(prepared_state.clone());
    }
    if !frontier_accepts_new_epoch(pre_state.minted_through_epoch, epoch) {
        return Err(CloseError::FrontierGap);
    }

    let mut candidate = prepared_state.clone();
    candidate.authorized_issuance = pre_state.authorized_issuance;
    for validator_index in order {
        let validator = candidate.validators[validator_index];
        if !eligible(validator) || amount == 0 {
            continue;
        }
        candidate.validators[validator_index].balance = validator
            .balance
            .checked_add(amount)
            .ok_or(CloseError::Arithmetic)?;
        candidate.authorized_issuance = candidate
            .authorized_issuance
            .checked_add(amount)
            .ok_or(CloseError::Arithmetic)?;
    }
    candidate.minted_through_epoch = epoch;
    Ok(candidate)
}

fn specification_close(
    pre_state: &EpochState,
    prepared_state: &EpochState,
    epoch: i64,
    amount: i64,
) -> Result<EpochState, CloseError> {
    if epoch <= pre_state.minted_through_epoch {
        return Ok(prepared_state.clone());
    }
    if !frontier_accepts_new_epoch(pre_state.minted_through_epoch, epoch) {
        return Err(CloseError::FrontierGap);
    }

    let eligible_indices: Vec<_> = prepared_state
        .validators
        .iter()
        .copied()
        .enumerate()
        .filter_map(|(index, validator)| eligible(validator).then_some(index))
        .collect();
    let total = amount
        .checked_mul(i64::try_from(eligible_indices.len()).unwrap())
        .ok_or(CloseError::Arithmetic)?;
    pre_state
        .authorized_issuance
        .checked_add(total)
        .ok_or(CloseError::Arithmetic)?;
    if amount > 0
        && eligible_indices.iter().any(|index| {
            prepared_state.validators[*index]
                .balance
                .checked_add(amount)
                .is_none()
        })
    {
        return Err(CloseError::Arithmetic);
    }

    let mut committed = prepared_state.clone();
    committed.authorized_issuance = pre_state.authorized_issuance + total;
    for index in eligible_indices {
        committed.validators[index].balance += amount;
    }
    committed.minted_through_epoch = epoch;
    Ok(committed)
}

fn historical_close(state: &mut HistoricalState, epoch: i64, amount: i64) {
    state.rewards += 1;
    state.withdrawals += 1;
    state.active_generation += 1;
    for index in 0..VALIDATOR_COUNT {
        if eligible(state.validators[index]) && !state.receipts.contains(&(index, epoch)) {
            state.validators[index].balance += amount;
            state.authorized_issuance += amount;
            state.receipts.insert((index, epoch));
        }
    }
}

fn order_from_keys(keys: [u16; VALIDATOR_COUNT]) -> [usize; VALIDATOR_COUNT] {
    let mut keyed = [(keys[0], 0), (keys[1], 1), (keys[2], 2)];
    keyed.sort_unstable();
    [keyed[0].1, keyed[1].1, keyed[2].1]
}

fn balance_strategy() -> impl Strategy<Value = i64> {
    prop_oneof![0i64..=1000, (i64::MAX - 32)..=i64::MAX]
}

fn validator_strategy() -> impl Strategy<Value = Validator> {
    (balance_strategy(), any::<bool>(), any::<bool>()).prop_map(|(balance, active, halted)| {
        Validator {
            balance,
            active,
            halted,
        }
    })
}

fn state_strategy() -> impl Strategy<Value = EpochState> {
    (
        prop::array::uniform3(validator_strategy()),
        0i64..=1000,
        0i64..=1000,
        0u64..=100,
        prop_oneof![0i64..=1000, (i64::MAX - 64)..=i64::MAX],
        -1i64..=5,
    )
        .prop_map(
            |(
                validators,
                rewards,
                withdrawals,
                active_generation,
                authorized_issuance,
                minted_through_epoch,
            )| EpochState {
                validators,
                rewards,
                withdrawals,
                active_generation,
                authorized_issuance,
                minted_through_epoch,
            },
        )
}

fn legal_epoch(frontier: i64, epoch_zero_bootstrap: bool) -> i64 {
    if frontier == -1 {
        if epoch_zero_bootstrap {
            0
        } else {
            1
        }
    } else {
        frontier + 1
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1024))]

    #[test]
    fn sequential_mint_refines_atomic_epoch_close(
        pre_state in state_strategy(),
        epoch in 0i64..=8,
        amount in 0i64..=32,
        order_keys in prop::array::uniform3(any::<u16>()),
    ) {
        let prepared_state = prepared_from(&pre_state);
        let implementation = implementation_close(
            &pre_state,
            &prepared_state,
            epoch,
            amount,
            order_from_keys(order_keys),
        );
        let specification = specification_close(&pre_state, &prepared_state, epoch, amount);
        prop_assert_eq!(implementation, specification);
    }

    #[test]
    fn validator_permutations_preserve_the_committed_state(
        pre_state in state_strategy(),
        epoch_zero_bootstrap in any::<bool>(),
        amount in 0i64..=32,
        left_keys in prop::array::uniform3(any::<u16>()),
        right_keys in prop::array::uniform3(any::<u16>()),
    ) {
        let epoch = legal_epoch(pre_state.minted_through_epoch, epoch_zero_bootstrap);
        let prepared_state = prepared_from(&pre_state);
        let left = implementation_close(
            &pre_state,
            &prepared_state,
            epoch,
            amount,
            order_from_keys(left_keys),
        );
        let right = implementation_close(
            &pre_state,
            &prepared_state,
            epoch,
            amount,
            order_from_keys(right_keys),
        );
        prop_assert_eq!(left, right);
    }

    #[test]
    fn failed_epoch_close_restores_every_field(
        mut pre_state in state_strategy(),
        epoch_zero_bootstrap in any::<bool>(),
        amount in 1i64..=32,
        failing_validator in 0usize..VALIDATOR_COUNT,
        order_keys in prop::array::uniform3(any::<u16>()),
    ) {
        for validator in &mut pre_state.validators {
            validator.active = true;
            validator.halted = false;
            validator.balance = 0;
        }
        pre_state.validators[failing_validator].balance = i64::MAX;
        pre_state.authorized_issuance = 0;
        let epoch = legal_epoch(pre_state.minted_through_epoch, epoch_zero_bootstrap);
        let prepared_state = prepared_from(&pre_state);
        let result = implementation_close(
            &pre_state,
            &prepared_state,
            epoch,
            amount,
            order_from_keys(order_keys),
        );
        prop_assert_eq!(result, Err(CloseError::Arithmetic));
        prop_assert_eq!(pre_state.validators[failing_validator].balance, i64::MAX);
    }

    #[test]
    fn zero_issuance_advances_the_frontier_without_changing_balances(
        pre_state in state_strategy(),
        epoch_zero_bootstrap in any::<bool>(),
        order_keys in prop::array::uniform3(any::<u16>()),
    ) {
        let epoch = legal_epoch(pre_state.minted_through_epoch, epoch_zero_bootstrap);
        let prepared_state = prepared_from(&pre_state);
        let committed = implementation_close(
            &pre_state,
            &prepared_state,
            epoch,
            0,
            order_from_keys(order_keys),
        ).unwrap();
        for index in 0..VALIDATOR_COUNT {
            prop_assert_eq!(
                committed.validators[index].balance,
                prepared_state.validators[index].balance,
            );
        }
        prop_assert_eq!(committed.authorized_issuance, pre_state.authorized_issuance);
        prop_assert_eq!(committed.minted_through_epoch, epoch);
    }

    #[test]
    fn repeated_successful_close_never_credits_twice(
        pre_state in state_strategy(),
        epoch_zero_bootstrap in any::<bool>(),
        amount in 0i64..=32,
        order_keys in prop::array::uniform3(any::<u16>()),
    ) {
        let epoch = legal_epoch(pre_state.minted_through_epoch, epoch_zero_bootstrap);
        let order = order_from_keys(order_keys);
        let prepared_state = prepared_from(&pre_state);
        if let Ok(first) = implementation_close(&pre_state, &prepared_state, epoch, amount, order) {
            let second_prepared = prepared_from(&first);
            let second = implementation_close(&first, &second_prepared, epoch, amount, order)
                .unwrap();
            for index in 0..VALIDATOR_COUNT {
                prop_assert_eq!(second.validators[index].balance, first.validators[index].balance);
            }
            prop_assert_eq!(second.authorized_issuance, first.authorized_issuance);
            prop_assert_eq!(second.minted_through_epoch, first.minted_through_epoch);
        }
    }

    #[test]
    fn skipped_epoch_is_rejected_without_state_change(
        pre_state in state_strategy(),
        gap in 2i64..=8,
        amount in 0i64..=32,
        order_keys in prop::array::uniform3(any::<u16>()),
    ) {
        let epoch = if pre_state.minted_through_epoch == -1 {
            gap
        } else {
            pre_state.minted_through_epoch + gap
        };
        let prepared_state = prepared_from(&pre_state);
        let result = implementation_close(
            &pre_state,
            &prepared_state,
            epoch,
            amount,
            order_from_keys(order_keys),
        );
        prop_assert_eq!(result, Err(CloseError::FrontierGap));
    }

    #[test]
    fn older_epoch_preserves_issuance_and_frontier(
        pre_state in state_strategy().prop_filter("requires a completed epoch", |state| {
            state.minted_through_epoch >= 0
        }),
        amount in 0i64..=32,
        order_keys in prop::array::uniform3(any::<u16>()),
    ) {
        let epoch = pre_state.minted_through_epoch;
        let prepared_state = prepared_from(&pre_state);
        let committed = implementation_close(
            &pre_state,
            &prepared_state,
            epoch,
            amount,
            order_from_keys(order_keys),
        ).unwrap();
        prop_assert_eq!(committed.validators, prepared_state.validators);
        prop_assert_eq!(committed.authorized_issuance, pre_state.authorized_issuance);
        prop_assert_eq!(committed.minted_through_epoch, pre_state.minted_through_epoch);
    }

    #[test]
    fn play_and_replay_use_the_same_frontier_transition(
        pre_state in state_strategy(),
        epoch in 0i64..=8,
        amount in 0i64..=32,
        order_keys in prop::array::uniform3(any::<u16>()),
    ) {
        let prepared_state = prepared_from(&pre_state);
        let order = order_from_keys(order_keys);
        let play = implementation_close(&pre_state, &prepared_state, epoch, amount, order);
        let replay = implementation_close(&pre_state, &prepared_state, epoch, amount, order);
        prop_assert_eq!(play, replay);
    }

    #[test]
    fn frontier_refines_historical_receipts_on_legal_traces(
        epoch_zero_bootstrap in any::<bool>(),
        epochs in proptest::collection::vec(
            (prop::array::uniform3((any::<bool>(), any::<bool>())), 0i64..=8),
            1..64,
        ),
    ) {
        let validators = [Validator { balance: 0, active: true, halted: false }; VALIDATOR_COUNT];
        let mut frontier = EpochState {
            validators,
            rewards: 0,
            withdrawals: 0,
            active_generation: 0,
            authorized_issuance: 0,
            minted_through_epoch: -1,
        };
        let mut historical = HistoricalState {
            validators,
            rewards: 0,
            withdrawals: 0,
            active_generation: 0,
            authorized_issuance: 0,
            receipts: BTreeSet::new(),
        };
        let mut epoch = if epoch_zero_bootstrap { 0 } else { 1 };
        for (eligibility, amount) in epochs {
            for (index, (active, halted)) in eligibility.into_iter().enumerate() {
                frontier.validators[index].active = active;
                frontier.validators[index].halted = halted;
                historical.validators[index].active = active;
                historical.validators[index].halted = halted;
            }
            let prepared = prepared_from(&frontier);
            frontier = implementation_close(&frontier, &prepared, epoch, amount, [0, 1, 2])
                .unwrap();
            historical_close(&mut historical, epoch, amount);
            prop_assert_eq!(frontier.validators, historical.validators);
            prop_assert_eq!(frontier.rewards, historical.rewards);
            prop_assert_eq!(frontier.withdrawals, historical.withdrawals);
            prop_assert_eq!(frontier.active_generation, historical.active_generation);
            prop_assert_eq!(frontier.authorized_issuance, historical.authorized_issuance);
            prop_assert_eq!(frontier.minted_through_epoch, epoch);
            epoch += 1;
        }
    }
}

#[test]
fn replay_protection_storage_is_constant_across_many_epochs() {
    let initial_size = size_of::<EpochState>();
    let mut state = EpochState {
        validators: [Validator {
            balance: 0,
            active: false,
            halted: false,
        }; VALIDATOR_COUNT],
        rewards: 0,
        withdrawals: 0,
        active_generation: 0,
        authorized_issuance: 0,
        minted_through_epoch: -1,
    };
    for epoch in 1..=100_000 {
        let prepared = prepared_from(&state);
        state = implementation_close(&state, &prepared, epoch, 0, [0, 1, 2]).unwrap();
        assert_eq!(size_of::<EpochState>(), initial_size);
    }
    assert_eq!(state.minted_through_epoch, 100_000);
}
