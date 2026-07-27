//! # Substitution — the entry points
//!
//! The recursion itself lives in [`super::substitute_drive`] (Leg-2: an
//! explicit heap worklist, so native stack is `O(1)` in term nesting depth and
//! sibling width) and the per-arm split/rebuild table lives in
//! [`super::substitute_combine`] (single-sourced, so the driver and its
//! recursive oracle twin cannot disagree about an arm).
//!
//! This file keeps what a caller sees: the trait, the metered wrappers, and the
//! variable resolvers.
//!
//! See `rholang/src/main/scala/coop/rchain/rholang/interpreter/Substitute.scala`
//! for the reference semantics and
//! `docs/design/audits/theta-depth-traversals-2026-07-26.md` for the audit,
//! measured constants, and proof standard.
//!
//! ## ⚠ `Expr` no longer has a `substitute` (sorting) entry point
//!
//! `SubstituteTrait<Expr>::substitute` existed and was **dead** — no production
//! and no test caller — and it had diverged from its live twin: its
//! `EMinusBody` arm rebuilt the term as an `EPlusBody`. Keeping a dead method
//! that computes the wrong thing is a trap; "fixing" it silently would change a
//! protocol surface nobody asked to change. It is therefore removed, along with
//! the trait impl that forced it to exist, and `Expr`'s live capability
//! survives as the inherent [`Substitute::substitute_expr_no_sort`].
//!
//! That is a deliberate reduction of the protocol surface. It is also why the
//! conversion is a hand-written driver rather than one mode-flagged driver over
//! a shared arm table: a mode flag would have made the dead method's behaviour
//! a *configuration* of the live one, and quietly promoted it.

use models::rhoapi::{Bundle, Expr, If, Match, New, Par, Receive, Send, Var, VarRef};
use models::rust::rholang::sorter::if_sort_matcher::IfSortMatcher;
use models::rust::rholang::sorter::match_sort_matcher::MatchSortMatcher;
use models::rust::rholang::sorter::new_sort_matcher::NewSortMatcher;
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::receive_sort_matcher::ReceiveSortMatcher;
use models::rust::rholang::sorter::send_sort_matcher::SendSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;
use rspace_plus_plus::rspace::history::Either;

use super::accounting::costs::Cost;
use super::env::Env;
use super::errors::InterpreterError;
use super::metering::MeteredMachine;
use super::substitute_drive::{
    maybe_substitute_var_view, maybe_substitute_var_ref_view, EnvView, SubCtx, SubVal, SubWork,
};

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/Substitute.scala
pub trait SubstituteTrait<A> {
    fn substitute(&self, term: A, depth: i32, env: &Env<Par>) -> Result<A, InterpreterError>;

    fn substitute_no_sort(&self, term: A, depth: i32, env: &Env<Par>)
        -> Result<A, InterpreterError>;
}

#[derive(Clone)]
pub struct Substitute {
    pub metering: MeteredMachine,
}

impl Substitute {
    pub fn substitute_and_charge<A>(
        &self,
        term: &A,
        depth: i32,
        env: &Env<Par>,
    ) -> Result<A, InterpreterError>
    where
        Self: SubstituteTrait<A>,
        A: Clone + prost::Message,
    {
        // scala 'charge' function built in here
        match self.substitute(term.clone(), depth, env) {
            Ok(subst_term) => {
                self.metering.reserve_substitution(Cost::create(
                    (subst_term.encoded_len() as i64).max(1),
                    "substitution",
                ))?;
                Ok(subst_term)
            }
            Err(th) => {
                self.metering.reserve_substitution(Cost::create(
                    (term.encoded_len() as i64).max(1),
                    "substitution",
                ))?;
                Err(th)
            }
        }
    }

    pub fn substitute_no_sort_and_charge<A>(
        &self,
        term: &A,
        depth: i32,
        env: &Env<Par>,
    ) -> Result<A, InterpreterError>
    where
        Self: SubstituteTrait<A>,
        A: Clone + prost::Message,
    {
        // scala 'charge' function built in here
        match self.substitute_no_sort(term.clone(), depth, env) {
            Ok(subst_term) => {
                self.metering.reserve_substitution(Cost::create(
                    (subst_term.encoded_len() as i64).max(1),
                    "substitution",
                ))?;
                Ok(subst_term)
            }
            Err(th) => {
                self.metering.reserve_substitution(Cost::create(
                    (term.encoded_len() as i64).max(1),
                    "substitution",
                ))?;
                Err(th)
            }
        }
    }

    // pub here for testing purposes
    pub fn maybe_substitute_var(
        &self,
        term: Var,
        depth: i32,
        env: &Env<Par>,
    ) -> Result<Either<Var, Par>, InterpreterError> {
        maybe_substitute_var_view(term, SubCtx::root(depth), EnvView::new(env))
    }

    /// `VarRef` resolution, exposed for the same reason as
    /// [`Substitute::maybe_substitute_var`].
    pub fn maybe_substitute_var_ref(
        &self,
        term: VarRef,
        depth: i32,
        env: &Env<Par>,
    ) -> Result<Either<VarRef, Par>, InterpreterError> {
        maybe_substitute_var_ref_view(term, SubCtx::root(depth), EnvView::new(env))
    }

    /// Substitute one `Expr` without sorting.
    ///
    /// Inherent rather than a `SubstituteTrait<Expr>` method: see the ⚠ note in
    /// this module's documentation. `Expr` has a `substitute_no_sort` and
    /// deliberately has no sorting counterpart.
    pub fn substitute_expr_no_sort(
        &self,
        term: Expr,
        depth: i32,
        env: &Env<Par>,
    ) -> Result<Expr, InterpreterError> {
        match self.sub_drive(SubWork::Expr(term, SubCtx::root(depth)), env)? {
            SubVal::Expr(e) => Ok(e),
            _ => unreachable!("substitute_expr_no_sort: the drive produced a non-Expr"),
        }
    }
}

// ===========================================================================
// The eight entry points — thin wrappers over the driver.
//
// `substitute_no_sort` starts a bounded drive. `substitute` is
// `substitute_no_sort` followed by exactly ONE `sort_match` of the result,
// which is what the recursive form did: the interior recursion always went
// through `substitute_no_sort`, so no sorter ran below the top level.
// ===========================================================================

macro_rules! substitute_entry {
    ($ty:ty, $work:ident, $val:ident, $sorter:ty, $name:literal) => {
        impl SubstituteTrait<$ty> for Substitute {
            fn substitute_no_sort(
                &self,
                term: $ty,
                depth: i32,
                env: &Env<Par>,
            ) -> Result<$ty, InterpreterError> {
                match self.sub_drive(SubWork::$work(term, SubCtx::root(depth)), env)? {
                    SubVal::$val(v) => Ok(v),
                    _ => unreachable!(concat!(
                        $name,
                        "::substitute_no_sort: the drive produced the wrong value type"
                    )),
                }
            }

            fn substitute(
                &self,
                term: $ty,
                depth: i32,
                env: &Env<Par>,
            ) -> Result<$ty, InterpreterError> {
                self.substitute_no_sort(term, depth, env)
                    .map(|t| <$sorter as Sortable<$ty>>::sort_match(&t))
                    .map(|st| st.term)
            }
        }
    };
}

impl SubstituteTrait<Par> for Substitute {
    fn substitute_no_sort(
        &self,
        term: Par,
        depth: i32,
        env: &Env<Par>,
    ) -> Result<Par, InterpreterError> {
        // `SubWork::Par` carries an `Option` so that a missing required child
        // raises `unwrap_option_safe`'s error in position; the root is always
        // present.
        match self.sub_drive(SubWork::Par(Some(term), SubCtx::root(depth)), env)? {
            SubVal::Par(p) => Ok(p),
            _ => unreachable!("Par::substitute_no_sort: the drive produced a non-Par"),
        }
    }

    fn substitute(&self, term: Par, depth: i32, env: &Env<Par>) -> Result<Par, InterpreterError> {
        self.substitute_no_sort(term, depth, env)
            .map(|p| ParSortMatcher::sort_match(&p))
            .map(|st| st.term)
    }
}

substitute_entry!(Send, Send, Send, SendSortMatcher, "Send");
substitute_entry!(Receive, Receive, Receive, ReceiveSortMatcher, "Receive");
substitute_entry!(New, New, New, NewSortMatcher, "New");
substitute_entry!(Match, Match, Match, MatchSortMatcher, "Match");
substitute_entry!(If, If, If, IfSortMatcher, "If");

impl SubstituteTrait<Bundle> for Substitute {
    fn substitute_no_sort(
        &self,
        term: Bundle,
        depth: i32,
        env: &Env<Par>,
    ) -> Result<Bundle, InterpreterError> {
        match self.sub_drive(SubWork::Bundle(term, SubCtx::root(depth)), env)? {
            SubVal::Bundle(b) => Ok(b),
            _ => unreachable!("Bundle::substitute_no_sort: the drive produced a non-Bundle"),
        }
    }

    /// ⚠ `Bundle` is the one entry point whose sorted form is **not**
    /// "no-sort, then sort the result". The recursive form substituted the
    /// bundle's body via `SubstituteTrait<Par>::substitute` — i.e. it sorted
    /// the BODY (once, at the body's top level) and then ran the inner-bundle
    /// merge on the sorted body. Merging is not sort-commuting, so the order
    /// matters and is reproduced here exactly.
    fn substitute(
        &self,
        mut term: Bundle,
        depth: i32,
        env: &Env<Par>,
    ) -> Result<Bundle, InterpreterError> {
        let body = term.body.take();
        let body = super::unwrap_option_safe(body)?;
        let sub_bundle = <Self as SubstituteTrait<Par>>::substitute(self, body, depth, env)?;
        Ok(super::substitute_combine::rebuild_bundle(term, sub_bundle))
    }
}
