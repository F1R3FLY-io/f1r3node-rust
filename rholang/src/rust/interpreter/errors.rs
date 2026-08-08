// See rholang/src/main/scala/coop/rchain/rholang/interpreter/errors.scala
use std::fmt;

use rspace_plus_plus::rspace::errors::RSpaceError;

// PartialEq here is needed for testing purposes
#[derive(thiserror::Error)]
pub enum InterpreterError {
    RSpaceError(RSpaceError),
    BugFoundError(String),
    UndefinedRequiredProtobufFieldError(String),
    NormalizerError(String),
    SyntaxError(String),
    LexerError(String),
    ParserError(String),
    EncodeError(String),
    DecodeError(String),
    UnexpectedBundleContent(String),
    UnrecognizedNormalizerError(String),
    OutOfPhlogistonsError,
    UserAbortError,
    TopLevelWildcardsNotAllowedError(String),
    TopLevelFreeVariablesNotAllowedError(String),
    TopLevelLogicalConnectivesNotAllowedError(String),
    SubstituteError(String),
    PatternReceiveError(String),
    SetupError(String),
    UnrecognizedInterpreterError(String),
    SortMatchError(String),
    ReduceError(String),
    MethodNotDefined {
        method: String,
        other_type: String,
    },
    MethodArgumentNumberMismatch {
        method: String,
        expected: usize,
        actual: usize,
    },
    OperatorNotDefined {
        op: String,
        other_type: String,
    },
    OperatorExpectedError {
        op: String,
        expected: String,
        other_type: String,
    },
    IfConditionTypeError {
        actual_type: String,
    },
    AggregateError {
        interpreter_errors: Vec<InterpreterError>,
    },

    UnexpectedProcContext {
        var_name: String,
        name_var_source_span: rholang_parser::SourceSpan,
        process_source_span: rholang_parser::SourceSpan,
    },

    UnexpectedReuseOfProcContextFree {
        var_name: String,
        first_use: rholang_parser::SourceSpan,
        second_use: rholang_parser::SourceSpan,
    },

    UnboundVariableRefSpan {
        var_name: String,
        source_span: rholang_parser::SourceSpan,
    },

    UnboundVariableRefPos {
        var_name: String,
        source_pos: rholang_parser::SourcePos,
    },

    ReceiveOnSameChannelsError {
        source_span: rholang_parser::SourceSpan,
    },

    UnexpectedNameContext {
        var_name: String,
        proc_var_source_span: rholang_parser::SourceSpan,
        name_source_span: rholang_parser::SourceSpan,
    },

    UnexpectedReuseOfNameContextFree {
        var_name: String,
        first_use: rholang_parser::SourceSpan,
        second_use: rholang_parser::SourceSpan,
    },

    OpenAIError(String),
    OllamaError(String),
    ChromaDBError(String),
    IllegalArgumentError(String),
    IoError(String),
    /// Raised when a non-deterministic process (OpenAI, Ollama, gRPC) fails during execution.
    /// Contains the underlying cause and the empty output that would have been produced.
    NonDeterministicProcessFailure {
        cause: Box<InterpreterError>,
        output_not_produced: Vec<Vec<u8>>,
    },
    /// Raised when a deterministic produce fails after a successful non-deterministic API call.
    /// Contains the underlying cause and the output that was produced by the API but not stored.
    ProduceFailureWithOutput {
        cause: Box<InterpreterError>,
        output_not_produced: Vec<Vec<u8>>,
    },
    /// Raised during replay when we encounter a failed non-deterministic produce that we cannot replay.
    CanNotReplayFailedNonDeterministicProcess,

    /// A reduction error tagged with the parallel-tree COORDINATE (path) of the detached task that
    /// produced it. Introduced by the detached-spawn / atomic-counter reduction driver: each detached
    /// child task wraps its error (or a captured panic) in `Located` before pushing it to the deploy's
    /// error sink, so the source position (which parallel branch / continuation) is preserved for
    /// display. The path is display-only (NOT consensus): `aggregate_evaluator_errors` and `handle_error`
    /// classify on `root_cause()`, which unwraps `Located`. `source` being named `source` makes thiserror
    /// auto-implement `std::error::Error::source()` for the wrapped error.
    Located {
        path: Vec<u32>,
        source: Box<InterpreterError>,
    },

    /// A `where` guard contains a construct the guard decider cannot evaluate.
    ///
    /// ★ **Why this is an error and not a `false` verdict.** A `where` guard is
    /// decided by `rho-pure-eval` — a deliberate *subset* of `Reduce::eval_expr`
    /// (see that crate's `decidable` module) — running inside the RSpace matcher
    /// and inside `eval_match`'s case-guard arm. Everything outside the subset
    /// yields `EvalError::UnsupportedExpression`. Collapsing that into "the
    /// guard did not hold" makes an *undecided* guard indistinguishable from a
    /// *refuted* one: the program compiles, runs, exits cleanly, admits nothing,
    /// and reports nothing. A guard as ordinary as `xs.length() == 1` then fails
    /// closed with no signal at all, and — because the very same expression
    /// evaluates fine in the receive's BODY — the failure is undiscoverable by
    /// experiment.
    ///
    /// So the undecidable case is refused instead, at the earliest point that
    /// can name it: the normalizer for source-level programs, and
    /// `Reduce::{eval_receive, eval_match}` for Pars that reach the reducer by
    /// another route. Refusing is a pure function of the guard term, so it is
    /// reproduced identically on every node and under replay.
    ///
    /// `clause` is `"where"` (a receive guard) or `"match … where"` (a case
    /// guard); `obstructions` names each undecidable node in the order the
    /// evaluator would have reached it.
    UndecidableGuard {
        clause: &'static str,
        obstructions: Vec<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum InterpreterErrorLeaf<'a> {
    RSpaceError(&'a RSpaceError),
    BugFoundError(&'a String),
    UndefinedRequiredProtobufFieldError(&'a String),
    NormalizerError(&'a String),
    SyntaxError(&'a String),
    LexerError(&'a String),
    ParserError(&'a String),
    EncodeError(&'a String),
    DecodeError(&'a String),
    UnexpectedBundleContent(&'a String),
    UnrecognizedNormalizerError(&'a String),
    OutOfPhlogistonsError,
    UserAbortError,
    TopLevelWildcardsNotAllowedError(&'a String),
    TopLevelFreeVariablesNotAllowedError(&'a String),
    TopLevelLogicalConnectivesNotAllowedError(&'a String),
    SubstituteError(&'a String),
    PatternReceiveError(&'a String),
    SetupError(&'a String),
    UnrecognizedInterpreterError(&'a String),
    SortMatchError(&'a String),
    ReduceError(&'a String),
    MethodNotDefined {
        method: &'a String,
        other_type: &'a String,
    },
    MethodArgumentNumberMismatch {
        method: &'a String,
        expected: usize,
        actual: usize,
    },
    OperatorNotDefined {
        op: &'a String,
        other_type: &'a String,
    },
    OperatorExpectedError {
        op: &'a String,
        expected: &'a String,
        other_type: &'a String,
    },
    IfConditionTypeError {
        actual_type: &'a String,
    },
    UnexpectedProcContext {
        var_name: &'a String,
        name_var_source_span: &'a rholang_parser::SourceSpan,
        process_source_span: &'a rholang_parser::SourceSpan,
    },
    UnexpectedReuseOfProcContextFree {
        var_name: &'a String,
        first_use: &'a rholang_parser::SourceSpan,
        second_use: &'a rholang_parser::SourceSpan,
    },
    UnboundVariableRefSpan {
        var_name: &'a String,
        source_span: &'a rholang_parser::SourceSpan,
    },
    UnboundVariableRefPos {
        var_name: &'a String,
        source_pos: &'a rholang_parser::SourcePos,
    },
    ReceiveOnSameChannelsError {
        source_span: &'a rholang_parser::SourceSpan,
    },
    UnexpectedNameContext {
        var_name: &'a String,
        proc_var_source_span: &'a rholang_parser::SourceSpan,
        name_source_span: &'a rholang_parser::SourceSpan,
    },
    UnexpectedReuseOfNameContextFree {
        var_name: &'a String,
        first_use: &'a rholang_parser::SourceSpan,
        second_use: &'a rholang_parser::SourceSpan,
    },
    OpenAIError(&'a String),
    OllamaError(&'a String),
    ChromaDBError(&'a String),
    IllegalArgumentError(&'a String),
    IoError(&'a String),
    CanNotReplayFailedNonDeterministicProcess,
    UndecidableGuard {
        clause: &'static str,
        obstructions: &'a Vec<String>,
    },
}

impl InterpreterErrorLeaf<'_> {
    fn to_owned(self) -> InterpreterError {
        match self {
            Self::RSpaceError(value) => InterpreterError::RSpaceError(value.clone()),
            Self::BugFoundError(value) => InterpreterError::BugFoundError(value.clone()),
            Self::UndefinedRequiredProtobufFieldError(value) => {
                InterpreterError::UndefinedRequiredProtobufFieldError(value.clone())
            }
            Self::NormalizerError(value) => InterpreterError::NormalizerError(value.clone()),
            Self::SyntaxError(value) => InterpreterError::SyntaxError(value.clone()),
            Self::LexerError(value) => InterpreterError::LexerError(value.clone()),
            Self::ParserError(value) => InterpreterError::ParserError(value.clone()),
            Self::EncodeError(value) => InterpreterError::EncodeError(value.clone()),
            Self::DecodeError(value) => InterpreterError::DecodeError(value.clone()),
            Self::UnexpectedBundleContent(value) => {
                InterpreterError::UnexpectedBundleContent(value.clone())
            }
            Self::UnrecognizedNormalizerError(value) => {
                InterpreterError::UnrecognizedNormalizerError(value.clone())
            }
            Self::OutOfPhlogistonsError => InterpreterError::OutOfPhlogistonsError,
            Self::UserAbortError => InterpreterError::UserAbortError,
            Self::TopLevelWildcardsNotAllowedError(value) => {
                InterpreterError::TopLevelWildcardsNotAllowedError(value.clone())
            }
            Self::TopLevelFreeVariablesNotAllowedError(value) => {
                InterpreterError::TopLevelFreeVariablesNotAllowedError(value.clone())
            }
            Self::TopLevelLogicalConnectivesNotAllowedError(value) => {
                InterpreterError::TopLevelLogicalConnectivesNotAllowedError(value.clone())
            }
            Self::SubstituteError(value) => InterpreterError::SubstituteError(value.clone()),
            Self::PatternReceiveError(value) => {
                InterpreterError::PatternReceiveError(value.clone())
            }
            Self::SetupError(value) => InterpreterError::SetupError(value.clone()),
            Self::UnrecognizedInterpreterError(value) => {
                InterpreterError::UnrecognizedInterpreterError(value.clone())
            }
            Self::SortMatchError(value) => InterpreterError::SortMatchError(value.clone()),
            Self::ReduceError(value) => InterpreterError::ReduceError(value.clone()),
            Self::MethodNotDefined { method, other_type } => InterpreterError::MethodNotDefined {
                method: method.clone(),
                other_type: other_type.clone(),
            },
            Self::MethodArgumentNumberMismatch {
                method,
                expected,
                actual,
            } => InterpreterError::MethodArgumentNumberMismatch {
                method: method.clone(),
                expected,
                actual,
            },
            Self::OperatorNotDefined { op, other_type } => InterpreterError::OperatorNotDefined {
                op: op.clone(),
                other_type: other_type.clone(),
            },
            Self::OperatorExpectedError {
                op,
                expected,
                other_type,
            } => InterpreterError::OperatorExpectedError {
                op: op.clone(),
                expected: expected.clone(),
                other_type: other_type.clone(),
            },
            Self::IfConditionTypeError { actual_type } => InterpreterError::IfConditionTypeError {
                actual_type: actual_type.clone(),
            },
            Self::UnexpectedProcContext {
                var_name,
                name_var_source_span,
                process_source_span,
            } => InterpreterError::UnexpectedProcContext {
                var_name: var_name.clone(),
                name_var_source_span: name_var_source_span.clone(),
                process_source_span: process_source_span.clone(),
            },
            Self::UnexpectedReuseOfProcContextFree {
                var_name,
                first_use,
                second_use,
            } => InterpreterError::UnexpectedReuseOfProcContextFree {
                var_name: var_name.clone(),
                first_use: first_use.clone(),
                second_use: second_use.clone(),
            },
            Self::UnboundVariableRefSpan {
                var_name,
                source_span,
            } => InterpreterError::UnboundVariableRefSpan {
                var_name: var_name.clone(),
                source_span: source_span.clone(),
            },
            Self::UnboundVariableRefPos {
                var_name,
                source_pos,
            } => InterpreterError::UnboundVariableRefPos {
                var_name: var_name.clone(),
                source_pos: source_pos.clone(),
            },
            Self::ReceiveOnSameChannelsError { source_span } => {
                InterpreterError::ReceiveOnSameChannelsError {
                    source_span: source_span.clone(),
                }
            }
            Self::UnexpectedNameContext {
                var_name,
                proc_var_source_span,
                name_source_span,
            } => InterpreterError::UnexpectedNameContext {
                var_name: var_name.clone(),
                proc_var_source_span: proc_var_source_span.clone(),
                name_source_span: name_source_span.clone(),
            },
            Self::UnexpectedReuseOfNameContextFree {
                var_name,
                first_use,
                second_use,
            } => InterpreterError::UnexpectedReuseOfNameContextFree {
                var_name: var_name.clone(),
                first_use: first_use.clone(),
                second_use: second_use.clone(),
            },
            Self::OpenAIError(value) => InterpreterError::OpenAIError(value.clone()),
            Self::OllamaError(value) => InterpreterError::OllamaError(value.clone()),
            Self::ChromaDBError(value) => InterpreterError::ChromaDBError(value.clone()),
            Self::IllegalArgumentError(value) => {
                InterpreterError::IllegalArgumentError(value.clone())
            }
            Self::IoError(value) => InterpreterError::IoError(value.clone()),
            Self::CanNotReplayFailedNonDeterministicProcess => {
                InterpreterError::CanNotReplayFailedNonDeterministicProcess
            }
            Self::UndecidableGuard {
                clause,
                obstructions,
            } => InterpreterError::UndecidableGuard {
                clause,
                obstructions: obstructions.clone(),
            },
        }
    }
}

impl InterpreterError {
    fn as_leaf(&self) -> Option<InterpreterErrorLeaf<'_>> {
        Some(match self {
            Self::RSpaceError(value) => InterpreterErrorLeaf::RSpaceError(value),
            Self::BugFoundError(value) => InterpreterErrorLeaf::BugFoundError(value),
            Self::UndefinedRequiredProtobufFieldError(value) => {
                InterpreterErrorLeaf::UndefinedRequiredProtobufFieldError(value)
            }
            Self::NormalizerError(value) => InterpreterErrorLeaf::NormalizerError(value),
            Self::SyntaxError(value) => InterpreterErrorLeaf::SyntaxError(value),
            Self::LexerError(value) => InterpreterErrorLeaf::LexerError(value),
            Self::ParserError(value) => InterpreterErrorLeaf::ParserError(value),
            Self::EncodeError(value) => InterpreterErrorLeaf::EncodeError(value),
            Self::DecodeError(value) => InterpreterErrorLeaf::DecodeError(value),
            Self::UnexpectedBundleContent(value) => {
                InterpreterErrorLeaf::UnexpectedBundleContent(value)
            }
            Self::UnrecognizedNormalizerError(value) => {
                InterpreterErrorLeaf::UnrecognizedNormalizerError(value)
            }
            Self::OutOfPhlogistonsError => InterpreterErrorLeaf::OutOfPhlogistonsError,
            Self::UserAbortError => InterpreterErrorLeaf::UserAbortError,
            Self::TopLevelWildcardsNotAllowedError(value) => {
                InterpreterErrorLeaf::TopLevelWildcardsNotAllowedError(value)
            }
            Self::TopLevelFreeVariablesNotAllowedError(value) => {
                InterpreterErrorLeaf::TopLevelFreeVariablesNotAllowedError(value)
            }
            Self::TopLevelLogicalConnectivesNotAllowedError(value) => {
                InterpreterErrorLeaf::TopLevelLogicalConnectivesNotAllowedError(value)
            }
            Self::SubstituteError(value) => InterpreterErrorLeaf::SubstituteError(value),
            Self::PatternReceiveError(value) => InterpreterErrorLeaf::PatternReceiveError(value),
            Self::SetupError(value) => InterpreterErrorLeaf::SetupError(value),
            Self::UnrecognizedInterpreterError(value) => {
                InterpreterErrorLeaf::UnrecognizedInterpreterError(value)
            }
            Self::SortMatchError(value) => InterpreterErrorLeaf::SortMatchError(value),
            Self::ReduceError(value) => InterpreterErrorLeaf::ReduceError(value),
            Self::MethodNotDefined { method, other_type } => {
                InterpreterErrorLeaf::MethodNotDefined { method, other_type }
            }
            Self::MethodArgumentNumberMismatch {
                method,
                expected,
                actual,
            } => InterpreterErrorLeaf::MethodArgumentNumberMismatch {
                method,
                expected: *expected,
                actual: *actual,
            },
            Self::OperatorNotDefined { op, other_type } => {
                InterpreterErrorLeaf::OperatorNotDefined { op, other_type }
            }
            Self::OperatorExpectedError {
                op,
                expected,
                other_type,
            } => InterpreterErrorLeaf::OperatorExpectedError {
                op,
                expected,
                other_type,
            },
            Self::IfConditionTypeError { actual_type } => {
                InterpreterErrorLeaf::IfConditionTypeError { actual_type }
            }
            Self::UnexpectedProcContext {
                var_name,
                name_var_source_span,
                process_source_span,
            } => InterpreterErrorLeaf::UnexpectedProcContext {
                var_name,
                name_var_source_span,
                process_source_span,
            },
            Self::UnexpectedReuseOfProcContextFree {
                var_name,
                first_use,
                second_use,
            } => InterpreterErrorLeaf::UnexpectedReuseOfProcContextFree {
                var_name,
                first_use,
                second_use,
            },
            Self::UnboundVariableRefSpan {
                var_name,
                source_span,
            } => InterpreterErrorLeaf::UnboundVariableRefSpan {
                var_name,
                source_span,
            },
            Self::UnboundVariableRefPos {
                var_name,
                source_pos,
            } => InterpreterErrorLeaf::UnboundVariableRefPos {
                var_name,
                source_pos,
            },
            Self::ReceiveOnSameChannelsError { source_span } => {
                InterpreterErrorLeaf::ReceiveOnSameChannelsError { source_span }
            }
            Self::UnexpectedNameContext {
                var_name,
                proc_var_source_span,
                name_source_span,
            } => InterpreterErrorLeaf::UnexpectedNameContext {
                var_name,
                proc_var_source_span,
                name_source_span,
            },
            Self::UnexpectedReuseOfNameContextFree {
                var_name,
                first_use,
                second_use,
            } => InterpreterErrorLeaf::UnexpectedReuseOfNameContextFree {
                var_name,
                first_use,
                second_use,
            },
            Self::OpenAIError(value) => InterpreterErrorLeaf::OpenAIError(value),
            Self::OllamaError(value) => InterpreterErrorLeaf::OllamaError(value),
            Self::ChromaDBError(value) => InterpreterErrorLeaf::ChromaDBError(value),
            Self::IllegalArgumentError(value) => InterpreterErrorLeaf::IllegalArgumentError(value),
            Self::IoError(value) => InterpreterErrorLeaf::IoError(value),
            Self::CanNotReplayFailedNonDeterministicProcess => {
                InterpreterErrorLeaf::CanNotReplayFailedNonDeterministicProcess
            }
            Self::UndecidableGuard {
                clause,
                obstructions,
            } => InterpreterErrorLeaf::UndecidableGuard {
                clause,
                obstructions,
            },
            Self::AggregateError { .. }
            | Self::NonDeterministicProcessFailure { .. }
            | Self::ProduceFailureWithOutput { .. }
            | Self::Located { .. } => return None,
        })
    }
}

impl Clone for InterpreterError {
    fn clone(&self) -> Self {
        enum Task<'a> {
            Visit(&'a InterpreterError),
            Aggregate(usize),
            NonDeterministic(&'a Vec<Vec<u8>>),
            Produce(&'a Vec<Vec<u8>>),
            Located(&'a Vec<u32>),
        }

        let mut tasks = vec![Task::Visit(self)];
        let mut values = Vec::new();
        while let Some(task) = tasks.pop() {
            match task {
                Task::Visit(error) => {
                    if let Some(leaf) = error.as_leaf() {
                        values.push(leaf.to_owned());
                        continue;
                    }
                    match error {
                        Self::AggregateError { interpreter_errors } => {
                            tasks.push(Task::Aggregate(interpreter_errors.len()));
                            tasks.extend(interpreter_errors.iter().rev().map(Task::Visit));
                        }
                        Self::NonDeterministicProcessFailure {
                            cause,
                            output_not_produced,
                        } => {
                            tasks.push(Task::NonDeterministic(output_not_produced));
                            tasks.push(Task::Visit(cause));
                        }
                        Self::ProduceFailureWithOutput {
                            cause,
                            output_not_produced,
                        } => {
                            tasks.push(Task::Produce(output_not_produced));
                            tasks.push(Task::Visit(cause));
                        }
                        Self::Located { path, source } => {
                            tasks.push(Task::Located(path));
                            tasks.push(Task::Visit(source));
                        }
                        _ => unreachable!(),
                    }
                }
                Task::Aggregate(count) => {
                    let interpreter_errors = values.split_off(values.len() - count);
                    values.push(Self::AggregateError { interpreter_errors });
                }
                Task::NonDeterministic(output_not_produced) => {
                    let cause = values.pop().expect("InterpreterError clone missing cause");
                    values.push(Self::NonDeterministicProcessFailure {
                        cause: Box::new(cause),
                        output_not_produced: output_not_produced.clone(),
                    });
                }
                Task::Produce(output_not_produced) => {
                    let cause = values.pop().expect("InterpreterError clone missing cause");
                    values.push(Self::ProduceFailureWithOutput {
                        cause: Box::new(cause),
                        output_not_produced: output_not_produced.clone(),
                    });
                }
                Task::Located(path) => {
                    let source = values.pop().expect("InterpreterError clone missing source");
                    values.push(Self::Located {
                        path: path.clone(),
                        source: Box::new(source),
                    });
                }
            }
        }
        debug_assert_eq!(values.len(), 1);
        values
            .pop()
            .expect("InterpreterError clone produced no root value")
    }
}

impl PartialEq for InterpreterError {
    fn eq(&self, other: &Self) -> bool {
        let mut work = vec![(self, other)];
        while let Some((left, right)) = work.pop() {
            match (left.as_leaf(), right.as_leaf()) {
                (Some(left), Some(right)) => {
                    if left != right {
                        return false;
                    }
                }
                (None, None) => match (left, right) {
                    (
                        Self::AggregateError {
                            interpreter_errors: left,
                        },
                        Self::AggregateError {
                            interpreter_errors: right,
                        },
                    ) => {
                        if left.len() != right.len() {
                            return false;
                        }
                        work.extend(left.iter().zip(right).rev());
                    }
                    (
                        Self::NonDeterministicProcessFailure {
                            cause: left_cause,
                            output_not_produced: left_output,
                        },
                        Self::NonDeterministicProcessFailure {
                            cause: right_cause,
                            output_not_produced: right_output,
                        },
                    )
                    | (
                        Self::ProduceFailureWithOutput {
                            cause: left_cause,
                            output_not_produced: left_output,
                        },
                        Self::ProduceFailureWithOutput {
                            cause: right_cause,
                            output_not_produced: right_output,
                        },
                    ) => {
                        if left_output != right_output {
                            return false;
                        }
                        work.push((left_cause, right_cause));
                    }
                    (
                        Self::Located {
                            path: left_path,
                            source: left_source,
                        },
                        Self::Located {
                            path: right_path,
                            source: right_source,
                        },
                    ) => {
                        if left_path != right_path {
                            return false;
                        }
                        work.push((left_source, right_source));
                    }
                    _ => return false,
                },
                _ => return false,
            }
        }
        true
    }
}

fn write_indent(formatter: &mut fmt::Formatter<'_>, depth: usize) -> fmt::Result {
    for _ in 0..depth {
        formatter.write_str("    ")?;
    }
    Ok(())
}

fn write_embedded_pretty_debug(
    value: &impl fmt::Debug,
    formatter: &mut fmt::Formatter<'_>,
    depth: usize,
) -> fmt::Result {
    let rendered = format!("{value:#?}");
    for (index, line) in rendered.split('\n').enumerate() {
        if index != 0 {
            formatter.write_str("\n")?;
            write_indent(formatter, depth)?;
        }
        formatter.write_str(line)?;
    }
    Ok(())
}

fn fmt_interpreter_error_debug(
    root: &InterpreterError,
    formatter: &mut fmt::Formatter<'_>,
    alternate: bool,
) -> fmt::Result {
    enum Task<'a> {
        Visit(&'a InterpreterError, usize),
        Text(&'static str),
        Indent(usize),
        Path(&'a Vec<u32>, usize),
        Output(&'a Vec<Vec<u8>>, usize),
    }

    let mut tasks = vec![Task::Visit(root, 0)];
    while let Some(task) = tasks.pop() {
        match task {
            Task::Text(text) => formatter.write_str(text)?,
            Task::Indent(depth) => write_indent(formatter, depth)?,
            Task::Path(path, depth) => {
                if alternate {
                    write_embedded_pretty_debug(path, formatter, depth)?;
                } else {
                    fmt::Debug::fmt(path, formatter)?;
                }
            }
            Task::Output(output, depth) => {
                if alternate {
                    write_embedded_pretty_debug(output, formatter, depth)?;
                } else {
                    fmt::Debug::fmt(output, formatter)?;
                }
            }
            Task::Visit(error, depth) => {
                if let Some(leaf) = error.as_leaf() {
                    if alternate {
                        write_embedded_pretty_debug(&leaf, formatter, depth)?;
                    } else {
                        fmt::Debug::fmt(&leaf, formatter)?;
                    }
                    continue;
                }
                match error {
                    InterpreterError::AggregateError { interpreter_errors } if alternate => {
                        formatter.write_str("AggregateError {\n")?;
                        write_indent(formatter, depth + 1)?;
                        if interpreter_errors.is_empty() {
                            formatter.write_str("interpreter_errors: [],\n")?;
                            write_indent(formatter, depth)?;
                            formatter.write_str("}")?;
                        } else {
                            formatter.write_str("interpreter_errors: [\n")?;
                            tasks.push(Task::Text("}"));
                            tasks.push(Task::Indent(depth));
                            tasks.push(Task::Text("],\n"));
                            tasks.push(Task::Indent(depth + 1));
                            for child in interpreter_errors.iter().rev() {
                                tasks.push(Task::Text(",\n"));
                                tasks.push(Task::Visit(child, depth + 2));
                                tasks.push(Task::Indent(depth + 2));
                            }
                        }
                    }
                    InterpreterError::AggregateError { interpreter_errors } => {
                        formatter.write_str("AggregateError { interpreter_errors: [")?;
                        tasks.push(Task::Text("] }"));
                        for (index, child) in interpreter_errors.iter().enumerate().rev() {
                            tasks.push(Task::Visit(child, depth));
                            if index != 0 {
                                tasks.push(Task::Text(", "));
                            }
                        }
                    }
                    InterpreterError::NonDeterministicProcessFailure {
                        cause,
                        output_not_produced,
                    } if alternate => {
                        formatter.write_str("NonDeterministicProcessFailure {\n")?;
                        tasks.push(Task::Text("}"));
                        tasks.push(Task::Indent(depth));
                        tasks.push(Task::Text(",\n"));
                        tasks.push(Task::Output(output_not_produced, depth + 1));
                        tasks.push(Task::Text("output_not_produced: "));
                        tasks.push(Task::Indent(depth + 1));
                        tasks.push(Task::Text(",\n"));
                        tasks.push(Task::Visit(cause, depth + 1));
                        tasks.push(Task::Text("cause: "));
                        tasks.push(Task::Indent(depth + 1));
                    }
                    InterpreterError::NonDeterministicProcessFailure {
                        cause,
                        output_not_produced,
                    } => {
                        formatter.write_str("NonDeterministicProcessFailure { cause: ")?;
                        tasks.push(Task::Text(" }"));
                        tasks.push(Task::Output(output_not_produced, depth));
                        tasks.push(Task::Text(", output_not_produced: "));
                        tasks.push(Task::Visit(cause, depth));
                    }
                    InterpreterError::ProduceFailureWithOutput {
                        cause,
                        output_not_produced,
                    } if alternate => {
                        formatter.write_str("ProduceFailureWithOutput {\n")?;
                        tasks.push(Task::Text("}"));
                        tasks.push(Task::Indent(depth));
                        tasks.push(Task::Text(",\n"));
                        tasks.push(Task::Output(output_not_produced, depth + 1));
                        tasks.push(Task::Text("output_not_produced: "));
                        tasks.push(Task::Indent(depth + 1));
                        tasks.push(Task::Text(",\n"));
                        tasks.push(Task::Visit(cause, depth + 1));
                        tasks.push(Task::Text("cause: "));
                        tasks.push(Task::Indent(depth + 1));
                    }
                    InterpreterError::ProduceFailureWithOutput {
                        cause,
                        output_not_produced,
                    } => {
                        formatter.write_str("ProduceFailureWithOutput { cause: ")?;
                        tasks.push(Task::Text(" }"));
                        tasks.push(Task::Output(output_not_produced, depth));
                        tasks.push(Task::Text(", output_not_produced: "));
                        tasks.push(Task::Visit(cause, depth));
                    }
                    InterpreterError::Located { path, source } if alternate => {
                        formatter.write_str("Located {\n")?;
                        tasks.push(Task::Text("}"));
                        tasks.push(Task::Indent(depth));
                        tasks.push(Task::Text(",\n"));
                        tasks.push(Task::Visit(source, depth + 1));
                        tasks.push(Task::Text("source: "));
                        tasks.push(Task::Indent(depth + 1));
                        tasks.push(Task::Text(",\n"));
                        tasks.push(Task::Path(path, depth + 1));
                        tasks.push(Task::Text("path: "));
                        tasks.push(Task::Indent(depth + 1));
                    }
                    InterpreterError::Located { path, source } => {
                        formatter.write_str("Located { path: ")?;
                        tasks.push(Task::Text(" }"));
                        tasks.push(Task::Visit(source, depth));
                        tasks.push(Task::Text(", source: "));
                        tasks.push(Task::Path(path, depth));
                    }
                    _ => unreachable!(),
                }
            }
        }
    }
    Ok(())
}

impl fmt::Debug for InterpreterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt_interpreter_error_debug(self, formatter, formatter.alternate())
    }
}

fn detach_interpreter_error_children(
    error: &mut InterpreterError,
    work: &mut Vec<InterpreterError>,
) {
    match error {
        InterpreterError::AggregateError { interpreter_errors } => work.append(interpreter_errors),
        InterpreterError::NonDeterministicProcessFailure { cause, .. }
        | InterpreterError::ProduceFailureWithOutput { cause, .. } => {
            let cause = std::mem::replace(cause, Box::new(InterpreterError::UserAbortError));
            work.push(*cause);
        }
        InterpreterError::Located { source, .. } => {
            let source = std::mem::replace(source, Box::new(InterpreterError::UserAbortError));
            work.push(*source);
        }
        _ => {}
    }
}

impl Drop for InterpreterError {
    fn drop(&mut self) {
        let mut work = Vec::new();
        detach_interpreter_error_children(self, &mut work);
        while let Some(mut error) = work.pop() {
            detach_interpreter_error_children(&mut error, &mut work);
        }
    }
}

pub fn illegal_argument_error(method_name: &str) -> InterpreterError {
    InterpreterError::IllegalArgumentError(format!("Incorrect arguments for {}", method_name))
}

fn fmt_interpreter_error_leaf(
    leaf: InterpreterErrorLeaf<'_>,
    f: &mut fmt::Formatter<'_>,
) -> fmt::Result {
    use InterpreterErrorLeaf as InterpreterError;

    match leaf {
        InterpreterError::BugFoundError(msg) => write!(f, "Bug found: {}", msg),

        InterpreterError::RSpaceError(msg) => write!(f, "RSpace Error: {}", msg),

        InterpreterError::UndefinedRequiredProtobufFieldError(field_name) => {
            write!(
                f,
                "A parsed Protobuf field was None, should be Some: {}",
                field_name
            )
        }

        InterpreterError::NormalizerError(msg) => write!(f, "Normalizer error: {}", msg),

        InterpreterError::SyntaxError(msg) => write!(f, "Syntax error: {}", msg),

        InterpreterError::LexerError(msg) => write!(f, "Lexer error: {}", msg),

        InterpreterError::ParserError(msg) => write!(f, "Parser error: {}", msg),

        InterpreterError::EncodeError(msg) => write!(f, "Encode error: {}", msg),

        InterpreterError::DecodeError(msg) => write!(f, "Decode error: {}", msg),

        InterpreterError::UnexpectedBundleContent(msg) => {
            write!(f, "Unexpected bundle content: {}", msg)
        }

        InterpreterError::UnrecognizedNormalizerError(msg) => {
            write!(f, "Unrecognized normalizer error: {}", msg)
        }

        InterpreterError::OutOfPhlogistonsError => {
            write!(f, "Computation ran out of phlogistons.")
        }

        InterpreterError::UserAbortError => {
            write!(f, "Computation aborted by user request.")
        }

        InterpreterError::TopLevelWildcardsNotAllowedError(wildcards) => {
            write!(f, "Top level wildcards are not allowed: {}", wildcards)
        }

        InterpreterError::TopLevelFreeVariablesNotAllowedError(free_vars) => {
            write!(f, "Top level free variables are not allowed: {}", free_vars)
        }

        InterpreterError::TopLevelLogicalConnectivesNotAllowedError(connectives) => write!(
            f,
            "Top level logical connectives are not allowed: {}",
            connectives
        ),

        InterpreterError::SubstituteError(msg) => write!(f, "Substitute error: {}", msg),

        InterpreterError::PatternReceiveError(connectives) => write!(
            f,
            "Invalid pattern in the receive: {}. Only logical AND is allowed.",
            connectives
        ),

        InterpreterError::SetupError(msg) => write!(f, "Setup error: {}", msg),

        InterpreterError::UnrecognizedInterpreterError(_) => {
            write!(f, "Unrecognized interpreter error.")
        }

        InterpreterError::SortMatchError(msg) => write!(f, "Sort match error: {}", msg),

        InterpreterError::ReduceError(msg) => write!(f, "Reduce error: {}", msg),

        InterpreterError::MethodNotDefined { method, other_type } => write!(
            f,
            "Error: Method `{}` is not defined on {}.",
            method, other_type
        ),

        InterpreterError::MethodArgumentNumberMismatch {
            method,
            expected,
            actual,
        } => {
            write!(
                f,
                "Error: Method `{}` expects {} Par argument(s), but got {} argument(s).",
                method, expected, actual
            )
        }

        InterpreterError::OperatorNotDefined { op, other_type } => write!(
            f,
            "Error: Operator `{}` is not defined on {}.",
            op, other_type
        ),

        InterpreterError::IfConditionTypeError { actual_type } => write!(
            f,
            "Error: `if` condition must evaluate to a boolean, but got {}.",
            actual_type
        ),

        InterpreterError::OperatorExpectedError {
            op,
            expected: _,
            other_type,
        } => write!(
            f,
            "Error: Operator `{}` is not defined on {}.",
            op, other_type
        ),

        InterpreterError::OpenAIError(msg) => write!(f, "OpenAI error: {}", msg),

        InterpreterError::OllamaError(msg) => write!(f, "Ollama error: {}", msg),

        InterpreterError::ChromaDBError(msg) => write!(f, "ChromaDB error: {}", msg),

        InterpreterError::IllegalArgumentError(msg) => write!(f, "Illegal argument: {}", msg),

        InterpreterError::IoError(msg) => write!(f, "IO error: {}", msg),

        // Display implementations for SourceSpan-based error variants
        InterpreterError::UnexpectedProcContext {
            var_name,
            name_var_source_span,
            process_source_span,
        } => {
            write!(
                f,
                "Name variable: {} at {} used in process context at {}",
                var_name, name_var_source_span, process_source_span
            )
        }

        InterpreterError::UnexpectedReuseOfProcContextFree {
            var_name,
            first_use,
            second_use,
        } => {
            write!(
                f,
                "Free variable {} is used twice as a binder (at {} and {}) in process context.",
                var_name, first_use, second_use
            )
        }

        InterpreterError::UnboundVariableRefSpan {
            var_name,
            source_span,
        } => {
            write!(
                f,
                "Variable reference: ={} at {} is unbound.",
                var_name, source_span
            )
        }

        InterpreterError::UnboundVariableRefPos {
            var_name,
            source_pos,
        } => {
            write!(
                f,
                "Variable reference: ={} at {} is unbound.",
                var_name, source_pos
            )
        }

        InterpreterError::ReceiveOnSameChannelsError { source_span } => {
            write!(
                f,
                "Receiving on the same channels is currently not allowed (at {}).",
                source_span
            )
        }

        InterpreterError::UnexpectedNameContext {
            var_name,
            proc_var_source_span,
            name_source_span,
        } => {
            write!(
                f,
                "Proc variable: {} at {} used in Name context at {}",
                var_name, proc_var_source_span, name_source_span
            )
        }

        InterpreterError::UnexpectedReuseOfNameContextFree {
            var_name,
            first_use,
            second_use,
        } => {
            write!(
                f,
                "Free variable {} is used twice as a binder (at {} and {}) in name context.",
                var_name, first_use, second_use
            )
        }

        InterpreterError::CanNotReplayFailedNonDeterministicProcess => {
            write!(f, "Cannot replay failed non-deterministic process")
        }

        InterpreterError::UndecidableGuard {
            clause,
            obstructions,
        } => {
            // The message must let the author act. It names the clause
            // (so they know which guard), every obstruction (so they know
            // what to remove), and the remedy — binding the value in the
            // PATTERN, which is where an evaluated value is already
            // available to the guard.
            write!(
                f,
                "`{}` guard cannot be decided: it contains {}. A guard is \
                     evaluated by the matcher, which decides a subset of the \
                     expression language and cannot evaluate these. Bind what \
                     the guard needs in the receive PATTERN instead, where the \
                     value has already been computed.",
                clause,
                obstructions.join(", ")
            )
        }
    }
}

impl fmt::Display for InterpreterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        enum Task<'a> {
            Visit(&'a InterpreterError),
            Debug(&'a InterpreterError),
            Text(&'static str),
        }

        let mut tasks = vec![Task::Visit(self)];
        while let Some(task) = tasks.pop() {
            match task {
                Task::Text(text) => formatter.write_str(text)?,
                Task::Debug(error) => {
                    fmt_interpreter_error_debug(error, formatter, false)?;
                }
                Task::Visit(error) => {
                    if let Some(leaf) = error.as_leaf() {
                        fmt_interpreter_error_leaf(leaf, formatter)?;
                        continue;
                    }
                    match error {
                        InterpreterError::AggregateError { interpreter_errors } => {
                            formatter.write_str("Error: Aggregate Error\n")?;
                            for (index, child) in interpreter_errors.iter().enumerate().rev() {
                                tasks.push(Task::Debug(child));
                                if index != 0 {
                                    tasks.push(Task::Text("\n"));
                                }
                            }
                        }
                        InterpreterError::NonDeterministicProcessFailure { cause, .. } => {
                            formatter.write_str("Non-deterministic process failure: ")?;
                            tasks.push(Task::Visit(cause));
                        }
                        InterpreterError::ProduceFailureWithOutput { cause, .. } => {
                            formatter.write_str("Produce failure with output: ")?;
                            tasks.push(Task::Visit(cause));
                        }
                        InterpreterError::Located { path, source } => {
                            formatter.write_str("[")?;
                            for (index, component) in path.iter().enumerate() {
                                if index != 0 {
                                    formatter.write_str(".")?;
                                }
                                fmt::Display::fmt(component, formatter)?;
                            }
                            formatter.write_str("] ")?;
                            tasks.push(Task::Visit(source));
                        }
                        _ => unreachable!(),
                    }
                }
            }
        }
        Ok(())
    }
}

impl InterpreterError {
    /// Peels any `Located` wrapper(s) to the underlying reduction error. Cost/abort classification in
    /// `aggregate_evaluator_errors` and `handle_error` must key on this so a `Located`-wrapped
    /// `UserAbortError` / `OutOfPhlogistonsError` is NOT misclassified into the generic cost arm (which
    /// would produce a wrong cost -> `ReplayCostMismatch` -> broken consensus). Idempotent on
    /// non-`Located` variants (returns `self`).
    pub fn root_cause(&self) -> &InterpreterError {
        let mut error = self;
        while let InterpreterError::Located { source, .. } = error {
            error = source;
        }
        error
    }
}

impl From<RSpaceError> for InterpreterError {
    fn from(err: RSpaceError) -> InterpreterError { InterpreterError::RSpaceError(err) }
}

impl From<InterpreterError> for RSpaceError {
    fn from(error: InterpreterError) -> Self { RSpaceError::InterpreterError(error.to_string()) }
}

impl From<openai_api_rs::v1::error::APIError> for InterpreterError {
    fn from(error: openai_api_rs::v1::error::APIError) -> Self {
        InterpreterError::OpenAIError(error.to_string())
    }
}

impl From<std::io::Error> for InterpreterError {
    fn from(error: std::io::Error) -> Self { InterpreterError::IoError(error.to_string()) }
}
