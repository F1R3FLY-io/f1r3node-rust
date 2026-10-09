//! The constructor of a factory call and the config of a new space (P4 design C.7, steps 2
//! and 5, I-17, and P5 design v3, A.7, A.8 and A.10).
//!
//! A factory accepts only the canonical constructor `[q?][FREE?][PARAMS?]` of
//! `SpaceFactoryConstructor`. [`FactoryConstructor::validate`] then applies the gates of this
//! phase in this order:
//!
//! 1. The family has a constructor schema in this phase, else `FamilyUnavailable`.
//! 2. The constructor parses, else `BadConstructorArgument`.
//! 3. The family takes the qualifier, else `BadConstructorArgument`. In native execution every
//!    qualifier passes (P7). Legacy execution refuses `seq` with `QualifierUnavailable`, because
//!    the Seq rules need the job order of native execution (P7, design v3 QT-5).
//! 4. A theory `free Th()` names a builtin: `Nat`, `Int`, `String`, `Bool` or `Any` (P7, TH-4).
//!    Another name fails with `UnknownTheory` and the nearest builtin within two edits, and a
//!    builtin with arguments fails with `BadConstructorArgument`. The config persists the
//!    identity of the theory, never a handle.
//! 5. The params are not positional, else `PositionalParamsUnsupported`.
//! 6. Each named param is in the schema of the family, else `UnknownParam` with a suggestion.
//!    The keys are checked in bytewise order.
//! 7. Each param that the family requires is present, else `MissingParam` (the `size` of an
//!    Array space, F7). The required keys are checked in bytewise order.
//! 8. The rule of each named param accepts its value, else `InvalidParam` (`defaults::ParamRule`:
//!    `size` and `cyclic` of F7, `priorities` of I-10, and the VectorDB params of P6). The values
//!    are checked in bytewise key order.
//! 9. A VectorDB space with a `mirror` has the `default` qualifier and a metric that ChromaDB
//!    indexes (`cosine`, `dot` or `euclidean`), else `InvalidParam` on `mirror` (P6, design v3
//!    4.2). The metric is the `metric` param, or `cosine` without it.
//!
//! The keys come before the values, as in the check of the send params of the agent (P5 design
//! v3, A.3.4). Every gate runs before the space exists, so a rejected call creates nothing.
//!
//! The config of the new space is a `SpaceConfig`. Its params are the named params of the
//! constructor, each in the canonical form of its rule. An absent optional param stays absent,
//! and its reader applies the default: a fixed Array space, or priority levels without a bound.
//! The agent stores the canonical map on `C(sid)` once (I-17), and nothing changes the map
//! later. [`family_of`] names the family of a stored config.

use std::collections::BTreeMap;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::Par;
use models::rust::reified::encoding::SendParams;
use models::rust::space_factory_urn::{
    SpaceFactoryConstructor, SpaceFactoryUrn, SpaceInner, SpaceOuter, SpaceQualifier,
};

use super::defaults::{
    constructor_schema, required_params, suggestion, ParamRule, METRIC_PARAM, MIRROR_PARAM,
};
use crate::rust::interpreter::spaces::errors::SpaceError;
use crate::rust::interpreter::spaces::namespace::{Durability, NativeSpace};
use crate::rust::interpreter::spaces::record::sole_expr;
use crate::rust::interpreter::spaces::theory::builtin::{BuiltinKind, BUILTIN_NAMES};
use crate::rust::interpreter::spaces::theory::identity::TheoryIdentity;
use crate::rust::interpreter::spaces::types::config::SpaceConfig;
use crate::rust::interpreter::spaces::types::qualifier::SpaceQualifier as ConfigQualifier;
use crate::rust::interpreter::tensor::exact::Metric;

/// The validated constructor of a factory call.
#[derive(Clone, Debug, PartialEq)]
pub struct FactoryConstructor {
    /// The family of the factory.
    pub family: SpaceFactoryUrn,
    /// The qualifier of the new space.
    pub qualifier: SpaceQualifier,
    /// The named params, which the schema of the family accepts, each in the canonical form of
    /// its rule.
    pub params: BTreeMap<String, Par>,
    /// The identity of the theory of the space, or `None` (P7, TH-4).
    pub theory: Option<TheoryIdentity>,
}

/// The execution model of a factory call (P7, design v3 QT-5). The Seq rules need the
/// deterministic job order of native execution, so gate 3 accepts `seq` only there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Execution {
    /// Legacy execution: host produces and consumes bypass the scheduler (F11).
    Legacy,
    /// Native funded execution (protocol v6), in native play and native replay.
    Native,
}

impl FactoryConstructor {
    /// Validates the constructor list `ctor` of a call to the factory of `family` in native
    /// execution, the model of every new deploy (decision 69). It is
    /// [`FactoryConstructor::validate_in`] with [`Execution::Native`].
    pub fn validate(family: SpaceFactoryUrn, ctor: &Par) -> Result<FactoryConstructor, SpaceError> {
        Self::validate_in(family, ctor, Execution::Native)
    }

    /// Validates the constructor list `ctor` of a call to the factory of `family` in the
    /// execution model `execution` with the gates 1 to 9 of this module, in order.
    pub fn validate_in(
        family: SpaceFactoryUrn,
        ctor: &Par,
        execution: Execution,
    ) -> Result<FactoryConstructor, SpaceError> {
        let schema = constructor_schema(family).ok_or_else(|| SpaceError::FamilyUnavailable {
            outer: family.outer.as_str().to_string(),
            inner: family.inner.as_str().to_string(),
        })?;
        let SpaceFactoryConstructor {
            qualifier,
            theory,
            params,
        } = SpaceFactoryConstructor::parse(ctor)
            .map_err(|error| SpaceError::BadConstructorArgument(error.to_string()))?;
        let qualifier = qualifier.unwrap_or_default();
        if !family.supports_qualifier(qualifier) {
            return Err(SpaceError::BadConstructorArgument(format!(
                "the {} outer store does not take the {qualifier} qualifier",
                family.outer.as_str()
            )));
        }
        // Changed by P7 (design v3 QT-1): a `temp` space lives for one deploy (I-4). `seq` stays
        // unavailable until its rules (SQ-1 to SQ-8) are in place.
        // match qualifier {
        //     SpaceQualifier::Default => {}
        //     SpaceQualifier::Temp | SpaceQualifier::Seq => {
        //         return Err(SpaceError::QualifierUnavailable(
        //             qualifier.as_str().to_string(),
        //         ))
        //     }
        // }
        // Changed by P7 step 2 (design v3 QT-5): `seq` runs in native execution, and legacy
        // execution keeps `QualifierUnavailable` for it.
        // match qualifier {
        //     SpaceQualifier::Default | SpaceQualifier::Temp => {}
        //     SpaceQualifier::Seq => {
        //         return Err(SpaceError::QualifierUnavailable(
        //             qualifier.as_str().to_string(),
        //         ))
        //     }
        // }
        match (qualifier, execution) {
            (SpaceQualifier::Default | SpaceQualifier::Temp, _)
            | (SpaceQualifier::Seq, Execution::Native) => {}
            (SpaceQualifier::Seq, Execution::Legacy) => {
                return Err(SpaceError::QualifierUnavailable(
                    qualifier.as_str().to_string(),
                ))
            }
        }
        // Changed by P7 step 4 (design v3 TH-4): gate 4 resolves the builtin theories.
        // if let Some(theory) = theory {
        //     return Err(SpaceError::TheoryUnavailable(theory.name));
        // }
        let theory = match theory {
            None => None,
            Some(free) => Some(theory_gate(&free.name, &free.args)?),
        };
        let SendParams { positional, named } = params.unwrap_or_default();
        if !positional.is_empty() {
            return Err(SpaceError::PositionalParamsUnsupported);
        }
        if let Some(key) = named.keys().find(|key| !schema.contains(&key.as_str())) {
            return Err(SpaceError::UnknownParam {
                key: key.clone(),
                suggestion: suggestion(key, schema).map(str::to_string),
            });
        }
        if let Some(key) = required_params(family)
            .iter()
            .find(|key| !named.contains_key(**key))
        {
            return Err(SpaceError::MissingParam {
                key: (*key).to_string(),
            });
        }
        let params = canonical_params(named)?;
        check_mirror(qualifier, &params)?;
        Ok(FactoryConstructor {
            family,
            qualifier,
            params,
            theory,
        })
    }

    /// The durability of the new space: durable for `default`, ephemeral for `temp` and `seq`.
    pub const fn durability(&self) -> Durability {
        match self.qualifier {
            SpaceQualifier::Default => Durability::Durable,
            SpaceQualifier::Temp | SpaceQualifier::Seq => Durability::Ephemeral,
        }
    }

    /// The config of the new space `space` (I-17), with the identity of its theory (P7).
    pub fn config(&self, space: NativeSpace) -> SpaceConfig {
        SpaceConfig {
            space_id: space.sid,
            outer: self.family.outer.as_str().to_string(),
            inner: self.family.inner.as_str().to_string(),
            qualifier: config_qualifier(self.qualifier),
            params: self.params.clone(),
            theory: self.theory.clone(),
        }
    }
}

/// Gate 4 (P7, TH-4): the identity of the builtin theory `name` without arguments. Another name
/// fails with `UnknownTheory` and the nearest builtin within two edits, and a builtin with
/// arguments fails with `BadConstructorArgument`. Until the F1r3lang merge, `args[0]` has no
/// meaning: after it, it is the capability token of an installed theory (P12).
fn theory_gate(name: &str, args: &[Par]) -> Result<TheoryIdentity, SpaceError> {
    match (BuiltinKind::of_name(name), args.is_empty()) {
        (Some(_), true) => Ok(TheoryIdentity::builtin(name)),
        (Some(_), false) => Err(SpaceError::BadConstructorArgument(format!(
            "the builtin theory {name} takes no arguments"
        ))),
        (None, _) => Err(SpaceError::UnknownTheory {
            name: name.to_string(),
            suggestion: suggestion(name, &BUILTIN_NAMES).map(str::to_string),
        }),
    }
}

/// The config qualifier of a constructor qualifier.
const fn config_qualifier(qualifier: SpaceQualifier) -> ConfigQualifier {
    match qualifier {
        SpaceQualifier::Default => ConfigQualifier::Default,
        SpaceQualifier::Temp => ConfigQualifier::Temp,
        SpaceQualifier::Seq => ConfigQualifier::Seq,
    }
}

/// Gate 9 (P6, design v3 4.2): a `mirror` needs the `default` qualifier and a metric that
/// ChromaDB indexes. The params are canonical, so `metric` is one of the six names.
fn check_mirror(
    qualifier: SpaceQualifier,
    params: &BTreeMap<String, Par>,
) -> Result<(), SpaceError> {
    if !params.contains_key(MIRROR_PARAM) {
        return Ok(());
    }
    let metric = match params.get(METRIC_PARAM) {
        Some(value) => match sole_expr(value) {
            Some(ExprInstance::GString(name)) => Metric::parse(name),
            _ => None,
        },
        None => Some(Metric::Cosine),
    };
    match (qualifier, metric) {
        (SpaceQualifier::Default, Some(Metric::Cosine | Metric::Dot | Metric::Euclidean)) => Ok(()),
        (
            SpaceQualifier::Default | SpaceQualifier::Temp | SpaceQualifier::Seq,
            Some(
                Metric::Cosine
                | Metric::Dot
                | Metric::Euclidean
                | Metric::Manhattan
                | Metric::Hamming
                | Metric::Jaccard,
            )
            | None,
        ) => Err(SpaceError::InvalidParam {
            key: MIRROR_PARAM.to_string(),
        }),
    }
}

/// Gate 8: each named param in the canonical form of its rule, in bytewise key order. The first
/// value that its rule refuses fails with `InvalidParam`. Gate 6 has checked every key against
/// the schema, and every schema key has a rule, so a key without a rule is inconsistent.
fn canonical_params(named: BTreeMap<String, Par>) -> Result<BTreeMap<String, Par>, SpaceError> {
    let mut params = BTreeMap::new();
    for (key, value) in named {
        let rule = ParamRule::of(&key).ok_or(SpaceError::Inconsistent(
            "a constructor param has no value rule",
        ))?;
        match rule.canonical(&value) {
            Some(canonical) => {
                params.insert(key, canonical);
            }
            None => return Err(SpaceError::InvalidParam { key }),
        }
    }
    Ok(params)
}

/// The family that a stored config names. A config without a supported pair is inconsistent.
pub fn family_of(config: &SpaceConfig) -> Result<SpaceFactoryUrn, SpaceError> {
    match (
        SpaceOuter::parse(&config.outer),
        SpaceInner::parse(&config.inner),
    ) {
        (Some(outer), Some(inner)) => {
            let family = SpaceFactoryUrn { outer, inner };
            match family.is_supported_pair() {
                true => Ok(family),
                false => Err(SpaceError::Inconsistent(
                    "the config record names an unsupported family",
                )),
            }
        }
        (None, _) | (Some(_), None) => Err(SpaceError::Inconsistent(
            "the config record names an unknown family",
        )),
    }
}

#[cfg(test)]
mod tests {
    use models::rust::reified::builders;
    use models::rust::reified::encoding::{encode_free, FreeTheory};
    use models::rust::reified::params::SendParam;
    use models::rust::utils::{new_elist_par, new_gbool_par, new_gint_par, new_gstring_par};

    use super::*;
    use crate::rust::interpreter::spaces::namespace::SID_LEN;

    fn list(ps: Vec<Par>) -> Par { new_elist_par(ps, Vec::new(), false, None, Vec::new(), false) }

    fn int(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

    fn string(value: &str) -> Par { new_gstring_par(value.to_string(), Vec::new(), false) }

    fn nat() -> Par { free("Nat", Vec::new()) }

    fn free(name: &str, args: Vec<Par>) -> Par {
        encode_free(&FreeTheory {
            name: name.to_string(),
            args,
        })
        .expect("a valid theory")
    }

    fn params(params: Vec<SendParam>) -> Par {
        builders::send_params(params)
            .expect("valid params")
            .expect("non-empty params")
    }

    fn validate(ctor: Vec<Par>) -> Result<FactoryConstructor, SpaceError> {
        FactoryConstructor::validate(SpaceFactoryUrn::HASHMAP_QUEUE, &list(ctor))
    }

    fn space() -> NativeSpace {
        NativeSpace {
            sid: [5; SID_LEN],
            durability: Durability::Durable,
        }
    }

    #[test]
    fn reified_factory_constructor_accepts_the_default_space() {
        for ctor in [Vec::new(), vec![string("default")]] {
            let validated = validate(ctor).expect("the default constructor is valid");
            assert_eq!(validated, FactoryConstructor {
                family: SpaceFactoryUrn::HASHMAP_QUEUE,
                qualifier: SpaceQualifier::Default,
                params: BTreeMap::new(),
                theory: None,
            });
            assert_eq!(validated.durability(), Durability::Durable);
        }
    }

    #[test]
    fn reified_factory_p4_gates_fail_closed_in_order() {
        // Changed by P7 step 1 (design v3 QT-1): `temp` creates an ephemeral space.
        // assert_eq!(
        //     validate(vec![string("temp")]),
        //     Err(SpaceError::QualifierUnavailable("temp".to_string()))
        // );
        let temp = validate(vec![string("temp")]).expect("a temp constructor is valid");
        assert_eq!(temp.qualifier, SpaceQualifier::Temp);
        assert_eq!(temp.durability(), Durability::Ephemeral);
        // Changed by P7 step 2 (design v3 QT-5): native execution accepts `seq`, so the theory
        // gate decides, and only legacy execution refuses `seq` at gate 3.
        // assert_eq!(
        //     validate(vec![string("seq"), nat()]),
        //     Err(SpaceError::QualifierUnavailable("seq".to_string()))
        // );
        // Changed by P7 step 4 (design v3 TH-4): gate 4 accepts the builtin theories, so an
        // unknown theory shows the order of gates 3 and 4.
        // assert_eq!(
        //     validate(vec![string("seq"), nat()]),
        //     Err(SpaceError::TheoryUnavailable("Nat".to_string()))
        // );
        assert_eq!(
            validate(vec![string("seq"), free("Bounded", Vec::new())]),
            Err(SpaceError::UnknownTheory {
                name: "Bounded".to_string(),
                suggestion: None,
            })
        );
        assert_eq!(
            validate(vec![string("seq"), nat()])
                .expect("a seq space with a builtin theory is valid natively")
                .theory,
            Some(TheoryIdentity::builtin("Nat"))
        );
        assert_eq!(
            FactoryConstructor::validate_in(
                SpaceFactoryUrn::HASHMAP_QUEUE,
                &list(vec![string("seq"), nat()]),
                Execution::Legacy
            ),
            Err(SpaceError::QualifierUnavailable("seq".to_string()))
        );
        let seq = validate(vec![string("seq")]).expect("a seq constructor is valid natively");
        assert_eq!(seq.qualifier, SpaceQualifier::Seq);
        assert_eq!(seq.durability(), Durability::Ephemeral);
        // Changed by P7 step 4 (design v3 TH-4): gate 4 accepts the builtin theories, so an
        // unknown theory shows the order of gates 4 and 5.
        // assert_eq!(
        //     validate(vec![nat()]),
        //     Err(SpaceError::TheoryUnavailable("Nat".to_string()))
        // );
        // assert_eq!(
        //     validate(vec![nat(), params(vec![SendParam::Positional(int(1))])]),
        //     Err(SpaceError::TheoryUnavailable("Nat".to_string()))
        // );
        assert_eq!(
            validate(vec![nat()])
                .expect("a builtin theory is valid")
                .theory,
            Some(TheoryIdentity::builtin("Nat"))
        );
        assert_eq!(
            validate(vec![
                free("Natt", Vec::new()),
                params(vec![SendParam::Positional(int(1))])
            ]),
            Err(SpaceError::UnknownTheory {
                name: "Natt".to_string(),
                suggestion: Some("Nat".to_string()),
            })
        );
        assert_eq!(
            validate(vec![nat(), params(vec![SendParam::Positional(int(1))])]),
            Err(SpaceError::PositionalParamsUnsupported)
        );
        assert_eq!(
            validate(vec![params(vec![
                SendParam::Positional(int(1)),
                SendParam::Named("size".to_string(), int(2)),
            ])]),
            Err(SpaceError::PositionalParamsUnsupported)
        );
        assert_eq!(
            validate(vec![params(vec![
                SendParam::Named("zeta".to_string(), int(1)),
                SendParam::Named("alpha".to_string(), int(2)),
            ])]),
            Err(SpaceError::UnknownParam {
                key: "alpha".to_string(),
                suggestion: None,
            })
        );
        assert_eq!(
            validate(vec![
                string("default"),
                params(vec![SendParam::Named("priority".to_string(), int(1))]),
            ]),
            Err(SpaceError::UnknownParam {
                key: "priority".to_string(),
                suggestion: None,
            })
        );
        // Changed by P7 step 3 (design v3 HS-7): every family has a schema, so the loop below
        // would check nothing. Gate 1 now refuses only a pair that is no family, which no
        // factory channel names.
        // for family in SpaceFactoryUrn::all() {
        //     if constructor_schema(family).is_some() {
        //         continue;
        //     }
        //     assert_eq!(
        //         FactoryConstructor::validate(family, &list(Vec::new())),
        //         Err(SpaceError::FamilyUnavailable {
        //             outer: family.outer.as_str().to_string(),
        //             inner: family.inner.as_str().to_string(),
        //         }),
        //         "{family}"
        //     );
        // }
        assert!(SpaceFactoryUrn::all().all(|family| constructor_schema(family).is_some()));
        for outer in [SpaceOuter::PathMap, SpaceOuter::Array, SpaceOuter::HashSet] {
            let pair = family(outer, SpaceInner::VectorDb);
            assert!(!pair.is_supported_pair(), "{pair:?}");
            assert_eq!(
                FactoryConstructor::validate(pair, &list(Vec::new())),
                Err(SpaceError::FamilyUnavailable {
                    outer: outer.as_str().to_string(),
                    inner: "vectordb".to_string(),
                }),
                "{pair:?}"
            );
        }
    }

    /// HS-6 and QT-5: a HashSet factory takes only `seq`, and only in native execution. Gate 3
    /// refuses `default` and `temp` with `BadConstructorArgument` in both execution models, and
    /// legacy execution refuses `seq` with `QualifierUnavailable`. Every gate runs before the
    /// params, so an unknown param does not change the error.
    #[test]
    fn reified_factory_hashset_takes_only_seq_in_native_execution() {
        let unknown = params(vec![SendParam::Named("zeta".to_string(), int(1))]);
        for inner in [
            SpaceInner::Bag,
            SpaceInner::Queue,
            SpaceInner::Stack,
            SpaceInner::Set,
            SpaceInner::Cell,
            SpaceInner::PriorityQueue,
        ] {
            let hashset = family(SpaceOuter::HashSet, inner);
            for execution in [Execution::Native, Execution::Legacy] {
                for (ctor, qualifier) in [
                    (Vec::new(), "default"),
                    (vec![string("default")], "default"),
                    (vec![string("temp")], "temp"),
                    (vec![string("temp"), unknown.clone()], "temp"),
                ] {
                    assert_eq!(
                        FactoryConstructor::validate_in(hashset, &list(ctor), execution),
                        Err(SpaceError::BadConstructorArgument(format!(
                            "the hashset outer store does not take the {qualifier} qualifier"
                        ))),
                        "{hashset} {execution:?} {qualifier}"
                    );
                }
            }
            for ctor in [vec![string("seq")], vec![string("seq"), unknown.clone()]] {
                assert_eq!(
                    FactoryConstructor::validate_in(hashset, &list(ctor), Execution::Legacy),
                    Err(SpaceError::QualifierUnavailable("seq".to_string())),
                    "{hashset}"
                );
            }
            let seq = FactoryConstructor::validate(hashset, &list(vec![string("seq")]))
                .expect("a seq HashSet constructor is valid natively");
            assert_eq!(seq.qualifier, SpaceQualifier::Seq);
            assert_eq!(seq.durability(), Durability::Ephemeral);
            assert!(seq.params.is_empty());
            assert_eq!(
                FactoryConstructor::validate(hashset, &list(vec![string("seq"), unknown.clone()])),
                Err(SpaceError::UnknownParam {
                    key: "zeta".to_string(),
                    suggestion: None,
                }),
                "{hashset}"
            );
        }
        let levels = FactoryConstructor::validate(
            family(SpaceOuter::HashSet, SpaceInner::PriorityQueue),
            &list(vec![
                string("seq"),
                params(vec![SendParam::Named("priorities".to_string(), int(3))]),
            ]),
        )
        .expect("a HashSet priority queue takes the params of its discipline");
        assert_eq!(levels.params.get("priorities"), Some(&int(3)));
        assert_eq!(
            FactoryConstructor::validate(
                family(SpaceOuter::HashSet, SpaceInner::Set),
                &list(vec![
                    string("seq"),
                    params(vec![SendParam::Named("priorities".to_string(), int(3))]),
                ]),
            ),
            Err(SpaceError::UnknownParam {
                key: "priorities".to_string(),
                suggestion: None,
            })
        );
    }

    fn family(outer: SpaceOuter, inner: SpaceInner) -> SpaceFactoryUrn {
        SpaceFactoryUrn { outer, inner }
    }

    fn flag(value: bool) -> Par { new_gbool_par(value, Vec::new(), false) }

    fn named(pairs: Vec<(&str, Par)>) -> Par {
        params(
            pairs
                .into_iter()
                .map(|(key, value)| SendParam::Named(key.to_string(), value))
                .collect(),
        )
    }

    #[test]
    fn reified_factory_p5_param_gates_fail_closed_in_order() {
        let array_queue = family(SpaceOuter::Array, SpaceInner::Queue);
        let array_pq = family(SpaceOuter::Array, SpaceInner::PriorityQueue);
        let hashmap_pq = family(SpaceOuter::HashMap, SpaceInner::PriorityQueue);
        let vector_bag = family(SpaceOuter::Vector, SpaceInner::Bag);
        let check = |family: SpaceFactoryUrn, ctor: Vec<Par>| {
            FactoryConstructor::validate(family, &list(ctor))
        };
        // Gate 6 before gate 7: an unknown key wins over a missing size.
        assert_eq!(
            check(array_queue, vec![named(vec![("sise", int(4))])]),
            Err(SpaceError::UnknownParam {
                key: "sise".to_string(),
                suggestion: Some("size".to_string()),
            })
        );
        assert_eq!(
            check(array_queue, vec![named(vec![("cylic", flag(true))])]),
            Err(SpaceError::UnknownParam {
                key: "cylic".to_string(),
                suggestion: Some("cyclic".to_string()),
            })
        );
        assert_eq!(
            check(hashmap_pq, vec![named(vec![("prioritis", int(3))])]),
            Err(SpaceError::UnknownParam {
                key: "prioritis".to_string(),
                suggestion: Some("priorities".to_string()),
            })
        );
        // Vector takes no size, and a queue takes no priorities.
        assert_eq!(
            check(vector_bag, vec![named(vec![("size", int(4))])]),
            Err(SpaceError::UnknownParam {
                key: "size".to_string(),
                suggestion: None,
            })
        );
        assert_eq!(
            validate(vec![named(vec![("priorities", int(3))])]),
            Err(SpaceError::UnknownParam {
                key: "priorities".to_string(),
                suggestion: None,
            })
        );
        // Gate 7 before gate 8: a missing size wins over a bad cyclic flag.
        for ctor in [Vec::new(), vec![string("default")], vec![named(vec![(
            "cyclic",
            int(1),
        )])]] {
            assert_eq!(
                check(array_queue, ctor),
                Err(SpaceError::MissingParam {
                    key: "size".to_string(),
                })
            );
        }
        // Gate 8: the values, in bytewise key order.
        for (pairs, key) in [
            (vec![("size", int(0))], "size"),
            (vec![("size", int(-3))], "size"),
            (vec![("size", flag(true))], "size"),
            (vec![("size", string("4"))], "size"),
            (vec![("cyclic", int(1)), ("size", int(0))], "cyclic"),
            (vec![("cyclic", flag(false)), ("size", int(0))], "size"),
        ] {
            assert_eq!(
                check(array_queue, vec![named(pairs)]),
                Err(SpaceError::InvalidParam {
                    key: key.to_string(),
                })
            );
        }
        for value in [int(0), int(-1), flag(true), string("3")] {
            assert_eq!(
                check(hashmap_pq, vec![named(vec![("priorities", value)])]),
                Err(SpaceError::InvalidParam {
                    key: "priorities".to_string(),
                })
            );
        }
        assert_eq!(
            check(array_pq, vec![named(vec![
                ("priorities", int(0)),
                ("size", int(2))
            ])]),
            Err(SpaceError::InvalidParam {
                key: "priorities".to_string(),
            })
        );
        // The earlier gates still come first.
        // Changed by P7 step 1 (design v3 QT-1): `temp` passes gate 3, so the gate of `seq`
        // shows the order.
        // assert_eq!(
        //     check(array_queue, vec![string("temp")]),
        //     Err(SpaceError::QualifierUnavailable("temp".to_string()))
        // );
        // Changed by P7 step 2 (design v3 QT-5): native execution accepts `seq`, so legacy
        // execution shows the order of gate 3, and natively the missing size decides.
        // assert_eq!(
        //     check(array_queue, vec![string("seq")]),
        //     Err(SpaceError::QualifierUnavailable("seq".to_string()))
        // );
        assert_eq!(
            FactoryConstructor::validate_in(
                array_queue,
                &list(vec![string("seq")]),
                Execution::Legacy
            ),
            Err(SpaceError::QualifierUnavailable("seq".to_string()))
        );
        assert_eq!(
            check(array_queue, vec![string("seq")]),
            Err(SpaceError::MissingParam {
                key: "size".to_string()
            })
        );
        // Changed by P7 step 4 (design v3 TH-4): gate 4 accepts the builtin theories, so the
        // missing size decides, and an unknown theory fails before it.
        // assert_eq!(
        //     check(array_queue, vec![nat()]),
        //     Err(SpaceError::TheoryUnavailable("Nat".to_string()))
        // );
        assert_eq!(
            check(array_queue, vec![nat()]),
            Err(SpaceError::MissingParam {
                key: "size".to_string()
            })
        );
        assert_eq!(
            check(array_queue, vec![free("Even", Vec::new())]),
            Err(SpaceError::UnknownTheory {
                name: "Even".to_string(),
                suggestion: None,
            })
        );
        assert_eq!(
            check(array_queue, vec![params(vec![SendParam::Positional(int(
                4
            ))])]),
            Err(SpaceError::PositionalParamsUnsupported)
        );
    }

    #[test]
    fn reified_factory_p5_params_are_stored_in_canonical_form() {
        let array_queue = family(SpaceOuter::Array, SpaceInner::Queue);
        let array_pq = family(SpaceOuter::Array, SpaceInner::PriorityQueue);
        let mut stray = int(8);
        stray.locally_free = vec![0b0000_0010];
        let validated = FactoryConstructor::validate(
            array_queue,
            &list(vec![
                string("default"),
                named(vec![("size", stray), ("cyclic", flag(true))]),
            ]),
        )
        .expect("a valid Array constructor");
        assert_eq!(
            validated.params,
            BTreeMap::from([
                ("cyclic".to_string(), flag(true)),
                ("size".to_string(), int(8))
            ])
        );
        let fixed = FactoryConstructor::validate(
            array_queue,
            &list(vec![named(vec![("size", int(i64::MAX))])]),
        )
        .expect("a fixed Array space without cyclic");
        assert_eq!(
            fixed.params,
            BTreeMap::from([("size".to_string(), int(i64::MAX))])
        );
        let config = fixed.config(space());
        assert_eq!(config.outer, "array");
        assert_eq!(config.inner, "queue");
        assert_eq!(SpaceConfig::from_par(&config.to_par()), Ok(config.clone()));
        assert_eq!(family_of(&config), Ok(array_queue));
        let bounded = FactoryConstructor::validate(
            array_pq,
            &list(vec![named(vec![("priorities", int(3)), ("size", int(1))])]),
        )
        .expect("a valid Array priority queue constructor");
        assert_eq!(
            bounded.params,
            BTreeMap::from([
                ("priorities".to_string(), int(3)),
                ("size".to_string(), int(1))
            ])
        );
        for family in SpaceFactoryUrn::all() {
            let Some(schema) = constructor_schema(family) else {
                continue;
            };
            let required: Vec<(&str, Par)> = required_params(family)
                .iter()
                .map(|key| (*key, int(2)))
                .collect();
            // Changed by P7 step 3 (design v3 HS-6): a HashSet family takes only `seq`, so its
            // minimal constructor names it.
            // let ctor = match required.is_empty() {
            //     true => Vec::new(),
            //     false => vec![named(required.clone())],
            // };
            let (mut ctor, qualifier, durability) =
                match family.supports_qualifier(SpaceQualifier::Default) {
                    true => (Vec::new(), SpaceQualifier::Default, Durability::Durable),
                    false => (
                        vec![string("seq")],
                        SpaceQualifier::Seq,
                        Durability::Ephemeral,
                    ),
                };
            if !required.is_empty() {
                ctor.push(named(required.clone()));
            }
            let minimal = FactoryConstructor::validate(family, &list(ctor))
                .unwrap_or_else(|error| panic!("{family}: {error:?}"));
            assert_eq!(minimal.family, family);
            // assert_eq!(minimal.qualifier, SpaceQualifier::Default);
            // assert_eq!(minimal.durability(), Durability::Durable);
            assert_eq!(minimal.qualifier, qualifier, "{family}");
            assert_eq!(minimal.durability(), durability, "{family}");
            assert_eq!(minimal.params.len(), required.len(), "{family}");
            assert!(minimal
                .params
                .keys()
                .all(|key| schema.contains(&key.as_str())));
            assert_eq!(family_of(&minimal.config(space())), Ok(family));
        }
    }

    #[test]
    fn reified_factory_rejects_bad_constructor_arguments() {
        for ctor in [
            string("default"),
            Par::default(),
            list(vec![int(1)]),
            list(vec![string("unknown")]),
            list(vec![nat(), string("default")]),
            list(vec![string("default"), string("temp")]),
            list(vec![list(Vec::new())]),
        ] {
            assert!(
                matches!(
                    FactoryConstructor::validate(SpaceFactoryUrn::HASHMAP_QUEUE, &ctor),
                    Err(SpaceError::BadConstructorArgument(_))
                ),
                "{ctor:?}"
            );
        }
    }

    #[test]
    fn reified_factory_config_is_the_canonical_space_config() {
        let validated = validate(Vec::new()).expect("the default constructor is valid");
        let config = validated.config(space());
        assert_eq!(
            config,
            SpaceConfig::new([5; SID_LEN], "hashmap", "queue", ConfigQualifier::Default)
        );
        assert_eq!(SpaceConfig::from_par(&config.to_par()), Ok(config.clone()));
        assert_eq!(family_of(&config), Ok(SpaceFactoryUrn::HASHMAP_QUEUE));
        assert_ne!(
            validated
                .config(NativeSpace {
                    sid: [6; SID_LEN],
                    durability: Durability::Durable,
                })
                .to_par(),
            config.to_par()
        );
        for (qualifier, expected) in [
            (SpaceQualifier::Default, ConfigQualifier::Default),
            (SpaceQualifier::Temp, ConfigQualifier::Temp),
            (SpaceQualifier::Seq, ConfigQualifier::Seq),
        ] {
            assert_eq!(config_qualifier(qualifier), expected);
            assert_eq!(qualifier.as_str(), expected.as_str());
            let durability = FactoryConstructor {
                qualifier,
                ..validated.clone()
            }
            .durability();
            assert_eq!(durability, match qualifier {
                SpaceQualifier::Default => Durability::Durable,
                SpaceQualifier::Temp | SpaceQualifier::Seq => Durability::Ephemeral,
            });
        }
    }

    #[test]
    fn reified_factory_family_of_rejects_unknown_and_unsupported_families() {
        let config = |outer: &str, inner: &str| {
            SpaceConfig::new([5; SID_LEN], outer, inner, ConfigQualifier::Default)
        };
        assert_eq!(
            family_of(&config("pathmap", "vectordb")),
            Err(SpaceError::Inconsistent(
                "the config record names an unsupported family"
            ))
        );
        for (outer, inner) in [
            ("tree", "queue"),
            ("hashmap", "deque"),
            ("HashMap", "queue"),
        ] {
            assert_eq!(
                family_of(&config(outer, inner)),
                Err(SpaceError::Inconsistent(
                    "the config record names an unknown family"
                )),
                "{outer}:{inner}"
            );
        }
        for family in SpaceFactoryUrn::all() {
            assert_eq!(
                family_of(&config(family.outer.as_str(), family.inner.as_str())),
                Ok(family)
            );
        }
    }

    /// Gate 9: a `mirror` needs the `default` qualifier and a metric that ChromaDB indexes. Gate
    /// 3 refuses `temp` and `seq` until P7, so this unit test is the only test of the pairs of
    /// `mirror` with these qualifiers before P7.
    #[test]
    fn reified_factory_gate_9_needs_the_default_qualifier_and_an_indexed_metric() {
        let refused = Err(SpaceError::InvalidParam {
            key: MIRROR_PARAM.to_string(),
        });
        let metrics = [
            None,
            Some("cosine"),
            Some("dot"),
            Some("euclidean"),
            Some("manhattan"),
            Some("hamming"),
            Some("jaccard"),
        ];
        for qualifier in [
            SpaceQualifier::Default,
            SpaceQualifier::Temp,
            SpaceQualifier::Seq,
        ] {
            for metric in metrics {
                let mut params = BTreeMap::new();
                if let Some(metric) = metric {
                    params.insert(METRIC_PARAM.to_string(), string(metric));
                }
                assert_eq!(
                    check_mirror(qualifier, &params),
                    Ok(()),
                    "{qualifier:?} {metric:?} without a mirror"
                );
                params.insert(MIRROR_PARAM.to_string(), string("chroma"));
                let expected = match (qualifier, metric) {
                    (SpaceQualifier::Default, None | Some("cosine" | "dot" | "euclidean")) => {
                        Ok(())
                    }
                    _ => refused.clone(),
                };
                assert_eq!(
                    check_mirror(qualifier, &params),
                    expected,
                    "{qualifier:?} {metric:?} with a mirror"
                );
            }
        }
    }

    #[test]
    fn reified_factory_theory_gate() {
        // TH-4: a builtin name without arguments gives the identity of the builtin, and the
        // config of the space persists it.
        for name in BUILTIN_NAMES {
            let validated =
                validate(vec![free(name, Vec::new())]).expect("a builtin theory is valid");
            assert_eq!(
                validated.theory,
                Some(TheoryIdentity::builtin(name)),
                "{name}"
            );
            assert_eq!(
                validated.config(space()).theory,
                Some(TheoryIdentity::builtin(name)),
                "{name}"
            );
        }
        // A builtin with arguments fails with `BadConstructorArgument`.
        assert_eq!(
            validate(vec![free("Nat", vec![int(1)])]),
            Err(SpaceError::BadConstructorArgument(
                "the builtin theory Nat takes no arguments".to_string()
            ))
        );
        // Another name fails with `UnknownTheory` and the nearest builtin within two edits, with
        // or without arguments.
        for (name, args, suggested) in [
            ("Natt", Vec::new(), Some("Nat")),
            ("Strng", Vec::new(), Some("String")),
            ("Boll", vec![int(1)], Some("Bool")),
            ("Bounded", vec![int(1), int(2)], None),
            ("Th", Vec::new(), None),
        ] {
            assert_eq!(
                validate(vec![free(name, args)]),
                Err(SpaceError::UnknownTheory {
                    name: name.to_string(),
                    suggestion: suggested.map(str::to_string),
                }),
                "{name}"
            );
        }
    }
}
