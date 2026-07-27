//! The `where`-guard contract: what a guard may contain, and what happens
//! when it contains something else.
//!
//! # The problem this module closes
//!
//! A `where` guard is not evaluated by the reducer. It is evaluated by
//! [`rho_pure_eval`], a deliberately *pure* evaluator that runs inside the
//! RSpace matcher (`Matcher::check_commit`) and inside `Reduce::eval_match`'s
//! case-guard arm. Purity is what makes a guard safe to decide from inside a
//! match attempt and from casper replay — but it is bought by implementing a
//! **subset** of `Reduce::eval_expr`. Everything outside the subset raises
//! `EvalError::UnsupportedExpression`.
//!
//! The guard sites answer a boolean. Collapsing "unsupported" into `false`
//! makes an **undecided** guard indistinguishable from a **refuted** one:
//!
//! ```text
//!   for (@terms <- c where terms.nth(0) == terms.nth(0)) { … }
//! ```
//!
//! is a syntactic *tautology* — the same expression on both sides of `==` —
//! and it admitted nothing. No error, no output, exit 0. The identical
//! expression in the receive's BODY evaluates fine, so an author who tests a
//! predicate interactively and then moves it into the guard gets silence with
//! nothing to read.
//!
//! # The remedy: refuse, and refuse early
//!
//! [`reject_undecidable_guard`] refuses such a guard. It is applied at two
//! layers, and the pair is what makes the coverage total:
//!
//! | layer | site | catches |
//! | --- | --- | --- |
//! | compile | `p_input_normalizer`, `p_match_normalizer` | every guard written in Rholang source |
//! | reduce | [`Reduce::eval_receive`], [`Reduce::eval_match`] | guards on `Par`s built programmatically, which never meet the normalizer |
//!
//! The second layer is not belt-and-braces. `Par`s reach `Reduce` from
//! embedders that construct `Receive.condition` directly rather than by
//! normalizing source text; for those the compile layer does not exist, and
//! the reduce layer is the only gate.
//!
//! ## Why refusing beats deciding-more
//!
//! Teaching [`rho_pure_eval`] to evaluate methods would remove the problem
//! rather than report it, and it is rejected for a reason that is not
//! effort:
//!
//! * **Guard evaluation is unmetered.** `check_commit` runs inside the
//!   matcher, which holds no cost-accounting handle, and it runs *once per
//!   candidate datum per bind* — `Π_j |pool_j|` times in the worst case
//!   (`space_matcher.rs`). The arithmetic fragment is safe under that because
//!   its cost is Θ(1) per node and the node count is fixed by the source
//!   text. Collection methods are not: `nth`, `slice`, `union`, `toByteArray`
//!   are Θ(|data|) in *attacker-supplied, run-time* data. Admitting them
//!   would open unmetered compute proportional to tuplespace contents.
//! * **It would move the subset boundary, not remove it.** Whatever is
//!   implemented, something is left out; the silence returns for the
//!   remainder unless the refusal exists anyway.
//!
//! ## Why refusing at compile time beats raising inside the matcher
//!
//! A guard verdict decides whether a COMM fires, so raising from inside
//! `check_commit` would put a new failure path through `consume`/`produce`
//! in **both** the play and the replay space, where the comm-event sequence
//! must be reproduced exactly. A normalizer refusal is a pure function of the
//! deploy text: same answer on every node, no partially-mutated space to roll
//! back, and no dependence on which data happened to be present.
//!
//! It is also, mechanically, the only option that does not break the
//! `Match::check_commit` signature — a trait implemented outside this
//! workspace.
//!
//! # ⚠ This changes which COMMs fire, and is therefore consensus-visible
//!
//! A program whose guard is undecidable previously normalized, ran, and
//! admitted nothing. It now fails. That is a different deploy outcome, so the
//! change ships with a coordinated version bump — `Validate::version`
//! (`casper/src/rust/validate.rs`) is exact-equality against the
//! genesis-anchored version and there is no per-feature or height-conditioned
//! gate to hide behind. No program that admitted a COMM before admits a
//! different one now: the refused set and the fired set are disjoint.

use models::rhoapi::Par;
use rho_pure_eval::{undecidable_nodes, SpatialSupport};

use super::errors::InterpreterError;

/// The clause label for a `for (… where …)` receive guard.
pub const RECEIVE_WHERE: &str = "where";

/// The clause label for a `match` case guard.
pub const MATCH_CASE_WHERE: &str = "match … where";

/// ★ The spatial-match capability BOTH guard sites are in.
///
/// `rholang` owns the spatial matcher and injects it as
/// `matcher::r#match::SpatialMatcherOracle` at both places a guard is decided,
/// so `EMatchesBody` **is** decidable in a guard and must not be refused. This
/// constant is the single place that fact is stated; if a site ever decided a
/// guard without an oracle it would have to pass `SpatialSupport::Absent` here
/// and the refusal would widen accordingly.
const GUARD_SPATIAL_SUPPORT: SpatialSupport = SpatialSupport::Available;

/// `Ok(())` iff every node of `guard` is one the guard decider can evaluate.
///
/// `clause` is the label carried into the error so the author is told *which*
/// guard was refused; use [`RECEIVE_WHERE`] or [`MATCH_CASE_WHERE`].
///
/// An empty or absent guard is trivially decidable — both guard sites treat
/// `Par::default()` as "no guard" and commit — so it is accepted without a
/// walk.
///
/// # Cost
///
/// Θ(n) in the size of the guard term, once per receive or case at normalize
/// time and once more when the reducer reaches it. It is never paid per
/// candidate datum, which is where guard evaluation itself is paid.
pub fn reject_undecidable_guard(guard: &Par, clause: &'static str) -> Result<(), InterpreterError> {
    if guard == &Par::default() {
        return Ok(());
    }
    let obstructions = undecidable_nodes(guard, GUARD_SPATIAL_SUPPORT);
    if obstructions.is_empty() {
        return Ok(());
    }
    Err(InterpreterError::UndecidableGuard {
        clause,
        obstructions: obstructions.iter().map(|n| n.to_string()).collect(),
    })
}

/// [`reject_undecidable_guard`] over an optional guard — the shape both the
/// normalizer and the reducer hold.
pub fn reject_undecidable_guard_opt(
    guard: Option<&Par>,
    clause: &'static str,
) -> Result<(), InterpreterError> {
    match guard {
        Some(g) => reject_undecidable_guard(g, clause),
        None => Ok(()),
    }
}
