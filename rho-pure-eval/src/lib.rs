//! Pure expression evaluator for Rholang Par values.
//!
//! Walks `par.exprs` and evaluates each `Expr` against an `Env<Par>` lookup
//! environment. Sends, receives, news, bundles, and other process-level
//! constructs sitting alongside the exprs in the input Par are carried
//! through as inert data — no side effects fire. The result is a Par
//! whose exprs slot has been replaced with the evaluated values.
//!
//! Determinism guarantee: same input Par + same Env produce the same
//! output Par byte-for-byte on every node. This is what makes it safe
//! to call from inside the rspace matcher (Option 3 of the where-clauses
//! plan) and from casper replay.
//!
//! Scope: this crate implements a subset of `Reduce::eval_expr` from
//! `rholang/src/rust/interpreter/reduce.rs` — enough to support `if`
//! conditions, `where` guards, and match-case guards. Built-in method
//! calls (`EMethodBody`) and the `EMatchExpr` variant are not yet
//! supported and return `EvalError::UnsupportedExpression`. See the
//! plan at docs/plans/where-clauses-and-match-guards-2026-04-29.md §3.9
//! for the longer-term roadmap.

mod env;
mod error;
mod eval;

pub use env::Env;
pub use error::EvalError;
pub use eval::eval;

#[cfg(test)]
mod tests;
