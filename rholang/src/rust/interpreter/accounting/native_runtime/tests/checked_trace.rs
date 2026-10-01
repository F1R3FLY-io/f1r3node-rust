use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::operation_context::{CausalPath, OperationOrder};
use rspace_plus_plus::rspace::trace::event::{Consume, Event, IOEvent, Produce, COMM};

use super::*;

#[path = "replay_ledger.rs"]
mod replay;

#[path = "replay_session.rs"]
mod session;

fn limits() -> NativeOperationTraceLimits {
    NativeOperationTraceLimits {
        events: 1000,
        source_entries: 10000,
        source_bytes: 1_000_000,
        telemetry_items: 10000,
        telemetry_bytes: 1_000_000,
    }
}

fn producer(source: &NativeProduceSource) -> Produce {
    Produce::new(
        Blake2b256Hash(source.channel.to_vec()),
        Blake2b256Hash(source.hash.to_vec()),
        source.persistent,
    )
}

fn consumer(source: &NativeConsumeSource) -> Consume {
    Consume {
        channel_hashes: source
            .channels
            .iter()
            .map(|hash| Blake2b256Hash(hash.to_vec()))
            .collect(),
        hash: Blake2b256Hash(source.hash.to_vec()),
        persistent: source.persistent,
    }
}

fn events(rows: &[NativeOperationRecord]) -> Vec<Event> {
    let mut order: Vec<_> = rows.iter().collect();
    order.sort_by_key(|row| OperationOrder {
        session: row.occurrence.session,
        path: CausalPath::from(row.occurrence.path.to_vec()),
    });
    let mut result = Vec::new();
    for row in order {
        if row.completion == RSpaceOperationCompletion::Rejected {
            continue;
        }
        result.push(Event::IoEvent(match &row.source {
            NativeOperationSource::Produce(source) => IOEvent::Produce(producer(source)),
            NativeOperationSource::Consume(source) => IOEvent::Consume(consumer(source)),
        }));
        if row.completion == RSpaceOperationCompletion::Matched {
            let source = &row.comm.as_ref().unwrap().source;
            result.push(Event::Comm(COMM {
                consume: consumer(&source.consume),
                produces: source.produces.iter().map(producer).collect(),
                peeks: source.peeks.iter().copied().collect(),
                times_repeated: source
                    .repetitions
                    .iter()
                    .map(|(source, count)| (producer(source), *count))
                    .collect(),
            }));
        }
    }
    result
}

fn trace_fixture() -> (
    NativeBudgetRecording,
    Arc<[NativeOperationRecord]>,
    Vec<Event>,
) {
    let (recording, mut rows) = fixture();
    let comm = Arc::make_mut(&mut Arc::make_mut(&mut rows)[1].comm.as_mut().unwrap().source);
    comm.repetitions = Arc::from([(produce(1), 0)]);
    let log = events(&rows);
    (recording, rows, log)
}

fn bind(
    recording: &NativeBudgetRecording,
    rows: Arc<[NativeOperationRecord]>,
    log: Vec<Event>,
) -> Result<CheckedNativeOperationTrace, NativeOperationTraceError> {
    check(recording, rows)
        .unwrap()
        .bind_trace(log.into(), limits(), &host())
}

#[test]
fn trace_covers_every_event_and_keeps_original_journal_indexes() {
    let (recording, mut rows, _) = trace_fixture();
    let rows_mut = Arc::make_mut(&mut rows);
    rows_mut[0].occurrence.path = Arc::from([(9, 0)]);
    rows_mut[1].occurrence.path = Arc::from([(1, 0), (9, 0)]);
    rows_mut[2].occurrence.path = Arc::from([(1, 0)]);
    let mut recording = recording;
    for attempt in Arc::make_mut(&mut recording.attempts) {
        let index = if attempt.occurrence.path[0].0 == 0 {
            0
        } else {
            1
        };
        attempt.occurrence.path = rows[index].occurrence.path.to_vec();
    }
    Arc::make_mut(&mut recording.retries)[0].occurrence.path = rows[2].occurrence.path.to_vec();
    let log = events(&rows);
    let trace = bind(&recording, rows, log).unwrap();
    assert_eq!(trace.operation_count(), 3);
    assert_eq!(trace.event_count(), 4);
    assert_eq!(
        (0..3)
            .map(|i| trace.journal_index(i).unwrap())
            .collect::<Vec<_>>(),
        [2, 1, 0]
    );
    assert_eq!(trace.operation(0).unwrap().predecessors.as_ref(), &[0]);
    assert!(trace.comm(0).is_none());
    assert!(trace.comm(1).is_some());
    assert!(trace.events(3).is_none());
}

#[test]
fn trace_rejects_each_omission_extra_event_and_wrong_event_kind() {
    let (recording, rows, log) = trace_fixture();
    for index in 0..log.len() {
        let mut changed = log.clone();
        changed.remove(index);
        assert!(matches!(
            bind(&recording, rows.clone(), changed),
            Err(NativeOperationTraceError::Coverage)
        ));
        let mut changed = log.clone();
        changed.insert(index, log[index].clone());
        assert!(matches!(
            bind(&recording, rows.clone(), changed),
            Err(NativeOperationTraceError::Coverage)
        ));
    }
    let mut changed = log.clone();
    changed.swap(1, 2);
    assert!(matches!(
        bind(&recording, rows.clone(), changed),
        Err(NativeOperationTraceError::Coverage)
    ));
    let mut changed = log;
    changed.swap(0, 3);
    assert!(matches!(
        bind(&recording, rows, changed),
        Err(NativeOperationTraceError::Source)
    ));
}

#[test]
fn trace_rejects_logical_source_mutations_ignored_by_hash_equality() {
    for mutation in 0..16 {
        let (recording, rows, mut log) = trace_fixture();
        if mutation < 3 {
            let Event::IoEvent(IOEvent::Produce(p)) = &mut log[0] else {
                panic!()
            };
            match mutation {
                0 => p.persistent = !p.persistent,
                1 => p.channel_hash.0[0] ^= 1,
                _ => p.hash.0[0] ^= 1,
            }
        } else {
            let Event::Comm(comm) = &mut log[2] else {
                panic!()
            };
            match mutation {
                3 => comm.consume.persistent = !comm.consume.persistent,
                4 => comm.consume.hash.0[0] ^= 1,
                5 => comm.consume.channel_hashes[0].0[0] ^= 1,
                6 => comm
                    .consume
                    .channel_hashes
                    .push(Blake2b256Hash(vec![0; 32])),
                7 => comm.produces[0].persistent = !comm.produces[0].persistent,
                8 => comm.produces[0].channel_hash.0[0] ^= 1,
                9 => comm.produces[0].hash.0[0] ^= 1,
                10 => comm.produces.push(comm.produces[0].clone()),
                11 => {
                    comm.peeks.insert(0);
                }
                12 => *comm.times_repeated.values_mut().next().unwrap() += 1,
                13 | 14 => {
                    let (mut source, count) = comm.times_repeated.pop_first().unwrap();
                    if mutation == 13 {
                        source.persistent = !source.persistent;
                    } else {
                        source.channel_hash.0[0] ^= 1;
                    }
                    comm.times_repeated.insert(source, count);
                }
                _ => comm.times_repeated.clear(),
            }
        }
        assert!(
            matches!(
                bind(&recording, rows, log),
                Err(NativeOperationTraceError::Source)
            ),
            "mutation {mutation}"
        );
    }
}

#[test]
fn introduction_owns_telemetry_even_when_trigger_is_not_selected() {
    let (recording, mut rows, _) = trace_fixture();
    let comm = Arc::make_mut(&mut Arc::make_mut(&mut rows)[1].comm.as_mut().unwrap().source);
    comm.produces = Arc::from([produce(9)]);
    comm.repetitions = Arc::from([(produce(9), 0)]);
    let mut log = events(&rows);
    let Event::IoEvent(IOEvent::Produce(intro)) = &mut log[1] else {
        panic!()
    };
    intro.output_value = vec![b"slot result".to_vec()];
    intro.failed = true;
    let trace = bind(&recording, rows, log).unwrap();
    let IOEvent::Produce(intro) = trace.introduction(1).unwrap() else {
        panic!()
    };
    assert!(intro.failed);
    assert!(intro.is_deterministic);
    assert_eq!(intro.output_value, [b"slot result".to_vec()]);
    assert_ne!(intro.hash, trace.comm(1).unwrap().produces[0].hash);
}

#[test]
fn trace_preserves_event_local_metadata_but_rejects_incoherent_comm_copies() {
    for mutation in 0..3 {
        let (recording, rows, mut log) = trace_fixture();
        let Event::IoEvent(IOEvent::Produce(intro)) = &mut log[1] else {
            panic!()
        };
        intro.output_value = vec![vec![1], vec![]];
        intro.failed = true;
        assert!(bind(&recording, rows.clone(), log.clone()).is_ok());
        let Event::Comm(comm) = &mut log[2] else {
            panic!()
        };
        match mutation {
            0 => comm.produces[0].output_value.push(vec![1]),
            1 => comm.produces[0].failed = true,
            _ => comm.produces[0].is_deterministic = false,
        }
        assert!(matches!(
            bind(&recording, rows, log),
            Err(NativeOperationTraceError::Telemetry)
        ));
    }
}

#[test]
fn trace_bounds_include_empty_output_items_and_all_host_work() {
    for dimension in 0..6 {
        let (recording, rows, mut log) = trace_fixture();
        let Event::IoEvent(IOEvent::Produce(intro)) = &mut log[0] else {
            panic!()
        };
        intro.output_value = vec![vec![], vec![], vec![7]];
        let mut bound = limits();
        let host = if dimension == 5 {
            HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(0)))
        } else {
            host()
        };
        match dimension {
            0 => bound.events = 3,
            1 => bound.source_entries = 0,
            2 => bound.source_bytes = 0,
            3 => bound.telemetry_items = 2,
            4 => bound.telemetry_bytes = 0,
            _ => (),
        }
        let result = check(&recording, rows)
            .unwrap()
            .bind_trace(log.into(), bound, &host);
        assert!(matches!(
            result,
            Err(NativeOperationTraceError::Limit | NativeOperationTraceError::Host(_))
        ));
    }
}

#[test]
fn comm_repetition_keys_cover_exactly_the_selected_producer_set() {
    for extra in [false, true] {
        let (recording, mut rows, _) = trace_fixture();
        let comm = Arc::make_mut(&mut Arc::make_mut(&mut rows)[1].comm.as_mut().unwrap().source);
        comm.repetitions = if extra {
            Arc::from([(produce(1), 0), (produce(9), 0)])
        } else {
            Arc::from([])
        };
        let log = events(&rows);
        assert!(matches!(
            bind(&recording, rows, log),
            Err(NativeOperationTraceError::Source)
        ));
    }
}

#[test]
fn identical_comms_keep_separate_occurrence_slots_and_introduction_results() {
    let (mut recording, mut rows, _) = trace_fixture();
    let (source, footprint, comm) = {
        let row = &rows[1];
        (row.source.clone(), row.footprint.clone(), row.comm.clone())
    };
    let row = &mut Arc::make_mut(&mut rows)[2];
    row.source = source;
    row.footprint = footprint;
    row.predecessors = Arc::from([1]);
    row.comm = comm;
    row.comm.as_mut().unwrap().observation = NativeObservationLink::Retry(1);
    row.completion = RSpaceOperationCompletion::Matched;
    recording.retries = Arc::from([
        NativeBudgetRetry {
            occurrence: occurrence(2, NativeAttemptStage::ProduceIntroduction),
            observation: recording.attempts[1].observation.clone(),
            accepted_attempt: 1,
            fresh_before: 3,
        },
        NativeBudgetRetry {
            occurrence: occurrence(2, NativeAttemptStage::Comm),
            observation: recording.attempts[2].observation.clone(),
            accepted_attempt: 2,
            fresh_before: 3,
        },
    ]);
    let mut log = events(&rows);
    for (index, result) in [(1, 10), (3, 20)] {
        let Event::IoEvent(IOEvent::Produce(intro)) = &mut log[index] else {
            panic!()
        };
        intro.output_value = vec![vec![result]];
    }
    let trace = bind(&recording, rows, log).unwrap();
    assert_eq!(trace.comm(1), trace.comm(2));
    assert_ne!(trace.journal_index(1), trace.journal_index(2));
    for (slot, result) in [(1, 10), (2, 20)] {
        let IOEvent::Produce(intro) = trace.introduction(slot).unwrap() else {
            panic!()
        };
        assert_eq!(intro.output_value, [vec![result]]);
    }
}

#[test]
fn consume_trigger_must_match_its_comm_source() {
    let (mut recording, mut rows, _) = trace_fixture();
    let row = &mut Arc::make_mut(&mut rows)[1];
    row.source = NativeOperationSource::Consume(row.comm.as_ref().unwrap().source.consume.clone());
    row.consume_peeks = Some(Arc::clone(&row.comm.as_ref().unwrap().source.peeks));
    let attempt = &mut Arc::make_mut(&mut recording.attempts)[1];
    attempt.occurrence.stage = NativeAttemptStage::ConsumeIntroduction;
    Arc::make_mut(&mut attempt.observation).kind = AuthorityByteEventKind::ConsumeIntroduction;
    let log = events(&rows);
    assert!(bind(&recording, rows.clone(), log).is_ok());
    let NativeOperationSource::Consume(source) = &mut Arc::make_mut(&mut rows)[1].source else {
        panic!()
    };
    source.persistent = !source.persistent;
    let log = events(&rows);
    assert!(matches!(
        bind(&recording, rows, log),
        Err(NativeOperationTraceError::Source)
    ));
}

#[test]
fn consume_source_comparisons_reserve_work_for_each_traversal() {
    let imported = |channels, host: &HostWorkBudget| {
        let (mut recording, mut rows, _) = trace_fixture();
        let row = &mut Arc::make_mut(&mut rows)[1];
        let source = Arc::make_mut(&mut row.comm.as_mut().unwrap().source);
        source.consume.channels = vec![[1; 32]; channels].into();
        row.source = NativeOperationSource::Consume(source.consume.clone());
        row.consume_peeks = Some(Arc::clone(&source.peeks));
        let attempt = &mut Arc::make_mut(&mut recording.attempts)[1];
        attempt.occurrence.stage = NativeAttemptStage::ConsumeIntroduction;
        Arc::make_mut(&mut attempt.observation).kind = AuthorityByteEventKind::ConsumeIntroduction;
        let log = events(&rows);
        check(&recording, rows)
            .unwrap()
            .bind_trace(log.into(), limits(), host)
    };
    let small = host();
    imported(1, &small).unwrap();
    let large = host();
    imported(256, &large).unwrap();
    let dimension = HostWorkDimension::VerificationBytes;
    let increase = large.usage(dimension).get() - small.usage(dimension).get();
    assert!(
        increase >= 3 * 255 * 32,
        "each of three source comparisons needs byte credit"
    );
    let mut limits = small.limits();
    limits.set(
        dimension,
        HostWorkLimit::new(small.usage(dimension).get() + 2 * 255 * 32),
    );
    let tight = HostWorkBudget::new(limits);
    assert!(matches!(
        imported(256, &tight),
        Err(NativeOperationTraceError::Host(_))
    ));
    assert!(tight.is_rejected());
}

#[test]
fn denied_comm_owns_a_slot_but_no_committed_introduction() {
    let (mut recording, mut rows, _) = trace_fixture();
    let attempt = &mut Arc::make_mut(&mut recording.attempts)[2];
    attempt.granted = false;
    Arc::make_mut(&mut attempt.observation)
        .measurement
        .as_mut()
        .unwrap()
        .transfer_bytes = 101;
    recording.used = 2;
    Arc::make_mut(&mut rows)[1].completion = RSpaceOperationCompletion::Rejected;
    let log = events(&rows);
    let trace = bind(&recording, rows, log).unwrap();
    assert_eq!(trace.operation_count(), 3);
    assert_eq!(trace.event_count(), 2);
    assert!(trace.events(1).unwrap().is_empty());
    assert!(trace.operation(1).unwrap().comm.is_some());
    assert!(trace.introduction(1).is_none());
    assert!(trace.comm(1).is_none());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    #[test]
    fn trace_projection_matches_rspace_order_and_partitions_every_event(
        paths in prop::collection::btree_set(prop::collection::vec((any::<u64>(), any::<u64>()), 0..8), 0..40),
        outcomes in prop::collection::vec(0u8..4, 40),
    ) {
        let mut rows = Vec::new();
        let mut attempts = Vec::new();
        for (index, path) in paths.into_iter().rev().enumerate() {
            let outcome = outcomes[index];
            let start = attempts.len();
            let intro = observation(index as u8, AuthorityByteEventKind::ProduceIntroduction, 0);
            attempts.push(NativeBudgetAttempt {
                occurrence: NativeBudgetOccurrence { session: [0; 32], path: path.clone(), stage: NativeAttemptStage::ProduceIntroduction },
                observation: intro,
                granted: outcome != 2,
            });
            let mut row = operation(index as u64, start, start + 1, NativeObservationLink::Attempt(start));
            row.occurrence.path = path.clone().into();
            if outcome == 1 || outcome == 3 {
                let source = produce(index as u8);
                let comm = NativeCommSource {
                    consume: NativeConsumeSource { channels: Arc::from([source.channel]), hash: source.hash, persistent: true },
                    produces: Arc::from([source.clone()]), peeks: Arc::from([]), repetitions: Arc::from([(source, 0)]),
                };
                row.comm = Some(NativeCommRecord { source: Arc::new(comm), observation: NativeObservationLink::Attempt(attempts.len()) });
                attempts.push(NativeBudgetAttempt {
                    occurrence: NativeBudgetOccurrence { session: [0; 32], path, stage: NativeAttemptStage::Comm },
                    observation: observation(index as u8, AuthorityByteEventKind::Comm, if outcome == 3 { 101 } else { 0 }), granted: outcome == 1,
                });
                row.budget_end += 1;
                row.completion = if outcome == 3 { RSpaceOperationCompletion::Rejected } else { RSpaceOperationCompletion::Matched };
            } else if outcome == 2 {
                Arc::make_mut(&mut attempts.last_mut().unwrap().observation).measurement.as_mut().unwrap().transfer_bytes = 101;
                row.completion = RSpaceOperationCompletion::Rejected;
            }
            rows.push(row);
        }
        let recording = NativeBudgetRecording { session: [0; 32], attempts: attempts.into(), retries: Arc::from([]), used: 0 };
        let log = events(&rows);
        let trace = bind(&recording, rows.into(), log.clone()).unwrap();
        let projected: Vec<_> = (0..trace.operation_count()).flat_map(|slot| trace.events(slot).unwrap().iter().cloned()).collect();
        prop_assert_eq!(projected, log);
        for slot in 0..trace.operation_count() {
            let row = trace.operation(slot).unwrap();
            let expected = match row.completion { RSpaceOperationCompletion::Stored => 1, RSpaceOperationCompletion::Matched => 2, RSpaceOperationCompletion::Rejected => 0 };
            prop_assert_eq!(trace.events(slot).unwrap().len(), expected);
            if slot > 0 {
                prop_assert!(trace.operation(slot - 1).unwrap().occurrence.path < row.occurrence.path);
            }
        }
    }
}
