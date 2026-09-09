use proptest::prelude::*;
use proptest::test_runner::Config as ProptestConfig;

const VALIDATORS: usize = 2;
const EPOCHS: usize = 3;
const INITIAL_AMOUNT: u64 = 3;
const EPOCH_AMOUNT: u64 = 1;
const BASE_FUNDS: [u64; VALIDATORS] = [0, 5];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    Absent,
    Bonded,
    Active,
    Withdrawing,
    Quarantined,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ValidatorState {
    phase: Phase,
    halted: bool,
    generation: Option<u8>,
    custody: u64,
    stake: u64,
    authorized_issued: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LifecycleState {
    epoch: u8,
    at_boundary: bool,
    validators: [ValidatorState; VALIDATORS],
    initial_credit: [u64; VALIDATORS],
    epoch_credit: [[u64; EPOCHS]; VALIDATORS],
    minted_through_epoch: Option<u8>,
}

#[derive(Clone, Debug)]
enum Action {
    Bond { validator: usize, amount: u64 },
    BeginEpoch,
    Activate { validator: usize },
    CloseEpoch,
    Withdraw { validator: usize },
    CompleteWithdrawal { validator: usize },
    Slash { validator: usize },
    Redeem { validator: usize },
}

impl LifecycleState {
    fn genesis() -> Self {
        Self {
            epoch: 0,
            at_boundary: false,
            validators: [
                ValidatorState {
                    phase: Phase::Active,
                    halted: false,
                    generation: Some(0),
                    custody: INITIAL_AMOUNT,
                    stake: 0,
                    authorized_issued: INITIAL_AMOUNT,
                },
                ValidatorState {
                    phase: Phase::Absent,
                    halted: false,
                    generation: None,
                    custody: BASE_FUNDS[1],
                    stake: 0,
                    authorized_issued: 0,
                },
            ],
            initial_credit: [INITIAL_AMOUNT, 0],
            epoch_credit: [[0; EPOCHS]; VALIDATORS],
            minted_through_epoch: None,
        }
    }

    fn apply(&mut self, action: &Action) -> bool {
        match *action {
            Action::Bond { validator, amount } => {
                let state = &mut self.validators[validator];
                if state.phase != Phase::Absent
                    || amount == 0
                    || amount > state.custody
                    || state.generation == Some(u8::MAX)
                {
                    return false;
                }
                state.custody -= amount;
                state.stake = amount;
                state.generation = Some(state.generation.map_or(0, |value| value + 1));
                state.phase = Phase::Bonded;
                true
            }
            Action::BeginEpoch => {
                if self.at_boundary || usize::from(self.epoch) + 1 >= EPOCHS {
                    return false;
                }
                self.epoch += 1;
                self.at_boundary = true;
                true
            }
            Action::Activate { validator } => {
                let state = &mut self.validators[validator];
                if !self.at_boundary || state.phase != Phase::Bonded {
                    return false;
                }
                state.phase = Phase::Active;
                true
            }
            Action::CloseEpoch => {
                let frontier_accepts = match self.minted_through_epoch {
                    None => self.epoch == 1,
                    Some(frontier) => self.epoch == frontier + 1,
                };
                if !self.at_boundary || !frontier_accepts {
                    return false;
                }
                for (validator, state) in self.validators.iter_mut().enumerate() {
                    if state.phase == Phase::Active && !state.halted {
                        state.custody += EPOCH_AMOUNT;
                        state.authorized_issued += EPOCH_AMOUNT;
                        self.epoch_credit[validator][usize::from(self.epoch)] = EPOCH_AMOUNT;
                    }
                }
                self.minted_through_epoch = Some(self.epoch);
                self.at_boundary = false;
                true
            }
            Action::Withdraw { validator } => {
                let state = &mut self.validators[validator];
                if !matches!(state.phase, Phase::Bonded | Phase::Active) {
                    return false;
                }
                state.phase = Phase::Withdrawing;
                true
            }
            Action::CompleteWithdrawal { validator } => {
                let state = &mut self.validators[validator];
                if state.phase != Phase::Withdrawing {
                    return false;
                }
                state.custody += state.stake;
                state.stake = 0;
                state.phase = Phase::Absent;
                true
            }
            Action::Slash { validator } => {
                let state = &mut self.validators[validator];
                if !matches!(
                    state.phase,
                    Phase::Bonded | Phase::Active | Phase::Withdrawing
                ) {
                    return false;
                }
                state.phase = Phase::Quarantined;
                state.halted = true;
                true
            }
            Action::Redeem { validator } => {
                let state = &mut self.validators[validator];
                if state.phase != Phase::Quarantined {
                    return false;
                }
                state.phase = Phase::Active;
                state.halted = false;
                true
            }
        }
    }

    fn assert_invariants(&self) -> Result<(), TestCaseError> {
        prop_assert_eq!(self.initial_credit, [INITIAL_AMOUNT, 0]);
        for (validator, base_funds) in BASE_FUNDS.iter().copied().enumerate() {
            let state = &self.validators[validator];
            prop_assert_eq!(
                state.custody + state.stake,
                base_funds + state.authorized_issued
            );
            for epoch in 0..EPOCHS {
                let credit = self.epoch_credit[validator][epoch];
                prop_assert!(credit <= EPOCH_AMOUNT);
                if credit > 0 {
                    let frontier_covers_epoch = self
                        .minted_through_epoch
                        .is_some_and(|frontier| epoch <= usize::from(frontier));
                    prop_assert!(frontier_covers_epoch);
                }
            }
        }
        if let Some(frontier) = self.minted_through_epoch {
            prop_assert!(frontier <= self.epoch);
        }
        Ok(())
    }
}

fn action_strategy() -> impl Strategy<Value = Action> {
    prop_oneof![
        (0usize..VALIDATORS, 0u64..=6)
            .prop_map(|(validator, amount)| Action::Bond { validator, amount }),
        Just(Action::BeginEpoch),
        (0usize..VALIDATORS).prop_map(|validator| Action::Activate { validator }),
        Just(Action::CloseEpoch),
        (0usize..VALIDATORS).prop_map(|validator| Action::Withdraw { validator }),
        (0usize..VALIDATORS).prop_map(|validator| Action::CompleteWithdrawal { validator }),
        (0usize..VALIDATORS).prop_map(|validator| Action::Slash { validator }),
        (0usize..VALIDATORS).prop_map(|validator| Action::Redeem { validator }),
    ]
}

fn local_action_strategy(validator: usize) -> impl Strategy<Value = Action> {
    (0u8..6, 0u64..=6).prop_map(move |(kind, amount)| match kind {
        0 => Action::Bond { validator, amount },
        1 => Action::Activate { validator },
        2 => Action::Withdraw { validator },
        3 => Action::CompleteWithdrawal { validator },
        4 => Action::Slash { validator },
        _ => Action::Redeem { validator },
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1024))]

    #[test]
    fn generated_lifecycles_preserve_extracted_formal_invariants(
        actions in proptest::collection::vec(action_strategy(), 0..128),
    ) {
        let mut state = LifecycleState::genesis();
        state.assert_invariants()?;
        for action in actions {
            let before = state.clone();
            let applied = state.apply(&action);
            state.assert_invariants()?;
            if let (Some(before_frontier), Some(after_frontier)) =
                (before.minted_through_epoch, state.minted_through_epoch)
            {
                prop_assert!(after_frontier >= before_frontier);
            }
            if state.validators.iter().zip(before.validators.iter()).any(
                |(after, prior)| after.generation != prior.generation,
            ) {
                prop_assert!(applied);
                let was_bond = matches!(action, Action::Bond { .. });
                prop_assert!(was_bond);
            }
        }
    }

    #[test]
    fn generated_lifecycles_replay_identically(
        actions in proptest::collection::vec(action_strategy(), 0..128),
    ) {
        let mut play = LifecycleState::genesis();
        let mut replay = LifecycleState::genesis();
        for action in &actions {
            prop_assert_eq!(play.apply(action), replay.apply(action));
        }
        prop_assert_eq!(play, replay);
    }

    #[test]
    fn distinct_validator_actions_commute(
        prefix in proptest::collection::vec(action_strategy(), 0..64),
        left in local_action_strategy(0),
        right in local_action_strategy(1),
    ) {
        let mut start = LifecycleState::genesis();
        for action in &prefix {
            start.apply(action);
        }
        let mut left_then_right = start.clone();
        left_then_right.apply(&left);
        left_then_right.apply(&right);
        let mut right_then_left = start;
        right_then_left.apply(&right);
        right_then_left.apply(&left);
        prop_assert_eq!(left_then_right, right_then_left);
    }
}
