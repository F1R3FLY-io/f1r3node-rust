use std::collections::BTreeSet;

use models::rhoapi::CostSignature;
use rspace_plus_plus::rspace::rspace_interface::RSpaceOperationSource;

use super::*;
use crate::rust::interpreter::accounting::authority::{self, ResourceMultiset};
use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::accounting::native_phlo_rules::{
    NativeBudgetReplayDecision, NativeStackTransferEvent, NativeStackTransferKind,
};
use crate::rust::interpreter::accounting::native_runtime::tests::config;
use crate::rust::interpreter::accounting::{
    canonical_observation_authority, AuthorityStackTransferReservation, RuntimeBudget,
};

const ORDERS: u64 = 256;
const LIMIT: u64 = 1_000;
const HUGE: u64 = 100_000;
const WEIGHTS: [u64; 4] = [0, 0, 1, 0];
const ROWS: usize = 16;
const CHANNELS: u8 = 3;
const DENIED_TRANSFERS: u8 = 2;

type Demand = ResourceMultiset<[u8; 32]>;

#[derive(Clone)]
struct RowPlan {
    channel: u8,
    owners: usize,
    introduction: u64,
    comm: Option<u64>,
    transfer: Option<u64>,
}

struct Fixture {
    recording: NativeBudgetRecording,
    rows: Arc<[NativeOperationRecord]>,
    log: Vec<Event>,
    plans: Vec<RowPlan>,
    allocation: Demand,
    transfer_granted: Vec<bool>,
    expected_realized: Demand,
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }

    fn below(&mut self, bound: usize) -> usize { (self.next() % bound as u64) as usize }
}

fn demand(owners: usize) -> Demand { authority::authority_demand(&authority(owners)).unwrap() }

fn scaled(base: &Demand, factor: u64) -> Demand {
    ResourceMultiset(
        base.0
            .iter()
            .map(|(lane, amount)| (*lane, amount * factor))
            .collect(),
    )
}

fn cells(count: u64) -> Vec<CostSignature> {
    vec![authority(1).regions[0].signature.clone().unwrap(); count as usize]
}

fn charged(
    id: u8,
    kind: AuthorityByteEventKind,
    owners: usize,
    amount: u64,
) -> Arc<ByteObservation> {
    Arc::new(ByteObservation {
        event_id: [id; 32],
        kind,
        authority: authority(owners),
        measurement: Some(ByteCharge {
            transfer_bytes: amount,
            ..ByteCharge::default()
        }),
        legacy_amount: None,
    })
}

fn introduction(slot: usize, plan: &RowPlan) -> Arc<ByteObservation> {
    charged(
        slot as u8,
        AuthorityByteEventKind::ProduceIntroduction,
        plan.owners,
        plan.introduction,
    )
}

fn comm_observation(slot: usize, plan: &RowPlan) -> Arc<ByteObservation> {
    charged(
        slot as u8 + 64,
        AuthorityByteEventKind::Comm,
        plan.owners,
        plan.comm.expect("a COMM row"),
    )
}

fn denied_transfer_hash(index: u8) -> [u8; 32] { [200 + index; 32] }

fn plans() -> Vec<RowPlan> {
    let table: [(usize, u64, Option<u64>, Option<u64>); ROWS] = [
        (1, 1, Some(1), Some(1)),
        (2, 1, Some(HUGE), None),
        (1, HUGE, None, Some(1)),
        (1, 1, Some(1), None),
        (1, 1, Some(1), None),
        (2, 2, Some(1), None),
        (2, 1, Some(1), None),
        (2, 1, Some(1), None),
        (1, HUGE, None, None),
        (1, 1, None, None),
        (2, 3, Some(2), None),
        (1, 2, None, None),
        (1, 1, Some(1), None),
        (2, 1, None, None),
        (1, 1, Some(1), None),
        (2, 2, Some(1), None),
    ];
    table
        .into_iter()
        .enumerate()
        .map(|(slot, (owners, introduction, comm, transfer))| RowPlan {
            channel: (slot % usize::from(CHANNELS)) as u8,
            owners,
            introduction,
            comm,
            transfer,
        })
        .collect()
}

fn fixture() -> Fixture {
    let plans = plans();
    let allocation = scaled(&demand(1), 3)
        .checked_add(&scaled(&demand(2), 2))
        .unwrap();
    let mut reserved = Demand::default();
    let mut realized = Demand::default();
    let mut used = 0_u64;
    let mut attempts = Vec::new();
    let mut transfers = Vec::new();
    let mut transfer_granted = Vec::new();
    let mut rows = Vec::new();
    let mut latest: Vec<Option<usize>> = vec![None; usize::from(CHANNELS)];
    let transfer_authority = canonical_observation_authority(&authority(1)).unwrap();
    with_contract(LIMIT, WEIGHTS, |contract| {
        let regions = journal_limits().budget.regions;
        let charge = |observation: &Arc<ByteObservation>| {
            contract
                .prepare(Arc::clone(observation), regions)
                .unwrap()
                .usage()
        };
        for (slot, plan) in plans.iter().enumerate() {
            if slot == ROWS / 2 {
                for index in 0..DENIED_TRANSFERS {
                    let need = scaled(&demand(1), 4);
                    assert!(!allocation.dominates(&reserved.checked_add(&need).unwrap()));
                    transfers.push(NativeStackTransferEvent {
                        kind: NativeStackTransferKind::Reserve {
                            produce_hash: denied_transfer_hash(index),
                            authority: transfer_authority.clone(),
                            cells: 4,
                            granted: false,
                        },
                        fresh_before: attempts.len(),
                    });
                }
            }
            let transfer = plan.transfer.map(|count| {
                let need = scaled(&demand(1), count);
                let granted = allocation.dominates(&reserved.checked_add(&need).unwrap());
                transfer_granted.push(granted);
                transfers.push(NativeStackTransferEvent {
                    kind: NativeStackTransferKind::Reserve {
                        produce_hash: [slot as u8; 32],
                        authority: transfer_authority.clone(),
                        cells: count,
                        granted,
                    },
                    fresh_before: attempts.len(),
                });
                if granted {
                    reserved = reserved.checked_add(&need).unwrap();
                }
                (granted, need, transfers.len() - 1)
            });
            assert!(
                transfer.as_ref().is_none_or(|(granted, ..)| *granted),
                "a denied transfer has no produce row"
            );
            let start = attempts.len();
            let intro = introduction(slot, plan);
            let intro_charge = charge(&intro);
            let intro_granted = used + intro_charge <= LIMIT;
            if intro_granted {
                used += intro_charge;
            }
            attempts.push(NativeBudgetAttempt {
                occurrence: occurrence(slot as u64, NativeAttemptStage::ProduceIntroduction),
                observation: intro,
                granted: intro_granted,
                cause: NativeDenialCause::Budget,
            });
            let mut row = operation(
                slot as u64,
                start,
                start + 1,
                NativeObservationLink::Attempt(start),
            );
            let source = NativeProduceSource {
                channel: [plan.channel; 32],
                hash: [slot as u8; 32],
                persistent: true,
            };
            row.source = NativeOperationSource::Produce(source.clone());
            row.footprint = Arc::from([Arc::from([plan.channel])]);
            row.predecessors = latest[usize::from(plan.channel)]
                .into_iter()
                .collect::<Vec<_>>()
                .into();
            latest[usize::from(plan.channel)] = Some(slot);
            row.completion = RSpaceOperationCompletion::Stored;
            if !intro_granted {
                row.completion = RSpaceOperationCompletion::Rejected;
                if let Some((true, need, reserve)) = &transfer {
                    reserved = reserved.checked_sub(need).unwrap();
                    transfers.push(NativeStackTransferEvent {
                        kind: NativeStackTransferKind::Release { reserve: *reserve },
                        fresh_before: attempts.len(),
                    });
                }
            } else {
                if let Some((true, need, _)) = &transfer {
                    realized = realized.checked_add(need).unwrap();
                }
                if plan.comm.is_some() {
                    let observation = comm_observation(slot, plan);
                    let comm_demand = demand(plan.owners);
                    let admitted =
                        allocation.dominates(&reserved.checked_add(&comm_demand).unwrap());
                    let comm_charge = charge(&observation);
                    let (granted, cause) = match (admitted, used + comm_charge <= LIMIT) {
                        (false, _) => (false, NativeDenialCause::Allocation),
                        (true, true) => (true, NativeDenialCause::Budget),
                        (true, false) => (false, NativeDenialCause::Budget),
                    };
                    if granted {
                        used += comm_charge;
                        reserved = reserved.checked_add(&comm_demand).unwrap();
                        realized = realized.checked_add(&comm_demand).unwrap();
                    }
                    row.comm = Some(NativeCommRecord {
                        source: Arc::new(NativeCommSource {
                            consume: NativeConsumeSource {
                                channels: Arc::from([source.channel]),
                                hash: [slot as u8 + 64; 32],
                                persistent: true,
                            },
                            produces: Arc::from([source.clone()]),
                            peeks: Arc::from([]),
                            repetitions: Arc::from([(source, 0)]),
                        }),
                        observation: NativeObservationLink::Attempt(attempts.len()),
                    });
                    attempts.push(NativeBudgetAttempt {
                        occurrence: occurrence(slot as u64, NativeAttemptStage::Comm),
                        observation,
                        granted,
                        cause,
                    });
                    row.budget_end += 1;
                    row.completion = if granted {
                        RSpaceOperationCompletion::Matched
                    } else {
                        RSpaceOperationCompletion::Rejected
                    };
                }
            }
            rows.push(row);
        }
    });
    let causes: BTreeSet<_> = attempts
        .iter()
        .map(|row| (row.occurrence.stage, row.granted, row.cause))
        .collect();
    for case in [
        (NativeAttemptStage::Comm, true, NativeDenialCause::Budget),
        (
            NativeAttemptStage::Comm,
            false,
            NativeDenialCause::Allocation,
        ),
        (NativeAttemptStage::Comm, false, NativeDenialCause::Budget),
        (
            NativeAttemptStage::ProduceIntroduction,
            false,
            NativeDenialCause::Budget,
        ),
    ] {
        assert!(causes.contains(&case), "the fixture lacks {case:?}");
    }
    assert!(transfers
        .iter()
        .any(|event| matches!(event.kind, NativeStackTransferKind::Release { .. })));
    let rows: Arc<[NativeOperationRecord]> = rows.into();
    let log = events(&rows);
    Fixture {
        recording: NativeBudgetRecording {
            session: [0; 32],
            attempts: attempts.into(),
            retries: Arc::from([]),
            transfers: transfers.into(),
            used,
            paths: fixture_paths(),
        },
        rows,
        log,
        plans,
        allocation,
        transfer_granted,
        expected_realized: realized,
    }
}

fn checked(
    fixture: &Fixture,
    allocation: Option<&Demand>,
) -> Result<CheckedNativeOperationJournal, NativeOperationJournalError> {
    with_contract(LIMIT, WEIGHTS, |contract| {
        let contract = match allocation {
            Some(allocation) => contract.with_authority_allocation(allocation.clone()),
            None => contract,
        };
        contract.check_operation_journal(
            [0; 32],
            &fixture.recording,
            Arc::clone(&fixture.rows),
            journal_limits(),
        )
    })
}

enum Step {
    Transfer(usize),
    Operation(usize),
    DeniedTransfer(u8),
}

struct Outcome {
    realized: Demand,
    frontier: BTreeSet<[u8; 32]>,
    observations: BTreeSet<[u8; 32]>,
    usage: u64,
    overdrawn: bool,
}

fn replay_once(fixture: &Fixture, seed: u64) -> Result<Outcome, String> {
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    budget
        .reset_for_native_execution(config(LIMIT, WEIGHTS))
        .map_err(|error| format!("budget: {error}"))?;
    budget.install_authority_allocation(fixture.allocation.clone());
    let replay = checked(fixture, Some(&fixture.allocation))
        .map_err(|error| format!("journal: {error:?}"))?
        .bind_trace(fixture.log.clone().into(), limits())
        .map_err(|error| format!("trace: {error}"))?
        .into_replay()
        .map_err(|error| format!("replay: {error}"))?
        .bind_runtime(budget.clone())
        .map_err(|error| format!("binding: {error}"))?;
    let _scope = budget.enter_comm_accounting_scope();
    let mut steps: Vec<Step> = Vec::new();
    let mut transfer_index = 0;
    for (slot, plan) in fixture.plans.iter().enumerate() {
        if plan.transfer.is_some() {
            if !fixture.transfer_granted[transfer_index] {
                return Err("a fixture transfer was denied".to_owned());
            }
            transfer_index += 1;
            steps.push(Step::Transfer(slot));
        } else {
            steps.push(Step::Operation(slot));
        }
    }
    for index in 0..DENIED_TRANSFERS {
        steps.push(Step::DeniedTransfer(index));
    }
    let mut rng = Rng(seed);
    let mut done = [false; ROWS];
    let mut pending: Vec<Option<AuthorityStackTransferReservation>> =
        (0..ROWS).map(|_| None).collect();
    let mut overdrawn = false;
    while !steps.is_empty() {
        let ready: Vec<usize> = steps
            .iter()
            .enumerate()
            .filter(|(_, step)| match step {
                Step::Operation(slot) => fixture.rows[*slot]
                    .predecessors
                    .iter()
                    .all(|prior| done[*prior]),
                Step::Transfer(_) | Step::DeniedTransfer(_) => true,
            })
            .map(|(index, _)| index)
            .collect();
        if ready.is_empty() {
            return Err("no step is ready".to_owned());
        }
        let step = steps.swap_remove(ready[rng.below(ready.len())]);
        match step {
            Step::Transfer(slot) => {
                let count = fixture.plans[slot].transfer.expect("a transfer row");
                let reservation = budget
                    .prepare_authority_stack_transfer([slot as u8; 32], cells(count), &authority(1))
                    .map_err(|error| format!("transfer {slot}: {error}"))?;
                pending[slot] = Some(reservation);
                steps.push(Step::Operation(slot));
            }
            Step::DeniedTransfer(index) => {
                match budget.prepare_authority_stack_transfer(
                    denied_transfer_hash(index),
                    cells(4),
                    &authority(1),
                ) {
                    Err(InterpreterError::OutOfPhlogistonsError) => {}
                    other => {
                        return Err(format!(
                            "denied transfer {index} replayed as {:?}",
                            other.map(|_| ())
                        ))
                    }
                }
            }
            Step::Operation(slot) => {
                let plan = &fixture.plans[slot];
                let row = &fixture.rows[slot];
                let NativeOperationSource::Produce(source) = &row.source else {
                    return Err("every fixture row is a produce".to_owned());
                };
                let live = producer(source);
                let mut ticket = replay
                    .reserve_current(RSpaceOperationSource::Produce(&live))
                    .map_err(|error| format!("reserve {slot}: {error}"))?;
                if ticket.slot() != slot {
                    return Err(format!("row {slot} bound slot {}", ticket.slot()));
                }
                ticket
                    .authenticate_footprint(&[plan.channel], &[])
                    .map_err(|error| format!("footprint {slot}: {error}"))?;
                let decision = ticket
                    .observe_introduction(&introduction(slot, plan))
                    .map_err(|error| format!("introduction {slot}: {error}"))?;
                let introduced = matches!(decision, NativeBudgetReplayDecision::Accepted { .. });
                if introduced && row.comm.is_some() {
                    let comm = &row.comm.as_ref().expect("a COMM row").source;
                    let actual = COMM {
                        consume: consumer(&comm.consume),
                        produces: comm.produces.iter().map(producer).collect(),
                        peeks: comm.peeks.iter().copied().collect(),
                        times_repeated: comm
                            .repetitions
                            .iter()
                            .map(|(source, count)| (producer(source), *count))
                            .collect(),
                    };
                    let decision = ticket
                        .observe_comm(&actual, &comm_observation(slot, plan))
                        .map_err(|error| format!("comm {slot}: {error}"))?;
                    let expected = match fixture.recording.attempts[row.budget_start + 1].cause {
                        NativeDenialCause::Allocation => true,
                        NativeDenialCause::Budget => false,
                    };
                    if ticket.allocation_denied() != expected {
                        return Err(format!("comm {slot} lost its denial cause: {decision:?}"));
                    }
                }
                let outcome = ticket.expected_outcome();
                ticket
                    .prepare_completion(outcome)
                    .map_err(|error| format!("completion {slot}: {error}"))?
                    .publish();
                if let Some(reservation) = pending[slot].take() {
                    if introduced {
                        reservation.commit();
                    }
                }
                done[slot] = true;
            }
        }
        let state = budget.authority_state.lock().expect("authority state");
        overdrawn |= !fixture.allocation.dominates(&state.reserved);
    }
    replay
        .check_complete()
        .map_err(|error| format!("complete: {error}"))?;
    let usage = replay.completed_usage();
    let observations = budget
        .byte_observations()
        .rows
        .iter()
        .map(|row| row.event_id)
        .collect();
    let state = budget.authority_state.lock().expect("authority state");
    Ok(Outcome {
        realized: state.realized.clone(),
        frontier: state.frontier.keys().copied().collect(),
        observations,
        usage,
        overdrawn,
    })
}

#[test]
fn native_replay_accepts_every_causal_order_with_recorded_allocation_decisions() {
    let fixture = fixture();
    assert!(matches!(
        checked(&fixture, None),
        Err(NativeOperationJournalError::Budget(
            NativeBudgetTraceError::Decision
        ))
    ));
    let first = replay_once(&fixture, 0).expect("the first order replays");
    assert_eq!(first.usage, fixture.recording.used);
    assert_eq!(first.realized, fixture.expected_realized);
    let granted: BTreeSet<_> = fixture
        .recording
        .attempts
        .iter()
        .filter(|row| row.granted)
        .map(|row| row.observation.event_id)
        .collect();
    assert_eq!(first.observations, granted);
    let mut failed = Vec::new();
    let mut overdrawn = 0;
    for seed in 0..ORDERS {
        match replay_once(&fixture, seed) {
            Ok(outcome) => {
                overdrawn += usize::from(outcome.overdrawn);
                if outcome.usage != first.usage
                    || outcome.realized != first.realized
                    || outcome.frontier != first.frontier
                    || outcome.observations != first.observations
                {
                    failed.push(format!("order {seed}: the authority state differs"));
                }
            }
            Err(error) => failed.push(format!("order {seed}: {error}")),
        }
    }
    assert!(
        failed.is_empty(),
        "{} orders failed: {failed:#?}",
        failed.len()
    );
    assert!(
        overdrawn > 0,
        "no order held more live demand than the allocation"
    );
}
