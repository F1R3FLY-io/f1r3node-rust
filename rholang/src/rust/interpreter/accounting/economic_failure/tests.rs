use models::rust::host_work::{HostWorkDimension, HostWorkLimit, HostWorkLimits};
use proptest::prelude::*;
use rholang_parser::{SourcePos, SourceSpan};
use rspace_plus_plus::rspace::errors::{HistoryError, RSpaceError, RadixTreeError, RootError};
use shared::rust::store::key_value_store::KvStoreError;

use super::*;

const KINDS: [PhloFailure; 4] = [
    PhloFailure::User,
    PhloFailure::Platform,
    PhloFailure::Certificate,
    PhloFailure::Unclassified,
];

fn bits(summary: EvaluationFailureSummary) -> u8 {
    KINDS.iter().enumerate().fold(0, |bits, (index, kind)| {
        bits | (u8::from(summary.contains(*kind)) << index)
    })
}

fn summary(bits: u8) -> EvaluationFailureSummary {
    KINDS
        .iter()
        .enumerate()
        .filter(|(index, _)| bits & (1 << index) != 0)
        .fold(EvaluationFailureSummary::default(), |summary, (_, kind)| {
            summary.union(EvaluationFailureSummary::single(*kind))
        })
}

fn budget() -> HostWorkBudget {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(1_000_000_000)))
}

fn span() -> SourceSpan {
    SourceSpan {
        start: SourcePos { line: 1, col: 2 },
        end: SourcePos { line: 1, col: 5 },
    }
}

#[test]
fn typed_variants_define_charge_class_without_message_heuristics() {
    let text = "user abort OutOfPhlogistons platform failure success".to_string();
    let user = vec![
        InterpreterError::UserAbortError,
        InterpreterError::MethodNotDefined {
            method: text.clone(),
            other_type: text.clone(),
        },
        InterpreterError::MethodArgumentNumberMismatch {
            method: text.clone(),
            expected: 2,
            actual: 3,
        },
        InterpreterError::OperatorNotDefined {
            op: text.clone(),
            other_type: text.clone(),
        },
        InterpreterError::OperatorExpectedError {
            op: text.clone(),
            expected: text.clone(),
            other_type: text.clone(),
        },
        InterpreterError::IfConditionTypeError {
            actual_type: text.clone(),
        },
    ];
    let platform = vec![
        InterpreterError::BugFoundError(text.clone()),
        InterpreterError::UndefinedRequiredProtobufFieldError(text.clone()),
        InterpreterError::HostWorkRejected,
        InterpreterError::DecodeError(text.clone()),
        InterpreterError::OpenAIError(text.clone()),
        InterpreterError::OllamaError(text.clone()),
        InterpreterError::ChromaDBError(text.clone()),
        InterpreterError::IoError(text.clone()),
        InterpreterError::CanNotReplayFailedNonDeterministicProcess,
        InterpreterError::RSpaceError(RSpaceError::InterpreterError(text.clone())),
        InterpreterError::RSpaceError(RSpaceError::BugFoundError(text.clone())),
        InterpreterError::RSpaceError(RSpaceError::ReportingError(text.clone())),
        InterpreterError::RSpaceError(RSpaceError::KvStoreError(KvStoreError::IoError(
            text.clone(),
        ))),
        InterpreterError::RSpaceError(RSpaceError::RadixTreeError(RadixTreeError::KeyNotFound(
            text.clone(),
        ))),
        InterpreterError::RSpaceError(RSpaceError::UnusedCommEvent {
            leftover_count: 1,
            leftover: vec![text.clone()],
        }),
        InterpreterError::RSpaceError(RSpaceError::HistoryError(HistoryError::RootError(
            RootError::UnknownRootError(text.clone()),
        ))),
    ];
    let certificate = vec![
        InterpreterError::OutOfPhlogistonsError,
        InterpreterError::RSpaceError(RSpaceError::OutOfPhlogistons),
    ];
    let unclassified = vec![
        InterpreterError::NormalizerError(text.clone()),
        InterpreterError::SyntaxError(text.clone()),
        InterpreterError::LexerError(text.clone()),
        InterpreterError::ParserError(text.clone()),
        InterpreterError::EncodeError(text.clone()),
        InterpreterError::UnexpectedBundleContent(text.clone()),
        InterpreterError::UnrecognizedNormalizerError(text.clone()),
        InterpreterError::TopLevelWildcardsNotAllowedError(text.clone()),
        InterpreterError::TopLevelFreeVariablesNotAllowedError(text.clone()),
        InterpreterError::TopLevelLogicalConnectivesNotAllowedError(text.clone()),
        InterpreterError::SubstituteError(text.clone()),
        InterpreterError::PatternReceiveError(text.clone()),
        InterpreterError::SetupError(text.clone()),
        InterpreterError::UnrecognizedInterpreterError(text.clone()),
        InterpreterError::SortMatchError(text.clone()),
        InterpreterError::ReduceError(text.clone()),
        InterpreterError::IllegalArgumentError(text.clone()),
        InterpreterError::UnexpectedProcContext {
            var_name: text.clone(),
            name_var_source_span: span(),
            process_source_span: span(),
        },
        InterpreterError::UnexpectedReuseOfProcContextFree {
            var_name: text.clone(),
            first_use: span(),
            second_use: span(),
        },
        InterpreterError::UnboundVariableRefSpan {
            var_name: text.clone(),
            source_span: span(),
        },
        InterpreterError::UnboundVariableRefPos {
            var_name: text.clone(),
            source_pos: SourcePos { line: 2, col: 3 },
        },
        InterpreterError::ReceiveOnSameChannelsError {
            source_span: span(),
        },
        InterpreterError::UnexpectedNameContext {
            var_name: text.clone(),
            proc_var_source_span: span(),
            name_source_span: span(),
        },
        InterpreterError::UnexpectedReuseOfNameContextFree {
            var_name: text,
            first_use: span(),
            second_use: span(),
        },
    ];
    for (errors, expected) in [
        (user, PhloFailure::User),
        (platform, PhloFailure::Platform),
        (certificate, PhloFailure::Certificate),
        (unclassified, PhloFailure::Unclassified),
    ] {
        for error in errors {
            assert_eq!(
                classify_errors(std::slice::from_ref(&error), None).unwrap(),
                EvaluationFailureSummary::single(expected),
                "{error:?}"
            );
        }
    }
}

#[test]
fn empty_success_is_distinct_from_an_empty_error_aggregate() {
    let success = classify_errors(&[], None).unwrap();
    assert_eq!(success, EvaluationFailureSummary::default());
    assert!(success.permits_retained_charge());
    let empty_error = classify_errors(
        &[InterpreterError::AggregateError {
            interpreter_errors: vec![],
        }],
        None,
    )
    .unwrap();
    assert_eq!(
        empty_error,
        EvaluationFailureSummary::single(PhloFailure::Unclassified)
    );
    assert!(!empty_error.permits_retained_charge());
}

#[test]
fn wrapper_platform_class_preserves_every_nested_failure_class() {
    let errors = [InterpreterError::NonDeterministicProcessFailure {
        cause: Box::new(InterpreterError::ProduceFailureWithOutput {
            cause: Box::new(InterpreterError::AggregateError {
                interpreter_errors: vec![
                    InterpreterError::UserAbortError,
                    InterpreterError::OutOfPhlogistonsError,
                    InterpreterError::AggregateError {
                        interpreter_errors: vec![],
                    },
                ],
            }),
            output_not_produced: vec![vec![1, 2]],
        }),
        output_not_produced: vec![vec![3]],
    }];
    assert_eq!(bits(classify_errors(&errors, None).unwrap()), 15);
}

#[test]
fn all_summary_combinations_obey_union_and_fail_closed_retention() {
    for a in 0..16 {
        let left = summary(a);
        assert_eq!(bits(left), a);
        assert_eq!(left.permits_retained_charge(), a & 14 == 0);
        assert_eq!(left.union(left), left);
        for b in 0..16 {
            let right = summary(b);
            assert_eq!(bits(left.union(right)), a | b);
            assert_eq!(left.union(right), right.union(left));
            for c in 0..16 {
                let third = summary(c);
                assert_eq!(
                    left.union(right).union(third),
                    left.union(right.union(third))
                );
            }
        }
    }
}

#[test]
fn classification_budget_exhaustion_returns_no_partial_success() {
    let errors = [InterpreterError::AggregateError {
        interpreter_errors: vec![
            InterpreterError::UserAbortError,
            InterpreterError::NonDeterministicProcessFailure {
                cause: Box::new(InterpreterError::OutOfPhlogistonsError),
                output_not_produced: vec![],
            },
            InterpreterError::IoError("storage".to_string()),
        ],
    }];
    let measured = budget();
    let expected = classify_errors(&errors, Some(&measured)).unwrap();
    assert_eq!(bits(expected), 7);
    assert_eq!(classify_errors(&errors, None).unwrap(), expected);
    let mut charged_dimensions = 0;
    for dimension in HostWorkDimension::ALL {
        let needed = measured.usage(dimension).get();
        if needed == 0 {
            continue;
        }
        charged_dimensions += 1;
        for available in [0, needed - 1, needed] {
            let mut limits = measured.limits();
            limits.set(dimension, HostWorkLimit::new(available));
            let constrained = HostWorkBudget::new(limits);
            let result = classify_errors(&errors, Some(&constrained));
            if available == needed {
                assert_eq!(result.unwrap(), expected);
            } else {
                assert!(result.is_err());
                assert!(constrained.is_rejected());
            }
        }
    }
    assert!(charged_dimensions > 0);
}

#[test]
fn parallel_recording_keeps_all_bits_and_detached_reporters_cannot_change_other_runs() {
    let recorder: std::sync::Arc<FailureRecorder> = std::sync::Arc::new(FailureRecorder::default());
    let start = std::sync::Arc::new(std::sync::Barrier::new(KINDS.len()));
    let handles: Vec<_> = KINDS
        .iter()
        .copied()
        .map(|kind| {
            let recorder = std::sync::Arc::clone(&recorder);
            let start = std::sync::Arc::clone(&start);
            std::thread::spawn(move || {
                start.wait();
                for _ in 0..64 {
                    recorder.record(EvaluationFailureSummary::single(kind));
                }
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }
    assert_eq!(bits(recorder.snapshot()), 15);
    let old: std::sync::Arc<FailureRecorder> = std::sync::Arc::new(FailureRecorder::default());
    let current: FailureRecorder = FailureRecorder::default();
    let frozen = old.snapshot();
    let detached = std::sync::Arc::clone(&old);
    let late = std::thread::spawn(move || {
        detached.record(EvaluationFailureSummary::single(PhloFailure::Platform))
    });
    current.record(EvaluationFailureSummary::single(PhloFailure::User));
    late.join().unwrap();
    assert_eq!(frozen, EvaluationFailureSummary::default());
    assert_eq!(bits(old.snapshot()), 2);
    assert_eq!(bits(current.snapshot()), 1);
}

impl FailureAtomic for loom::sync::atomic::AtomicU8 {
    fn include(&self, bits: u8) { self.fetch_or(bits, loom::sync::atomic::Ordering::AcqRel); }
    fn read(&self) -> u8 { self.load(loom::sync::atomic::Ordering::Acquire) }
}

#[test]
fn loom_production_recorder_union_is_monotone_and_never_loses_reports() {
    loom::model(|| {
        let recorder =
            loom::sync::Arc::new(FailureRecorder::<loom::sync::atomic::AtomicU8>::default());
        let left = loom::sync::Arc::clone(&recorder);
        let right = loom::sync::Arc::clone(&recorder);
        let a = loom::thread::spawn(move || left.record(summary(5)));
        let b = loom::thread::spawn(move || right.record(summary(10)));
        let frozen = recorder.snapshot();
        a.join().unwrap();
        b.join().unwrap();
        let complete = recorder.snapshot();
        assert_eq!(bits(complete), 15);
        assert_eq!(frozen.union(complete), complete);
        assert!(!complete.permits_retained_charge());
    });
}

#[test]
fn loom_old_reporters_and_owned_results_do_not_cross_evaluation_boundaries() {
    loom::model(|| {
        let old = loom::sync::Arc::new(FailureRecorder::<loom::sync::atomic::AtomicU8>::default());
        let current = FailureRecorder::<loom::sync::atomic::AtomicU8>::default();
        let frozen = old.snapshot();
        let reporter = loom::sync::Arc::clone(&old);
        let late = loom::thread::spawn(move || reporter.record(summary(2)));
        current.record(summary(1));
        late.join().unwrap();
        assert_eq!(bits(frozen), 0);
        assert_eq!(bits(old.snapshot()), 2);
        assert_eq!(bits(current.snapshot()), 1);
    });
}

fn error_tree() -> impl Strategy<Value = (InterpreterError, u8)> {
    let leaves = (0_u8..4, "[a-zA-Z ]{0,40}").prop_map(|(kind, text)| match kind {
        0 => (InterpreterError::UserAbortError, 1),
        1 => (InterpreterError::IoError(text), 2),
        2 => (InterpreterError::OutOfPhlogistonsError, 4),
        _ => (InterpreterError::ReduceError(text), 8),
    });
    leaves.prop_recursive(4, 64, 6, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(|children| {
                let bits = if children.is_empty() {
                    8
                } else {
                    children.iter().fold(0, |bits, child| bits | child.1)
                };
                (
                    InterpreterError::AggregateError {
                        interpreter_errors: children.into_iter().map(|child| child.0).collect(),
                    },
                    bits,
                )
            }),
            inner.clone().prop_map(|(error, bits)| (
                InterpreterError::NonDeterministicProcessFailure {
                    cause: Box::new(error),
                    output_not_produced: vec![]
                },
                bits | 2
            )),
            inner.prop_map(|(error, bits)| (
                InterpreterError::ProduceFailureWithOutput {
                    cause: Box::new(error),
                    output_not_produced: vec![vec![1]]
                },
                bits | 2
            )),
        ]
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    #[test]
    fn generated_nested_errors_preserve_union_under_permutation_and_duplication(forest in prop::collection::vec(error_tree(), 0..10)) {
        let expected = forest.iter().fold(0, |bits, tree| bits | tree.1);
        let errors: Vec<_> = forest.into_iter().map(|tree| tree.0).collect();
        let actual = classify_errors(&errors, Some(&budget())).unwrap();
        prop_assert_eq!(bits(actual), expected);
        let reversed: Vec<_> = errors.iter().rev().cloned().collect();
        prop_assert_eq!(classify_errors(&reversed, None).unwrap(), actual);
        let doubled: Vec<_> = errors.iter().chain(&errors).cloned().collect();
        prop_assert_eq!(classify_errors(&doubled, None).unwrap(), actual);
        prop_assert_eq!(actual.permits_retained_charge(), expected & 14 == 0);
    }
}
