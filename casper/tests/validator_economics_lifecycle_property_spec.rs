use std::collections::BTreeSet;

use proptest::prelude::*;
use proptest::test_runner::{Config as ProptestConfig, TestCaseError, TestCaseResult};

const VALIDATORS: usize = 2;
const PROPOSALS: usize = 3;
const HANDLER_COST: u64 = 3;
const CLIENT_FEE: u64 = 1;
const BOND_AMOUNT: u64 = 2;
const EPOCH_AMOUNT: u64 = 1;
const INITIAL_TOTAL: u128 = 24;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    Absent,
    Active,
    Withdrawing,
    Quarantined,
    Burned,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct EconomicsState {
    general: [u64; VALIDATORS],
    fuel: [u64; VALIDATORS],
    stake: [u64; VALIDATORS],
    quarantine: [u64; VALIDATORS],
    phase: [Phase; VALIDATORS],
    generation: [Option<u8>; VALIDATORS],
    quarantine_generation: [Option<u8>; VALIDATORS],
    halted: [bool; VALIDATORS],
    issued: u64,
    burned: u64,
    minted: BTreeSet<(usize, u8)>,
    resolved: BTreeSet<(usize, u8)>,
    reserved: [u64; VALIDATORS],
    owner: [Option<usize>; PROPOSALS],
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Action {
    TopUp {
        source: usize,
        validator: usize,
        amount: u64,
    },
    Execute {
        client: usize,
        proposer: usize,
    },
    Reserve {
        proposal: usize,
        validator: usize,
    },
    Commit {
        proposal: usize,
    },
    Cancel {
        proposal: usize,
    },
    IssueEpoch {
        validator: usize,
        epoch: u8,
    },
    RequestWithdrawal {
        validator: usize,
    },
    CompleteWithdrawal {
        validator: usize,
    },
    Bond {
        validator: usize,
    },
    Slash {
        validator: usize,
    },
    Vindicate {
        validator: usize,
        generation: u8,
    },
    Guilty {
        validator: usize,
        generation: u8,
    },
    BurnQuarantine {
        validator: usize,
        generation: u8,
    },
}

impl EconomicsState {
    fn initial() -> Self {
        Self {
            general: [4, 6],
            fuel: [6, 6],
            stake: [2, 0],
            quarantine: [0, 0],
            phase: [Phase::Active, Phase::Absent],
            generation: [Some(0), None],
            quarantine_generation: [None, None],
            halted: [false, false],
            issued: 0,
            burned: 0,
            minted: BTreeSet::new(),
            resolved: BTreeSet::new(),
            reserved: [0, 0],
            owner: [None, None, None],
        }
    }

    fn apply(&mut self, action: &Action) -> bool {
        let mut candidate = self.clone();
        if !candidate.apply_valid(action) {
            return false;
        }
        *self = candidate;
        true
    }

    fn apply_valid(&mut self, action: &Action) -> bool {
        match *action {
            Action::TopUp {
                source,
                validator,
                amount,
            } => {
                if amount == 0
                    || self.general[source] < amount
                    || self.phase[validator] == Phase::Quarantined
                {
                    return false;
                }
                let Some(next_fuel) = self.fuel[validator].checked_add(amount) else {
                    return false;
                };
                self.general[source] -= amount;
                self.fuel[validator] = next_fuel;
                true
            }
            Action::Execute { client, proposer } => {
                let Some(available_fuel) = self.fuel[proposer].checked_sub(self.reserved[proposer])
                else {
                    return false;
                };
                if self.phase[proposer] != Phase::Active
                    || self.halted[proposer]
                    || available_fuel < HANDLER_COST
                    || self.general[client] < CLIENT_FEE
                {
                    return false;
                }
                let Some(next_burned) = self.burned.checked_add(HANDLER_COST) else {
                    return false;
                };
                self.general[client] -= CLIENT_FEE;
                let Some(next_proposer_general) = self.general[proposer].checked_add(CLIENT_FEE)
                else {
                    return false;
                };
                self.general[proposer] = next_proposer_general;
                self.fuel[proposer] -= HANDLER_COST;
                self.burned = next_burned;
                true
            }
            Action::Reserve {
                proposal,
                validator,
            } => {
                if self.owner[proposal].is_some()
                    || self.phase[validator] != Phase::Active
                    || self.halted[validator]
                    || self.fuel[validator] - self.reserved[validator] < HANDLER_COST
                {
                    return false;
                }
                self.reserved[validator] += HANDLER_COST;
                self.owner[proposal] = Some(validator);
                true
            }
            Action::Commit { proposal } => {
                let Some(validator) = self.owner[proposal] else {
                    return false;
                };
                if self.reserved[validator] < HANDLER_COST || self.fuel[validator] < HANDLER_COST {
                    return false;
                }
                let Some(next_burned) = self.burned.checked_add(HANDLER_COST) else {
                    return false;
                };
                self.reserved[validator] -= HANDLER_COST;
                self.fuel[validator] -= HANDLER_COST;
                self.owner[proposal] = None;
                self.burned = next_burned;
                true
            }
            Action::Cancel { proposal } => {
                let Some(validator) = self.owner[proposal] else {
                    return false;
                };
                if self.reserved[validator] < HANDLER_COST {
                    return false;
                }
                self.reserved[validator] -= HANDLER_COST;
                self.owner[proposal] = None;
                true
            }
            Action::IssueEpoch { validator, epoch } => {
                if self.phase[validator] != Phase::Active
                    || self.halted[validator]
                    || self.minted.contains(&(validator, epoch))
                {
                    return false;
                }
                let Some(next_fuel) = self.fuel[validator].checked_add(EPOCH_AMOUNT) else {
                    return false;
                };
                let Some(next_issued) = self.issued.checked_add(EPOCH_AMOUNT) else {
                    return false;
                };
                self.fuel[validator] = next_fuel;
                self.issued = next_issued;
                self.minted.insert((validator, epoch));
                true
            }
            Action::RequestWithdrawal { validator } => {
                if self.phase[validator] != Phase::Active {
                    return false;
                }
                self.phase[validator] = Phase::Withdrawing;
                true
            }
            Action::CompleteWithdrawal { validator } => {
                if self.phase[validator] != Phase::Withdrawing {
                    return false;
                }
                let Some(next_general) = self.general[validator].checked_add(self.stake[validator])
                else {
                    return false;
                };
                self.general[validator] = next_general;
                self.stake[validator] = 0;
                self.phase[validator] = Phase::Absent;
                true
            }
            Action::Bond { validator } => {
                if self.phase[validator] != Phase::Absent
                    || self.stake[validator] != 0
                    || self.general[validator] < BOND_AMOUNT
                {
                    return false;
                }
                let next_generation = match self.generation[validator] {
                    None => 0,
                    Some(generation) => {
                        let Some(next) = generation.checked_add(1) else {
                            return false;
                        };
                        next
                    }
                };
                self.general[validator] -= BOND_AMOUNT;
                self.stake[validator] = BOND_AMOUNT;
                self.generation[validator] = Some(next_generation);
                self.phase[validator] = Phase::Active;
                true
            }
            Action::Slash { validator } => {
                if !matches!(self.phase[validator], Phase::Active | Phase::Withdrawing)
                    || self.reserved[validator] != 0
                    || self.quarantine[validator] != 0
                {
                    return false;
                }
                self.quarantine[validator] = self.fuel[validator];
                self.fuel[validator] = 0;
                self.quarantine_generation[validator] = self.generation[validator];
                self.phase[validator] = Phase::Quarantined;
                self.halted[validator] = true;
                true
            }
            Action::Vindicate {
                validator,
                generation,
            } => self.resolve(validator, generation, Resolution::Vindicated),
            Action::Guilty {
                validator,
                generation,
            } => self.resolve(validator, generation, Resolution::Guilty),
            Action::BurnQuarantine {
                validator,
                generation,
            } => self.resolve(validator, generation, Resolution::Burned),
        }
    }

    fn resolve(&mut self, validator: usize, generation: u8, outcome: Resolution) -> bool {
        if self.phase[validator] != Phase::Quarantined
            || self.quarantine_generation[validator] != Some(generation)
            || self.resolved.contains(&(validator, generation))
        {
            return false;
        }
        let quarantined = self.quarantine[validator];
        match outcome {
            Resolution::Vindicated => {
                let Some(next_fuel) = self.fuel[validator].checked_add(quarantined) else {
                    return false;
                };
                self.fuel[validator] = next_fuel;
                self.phase[validator] = Phase::Active;
                self.halted[validator] = false;
            }
            Resolution::Guilty => {
                let penalty = u64::from(quarantined > 0);
                let beneficiary = 1 - validator;
                let Some(next_general) = self.general[beneficiary].checked_add(penalty) else {
                    return false;
                };
                let Some(next_fuel) = self.fuel[validator].checked_add(quarantined - penalty)
                else {
                    return false;
                };
                self.general[beneficiary] = next_general;
                self.fuel[validator] = next_fuel;
                self.phase[validator] = Phase::Active;
                self.halted[validator] = false;
            }
            Resolution::Burned => {
                let Some(next_burned) = self.burned.checked_add(quarantined) else {
                    return false;
                };
                self.burned = next_burned;
                self.phase[validator] = Phase::Burned;
            }
        }
        self.quarantine[validator] = 0;
        self.resolved.insert((validator, generation));
        true
    }

    fn assert_invariants(&self) -> TestCaseResult {
        let custody = self
            .general
            .iter()
            .chain(self.fuel.iter())
            .chain(self.stake.iter())
            .chain(self.quarantine.iter())
            .map(|amount| u128::from(*amount))
            .sum::<u128>();
        prop_assert_eq!(
            custody + u128::from(self.burned),
            INITIAL_TOTAL + u128::from(self.issued)
        );
        for validator in 0..VALIDATORS {
            let reservation_count = self
                .owner
                .iter()
                .filter(|owner| **owner == Some(validator))
                .count();
            prop_assert_eq!(
                self.reserved[validator],
                u64::try_from(reservation_count).unwrap() * HANDLER_COST
            );
            prop_assert!(self.reserved[validator] <= self.fuel[validator]);
            if self.phase[validator] != Phase::Quarantined {
                prop_assert_eq!(self.quarantine[validator], 0);
            }
            if self.phase[validator] == Phase::Quarantined {
                prop_assert!(self.halted[validator]);
                prop_assert_eq!(
                    self.quarantine_generation[validator],
                    self.generation[validator]
                );
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum Resolution {
    Vindicated,
    Guilty,
    Burned,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SnapshotRuntime {
    Ordinary,
    Replay,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ValidatorFuelSnapshot {
    root: u64,
    proposer: usize,
    balance: u64,
    runtime: SnapshotRuntime,
    captured_before_rig: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum FuelReplayError {
    ArithmeticOverflow,
    SnapshotDuringReplay,
    ReplayRuntimeSnapshot,
    LateSnapshot,
    WrongRoot,
    WrongProposer,
    StaleBalance,
    InsufficientFuel,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PlayFuelMachine {
    remaining: u64,
    burned: u64,
    admitted: Vec<u64>,
    deferred: Vec<u64>,
}

impl PlayFuelMachine {
    fn select(initial_fuel: u64, candidate_count: usize) -> Result<Self, FuelReplayError> {
        let mut state = Self {
            remaining: initial_fuel,
            burned: 0,
            admitted: Vec::with_capacity(candidate_count),
            deferred: Vec::new(),
        };
        for index in 0..candidate_count {
            if state.remaining < HANDLER_COST {
                state
                    .deferred
                    .extend(std::iter::repeat_n(HANDLER_COST, candidate_count - index));
                break;
            }
            state.remaining -= HANDLER_COST;
            state.burned = state
                .burned
                .checked_add(HANDLER_COST)
                .ok_or(FuelReplayError::ArithmeticOverflow)?;
            state.admitted.push(HANDLER_COST);
        }
        Ok(state)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ReplayFuelMachine {
    root: u64,
    proposer: usize,
    fuel: u64,
    burned: u64,
    certificate_cursor: usize,
    recorded_trace: Vec<u64>,
    rigged: bool,
}

impl ReplayFuelMachine {
    fn new(root: u64, proposer: usize, fuel: u64) -> Self {
        Self {
            root,
            proposer,
            fuel,
            burned: 0,
            certificate_cursor: 0,
            recorded_trace: Vec::new(),
            rigged: false,
        }
    }

    fn capture_snapshot(&self) -> Result<ValidatorFuelSnapshot, FuelReplayError> {
        if self.rigged {
            return Err(FuelReplayError::SnapshotDuringReplay);
        }
        Ok(ValidatorFuelSnapshot {
            root: self.root,
            proposer: self.proposer,
            balance: self.fuel,
            runtime: SnapshotRuntime::Ordinary,
            captured_before_rig: true,
        })
    }

    fn replay_one(
        &mut self,
        cost: u64,
        recorded_event: u64,
        snapshot: ValidatorFuelSnapshot,
    ) -> Result<(), FuelReplayError> {
        let mut candidate = self.clone();
        candidate.rigged = true;
        if snapshot.runtime != SnapshotRuntime::Ordinary {
            return Err(FuelReplayError::ReplayRuntimeSnapshot);
        }
        if !snapshot.captured_before_rig {
            return Err(FuelReplayError::LateSnapshot);
        }
        if snapshot.root != candidate.root {
            return Err(FuelReplayError::WrongRoot);
        }
        if snapshot.proposer != candidate.proposer {
            return Err(FuelReplayError::WrongProposer);
        }
        if snapshot.balance != candidate.fuel {
            return Err(FuelReplayError::StaleBalance);
        }
        let Some(next_fuel) = candidate.fuel.checked_sub(cost) else {
            return Err(FuelReplayError::InsufficientFuel);
        };
        let Some(next_burned) = candidate.burned.checked_add(cost) else {
            return Err(FuelReplayError::ArithmeticOverflow);
        };
        let Some(next_root) = candidate.root.checked_add(1) else {
            return Err(FuelReplayError::ArithmeticOverflow);
        };
        let Some(next_cursor) = candidate.certificate_cursor.checked_add(1) else {
            return Err(FuelReplayError::ArithmeticOverflow);
        };
        candidate.fuel = next_fuel;
        candidate.burned = next_burned;
        candidate.root = next_root;
        candidate.certificate_cursor = next_cursor;
        candidate.recorded_trace.push(recorded_event);
        candidate.rigged = false;
        *self = candidate;
        Ok(())
    }

    fn replay_admitted(
        root: u64,
        proposer: usize,
        initial_fuel: u64,
        admitted: &[u64],
    ) -> Result<Self, FuelReplayError> {
        let mut replay = Self::new(root, proposer, initial_fuel);
        for (index, cost) in admitted.iter().copied().enumerate() {
            let snapshot = replay.capture_snapshot()?;
            let event = u64::try_from(index).map_err(|_| FuelReplayError::ArithmeticOverflow)?;
            replay.replay_one(cost, event, snapshot)?;
        }
        Ok(replay)
    }
}

fn fuel_model_failure(error: FuelReplayError) -> TestCaseError {
    TestCaseError::fail(format!("fuel replay model failed: {error:?}"))
}

fn assert_transition(
    before: &EconomicsState,
    after: &EconomicsState,
    action: &Action,
    applied: bool,
) -> TestCaseResult {
    if !applied {
        prop_assert_eq!(after, before);
        return Ok(());
    }
    match *action {
        Action::TopUp {
            source,
            validator,
            amount,
        } => {
            prop_assert_eq!(after.general[source], before.general[source] - amount);
            prop_assert_eq!(after.fuel[validator], before.fuel[validator] + amount);
            prop_assert_eq!(after.issued, before.issued);
            prop_assert_eq!(after.burned, before.burned);
        }
        Action::Execute { client, proposer } => {
            let mut expected_general = before.general;
            expected_general[client] -= CLIENT_FEE;
            expected_general[proposer] += CLIENT_FEE;
            prop_assert_eq!(after.general, expected_general);
            prop_assert_eq!(after.fuel[proposer], before.fuel[proposer] - HANDLER_COST);
            prop_assert_eq!(after.burned, before.burned + HANDLER_COST);
            prop_assert_eq!(after.issued, before.issued);
        }
        Action::Reserve {
            proposal,
            validator,
        } => {
            prop_assert_eq!(after.general, before.general);
            prop_assert_eq!(after.fuel, before.fuel);
            prop_assert_eq!(
                after.reserved[validator],
                before.reserved[validator] + HANDLER_COST
            );
            prop_assert_eq!(after.owner[proposal], Some(validator));
        }
        Action::Commit { proposal } => {
            let validator = before.owner[proposal].unwrap();
            prop_assert_eq!(after.general, before.general);
            prop_assert_eq!(after.fuel[validator], before.fuel[validator] - HANDLER_COST);
            prop_assert_eq!(after.burned, before.burned + HANDLER_COST);
        }
        Action::Cancel { .. } => {
            prop_assert_eq!(after.general, before.general);
            prop_assert_eq!(after.fuel, before.fuel);
            prop_assert_eq!(after.burned, before.burned);
        }
        Action::IssueEpoch { validator, .. } => {
            prop_assert_eq!(after.general, before.general);
            prop_assert_eq!(after.fuel[validator], before.fuel[validator] + EPOCH_AMOUNT);
            prop_assert_eq!(after.issued, before.issued + EPOCH_AMOUNT);
        }
        Action::RequestWithdrawal { .. } => {
            prop_assert_eq!(after.general, before.general);
            prop_assert_eq!(after.fuel, before.fuel);
            prop_assert_eq!(after.stake, before.stake);
        }
        Action::CompleteWithdrawal { validator } => {
            prop_assert_eq!(after.fuel, before.fuel);
            prop_assert_eq!(
                after.general[validator],
                before.general[validator] + before.stake[validator]
            );
            prop_assert_eq!(after.stake[validator], 0);
        }
        Action::Bond { validator } => {
            prop_assert_eq!(after.fuel, before.fuel);
            prop_assert_eq!(
                after.general[validator],
                before.general[validator] - BOND_AMOUNT
            );
            prop_assert_eq!(
                after.stake[validator],
                before.stake[validator] + BOND_AMOUNT
            );
        }
        Action::Slash { validator } => {
            prop_assert_eq!(after.general, before.general);
            prop_assert_eq!(after.fuel[validator], 0);
            prop_assert_eq!(after.quarantine[validator], before.fuel[validator]);
            prop_assert_eq!(after.issued, before.issued);
            prop_assert_eq!(after.burned, before.burned);
        }
        Action::Vindicate { validator, .. } => {
            prop_assert_eq!(after.general, before.general);
            prop_assert_eq!(
                after.fuel[validator],
                before.fuel[validator] + before.quarantine[validator]
            );
            prop_assert_eq!(after.issued, before.issued);
        }
        Action::Guilty { validator, .. } => {
            let penalty = u64::from(before.quarantine[validator] > 0);
            let beneficiary = 1 - validator;
            prop_assert_eq!(
                after.general[beneficiary],
                before.general[beneficiary] + penalty
            );
            prop_assert_eq!(
                after.fuel[validator],
                before.fuel[validator] + before.quarantine[validator] - penalty
            );
            prop_assert_eq!(after.issued, before.issued);
            prop_assert_eq!(after.burned, before.burned);
        }
        Action::BurnQuarantine { validator, .. } => {
            prop_assert_eq!(after.general, before.general);
            prop_assert_eq!(after.fuel, before.fuel);
            prop_assert_eq!(after.burned, before.burned + before.quarantine[validator]);
            prop_assert_eq!(after.issued, before.issued);
        }
    }
    Ok(())
}

fn action_strategy() -> impl Strategy<Value = Action> {
    prop_oneof![
        (0usize..VALIDATORS, 0usize..VALIDATORS, 0u64..=3).prop_map(
            |(source, validator, amount)| Action::TopUp {
                source,
                validator,
                amount,
            }
        ),
        (0usize..VALIDATORS, 0usize..VALIDATORS)
            .prop_map(|(client, proposer)| Action::Execute { client, proposer }),
        (0usize..PROPOSALS, 0usize..VALIDATORS).prop_map(|(proposal, validator)| Action::Reserve {
            proposal,
            validator,
        }),
        (0usize..PROPOSALS).prop_map(|proposal| Action::Commit { proposal }),
        (0usize..PROPOSALS).prop_map(|proposal| Action::Cancel { proposal }),
        (0usize..VALIDATORS, 0u8..=4)
            .prop_map(|(validator, epoch)| Action::IssueEpoch { validator, epoch }),
        (0usize..VALIDATORS).prop_map(|validator| Action::RequestWithdrawal { validator }),
        (0usize..VALIDATORS).prop_map(|validator| Action::CompleteWithdrawal { validator }),
        (0usize..VALIDATORS).prop_map(|validator| Action::Bond { validator }),
        (0usize..VALIDATORS).prop_map(|validator| Action::Slash { validator }),
        (0usize..VALIDATORS, 0u8..=4).prop_map(|(validator, generation)| Action::Vindicate {
            validator,
            generation,
        }),
        (0usize..VALIDATORS, 0u8..=4).prop_map(|(validator, generation)| Action::Guilty {
            validator,
            generation,
        }),
        (0usize..VALIDATORS, 0u8..=4).prop_map(|(validator, generation)| Action::BurnQuarantine {
            validator,
            generation,
        }),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1024))]

    #[test]
    fn generated_lifecycles_preserve_every_extracted_economic_invariant(
        actions in proptest::collection::vec(action_strategy(), 0..128),
    ) {
        let mut state = EconomicsState::initial();
        state.assert_invariants()?;
        for action in actions {
            let before = state.clone();
            let applied = state.apply(&action);
            assert_transition(&before, &state, &action, applied)?;
            state.assert_invariants()?;
        }
    }

    #[test]
    fn independent_play_and_replay_select_the_same_maximal_prefix(
        initial_fuel in 0u64..=384,
        candidate_count in 0usize..128,
        root in 0u64..=10_000,
        proposer in 0usize..VALIDATORS,
    ) {
        let play =
            PlayFuelMachine::select(initial_fuel, candidate_count).map_err(fuel_model_failure)?;
        let left = ReplayFuelMachine::replay_admitted(
            root,
            proposer,
            initial_fuel,
            &play.admitted,
        )
        .map_err(fuel_model_failure)?;
        let right = ReplayFuelMachine::replay_admitted(
            root,
            proposer,
            initial_fuel,
            &play.admitted,
        )
        .map_err(fuel_model_failure)?;
        let expected_admitted =
            usize::try_from(initial_fuel / HANDLER_COST).unwrap_or(usize::MAX).min(candidate_count);
        prop_assert_eq!(play.admitted.len(), expected_admitted);
        prop_assert_eq!(play.deferred.len(), candidate_count - expected_admitted);
        prop_assert!(play.admitted.iter().all(|cost| *cost == HANDLER_COST));
        prop_assert!(play.deferred.iter().all(|cost| *cost == HANDLER_COST));
        prop_assert_eq!(play.remaining + play.burned, initial_fuel);
        prop_assert_eq!(left.fuel, play.remaining);
        prop_assert_eq!(left.burned, play.burned);
        prop_assert_eq!(left.fuel + left.burned, initial_fuel);
        prop_assert_eq!(left.certificate_cursor, play.admitted.len());
        prop_assert_eq!(left.recorded_trace.len(), play.admitted.len());
        prop_assert_eq!(
            left.root,
            root + u64::try_from(play.admitted.len()).unwrap_or(u64::MAX)
        );
        prop_assert_eq!(left, right);
    }

    #[test]
    fn invalid_replay_snapshots_are_atomic(
        initial_fuel in HANDLER_COST..=384,
        root in 0u64..=10_000,
        proposer in 0usize..VALIDATORS,
        defect in 0u8..5,
    ) {
        let mut replay = ReplayFuelMachine::new(root, proposer, initial_fuel);
        let mut snapshot = replay.capture_snapshot().map_err(fuel_model_failure)?;
        match defect {
            0 => snapshot.root += 1,
            1 => snapshot.proposer = (snapshot.proposer + 1) % VALIDATORS,
            2 => snapshot.balance += 1,
            3 => snapshot.runtime = SnapshotRuntime::Replay,
            _ => snapshot.captured_before_rig = false,
        }
        let before = replay.clone();
        prop_assert!(replay.replay_one(HANDLER_COST, 0, snapshot).is_err());
        prop_assert_eq!(replay, before);
    }

    #[test]
    fn disjoint_top_ups_and_reservations_commute(
        left_amount in 1u64..=3,
        right_amount in 1u64..=3,
    ) {
        let left_actions = [
            Action::TopUp { source: 0, validator: 0, amount: left_amount },
            Action::TopUp { source: 1, validator: 1, amount: right_amount },
            Action::Reserve { proposal: 0, validator: 0 },
            Action::Reserve { proposal: 1, validator: 1 },
        ];
        let right_actions = [
            left_actions[1].clone(),
            left_actions[0].clone(),
            left_actions[3].clone(),
            left_actions[2].clone(),
        ];
        let mut left = EconomicsState::initial();
        left.phase[1] = Phase::Active;
        left.generation[1] = Some(0);
        let mut right = left.clone();
        for action in &left_actions {
            prop_assert!(left.apply(action));
        }
        for action in &right_actions {
            prop_assert!(right.apply(action));
        }
        prop_assert_eq!(left, right);
    }

    #[test]
    fn general_custody_cannot_substitute_for_validator_fuel(
        general in HANDLER_COST..=10_000,
    ) {
        let mut state = EconomicsState::initial();
        state.general[0] = general;
        state.fuel[0] = HANDLER_COST - 1;
        let before = state.clone();
        let admitted = state.apply(&Action::Execute { client: 1, proposer: 0 });
        prop_assert!(!admitted);
        prop_assert_eq!(state, before);
    }
}

#[test]
fn six_fuel_admits_two_handlers_and_replay_depletes_exactly() {
    let expected_play = PlayFuelMachine {
        remaining: 0,
        burned: 6,
        admitted: vec![3, 3],
        deferred: vec![3],
    };
    assert_eq!(PlayFuelMachine::select(6, 3), Ok(expected_play.clone()));
    let expected_replay = ReplayFuelMachine {
        root: 12,
        proposer: 1,
        fuel: 0,
        burned: 6,
        certificate_cursor: 2,
        recorded_trace: vec![0, 1],
        rigged: false,
    };
    assert_eq!(
        ReplayFuelMachine::replay_admitted(10, 1, 6, &expected_play.admitted),
        Ok(expected_replay)
    );
}

#[test]
fn a_snapshot_cannot_be_captured_after_replay_rigging() {
    let mut replay = ReplayFuelMachine::new(10, 1, 6);
    replay.rigged = true;
    assert_eq!(
        replay.capture_snapshot(),
        Err(FuelReplayError::SnapshotDuringReplay)
    );
}

#[test]
fn a_forged_snapshot_cannot_replace_actual_settlement() {
    let mut replay = ReplayFuelMachine::new(10, 1, HANDLER_COST - 1);
    let forged = ValidatorFuelSnapshot {
        root: replay.root,
        proposer: replay.proposer,
        balance: HANDLER_COST,
        runtime: SnapshotRuntime::Ordinary,
        captured_before_rig: true,
    };
    let before = replay.clone();
    assert_eq!(
        replay.replay_one(HANDLER_COST, 0, forged),
        Err(FuelReplayError::StaleBalance)
    );
    assert_eq!(replay, before);
}

#[test]
fn a_candidate_cannot_fund_its_own_handler() {
    let mut state = EconomicsState::initial();
    state.fuel[0] = 0;
    assert!(!state.apply(&Action::Execute {
        client: 1,
        proposer: 0,
    }));
    assert!(state.apply(&Action::TopUp {
        source: 1,
        validator: 0,
        amount: HANDLER_COST,
    }));
    assert!(state.apply(&Action::Execute {
        client: 1,
        proposer: 0,
    }));
}

#[test]
fn stale_and_duplicate_resolutions_leave_every_field_unchanged() {
    let mut state = EconomicsState::initial();
    assert!(state.apply(&Action::Slash { validator: 0 }));
    assert!(state.apply(&Action::Vindicate {
        validator: 0,
        generation: 0,
    }));
    let resolved = state.clone();
    assert!(!state.apply(&Action::Vindicate {
        validator: 0,
        generation: 0,
    }));
    assert_eq!(state, resolved);
    assert!(state.apply(&Action::RequestWithdrawal { validator: 0 }));
    assert!(state.apply(&Action::CompleteWithdrawal { validator: 0 }));
    assert!(state.apply(&Action::Bond { validator: 0 }));
    assert!(state.apply(&Action::Slash { validator: 0 }));
    let generation_one = state.clone();
    assert!(!state.apply(&Action::Guilty {
        validator: 0,
        generation: 0,
    }));
    assert_eq!(state, generation_one);
}

#[test]
fn top_up_waits_for_quarantine_resolution() {
    let mut state = EconomicsState::initial();
    assert!(state.apply(&Action::Slash { validator: 0 }));
    let quarantined = state.quarantine[0];
    let pending = state.clone();
    assert!(!state.apply(&Action::TopUp {
        source: 1,
        validator: 0,
        amount: 2,
    }));
    assert_eq!(state, pending);
    assert!(state.apply(&Action::Vindicate {
        validator: 0,
        generation: 0,
    }));
    assert!(state.apply(&Action::TopUp {
        source: 1,
        validator: 0,
        amount: 2,
    }));
    assert_eq!(state.fuel[0], quarantined + 2);
    state.assert_invariants().unwrap();
}
