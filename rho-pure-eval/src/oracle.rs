//! Caller-injected spatial-match oracle (the `SpatialMatch` seam).
//!
//! # Why a seam and not a direct call
//!
//! `EMatchesBody` asks "does this target match this pattern?", and the
//! answer is owned by the RSpace spatial matcher
//! (`rholang/src/rust/interpreter/matcher/spatial_matcher.rs`). This
//! crate cannot call it: the dependency runs the other way round —
//! `rholang/Cargo.toml` declares `rho-pure-eval = { path = ".." }`, and
//! this crate's only dependencies are `models`, `shared` and the `num`
//! crates. A direct call would be a dependency cycle.
//!
//! So the matcher is injected by the caller instead, as a trait object:
//! `rholang` owns the implementation, `rho-pure-eval` owns the question.
//! No crate moves, nothing is hoisted, and the whole spatial fragment
//! becomes guard-reachable through one additional `eval` arm.
//!
//! # The contract implementors must uphold
//!
//! `rho-pure-eval` is called from inside the RSpace matcher and from
//! casper replay, under the determinism guarantee stated in
//! [`crate`]'s module docs: *same input Par + same Env produce the same
//! output Par byte-for-byte on every node*. An injected oracle sits
//! underneath that guarantee, so every implementation MUST be:
//!
//! | Property | Meaning |
//! | --- | --- |
//! | **pure** | no observable side effects; no I/O, no clock, no RNG |
//! | **total** | returns for every `(target, pattern)` pair; never panics |
//! | **deterministic** | the verdict depends only on `(target, pattern)` — not on call order, not on any state carried in `&self`, not on the node it runs on |
//!
//! The canonical way to satisfy "deterministic" with a stateful matcher
//! is to construct the matcher state *inside* `matches` and drop it
//! before returning, so no residue can leak from one call into the next.
//! That is exactly what the production implementation in
//! `rholang::rust::interpreter::matcher::r#match::SpatialMatcherOracle`
//! does with `SpatialMatcherContext` (which mutates its own `free_map`).
//!
//! # Bindings are deliberately not returned
//!
//! `matches` answers a yes/no question. The spatial matcher can also
//! produce the substitution that witnesses the match, but a guard has no
//! way to consume one: a guard is evaluated *after* every bind has
//! already been resolved, and its verdict may only be "commit" or "do
//! not commit". Returning `bool` keeps the seam as narrow as the
//! question, and keeps `EMatches` free of any binder semantics.

use models::rhoapi::Par;

/// Decides whether a Rholang term matches a Rholang pattern.
///
/// Implementations must be pure, total and deterministic — see the
/// [module docs](self) for the full contract. `Sync` is required
/// because guards are evaluated concurrently from the RSpace matcher on
/// every node; an oracle that needed exclusive access to shared mutable
/// state could not honour the determinism contract anyway.
pub trait SpatialMatch: Sync {
    /// Returns `true` iff `target` matches `pattern`.
    ///
    /// `target` is a fully evaluated term; `pattern` is a pattern whose
    /// free variables are binders, and is passed through verbatim (it is
    /// never evaluated — evaluating it would try to resolve those free
    /// variables and fail).
    fn matches(&self, target: &Par, pattern: &Par) -> bool;

    /// Whether this oracle can answer [`matches`](Self::matches) at all.
    ///
    /// The one implementation that returns `false` is [`NoSpatialMatch`],
    /// the default used by [`crate::eval`]. When an oracle is
    /// unavailable, [`crate::eval_with`] rejects `EMatchesBody` with
    /// `EvalError::UnsupportedExpression { kind: "EMatchesBody" }`
    /// *without evaluating either operand* — byte-for-byte the behaviour
    /// this crate had before the seam existed.
    ///
    /// Real oracles never override this.
    fn is_available(&self) -> bool { true }
}

/// The absent oracle: the default for [`crate::eval`].
///
/// With `NoSpatialMatch` injected, `EMatchesBody` evaluation is refused
/// exactly as it was before the seam was introduced — same error, same
/// `kind` string, same short-circuit (neither operand is evaluated).
/// This is what makes the seam a purely *additive* change: every
/// pre-existing caller of [`crate::eval`] observes identical behaviour.
///
/// [`matches`](SpatialMatch::matches) is never consulted for this type
/// (`is_available` gates it), but it is still defined — totally, without
/// panicking — so the type cannot become a trap if some future caller
/// invokes it directly.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NoSpatialMatch;

impl SpatialMatch for NoSpatialMatch {
    /// Unreachable through [`crate::eval_with`], which checks
    /// [`is_available`](SpatialMatch::is_available) first. Defined as a
    /// total function returning `false` rather than a panic so that the
    /// determinism contract holds unconditionally.
    fn matches(&self, _target: &Par, _pattern: &Par) -> bool { false }

    fn is_available(&self) -> bool { false }
}
