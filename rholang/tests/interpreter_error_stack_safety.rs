#[path = "support/interpreter_error_oracle.rs"]
mod interpreter_error_oracle;

use std::error::Error;

use interpreter_error_oracle::{
    clone_recursive, eq_recursive, root_cause_recursive, RecursiveDebug, RecursiveDisplay,
};
use rholang::rust::interpreter::errors::InterpreterError;

fn shallow_corpus() -> Vec<InterpreterError> {
    vec![
        InterpreterError::AggregateError {
            interpreter_errors: vec![
                InterpreterError::BugFoundError("leaf-a".to_string()),
                InterpreterError::Located {
                    path: vec![2, 7],
                    source: Box::new(InterpreterError::NonDeterministicProcessFailure {
                        cause: Box::new(InterpreterError::ReduceError("leaf-b".to_string())),
                        output_not_produced: vec![vec![0, 1], vec![255]],
                    }),
                },
                InterpreterError::ProduceFailureWithOutput {
                    cause: Box::new(InterpreterError::AggregateError {
                        interpreter_errors: vec![
                            InterpreterError::UserAbortError,
                            InterpreterError::CanNotReplayFailedNonDeterministicProcess,
                        ],
                    }),
                    output_not_produced: vec![vec![3, 5, 8]],
                },
            ],
        },
        InterpreterError::AggregateError {
            interpreter_errors: Vec::new(),
        },
        InterpreterError::Located {
            path: Vec::new(),
            source: Box::new(InterpreterError::OutOfPhlogistonsError),
        },
    ]
}

#[test]
fn iterative_lifecycle_matches_the_bounded_recursive_oracle() {
    for error in shallow_corpus() {
        let recursive_clone = clone_recursive(&error);
        let iterative_clone = error.clone();

        assert!(eq_recursive(&error, &iterative_clone));
        assert!(eq_recursive(&iterative_clone, &recursive_clone));
        assert_eq!(error, iterative_clone);
        assert_eq!(
            format!("{error:?}"),
            format!("{:?}", RecursiveDebug(&recursive_clone))
        );
        assert_eq!(
            format!("{error:#?}"),
            format!("{:#?}", RecursiveDebug(&recursive_clone))
        );
        assert_eq!(
            error.to_string(),
            RecursiveDisplay(&recursive_clone).to_string()
        );
        assert_eq!(error.root_cause(), root_cause_recursive(&recursive_clone));
    }
}

#[test]
fn thiserror_source_exposes_one_located_edge_without_traversal() {
    let error = InterpreterError::Located {
        path: vec![1],
        source: Box::new(InterpreterError::Located {
            path: vec![2],
            source: Box::new(InterpreterError::UserAbortError),
        }),
    };

    let first = error.source().expect("Located must expose its source");
    let second = first
        .source()
        .expect("nested Located must expose its source");
    assert!(second.source().is_none());
}

#[test]
fn lifecycle_depth_20000_uses_a_256_kib_native_stack() {
    std::thread::Builder::new()
        .name("interpreter-error-lifecycle-depth-gate".to_string())
        .stack_size(256 * 1024)
        .spawn(|| {
            let mut located = InterpreterError::UserAbortError;
            for depth in 0..20_000_u32 {
                located = InterpreterError::Located {
                    path: vec![depth],
                    source: Box::new(located),
                };
            }
            assert!(matches!(
                located.root_cause(),
                InterpreterError::UserAbortError
            ));
            drop(located);

            let mut error = InterpreterError::ReduceError("root".to_string());
            for depth in 0..20_000_u32 {
                error = match depth % 4 {
                    0 => InterpreterError::Located {
                        path: vec![depth, depth + 1],
                        source: Box::new(error),
                    },
                    1 => InterpreterError::NonDeterministicProcessFailure {
                        cause: Box::new(error),
                        output_not_produced: vec![vec![(depth & 0xff) as u8]],
                    },
                    2 => InterpreterError::ProduceFailureWithOutput {
                        cause: Box::new(error),
                        output_not_produced: vec![vec![((depth + 1) & 0xff) as u8]],
                    },
                    _ => InterpreterError::AggregateError {
                        interpreter_errors: vec![error],
                    },
                };
            }

            let cloned = error.clone();
            assert_eq!(error, cloned);
            let debug = format!("{error:?}");
            let display = error.to_string();
            assert!(debug.starts_with("AggregateError"));
            assert!(display.starts_with("Error: Aggregate Error"));
            drop(cloned);
            drop(error);
        })
        .expect("failed to spawn InterpreterError lifecycle gate")
        .join()
        .expect("InterpreterError lifecycle exhausted the 256 KiB native stack");
}
