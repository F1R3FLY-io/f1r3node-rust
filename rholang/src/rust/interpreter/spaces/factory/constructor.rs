//! The constructor of a factory call and the config of a new space (P4 design C.7, steps 2
//! and 5, and I-17).
//!
//! A factory accepts only the canonical constructor `[q?][FREE?][PARAMS?]` of
//! `SpaceFactoryConstructor`. [`FactoryConstructor::validate`] then applies the gates of this
//! phase in this order:
//!
//! 1. The family has a schema in this phase, else `FamilyUnavailable`.
//! 2. The constructor parses, else `BadConstructorArgument`.
//! 3. The family takes the qualifier, else `BadConstructorArgument`. The qualifier is
//!    `default`, else `QualifierUnavailable` (`temp` and `seq` arrive in P7).
//! 4. The constructor names no theory, else `TheoryUnavailable` (P7).
//! 5. The params are not positional, else `PositionalParamsUnsupported`.
//! 6. Each named param is in the schema of the family, else `UnknownParam` with a suggestion.
//!    The keys are checked in bytewise order.
//!
//! The config of the new space is a `SpaceConfig`. The agent stores its canonical map on
//! `C(sid)` once (I-17), and nothing changes the map later. [`family_of`] names the family of a
//! stored config.

use std::collections::BTreeMap;

use models::rhoapi::Par;
use models::rust::reified::encoding::SendParams;
use models::rust::space_factory_urn::{
    SpaceFactoryConstructor, SpaceFactoryUrn, SpaceInner, SpaceOuter, SpaceQualifier,
};

use super::defaults::{constructor_schema, suggestion};
use crate::rust::interpreter::spaces::errors::SpaceError;
use crate::rust::interpreter::spaces::namespace::{Durability, NativeSpace};
use crate::rust::interpreter::spaces::types::config::SpaceConfig;
use crate::rust::interpreter::spaces::types::qualifier::SpaceQualifier as ConfigQualifier;

/// The validated constructor of a factory call.
#[derive(Clone, Debug, PartialEq)]
pub struct FactoryConstructor {
    /// The family of the factory.
    pub family: SpaceFactoryUrn,
    /// The qualifier of the new space.
    pub qualifier: SpaceQualifier,
    /// The named params, which the schema of the family accepts.
    pub params: BTreeMap<String, Par>,
}

impl FactoryConstructor {
    /// Validates the constructor list `ctor` of a call to the factory of `family`.
    pub fn validate(family: SpaceFactoryUrn, ctor: &Par) -> Result<FactoryConstructor, SpaceError> {
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
        match qualifier {
            SpaceQualifier::Default => {}
            SpaceQualifier::Temp | SpaceQualifier::Seq => {
                return Err(SpaceError::QualifierUnavailable(
                    qualifier.as_str().to_string(),
                ))
            }
        }
        if let Some(theory) = theory {
            return Err(SpaceError::TheoryUnavailable(theory.name));
        }
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
        Ok(FactoryConstructor {
            family,
            qualifier,
            params: named,
        })
    }

    /// The durability of the new space: durable for `default`, ephemeral for `temp` and `seq`.
    pub const fn durability(&self) -> Durability {
        match self.qualifier {
            SpaceQualifier::Default => Durability::Durable,
            SpaceQualifier::Temp | SpaceQualifier::Seq => Durability::Ephemeral,
        }
    }

    /// The config of the new space `space` (I-17). Its theory is `Nil` until P7.
    pub fn config(&self, space: NativeSpace) -> SpaceConfig {
        SpaceConfig {
            space_id: space.sid,
            outer: self.family.outer.as_str().to_string(),
            inner: self.family.inner.as_str().to_string(),
            qualifier: config_qualifier(self.qualifier),
            params: self.params.clone(),
        }
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
    use models::rust::utils::{new_elist_par, new_gint_par, new_gstring_par};

    use super::*;
    use crate::rust::interpreter::spaces::namespace::SID_LEN;

    fn list(ps: Vec<Par>) -> Par { new_elist_par(ps, Vec::new(), false, None, Vec::new(), false) }

    fn int(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

    fn string(value: &str) -> Par { new_gstring_par(value.to_string(), Vec::new(), false) }

    fn nat() -> Par {
        encode_free(&FreeTheory {
            name: "Nat".to_string(),
            args: Vec::new(),
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
            });
            assert_eq!(validated.durability(), Durability::Durable);
        }
    }

    #[test]
    fn reified_factory_p4_gates_fail_closed_in_order() {
        assert_eq!(
            validate(vec![string("temp")]),
            Err(SpaceError::QualifierUnavailable("temp".to_string()))
        );
        assert_eq!(
            validate(vec![string("seq"), nat()]),
            Err(SpaceError::QualifierUnavailable("seq".to_string()))
        );
        assert_eq!(
            validate(vec![nat()]),
            Err(SpaceError::TheoryUnavailable("Nat".to_string()))
        );
        assert_eq!(
            validate(vec![nat(), params(vec![SendParam::Positional(int(1))])]),
            Err(SpaceError::TheoryUnavailable("Nat".to_string()))
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
        for family in SpaceFactoryUrn::all() {
            if family == SpaceFactoryUrn::HASHMAP_QUEUE {
                continue;
            }
            assert_eq!(
                FactoryConstructor::validate(family, &list(Vec::new())),
                Err(SpaceError::FamilyUnavailable {
                    outer: family.outer.as_str().to_string(),
                    inner: family.inner.as_str().to_string(),
                }),
                "{family}"
            );
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
}
