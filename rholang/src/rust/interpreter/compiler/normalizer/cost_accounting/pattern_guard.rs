//! Pattern-position rejection of cost syntax.
//!
//! A signed term `{P}_s` and a token stack `s :: S` are PROCESS forms — in
//! process position `compiler::normalize` recognizes them (resolving `s`, lowering
//! the inner ordinarily); in PATTERN position they are not valid match/receive
//! patterns and must be rejected. f1r3node's normalizer does not run rholang-lib's
//! resolver (which performs the same rejection for tooling/LSP — and the `rholang`
//! crate has no `rholang-lib` dependency), so the guard is applied here at every
//! pattern entry point (match cases, receive-bind patterns, contract formals).
//!
//! The scan reuses the parser's `iter_preorder_dfs`, which descends into quoted
//! names and (by this project's traversal extension) into signed-term bodies and
//! signature/stack sub-processes — so nested occurrences such as `@{P}_s` or
//! `[ {P}_s, x ]` are caught too.

use rholang_parser::ast::{AnnProc, Name, Proc};

use crate::rust::interpreter::errors::InterpreterError;

fn cost_syntax_in_pattern_error() -> InterpreterError {
    InterpreterError::NormalizerError(
        "cost-accounting: signed terms `{P}_s` and token stacks `s :: S` cannot appear in pattern \
         position — they are process forms (recognized + metered), not match/receive patterns"
            .to_string(),
    )
}

/// Reject any signed term or token stack anywhere in a pattern process subtree.
/// The borrow is tied to `'ast` because `iter_preorder_dfs` (which follows the
/// arena's `'ast` child pointers) requires it.
pub fn reject_cost_syntax_in_pattern<'ast>(
    pattern: &'ast AnnProc<'ast>,
) -> Result<(), InterpreterError> {
    if pattern
        .iter_preorder_dfs()
        .any(|node| matches!(node.proc, Proc::SignedTerm { .. } | Proc::TokenStack { .. }))
    {
        Err(cost_syntax_in_pattern_error())
    } else {
        Ok(())
    }
}

/// Reject cost syntax in a name-pattern (a receive bind or contract formal): only
/// a quoted name `@P` can carry a process subtree.
pub fn reject_cost_syntax_in_name_pattern<'ast>(
    name: &'ast Name<'ast>,
) -> Result<(), InterpreterError> {
    match name {
        Name::Quote(proc) => reject_cost_syntax_in_pattern(proc),
        Name::NameVar(_) => Ok(()),
    }
}
