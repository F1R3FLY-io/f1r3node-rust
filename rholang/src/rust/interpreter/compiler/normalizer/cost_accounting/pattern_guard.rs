//! Pattern-position rejection of cost syntax.
//!
//! A signed term `{P}_s` lowers to a `for` (a fuel gate) and a purse lowers to
//! a send — neither is a valid *pattern*. In process position the dispatch in
//! `compiler::normalize` lowers them; in pattern position they must be
//! rejected instead. f1r3node's normalizer does not run rholang-lib's resolver
//! (which performs the same rejection for tooling/LSP), so the guard is
//! applied here at every pattern entry point (match cases, receive-bind
//! patterns, contract formals).
//!
//! The scan reuses the parser's `iter_preorder_dfs`, which descends into
//! quoted names and (by this project's traversal extension) into signed-term
//! bodies and signature/purse sub-processes — so nested occurrences such as
//! `@{P}_s` or `[ {P}_s, x ]` are caught too.

use rholang_parser::ast::{AnnProc, Name, Proc};

use crate::rust::interpreter::errors::InterpreterError;

fn cost_syntax_in_pattern_error() -> InterpreterError {
    InterpreterError::NormalizerError(
        "cost-accounting: signed terms `{P}_s` and purses cannot appear in pattern position — \
         they lower to a `for`/send, which is not a valid match or receive pattern"
            .to_string(),
    )
}

/// Reject any signed term or purse anywhere in a pattern process subtree.
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

/// Reject cost syntax in a name-pattern (a receive bind or contract formal):
/// only a quoted name `@P` can carry a process subtree.
pub fn reject_cost_syntax_in_name_pattern<'ast>(
    name: &'ast Name<'ast>,
) -> Result<(), InterpreterError> {
    match name {
        Name::Quote(proc) => reject_cost_syntax_in_pattern(proc),
        Name::NameVar(_) => Ok(()),
    }
}
