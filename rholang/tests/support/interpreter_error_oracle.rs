use std::fmt;

use rholang::rust::interpreter::errors::InterpreterError;

pub fn clone_recursive(error: &InterpreterError) -> InterpreterError {
    match error {
        InterpreterError::AggregateError { interpreter_errors } => {
            InterpreterError::AggregateError {
                interpreter_errors: interpreter_errors.iter().map(clone_recursive).collect(),
            }
        }
        InterpreterError::NonDeterministicProcessFailure {
            cause,
            output_not_produced,
        } => InterpreterError::NonDeterministicProcessFailure {
            cause: Box::new(clone_recursive(cause)),
            output_not_produced: output_not_produced.clone(),
        },
        InterpreterError::ProduceFailureWithOutput {
            cause,
            output_not_produced,
        } => InterpreterError::ProduceFailureWithOutput {
            cause: Box::new(clone_recursive(cause)),
            output_not_produced: output_not_produced.clone(),
        },
        InterpreterError::Located { path, source } => InterpreterError::Located {
            path: path.clone(),
            source: Box::new(clone_recursive(source)),
        },
        _ => error.clone(),
    }
}

pub fn eq_recursive(left: &InterpreterError, right: &InterpreterError) -> bool {
    match (left, right) {
        (
            InterpreterError::AggregateError {
                interpreter_errors: left,
            },
            InterpreterError::AggregateError {
                interpreter_errors: right,
            },
        ) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| eq_recursive(left, right))
        }
        (
            InterpreterError::NonDeterministicProcessFailure {
                cause: left_cause,
                output_not_produced: left_output,
            },
            InterpreterError::NonDeterministicProcessFailure {
                cause: right_cause,
                output_not_produced: right_output,
            },
        )
        | (
            InterpreterError::ProduceFailureWithOutput {
                cause: left_cause,
                output_not_produced: left_output,
            },
            InterpreterError::ProduceFailureWithOutput {
                cause: right_cause,
                output_not_produced: right_output,
            },
        ) => left_output == right_output && eq_recursive(left_cause, right_cause),
        (
            InterpreterError::Located {
                path: left_path,
                source: left_source,
            },
            InterpreterError::Located {
                path: right_path,
                source: right_source,
            },
        ) => left_path == right_path && eq_recursive(left_source, right_source),
        (InterpreterError::AggregateError { .. }, _)
        | (InterpreterError::NonDeterministicProcessFailure { .. }, _)
        | (InterpreterError::ProduceFailureWithOutput { .. }, _)
        | (InterpreterError::Located { .. }, _)
        | (_, InterpreterError::AggregateError { .. })
        | (_, InterpreterError::NonDeterministicProcessFailure { .. })
        | (_, InterpreterError::ProduceFailureWithOutput { .. })
        | (_, InterpreterError::Located { .. }) => false,
        _ => left == right,
    }
}

pub fn root_cause_recursive(error: &InterpreterError) -> &InterpreterError {
    match error {
        InterpreterError::Located { source, .. } => root_cause_recursive(source),
        other => other,
    }
}

pub struct RecursiveDebug<'a>(pub &'a InterpreterError);

struct RecursiveSliceDebug<'a>(&'a [InterpreterError]);

impl fmt::Debug for RecursiveSliceDebug<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_list()
            .entries(self.0.iter().map(RecursiveDebug))
            .finish()
    }
}

impl fmt::Debug for RecursiveDebug<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            InterpreterError::AggregateError { interpreter_errors } => formatter
                .debug_struct("AggregateError")
                .field(
                    "interpreter_errors",
                    &RecursiveSliceDebug(interpreter_errors),
                )
                .finish(),
            InterpreterError::NonDeterministicProcessFailure {
                cause,
                output_not_produced,
            } => formatter
                .debug_struct("NonDeterministicProcessFailure")
                .field("cause", &RecursiveDebug(cause))
                .field("output_not_produced", output_not_produced)
                .finish(),
            InterpreterError::ProduceFailureWithOutput {
                cause,
                output_not_produced,
            } => formatter
                .debug_struct("ProduceFailureWithOutput")
                .field("cause", &RecursiveDebug(cause))
                .field("output_not_produced", output_not_produced)
                .finish(),
            InterpreterError::Located { path, source } => formatter
                .debug_struct("Located")
                .field("path", path)
                .field("source", &RecursiveDebug(source))
                .finish(),
            leaf => fmt::Debug::fmt(leaf, formatter),
        }
    }
}

pub struct RecursiveDisplay<'a>(pub &'a InterpreterError);

impl fmt::Display for RecursiveDisplay<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            InterpreterError::AggregateError { interpreter_errors } => {
                formatter.write_str("Error: Aggregate Error\n")?;
                for (index, error) in interpreter_errors.iter().enumerate() {
                    if index != 0 {
                        formatter.write_str("\n")?;
                    }
                    fmt::Debug::fmt(&RecursiveDebug(error), formatter)?;
                }
                Ok(())
            }
            InterpreterError::NonDeterministicProcessFailure { cause, .. } => {
                write!(
                    formatter,
                    "Non-deterministic process failure: {}",
                    RecursiveDisplay(cause)
                )
            }
            InterpreterError::ProduceFailureWithOutput { cause, .. } => write!(
                formatter,
                "Produce failure with output: {}",
                RecursiveDisplay(cause)
            ),
            InterpreterError::Located { path, source } => {
                formatter.write_str("[")?;
                for (index, component) in path.iter().enumerate() {
                    if index != 0 {
                        formatter.write_str(".")?;
                    }
                    fmt::Display::fmt(component, formatter)?;
                }
                write!(formatter, "] {}", RecursiveDisplay(source))
            }
            leaf => fmt::Display::fmt(leaf, formatter),
        }
    }
}
