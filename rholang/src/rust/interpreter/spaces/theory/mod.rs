//! The theory seam of the native spaces (P7 design v3, W5: TH-1 to TH-10).
//!
//! A space may name a theory in its factory call, `free Th()`. The factory persists the
//! [identity](identity::TheoryIdentity) of the theory in the config of the space (TH-2, TH-4),
//! never a handle: the identity holds content only. Each config read resolves the identity to a
//! checker of this build (TH-3, TH-7). The checker then gives a [verdict](TheoryVerdict) on each
//! value of a send before any staging (TH-5) and on each stored datum that a receive matches,
//! after the `where` guard (TH-6). A verdict other than `Proven` fails closed: on a send it
//! aborts the deploy, and on a match it is a guard refusal.
//!
//! The shapes follow the neutral theorem channel of F1r3lang (`grammar-core/src/theorem.rs`,
//! `AdmissionDecision` with `Proven`, `Refuted` and `Undetermined`, and the structural work unit
//! `STRUCTURAL_ADMISSION_WORK_UNITS = 1`), so the post-merge seam (P12) can map an installed
//! theory onto this trait.
//!
//! This build resolves only the structural builtins `Nat`, `Int`, `String`, `Bool` and `Any`
//! ([`builtin`]). Each check of a builtin reports one work unit, the same on a hit and a miss and
//! in play and replay (TH-8). Admission adds no phlo: the introduction bytes of the send already
//! pay for the value.

pub mod builtin;
pub mod identity;

use std::sync::Arc;

use models::rhoapi::{ListParWithRandom, Par};

use self::builtin::BuiltinTheory;
use self::identity::TheoryIdentity;
use crate::rust::interpreter::spaces::errors::SpaceError;

/// The logical work and the evidence of one check, or the limits of a checker (the limit
/// profile of an identity). F1r3lang calls them `AdmissionUsage` and `AdmissionBudget`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TheoryUsage {
    /// The units of logical work.
    pub work_units: u64,
    /// The bytes of evidence.
    pub evidence_bytes: u64,
}

impl TheoryUsage {
    /// The usage of `work_units` units of work and `evidence_bytes` bytes of evidence.
    pub const fn new(work_units: u64, evidence_bytes: u64) -> Self {
        Self {
            work_units,
            evidence_bytes,
        }
    }
}

/// The reason of a verdict that is not `Proven`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TheoryReason {
    /// The value is outside the theory.
    OutsideTheory,
    /// The check needs more units of work than the limit profile gives.
    WorkBudgetExhausted { required: u64, available: u64 },
}

/// The verdict of one check (F1r3lang `AdmissionDecision`). Only `Proven` admits: `Refuted` and
/// `Undetermined` both fail closed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TheoryVerdict {
    /// The value is in the theory.
    Proven(TheoryUsage),
    /// The value is not in the theory.
    Refuted {
        reason: TheoryReason,
        usage: TheoryUsage,
    },
    /// The checker cannot decide within its limits.
    Undetermined {
        reason: TheoryReason,
        usage: TheoryUsage,
    },
}

impl TheoryVerdict {
    /// The usage that the verdict reports.
    pub const fn usage(&self) -> TheoryUsage {
        match self {
            TheoryVerdict::Proven(usage)
            | TheoryVerdict::Refuted { usage, .. }
            | TheoryVerdict::Undetermined { usage, .. } => *usage,
        }
    }

    /// True for `Proven`.
    pub const fn is_proven(&self) -> bool { matches!(self, TheoryVerdict::Proven(_)) }
}

/// A lease that a checker attaches to a `Proven` verdict, which the commit of the transaction
/// must revalidate under the authority of its language (TH-9). The acquire, the revalidation and
/// the release belong to the guard composition of P12. Until then, a transaction with two or
/// more leases aborts with `NestedCommitGuard` before its commit. The builtins give no lease.
pub trait TheoryCommitLease: Send + Sync + std::fmt::Debug {}

/// A checker of a theory (TH-1, TH-6, TH-9). Each method is a deterministic function of its
/// inputs and the limit profile of the identity.
pub trait SpaceTheory: Send + Sync + std::fmt::Debug {
    /// The identity that the config of the space persists.
    fn identity(&self) -> &TheoryIdentity;

    /// The admission of one value of a send (TH-5).
    fn admit(&self, value: &Par) -> Result<TheoryVerdict, SpaceError>;

    /// The check of the stored datum `stored`, whose payload a receive matched with the
    /// captures `captures` (TH-6).
    fn check_match(
        &self,
        stored: &ListParWithRandom,
        captures: &ListParWithRandom,
    ) -> Result<TheoryVerdict, SpaceError>;

    /// The lease of a `Proven` verdict, or `None` (TH-9).
    fn commit_lease(&self) -> Option<Box<dyn TheoryCommitLease>> { None }
}

/// The checker of `identity` in this build (TH-3): a structural builtin, or under `cfg(test)`
/// one of the test checkers of [`test_theories`]. Every other identity names no checker of this
/// build and fails with `TheoryUnavailable`. The resolution is a pure function of the identity.
pub fn resolve(identity: &TheoryIdentity) -> Result<Arc<dyn SpaceTheory>, SpaceError> {
    if let Some(builtin) = BuiltinTheory::of_identity(identity) {
        return Ok(Arc::new(builtin));
    }
    #[cfg(test)]
    if let Some(theory) = test_theories::resolve(identity) {
        return Ok(theory);
    }
    Err(SpaceError::TheoryUnavailable(identity.theorem_id.clone()))
}

/// The name of the theory of `theory` in the errors of a check: its theorem id.
pub fn theory_name(theory: &dyn SpaceTheory) -> String { theory.identity().theorem_id.clone() }

/// TH-5: the admission of the values `values` of a send, in order. The first value that is not
/// `Proven` aborts with `TheoryRefuted` or `TheoryUndetermined`, which name the theory and the
/// index of the value. The leases of the `Proven` verdicts are returned in order (TH-9).
pub fn admit_values(
    theory: &dyn SpaceTheory,
    values: &[Par],
) -> Result<Vec<Box<dyn TheoryCommitLease>>, SpaceError> {
    let mut leases = Vec::new();
    for (index, value) in values.iter().enumerate() {
        match theory.admit(value)? {
            TheoryVerdict::Proven(_) => leases.extend(theory.commit_lease()),
            TheoryVerdict::Refuted { .. } => {
                return Err(SpaceError::TheoryRefuted {
                    theory: theory_name(theory),
                    index,
                })
            }
            TheoryVerdict::Undetermined { .. } => {
                return Err(SpaceError::TheoryUndetermined {
                    theory: theory_name(theory),
                    index,
                })
            }
        }
    }
    Ok(leases)
}

/// Test checkers (TH-10): a checker with a limit profile of zero work units, a checker that
/// refutes every stored datum at a match, and a checker that gives a lease with each `Proven`
/// verdict. Only test builds resolve them, so their behavior is checked by library tests.
#[cfg(test)]
pub mod test_theories {
    use std::sync::Arc;

    use models::rhoapi::{ListParWithRandom, Par};

    use super::identity::TheoryIdentity;
    use super::{SpaceTheory, TheoryCommitLease, TheoryReason, TheoryUsage, TheoryVerdict};
    use crate::rust::interpreter::spaces::errors::SpaceError;

    /// The checker ABI of the test checkers.
    pub const TEST_ABI: &str = "rrs:test:v1";

    /// The behavior of a test checker.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum TestBehavior {
        /// Admits every value and every match, and gives a lease with each verdict.
        Leasing,
        /// Admits every value, and refutes every stored datum at a match.
        RefuteMatch,
        /// Refutes every value.
        RefuteValue,
    }

    impl TestBehavior {
        /// The theorem id of the behavior.
        pub const fn theorem_id(self) -> &'static str {
            match self {
                TestBehavior::Leasing => "TestLeasing",
                TestBehavior::RefuteMatch => "TestRefuteMatch",
                TestBehavior::RefuteValue => "TestRefuteValue",
            }
        }

        fn of_theorem_id(id: &str) -> Option<Self> {
            [
                TestBehavior::Leasing,
                TestBehavior::RefuteMatch,
                TestBehavior::RefuteValue,
            ]
            .into_iter()
            .find(|behavior| behavior.theorem_id() == id)
        }
    }

    /// The identity of the test checker `behavior`.
    pub fn identity(behavior: TestBehavior) -> TheoryIdentity {
        TheoryIdentity {
            checker_abi: TEST_ABI.to_string(),
            ..TheoryIdentity::builtin(behavior.theorem_id())
        }
    }

    /// The lease of the leasing checker.
    #[derive(Debug)]
    pub struct TestLease;

    impl TheoryCommitLease for TestLease {}

    /// A test checker.
    #[derive(Debug)]
    pub struct TestTheory {
        identity: TheoryIdentity,
        behavior: TestBehavior,
    }

    impl SpaceTheory for TestTheory {
        fn identity(&self) -> &TheoryIdentity { &self.identity }

        fn admit(&self, _value: &Par) -> Result<TheoryVerdict, SpaceError> {
            let usage = TheoryUsage::new(1, 0);
            Ok(match self.behavior {
                TestBehavior::Leasing | TestBehavior::RefuteMatch => TheoryVerdict::Proven(usage),
                TestBehavior::RefuteValue => TheoryVerdict::Refuted {
                    reason: TheoryReason::OutsideTheory,
                    usage,
                },
            })
        }

        fn check_match(
            &self,
            _stored: &ListParWithRandom,
            _captures: &ListParWithRandom,
        ) -> Result<TheoryVerdict, SpaceError> {
            let usage = TheoryUsage::new(1, 0);
            Ok(match self.behavior {
                TestBehavior::Leasing | TestBehavior::RefuteValue => TheoryVerdict::Proven(usage),
                TestBehavior::RefuteMatch => TheoryVerdict::Refuted {
                    reason: TheoryReason::OutsideTheory,
                    usage,
                },
            })
        }

        fn commit_lease(&self) -> Option<Box<dyn TheoryCommitLease>> {
            match self.behavior {
                TestBehavior::Leasing => Some(Box::new(TestLease)),
                TestBehavior::RefuteMatch | TestBehavior::RefuteValue => None,
            }
        }
    }

    /// The test checker of `identity`, when its ABI is [`TEST_ABI`].
    pub fn resolve(identity: &TheoryIdentity) -> Option<Arc<dyn SpaceTheory>> {
        match identity.checker_abi == TEST_ABI {
            true => TestBehavior::of_theorem_id(&identity.theorem_id).map(|behavior| {
                Arc::new(TestTheory {
                    identity: identity.clone(),
                    behavior,
                }) as Arc<dyn SpaceTheory>
            }),
            false => None,
        }
    }
}

#[cfg(test)]
mod tests;
