use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use proptest::prelude::*;

use super::*;

#[path = "checked_trace.rs"]
mod trace;
use crate::rust::interpreter::accounting::authority::AuthorityByteEventKind;
use crate::rust::interpreter::accounting::byte_accounting::ByteCharge;
use crate::rust::interpreter::accounting::byte_receipts::ByteObservation;
use crate::rust::interpreter::accounting::native_phlo_rules::{
    NativeBudgetAttempt, NativeBudgetOccurrence,
};
use crate::rust::interpreter::accounting::native_runtime::tests::{
    authority, journal_limits, with_contract,
};
use crate::rust::interpreter::accounting::native_runtime::{
    NativeBudgetRetry, NativeCommRecord, NativeCommSource, NativeConsumeSource,
    NativeOperationOccurrence, NativeProduceSource,
};

fn host() -> HostWorkBudget {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(100_000_000)))
}

fn occurrence(id: u64, stage: NativeAttemptStage) -> NativeBudgetOccurrence {
    NativeBudgetOccurrence {
        session: [0; 32],
        path: vec![(id, 0)],
        stage,
    }
}

fn observation(id: u8, kind: AuthorityByteEventKind, amount: u64) -> Arc<ByteObservation> {
    Arc::new(ByteObservation {
        event_id: [id; 32],
        kind,
        authority: authority(1),
        measurement: Some(ByteCharge {
            transfer_bytes: amount,
            ..ByteCharge::default()
        }),
        legacy_amount: None,
    })
}

fn produce(id: u8) -> NativeProduceSource {
    NativeProduceSource {
        channel: [id; 32],
        hash: [id; 32],
        persistent: true,
    }
}

fn operation(
    id: u64,
    start: usize,
    end: usize,
    introduction: NativeObservationLink,
) -> NativeOperationRecord {
    NativeOperationRecord {
        occurrence: NativeOperationOccurrence {
            session: [0; 32],
            path: vec![(id, 0)].into(),
        },
        source: NativeOperationSource::Produce(produce(id as u8)),
        consume_peeks: None,
        footprint: vec![Arc::from(vec![id as u8])].into(),
        predecessors: Arc::from([]),
        introduction,
        comm: None,
        completion: RSpaceOperationCompletion::Stored,
        budget_start: start,
        budget_end: end,
    }
}

fn fixture() -> (NativeBudgetRecording, Arc<[NativeOperationRecord]>) {
    let first = observation(0, AuthorityByteEventKind::ProduceIntroduction, 1);
    let attempts = vec![
        NativeBudgetAttempt {
            occurrence: occurrence(0, NativeAttemptStage::ProduceIntroduction),
            observation: first.clone(),
            granted: true,
        },
        NativeBudgetAttempt {
            occurrence: occurrence(1, NativeAttemptStage::ProduceIntroduction),
            observation: observation(1, AuthorityByteEventKind::ProduceIntroduction, 1),
            granted: true,
        },
        NativeBudgetAttempt {
            occurrence: occurrence(1, NativeAttemptStage::Comm),
            observation: observation(2, AuthorityByteEventKind::Comm, 1),
            granted: true,
        },
    ];
    let retries = vec![NativeBudgetRetry {
        occurrence: occurrence(2, NativeAttemptStage::ProduceIntroduction),
        observation: first,
        accepted_attempt: 0,
        fresh_before: 3,
    }];
    let mut rows = vec![
        operation(0, 0, 1, NativeObservationLink::Attempt(0)),
        operation(1, 1, 3, NativeObservationLink::Attempt(1)),
        operation(2, 3, 3, NativeObservationLink::Retry(0)),
    ];
    rows[1].comm = Some(NativeCommRecord {
        source: Arc::new(NativeCommSource {
            consume: NativeConsumeSource {
                channels: Arc::from([[1; 32]]),
                hash: [1; 32],
                persistent: false,
            },
            produces: Arc::from([produce(1)]),
            peeks: Arc::from([]),
            repetitions: Arc::from([]),
        }),
        observation: NativeObservationLink::Attempt(2),
    });
    rows[1].completion = RSpaceOperationCompletion::Matched;
    rows[2].footprint = Arc::clone(&rows[0].footprint);
    rows[2].predecessors = Arc::from([0]);
    (
        NativeBudgetRecording {
            session: [0; 32],
            attempts: attempts.into(),
            retries: retries.into(),
            used: 3,
        },
        rows.into(),
    )
}

fn consume_fixture(
    channels: usize,
    peeks: Option<Arc<[i32]>>,
) -> (NativeBudgetRecording, Arc<[NativeOperationRecord]>) {
    let recording = NativeBudgetRecording {
        session: [0; 32],
        attempts: vec![NativeBudgetAttempt {
            occurrence: occurrence(0, NativeAttemptStage::ConsumeIntroduction),
            observation: observation(0, AuthorityByteEventKind::ConsumeIntroduction, 1),
            granted: true,
        }]
        .into(),
        retries: Arc::from([]),
        used: 1,
    };
    let mut row = operation(0, 0, 1, NativeObservationLink::Attempt(0));
    row.source = NativeOperationSource::Consume(NativeConsumeSource {
        channels: vec![[0; 32]; channels].into(),
        hash: [0; 32],
        persistent: false,
    });
    row.consume_peeks = peeks;
    (recording, vec![row].into())
}

#[test]
fn journal_rejects_missing_unsorted_duplicate_and_out_of_range_peeks() {
    for peeks in [
        None,
        Some(vec![-1]),
        Some(vec![2]),
        Some(vec![0, 0]),
        Some(vec![1, 0]),
    ] {
        let (recording, rows) = consume_fixture(2, peeks.map(Into::into));
        assert!(matches!(
            check(&recording, rows),
            Err(NativeOperationJournalError::Peeks)
        ));
    }
    let (recording, mut rows) = fixture();
    Arc::make_mut(&mut rows)[0].consume_peeks = Some(Arc::from([]));
    assert!(matches!(
        check(&recording, rows),
        Err(NativeOperationJournalError::Peeks)
    ));
}

proptest! {
    #[test]
    fn journal_preserves_every_valid_peek_subset(
        channels in 1usize..64,
        selected in prop::collection::vec(any::<u8>(), 0..64),
    ) {
        let peeks: Arc<[i32]> = selected.into_iter()
            .map(|index| (usize::from(index) % channels) as i32)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter().collect::<Vec<_>>().into();
        let (recording, rows) = consume_fixture(channels, Some(peeks.clone()));
        let checked = check(&recording, rows).unwrap();
        prop_assert_eq!(checked.operations[0].consume_peeks.as_ref(), Some(&peeks));
    }
}

fn check(
    recording: &NativeBudgetRecording,
    rows: Arc<[NativeOperationRecord]>,
) -> Result<CheckedNativeOperationJournal, NativeOperationJournalError> {
    with_contract(100, [0, 0, 1, 0], |contract| {
        contract.check_operation_journal([0; 32], recording, rows, journal_limits(), &host())
    })
}

#[test]
fn journal_accepts_complete_evidence_and_empty_execution() {
    let (recording, rows) = fixture();
    let checked = check(&recording, rows).unwrap();
    assert_eq!(checked.total(), 3);
    assert_eq!(checked.operation_count(), 3);
    assert_eq!(checked.attempt_count(), 3);
    assert_eq!(checked.retry_count(), 1);
    let empty = NativeBudgetRecording {
        session: [0; 32],
        attempts: Arc::from([]),
        retries: Arc::from([]),
        used: 0,
    };
    assert_eq!(check(&empty, Arc::from([])).unwrap().total(), 0);
}

#[test]
fn journal_rejects_mutated_sessions_occurrences_links_cuts_lifecycle_and_conflicts() {
    for mutation in 0..30 {
        let (mut recording, mut operations) = fixture();
        let rows = Arc::make_mut(&mut operations);
        match mutation {
            0 => recording.session[0] = 1,
            1 => Arc::make_mut(&mut recording.attempts)[0].occurrence.session[0] = 1,
            2 => rows[0].occurrence.session[0] = 1,
            3 => {
                Arc::make_mut(&mut recording.retries)[0].occurrence =
                    recording.attempts[0].occurrence.clone()
            }
            4 => rows[1].occurrence = rows[0].occurrence.clone(),
            5 => Arc::make_mut(&mut recording.attempts)[0].granted = false,
            6 => recording.used += 1,
            7 => Arc::make_mut(&mut recording.retries)[0].accepted_attempt = usize::MAX,
            8 => Arc::make_mut(&mut recording.retries)[0].fresh_before = 0,
            9 => Arc::make_mut(&mut recording.retries)[0].fresh_before = 4,
            10 => {
                Arc::make_mut(&mut recording.retries)[0].occurrence.stage = NativeAttemptStage::Comm
            }
            11 => {
                Arc::make_mut(&mut recording.retries)[0].observation =
                    observation(0, AuthorityByteEventKind::ProduceIntroduction, 2)
            }
            12 => rows[0].introduction = NativeObservationLink::Attempt(usize::MAX),
            13 => rows[2].introduction = NativeObservationLink::Retry(usize::MAX),
            14 => rows[1].introduction = rows[0].introduction,
            15 => {
                rows[0].introduction = NativeObservationLink::Attempt(1);
                rows[1].introduction = NativeObservationLink::Attempt(0);
            }
            16 => rows[0].budget_start = 2,
            17 => rows[0].budget_end = 4,
            18 => rows[0].budget_end = 0,
            19 => rows[2].budget_end = 2,
            20 => rows[0].completion = RSpaceOperationCompletion::Rejected,
            21 => rows[1].completion = RSpaceOperationCompletion::Stored,
            22 => rows[2].footprint = vec![Arc::from([0u8]), Arc::from([0u8])].into(),
            23 => rows[2].footprint = vec![Arc::from([1u8]), Arc::from([0u8])].into(),
            24 => rows[2].predecessors = Arc::from([]),
            25 => rows[2].predecessors = Arc::from([2]),
            26 => rows[2].predecessors = Arc::from([0, 0]),
            27 => {
                rows[0].budget_end = 3;
                rows[2].budget_start = 2;
            }
            28 => {
                Arc::make_mut(&mut recording.attempts).swap(1, 2);
                rows[1].introduction = NativeObservationLink::Attempt(2);
                rows[1].comm.as_mut().unwrap().observation = NativeObservationLink::Attempt(1);
            }
            29 => rows[1].comm = None,
            _ => unreachable!(),
        }
        assert!(
            check(&recording, operations).is_err(),
            "mutation {mutation} was accepted"
        );
    }
}

#[test]
fn journal_rejects_missing_publications_and_duplicate_ownership() {
    let (recording, rows) = fixture();
    assert!(check(&recording, rows[..2].to_vec().into()).is_err());
    let mut duplicate = rows.to_vec();
    duplicate[2].introduction = rows[0].introduction;
    assert!(check(&recording, duplicate.into()).is_err());
}

#[test]
fn journal_bounds_all_variable_collections_before_import() {
    for limit in 0..8 {
        let (recording, rows) = fixture();
        let mut limits = journal_limits();
        match limit {
            0 => limits.operations = 2,
            1 => limits.budget.attempts = 3,
            2 => limits.budget.path_segments = 0,
            3 => limits.total_path_segments = 6,
            4 => limits.source_entries = 0,
            5 => limits.footprint_entries = 2,
            6 => limits.footprint_bytes = 2,
            7 => limits.predecessor_edges = 0,
            _ => unreachable!(),
        }
        let result = with_contract(100, [0, 0, 1, 0], |contract| {
            contract.check_operation_journal([0; 32], &recording, rows, limits, &host())
        });
        assert!(
            matches!(result, Err(NativeOperationJournalError::Limit)),
            "limit {limit}"
        );
    }
}

#[test]
fn journal_preserves_both_independent_operation_orders() {
    for swapped in [false, true] {
        let (recording, rows) = fixture();
        let mut rows = rows.to_vec();
        rows[0].budget_end = 3;
        rows[1].budget_start = 0;
        if swapped {
            rows.swap(0, 1);
            rows[2].predecessors = Arc::from([1]);
        }
        assert!(check(&recording, rows.into()).is_ok());
    }
}

#[test]
fn journal_structural_check_does_not_claim_rspace_authentication() {
    let (recording, rows) = fixture();
    let mut rows = rows.to_vec();
    let NativeOperationSource::Produce(source) = &mut rows[0].source else {
        unreachable!()
    };
    source.hash = [99; 32];
    rows[2].footprint = Arc::from([]);
    rows[2].predecessors = Arc::from([]);
    assert!(check(&recording, rows.into()).is_ok());
}

fn stage_fixture(
    intro_retry: bool,
    comm_retry: bool,
) -> (NativeBudgetRecording, Arc<[NativeOperationRecord]>) {
    let (_, template) = fixture();
    let comm_source = Arc::clone(&template[1].comm.as_ref().unwrap().source);
    let mut attempts = vec![
        NativeBudgetAttempt {
            occurrence: occurrence(0, NativeAttemptStage::ProduceIntroduction),
            observation: observation(0, AuthorityByteEventKind::ProduceIntroduction, 0),
            granted: true,
        },
        NativeBudgetAttempt {
            occurrence: occurrence(0, NativeAttemptStage::Comm),
            observation: observation(1, AuthorityByteEventKind::Comm, 0),
            granted: true,
        },
    ];
    let mut retries = Vec::new();
    let mut links = Vec::new();
    for (accepted, is_retry) in [intro_retry, comm_retry].into_iter().enumerate() {
        let original = &attempts[accepted];
        let occurrence = occurrence(1, original.occurrence.stage);
        let observation = Arc::clone(&original.observation);
        if is_retry {
            links.push(NativeObservationLink::Retry(retries.len()));
            retries.push(NativeBudgetRetry {
                occurrence,
                observation,
                accepted_attempt: accepted,
                fresh_before: attempts.len(),
            });
        } else {
            links.push(NativeObservationLink::Attempt(attempts.len()));
            attempts.push(NativeBudgetAttempt {
                occurrence,
                observation,
                granted: true,
            });
        }
    }
    let end = attempts.len();
    let mut rows = vec![
        operation(0, 0, 2, NativeObservationLink::Attempt(0)),
        operation(1, 2, end, links[0]),
    ];
    for (row, link) in rows
        .iter_mut()
        .zip([NativeObservationLink::Attempt(1), links[1]])
    {
        row.comm = Some(NativeCommRecord {
            source: Arc::clone(&comm_source),
            observation: link,
        });
        row.completion = RSpaceOperationCompletion::Matched;
    }
    attempts.push(NativeBudgetAttempt {
        occurrence: occurrence(2, NativeAttemptStage::ProduceIntroduction),
        observation: observation(2, AuthorityByteEventKind::ProduceIntroduction, 0),
        granted: true,
    });
    rows.push(operation(
        2,
        end,
        end + 1,
        NativeObservationLink::Attempt(end),
    ));
    (
        NativeBudgetRecording {
            session: [0; 32],
            attempts: attempts.into(),
            retries: retries.into(),
            used: 0,
        },
        rows.into(),
    )
}

#[test]
fn journal_stage_order_matches_all_four_proven_link_combinations() {
    for (intro_retry, comm_retry) in [(false, false), (false, true), (true, false), (true, true)] {
        let (mut recording, mut rows) = stage_fixture(intro_retry, comm_retry);
        assert_eq!(check(&recording, Arc::clone(&rows)).unwrap().total(), 0);
        if intro_retry && comm_retry {
            assert_eq!(rows[1].budget_start, rows[1].budget_end);
            assert_eq!(
                recording.retries[0].fresh_before,
                recording.retries[1].fresh_before
            );
        }
        match (intro_retry, comm_retry) {
            (false, false) => {
                Arc::make_mut(&mut recording.attempts).swap(2, 3);
                let row = &mut Arc::make_mut(&mut rows)[1];
                row.introduction = NativeObservationLink::Attempt(3);
                row.comm.as_mut().unwrap().observation = NativeObservationLink::Attempt(2);
            }
            (false, true) => Arc::make_mut(&mut recording.retries)[0].fresh_before = 2,
            (true, false) => Arc::make_mut(&mut recording.retries)[0].fresh_before = 3,
            (true, true) => {
                Arc::make_mut(&mut recording.retries)[0].fresh_before = 3;
                Arc::make_mut(&mut rows)[1].budget_end = 3;
            }
        }
        assert!(
            matches!(
                check(&recording, rows),
                Err(NativeOperationJournalError::Stage)
            ),
            "reversed order: intro_retry={intro_retry}, comm_retry={comm_retry}"
        );
    }
}

#[test]
fn journal_completion_matches_every_proven_lifecycle_case() {
    let (_, template) = fixture();
    let source = Arc::clone(&template[1].comm.as_ref().unwrap().source);
    for intro in [true, false] {
        for comm in [None, Some(true), Some(false)] {
            for completion in [
                RSpaceOperationCompletion::Stored,
                RSpaceOperationCompletion::Matched,
                RSpaceOperationCompletion::Rejected,
            ] {
                let mut attempts = vec![NativeBudgetAttempt {
                    occurrence: occurrence(0, NativeAttemptStage::ProduceIntroduction),
                    observation: observation(
                        0,
                        AuthorityByteEventKind::ProduceIntroduction,
                        u64::from(!intro),
                    ),
                    granted: intro,
                }];
                let mut row = operation(0, 0, 1, NativeObservationLink::Attempt(0));
                row.completion = completion;
                if let Some(granted) = comm {
                    attempts.push(NativeBudgetAttempt {
                        occurrence: occurrence(0, NativeAttemptStage::Comm),
                        observation: observation(
                            1,
                            AuthorityByteEventKind::Comm,
                            u64::from(!granted),
                        ),
                        granted,
                    });
                    row.budget_end = 2;
                    row.comm = Some(NativeCommRecord {
                        source: Arc::clone(&source),
                        observation: NativeObservationLink::Attempt(1),
                    });
                }
                let recording = NativeBudgetRecording {
                    session: [0; 32],
                    attempts: attempts.into(),
                    retries: Arc::from([]),
                    used: 0,
                };
                let result = with_contract(0, [0, 0, 1, 0], |contract| {
                    contract.check_operation_journal(
                        [0; 32],
                        &recording,
                        Arc::from([row]),
                        journal_limits(),
                        &host(),
                    )
                });
                let valid = matches!(
                    (intro, comm, completion),
                    (true, None, RSpaceOperationCompletion::Stored)
                        | (true, Some(true), RSpaceOperationCompletion::Matched)
                        | (false, None, RSpaceOperationCompletion::Rejected)
                        | (true, Some(false), RSpaceOperationCompletion::Rejected)
                );
                assert_eq!(
                    result.is_ok(),
                    valid,
                    "intro={intro}, comm={comm:?}, completion={completion:?}"
                );
                if !valid {
                    assert!(matches!(
                        result,
                        Err(NativeOperationJournalError::Lifecycle)
                    ));
                }
            }
        }
    }
}

#[test]
fn journal_preserves_overflow_denial_and_rejects_retry_of_denied_attempt() {
    let recording = NativeBudgetRecording {
        session: [0; 32],
        attempts: Arc::from([NativeBudgetAttempt {
            occurrence: occurrence(0, NativeAttemptStage::ProduceIntroduction),
            observation: observation(0, AuthorityByteEventKind::ProduceIntroduction, u64::MAX),
            granted: false,
        }]),
        retries: Arc::from([]),
        used: 0,
    };
    let mut row = operation(0, 0, 1, NativeObservationLink::Attempt(0));
    row.completion = RSpaceOperationCompletion::Rejected;
    with_contract(u64::MAX, [0, 0, 2, 0], |contract| {
        assert_eq!(
            contract
                .check_operation_journal(
                    [0; 32],
                    &recording,
                    Arc::from([row.clone()]),
                    journal_limits(),
                    &host()
                )
                .unwrap()
                .total(),
            0
        );
        let mut invalid = recording.clone();
        invalid.retries = Arc::from([NativeBudgetRetry {
            occurrence: occurrence(1, NativeAttemptStage::ProduceIntroduction),
            observation: Arc::clone(&recording.attempts[0].observation),
            accepted_attempt: 0,
            fresh_before: 1,
        }]);
        let retry = operation(1, 1, 1, NativeObservationLink::Retry(0));
        assert!(matches!(
            contract.check_operation_journal(
                [0; 32],
                &invalid,
                Arc::from([row, retry]),
                journal_limits(),
                &host()
            ),
            Err(NativeOperationJournalError::Retry)
        ));
    });
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn journal_generated_retry_cuts_match_the_formal_stage_contract(
        intro_retry in any::<bool>(),
        comm_retry in any::<bool>(),
        intro_cut in prop_oneof![0usize..8, any::<usize>()],
        comm_cut in prop_oneof![0usize..8, any::<usize>()],
    ) {
        let (mut recording, mut rows) = stage_fixture(intro_retry, comm_retry);
        let count = recording.attempts.len();
        Arc::make_mut(&mut rows)[1].budget_end = count;
        let mut retry = 0;
        if intro_retry {
            Arc::make_mut(&mut recording.retries)[retry].fresh_before = intro_cut;
            retry += 1;
        }
        if comm_retry {
            Arc::make_mut(&mut recording.retries)[retry].fresh_before = comm_cut;
        }
        let intro_end = if intro_retry { intro_cut } else { 3 };
        let comm_start = if comm_retry { comm_cut } else if intro_retry { 2 } else { 3 };
        let expected = (!intro_retry || (2..=count).contains(&intro_cut))
            && (!comm_retry || (2..=count).contains(&comm_cut))
            && intro_end <= comm_start;
        prop_assert_eq!(check(&recording, rows).is_ok(), expected);
    }

    #[test]
    fn journal_generated_ownership_matches_exact_publication_coverage(
        total in 0usize..8,
        links in prop::collection::vec(0usize..10, 0..12),
    ) {
        let attempts = (0..total).map(|index| NativeBudgetAttempt {
            occurrence: occurrence(index as u64, NativeAttemptStage::ProduceIntroduction),
            observation: observation(index as u8, AuthorityByteEventKind::ProduceIntroduction, 0),
            granted: true,
        }).collect::<Vec<_>>();
        let recording = NativeBudgetRecording {
            session: [0; 32], attempts: attempts.into(), retries: Arc::from([]), used: 0,
        };
        let rows = links.iter().map(|index| operation(*index as u64, 0, total, NativeObservationLink::Attempt(*index)))
            .collect::<Vec<_>>();
        let mut reference = links;
        reference.sort_unstable();
        let expected = reference == (0..total).collect::<Vec<_>>();
        prop_assert_eq!(check(&recording, rows.into()).is_ok(), expected);
    }

    #[test]
    fn journal_host_rejection_never_changes_input_evidence(limit in 0u64..10000) {
        let (recording, rows) = fixture();
        let before = format!("{recording:?}{rows:?}");
        let budget = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(limit)));
        let result = with_contract(100, [0, 0, 1, 0], |contract| {
            contract.check_operation_journal([0; 32], &recording, Arc::clone(&rows), journal_limits(), &budget)
        });
        prop_assert_eq!(before, format!("{recording:?}{rows:?}"));
        if budget.is_rejected() { prop_assert!(result.is_err()); }
        if let Ok(checked) = result { prop_assert_eq!(checked.total(), 3); }
    }
}
