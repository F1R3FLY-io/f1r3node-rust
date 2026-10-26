// Wave 4 cost-coupling stub.
//
// # Why this exists
//
// The Wave 2 plan calls for a `NoopMetering` stub that lets
// Wave 4 handlers be registered + dispatched with
// `reserve_primitive` / `reserve_incremental_primitive` hooks
// in place BEFORE cost-accounted-rho's full `MeteredMachine`
// lands on dev (Wave 6).  The stub matches the method
// signatures the handlers will call, so Wave 6 can swap the
// implementation without touching handler code.
//
// # Trait-based abstraction (vs. concrete-type-swap)
//
// Handlers dispatch through [`Metering`] (a trait) rather than
// a concrete type.  Rationale:
//
//   1. **Wave 6 swap cost**: a concrete-type swap would require
//      touching every handler call site when `MeteredMachine`
//      lands.  Trait-based dispatch lets the swap happen at
//      construction time only (hand handlers a different
//      implementation).
//   2. **Test substitution**: handler tests on Wave 4 use
//      [`NoopMetering`]; cost-boundary tests (yet-to-land) can
//      use a `TrackingMetering` test double that records every
//      reserve call without charging.
//   3. **Dyn-compatibility**: both methods take `&self` + a
//      `Cost` by value + return `Result<(), InterpreterError>`.
//      Object-safe; handlers can hold `&dyn Metering` or
//      `Arc<dyn Metering>` as needed.
//
// # Hard-fork surface (consensus-observable)
//
// Under Wave 4 + 5, every `reserve_primitive` call succeeds
// (NoopMetering) → zero-cost deploys always complete.  Wave 6
// swaps to [`MeteredMachine`](super::...) and phlo-exhausting
// workloads flip from success to failure.  This is one of the
// six documented Wave 6 hard-fork faces (see the Wave 2 plan
// doc's "Explicit Hard-Fork Surface" section):
//
//   - Deploy outcomes for phlo-exhausting workloads flip
//     success → failure
//   - Handler dispatch pre-charge behavior lights up →
//     different failure / side-effect ordering on the
//     boundary case
//
// # No feature flag
//
// Per the plan: NoopMetering is dispatched LIVE under Wave 4
// (not behind `#[cfg(feature = "metering")]`).  Wave 6 commits
// to the hard fork at the construction site, not at a build
// gate.

use super::costs::Cost;
use crate::rust::interpreter::errors::InterpreterError;

/// The hook surface Wave 4 handlers dispatch through for cost
/// coupling.  Both methods are object-safe so handlers can
/// hold `&dyn Metering` / `Arc<dyn Metering>` for runtime
/// substitution between [`NoopMetering`] (Wave 4) and
/// `MeteredMachine` (Wave 6).
pub trait Metering: Send + Sync {
    /// Reserve a primitive-weight cost against the deploy's
    /// budget.  Charged BEFORE the primitive runs so a
    /// phlo-exhausting deploy fails before the side effect.
    ///
    /// # Zero/negative-weight behavior
    ///
    /// Production implementations (e.g. the Wave 6
    /// `MeteredMachine`) SHOULD reject zero- or negative-value
    /// `Cost` with [`InterpreterError::BugFoundError`].
    /// [`NoopMetering`] deliberately does NOT (stub laxness —
    /// see the type's docstring); the Wave 6 real
    /// implementation lights up the enforcement.
    ///
    /// # Returns
    ///
    ///   - `Ok(())` if the reservation succeeds (budget had
    ///     enough phlogiston, cost was positive).
    ///   - [`InterpreterError::BugFoundError`] on malformed
    ///     cost (zero/negative weight; implementations may
    ///     also surface other invariant violations here).
    ///   - Other variants: future real implementations may
    ///     surface phlo-exhaustion through a different error
    ///     variant — handler callers MUST propagate the error,
    ///     not match on the specific variant.
    fn reserve_primitive(&self, amount: Cost) -> Result<(), InterpreterError>;

    /// Reserve an INCREMENTAL primitive-weight cost — same as
    /// [`reserve_primitive`](Self::reserve_primitive) but
    /// permits a zero-value `Cost` as a no-op.  Used by
    /// handlers that scale cost on a length parameter: a
    /// zero-length write charges nothing but still dispatches.
    ///
    /// Production implementations SHOULD surface negative
    /// values as [`InterpreterError::BugFoundError`] (negative
    /// incremental cost indicates a weight-function bug, not a
    /// legitimate deploy input).  [`NoopMetering`] deliberately
    /// does NOT (stub laxness — see the type's docstring).
    fn reserve_incremental_primitive(&self, amount: Cost) -> Result<(), InterpreterError>;
}

/// Wave 4 stub: every reserve call returns `Ok(())` immediately
/// with no state mutation.  Handlers dispatch through this
/// transparently; the pre-charge call succeeds unconditionally.
///
/// # Why no state at all
///
/// `MeteredMachine` tracks a per-deploy budget + per-charge
/// attempt log for consensus reconciliation.  NoopMetering
/// deliberately holds ZERO state — no budget, no log, no mutex
/// — because any state here would be dead weight under Wave 4
/// dispatch AND would create a false consensus surface that
/// future readers might misinterpret.  Fresh construction is
/// cheap: `NoopMetering` is a zero-sized type.
///
/// # Hard-fork face
///
/// This type's existence IS a Wave 6 fork marker: a validator
/// running Wave 4 code with `NoopMetering` wired in has
/// different observable behavior from one running Wave 6 code
/// with `MeteredMachine` wired in (same deploy outcomes for
/// zero-phlo workloads; divergent for phlo-exhausting
/// workloads).  Flagged in the module docstring.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopMetering;

impl NoopMetering {
    /// Fresh stub.  Equivalent to [`NoopMetering::default`];
    /// kept as a named constructor for grep-ability at Wave 6
    /// swap sites.
    pub const fn new() -> Self { Self }
}

impl Metering for NoopMetering {
    #[inline]
    fn reserve_primitive(&self, _amount: Cost) -> Result<(), InterpreterError> { Ok(()) }

    #[inline]
    fn reserve_incremental_primitive(&self, _amount: Cost) -> Result<(), InterpreterError> {
        Ok(())
    }
}

// Compile-time witness that `NoopMetering: Send + Sync + dyn-safe`
// — required by Wave 4's dispatch pattern (handlers hold
// `Arc<dyn Metering>` or `&dyn Metering` across async
// boundaries).  Catches a future regression that broke
// dyn-compat (e.g., adding a method with a generic parameter
// to the trait).
const _NOOP_METERING_IS_DYN_SAFE: fn() = || {
    fn assert_dyn_safe(_: &dyn Metering) {}
    assert_dyn_safe(&NoopMetering);
};

const _NOOP_METERING_IS_SEND_SYNC: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<NoopMetering>();
};

#[cfg(test)]
mod tests {
    use std::borrow::Cow;
    use std::sync::Arc;

    use super::*;

    fn cost(value: i64, op: &'static str) -> Cost {
        Cost {
            value,
            operation: Cow::Borrowed(op),
        }
    }

    /// `reserve_primitive` returns `Ok(())` for any positive
    /// value.
    #[test]
    fn noop_reserve_primitive_accepts_positive() {
        let m = NoopMetering::new();
        assert!(m.reserve_primitive(cost(1, "op")).is_ok());
        assert!(m.reserve_primitive(cost(1_000_000, "op")).is_ok());
        assert!(m.reserve_primitive(cost(i64::MAX, "op")).is_ok());
    }

    /// LOAD-BEARING Wave-4 contract: NoopMetering accepts even
    /// zero/negative values.  The real [`MeteredMachine`]
    /// (yet-to-land, Wave 6) rejects these as BugFoundError —
    /// so a handler whose weight function returns a bad value
    /// goes undetected under Wave 4 but surfaces loudly under
    /// Wave 6.  Pin against a future refactor that tightened
    /// the stub (which would turn a Wave-6-only bug into a
    /// Wave-4 regression).
    #[test]
    fn noop_reserve_primitive_accepts_zero_and_negative() {
        let m = NoopMetering::new();
        assert!(m.reserve_primitive(cost(0, "op")).is_ok());
        assert!(m.reserve_primitive(cost(-1, "op")).is_ok());
        assert!(m.reserve_primitive(cost(i64::MIN, "op")).is_ok());
    }

    /// `reserve_incremental_primitive` has the same stub
    /// behavior.  Real Wave-6 impl permits zero (unlike
    /// `reserve_primitive`) but rejects negative.
    #[test]
    fn noop_reserve_incremental_primitive_accepts_anything() {
        let m = NoopMetering::new();
        assert!(m.reserve_incremental_primitive(cost(0, "op")).is_ok());
        assert!(m.reserve_incremental_primitive(cost(1, "op")).is_ok());
        assert!(m.reserve_incremental_primitive(cost(-1, "op")).is_ok());
    }

    /// Repeat calls don't accumulate state — the stub holds
    /// zero state by design.
    #[test]
    fn noop_many_calls_stay_ok() {
        let m = NoopMetering::new();
        for i in 0..10_000 {
            assert!(m.reserve_primitive(cost(i, "op")).is_ok());
            assert!(m.reserve_incremental_primitive(cost(i, "op")).is_ok());
        }
    }

    /// `Default` + `new` produce equivalent instances.
    #[test]
    fn noop_default_and_new_both_work() {
        let a: NoopMetering = Default::default();
        let b = NoopMetering::new();
        assert!(a.reserve_primitive(cost(1, "op")).is_ok());
        assert!(b.reserve_primitive(cost(1, "op")).is_ok());
    }

    /// Dyn-compatibility: handlers hold `&dyn Metering` for
    /// Wave 6 swap flexibility.
    #[test]
    fn noop_dispatches_through_dyn_metering() {
        let m: Arc<dyn Metering> = Arc::new(NoopMetering::new());
        assert!(m.reserve_primitive(cost(42, "op")).is_ok());
        assert!(m.reserve_incremental_primitive(cost(42, "op")).is_ok());
    }

    /// Zero-sized type pin: `NoopMetering` holds no state,
    /// so an `Arc<dyn Metering>` wrapping it has exactly the
    /// pointer-pair overhead (vtable + data) with zero data.
    #[test]
    fn noop_is_zero_sized() {
        use std::mem::size_of;
        assert_eq!(size_of::<NoopMetering>(), 0);
    }
}
