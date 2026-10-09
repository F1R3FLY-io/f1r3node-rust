//! The structural builtin theories (P7 design v3, TH-1 and TH-3).
//!
//! A builtin decides a value by its shape alone. `sole(v)` holds when `v` has exactly one
//! expression, every other field of `v` is empty, and `v` uses no connective:
//!
//! | Theory | A value is in the theory when |
//! |---|---|
//! | `Nat` | `sole(v)` and the expression is a `GInt` of at least 0 or a non-negative `GBigInt` |
//! | `Int` | `sole(v)` and the expression is a `GInt` or a `GBigInt` |
//! | `String` | `sole(v)` and the expression is a `GString` |
//! | `Bool` | `sole(v)` and the expression is a `GBool` |
//! | `Any` | always |
//!
//! So the string `"Nat(42)"` is not in `Nat`: the legacy prefix check on the text of a value is
//! gone. Each check reserves its one unit of work as one `VerificationOperations` unit before it
//! reads the value, and it reports the usage `{work_units: 1, evidence_bytes: 0}`. A limit
//! profile of zero units gives `Undetermined` before any reservation or read. At a match, a
//! builtin admits each value of the stored payload again, for one unit per stored datum: a
//! payload that was admitted when it was sent is always `Proven`.

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{ListParWithRandom, Par};

use super::identity::{TheoryIdentity, BUILTIN_CATEGORY, BUILTIN_LANGUAGE, STRUCTURAL_ABI};
use super::{SpaceTheory, TheoryReason, TheoryUsage, TheoryVerdict};
use crate::rust::interpreter::reified::costs::ScanMeter;
use crate::rust::interpreter::spaces::errors::SpaceError;

/// The names of the builtin theories, in bytewise order.
pub const BUILTIN_NAMES: [&str; 5] = ["Any", "Bool", "Int", "Nat", "String"];

/// The work of one structural check (F1r3lang `STRUCTURAL_ADMISSION_WORK_UNITS`).
pub const STRUCTURAL_WORK_UNITS: u64 = 1;

/// A structural builtin theory.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BuiltinKind {
    Nat,
    Int,
    String,
    Bool,
    Any,
}

impl BuiltinKind {
    /// The builtin named `name`.
    pub fn of_name(name: &str) -> Option<Self> {
        match name {
            "Nat" => Some(BuiltinKind::Nat),
            "Int" => Some(BuiltinKind::Int),
            "String" => Some(BuiltinKind::String),
            "Bool" => Some(BuiltinKind::Bool),
            "Any" => Some(BuiltinKind::Any),
            _ => None,
        }
    }

    /// True when `value` is in the theory.
    pub fn contains(self, value: &Par) -> bool {
        match self {
            BuiltinKind::Any => true,
            kind => match sole(value) {
                Some(expr) => kind.admits_expr(expr),
                None => false,
            },
        }
    }

    fn admits_expr(self, expr: &ExprInstance) -> bool {
        match (self, expr) {
            (BuiltinKind::Nat, ExprInstance::GInt(value)) => *value >= 0,
            (BuiltinKind::Nat, ExprInstance::GBigInt(bytes)) => non_negative(bytes),
            (BuiltinKind::Int, ExprInstance::GInt(_) | ExprInstance::GBigInt(_)) => true,
            (BuiltinKind::String, ExprInstance::GString(_)) => true,
            (BuiltinKind::Bool, ExprInstance::GBool(_)) => true,
            (BuiltinKind::Any, _) => true,
            (BuiltinKind::Nat | BuiltinKind::Int | BuiltinKind::String | BuiltinKind::Bool, _) => {
                false
            }
        }
    }
}

/// The one expression of `value` when `value` has exactly one expression, every other field is
/// empty and it uses no connective.
pub fn sole(value: &Par) -> Option<&ExprInstance> {
    let Par {
        sends,
        receives,
        news,
        exprs,
        matches,
        unforgeables,
        bundles,
        connectives,
        conditionals,
        locally_free,
        connective_used,
        cost_signed_terms,
        cost_stacks,
    } = value;
    let others_empty = sends.is_empty()
        && receives.is_empty()
        && news.is_empty()
        && matches.is_empty()
        && unforgeables.is_empty()
        && bundles.is_empty()
        && connectives.is_empty()
        && conditionals.is_empty()
        && locally_free.is_empty()
        && cost_signed_terms.is_empty()
        && cost_stacks.is_empty()
        && !connective_used;
    match (others_empty, exprs.as_slice()) {
        (true, [expr]) => expr.expr_instance.as_ref(),
        _ => None,
    }
}

/// True when the two's complement big-endian `bytes` encode a number of at least 0. The empty
/// encoding is 0.
fn non_negative(bytes: &[u8]) -> bool {
    match bytes.first() {
        Some(first) => first & 0x80 == 0,
        None => true,
    }
}

/// A structural builtin theory with its identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuiltinTheory {
    kind: BuiltinKind,
    identity: TheoryIdentity,
}

impl BuiltinTheory {
    /// The builtin of `identity` (TH-3): the structural ABI and the builtin language and
    /// category, a builtin theorem id, no args, and any limit profile.
    pub fn of_identity(identity: &TheoryIdentity) -> Option<Self> {
        let structural = identity.version == super::identity::IDENTITY_VERSION
            && identity.language_fingerprint == BUILTIN_LANGUAGE
            && identity.category == BUILTIN_CATEGORY
            && identity.category_name == BUILTIN_CATEGORY
            && identity.checker_abi == STRUCTURAL_ABI
            && identity.args.is_empty();
        match structural {
            true => BuiltinKind::of_name(&identity.theorem_id).map(|kind| BuiltinTheory {
                kind,
                identity: identity.clone(),
            }),
            false => None,
        }
    }

    /// The kind of the builtin.
    pub const fn kind(&self) -> BuiltinKind { self.kind }

    /// One structural check of `value`, with the work reserved first.
    fn check(&self, value: &Par, meter: &ScanMeter) -> Result<TheoryVerdict, SpaceError> {
        let usage = TheoryUsage::new(STRUCTURAL_WORK_UNITS, 0);
        let available = self.identity.limit_profile.work_units;
        if available < STRUCTURAL_WORK_UNITS {
            return Ok(TheoryVerdict::Undetermined {
                reason: TheoryReason::WorkBudgetExhausted {
                    required: STRUCTURAL_WORK_UNITS,
                    available,
                },
                usage,
            });
        }
        meter.charge(1, 0, 0)?;
        Ok(match self.kind.contains(value) {
            true => TheoryVerdict::Proven(usage),
            false => TheoryVerdict::Refuted {
                reason: TheoryReason::OutsideTheory,
                usage,
            },
        })
    }
}

impl SpaceTheory for BuiltinTheory {
    fn identity(&self) -> &TheoryIdentity { &self.identity }

    fn admit(&self, value: &Par, meter: &ScanMeter) -> Result<TheoryVerdict, SpaceError> {
        self.check(value, meter)
    }

    /// The stored payload again, value by value, for one unit of work: a payload that was
    /// admitted is always `Proven`. The captures are part of the payload, so they need no
    /// check of their own.
    fn check_match(
        &self,
        stored: &ListParWithRandom,
        _captures: &ListParWithRandom,
        meter: &ScanMeter,
    ) -> Result<TheoryVerdict, SpaceError> {
        let usage = TheoryUsage::new(STRUCTURAL_WORK_UNITS, 0);
        let available = self.identity.limit_profile.work_units;
        if available < STRUCTURAL_WORK_UNITS {
            return Ok(TheoryVerdict::Undetermined {
                reason: TheoryReason::WorkBudgetExhausted {
                    required: STRUCTURAL_WORK_UNITS,
                    available,
                },
                usage,
            });
        }
        meter.charge(1, 0, 0)?;
        Ok(
            match stored.pars.iter().all(|value| self.kind.contains(value)) {
                true => TheoryVerdict::Proven(usage),
                false => TheoryVerdict::Refuted {
                    reason: TheoryReason::OutsideTheory,
                    usage,
                },
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use models::rhoapi::{Bundle, Expr, GUnforgeable};
    use models::rust::host_work::{
        HostWorkDimension, HostWorkLimit, HostWorkLimits, HostWorkUsage,
    };
    use models::rust::utils::{new_gbool_par, new_gint_par, new_gstring_par};

    use super::*;
    use crate::rust::interpreter::host_work::HostWorkBudget;
    use crate::rust::interpreter::spaces::record::expr_par;

    fn metered() -> (HostWorkBudget, ScanMeter) {
        let budget = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(1_000)));
        (budget.clone(), ScanMeter::new(Some(budget)))
    }

    fn theory(name: &str) -> BuiltinTheory {
        BuiltinTheory::of_identity(&TheoryIdentity::builtin(name)).expect("a builtin")
    }

    fn int(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

    fn big(bytes: Vec<u8>) -> Par { expr_par(ExprInstance::GBigInt(bytes)) }

    fn string(value: &str) -> Par { new_gstring_par(value.to_string(), Vec::new(), false) }

    #[test]
    fn theory_builtins_decide_by_shape() {
        let flagged = Par {
            connective_used: true,
            ..int(1)
        };
        let two = Par {
            exprs: vec![
                Expr {
                    expr_instance: Some(ExprInstance::GInt(1)),
                },
                Expr {
                    expr_instance: Some(ExprInstance::GInt(2)),
                },
            ],
            ..Par::default()
        };
        let bundled = Par {
            bundles: vec![Bundle {
                body: Some(int(1)),
                write_flag: true,
                read_flag: false,
            }],
            ..int(1)
        };
        let private = Par {
            unforgeables: vec![GUnforgeable::default()],
            ..Par::default()
        };
        let cases: Vec<(Par, [bool; 5])> = vec![
            // Nat, Int, String, Bool, Any
            (int(0), [true, true, false, false, true]),
            (int(42), [true, true, false, false, true]),
            (int(-1), [false, true, false, false, true]),
            (big(vec![0x01, 0x00]), [true, true, false, false, true]),
            (big(Vec::new()), [true, true, false, false, true]),
            (big(vec![0xff]), [false, true, false, false, true]),
            (string("Nat(42)"), [false, false, true, false, true]),
            (new_gbool_par(true, Vec::new(), false), [
                false, false, false, true, true,
            ]),
            (Par::default(), [false, false, false, false, true]),
            (flagged, [false, false, false, false, true]),
            (two, [false, false, false, false, true]),
            (bundled, [false, false, false, false, true]),
            (private, [false, false, false, false, true]),
        ];
        for (value, expected) in cases {
            for (name, admitted) in ["Nat", "Int", "String", "Bool", "Any"].iter().zip(expected) {
                let (_, meter) = metered();
                let verdict = theory(name).admit(&value, &meter).expect("a verdict");
                assert_eq!(verdict.is_proven(), admitted, "{name} {value:?}");
                assert_eq!(verdict.usage(), TheoryUsage::new(1, 0), "{name}");
                assert!(
                    !matches!(verdict, TheoryVerdict::Undetermined { .. }),
                    "{name}: a structural check always decides"
                );
            }
        }
    }

    #[test]
    fn theory_builtins_reserve_one_operation_per_check() {
        let (budget, meter) = metered();
        let nat = theory("Nat");
        for value in [int(1), int(-1), string("x")] {
            nat.admit(&value, &meter).expect("a verdict");
        }
        let stored = ListParWithRandom {
            pars: vec![int(1), int(2), int(3)],
            ..ListParWithRandom::default()
        };
        assert!(nat
            .check_match(&stored, &ListParWithRandom::default(), &meter)
            .expect("a verdict")
            .is_proven());
        assert_eq!(
            budget.usage(HostWorkDimension::VerificationOperations),
            HostWorkUsage::new(4),
            "one unit per value at a send and one per stored datum at a match"
        );
        assert_eq!(
            budget.usage(HostWorkDimension::VerificationBytes),
            HostWorkUsage::new(0)
        );
        let refused = ListParWithRandom {
            pars: vec![int(1), int(-2)],
            ..ListParWithRandom::default()
        };
        assert!(!nat
            .check_match(&refused, &ListParWithRandom::default(), &meter)
            .expect("a verdict")
            .is_proven());
    }

    #[test]
    fn theory_builtins_with_zero_work_are_undetermined_before_any_read() {
        let identity = TheoryIdentity {
            limit_profile: TheoryUsage::new(0, 0),
            ..TheoryIdentity::builtin("Nat")
        };
        let starved = BuiltinTheory::of_identity(&identity).expect("any limit profile resolves");
        let (budget, meter) = metered();
        assert_eq!(
            starved.admit(&int(1), &meter).expect("a verdict"),
            TheoryVerdict::Undetermined {
                reason: TheoryReason::WorkBudgetExhausted {
                    required: 1,
                    available: 0,
                },
                usage: TheoryUsage::new(1, 0),
            }
        );
        assert!(matches!(
            starved.check_match(
                &ListParWithRandom::default(),
                &ListParWithRandom::default(),
                &meter
            ),
            Ok(TheoryVerdict::Undetermined { .. })
        ));
        assert_eq!(
            budget.usage(HostWorkDimension::VerificationOperations),
            HostWorkUsage::new(0)
        );
    }

    #[test]
    fn theory_builtins_resolve_only_the_structural_identities() {
        for name in BUILTIN_NAMES {
            assert_eq!(
                BuiltinTheory::of_identity(&TheoryIdentity::builtin(name)).map(|b| b.identity),
                Some(TheoryIdentity::builtin(name))
            );
        }
        let base = TheoryIdentity::builtin("Nat");
        for other in [
            TheoryIdentity {
                theorem_id: "Real".to_string(),
                ..base.clone()
            },
            TheoryIdentity {
                checker_abi: "rrs:structural:v2".to_string(),
                ..base.clone()
            },
            TheoryIdentity {
                language_fingerprint: "mettail:rholang".to_string(),
                ..base.clone()
            },
            TheoryIdentity {
                category: "Name".to_string(),
                ..base.clone()
            },
            TheoryIdentity {
                version: 2,
                ..base.clone()
            },
            TheoryIdentity {
                args: vec![int(1)],
                ..base.clone()
            },
        ] {
            assert_eq!(BuiltinTheory::of_identity(&other), None, "{other:?}");
        }
    }
}
