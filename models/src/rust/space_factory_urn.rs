//! The URNs of the Reified RSpaces factories and the constructor of a factory call (plan
//! decisions 7, 18 and 21, P4 design A.5 and C.7).
//!
//! A factory URN names an outer store and an inner collection, outer first:
//! `rho:space:<outer>:<inner>`. The 32 supported pairs are the space families. Family `i`
//! has the factory channel `[0xE0, i]` and the body ref `200 + i`. Two FIPS aliases name the
//! families `hashmap:bag` and `pathmap:bag`.
//!
//! | Outer     | Family indices | Inner collections                               |
//! |-----------|----------------|-------------------------------------------------|
//! | `hashmap` | 0 to 6         | bag, queue, stack, set, cell, priorityqueue, vectordb |
//! | `pathmap` | 7 to 12        | bag to priorityqueue                            |
//! | `array`   | 13 to 18       | bag to priorityqueue                            |
//! | `vector`  | 19 to 25       | bag to vectordb                                 |
//! | `hashset` | 26 to 31       | bag to priorityqueue, with the `seq` qualifier only |
//!
//! A factory call carries its constructor as one list. The list holds an optional qualifier
//! string, an optional FREE tuple and an optional PARAMS tuple, in this order. The FREE and
//! PARAMS tuples use the strict decoders of `reified::encoding`, so a constructor has one
//! canonical form.

use std::fmt;

use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::{EList, Expr, Par};
use crate::rust::reified::encoding::{
    decode_free_value, encode_free, encode_params, sole_expr, split_trailing_params, FreeTheory,
    ReifiedDecodeError, ReifiedEncodeError, SendParams,
};

/// The URN prefix of every space factory.
pub const SPACE_URN_PREFIX: &str = "rho:space:";

/// The FIPS alias of `rho:space:hashmap:bag`.
pub const HASHMAP_BAG_ALIAS: &str = "rho:space:HashMapBagSpace";

/// The FIPS alias of `rho:space:pathmap:bag`.
pub const PATHMAP_BAG_ALIAS: &str = "rho:space:PathMapSpace";

/// The number of space families. The family indices are `0..FACTORY_FAMILY_COUNT`.
pub const FACTORY_FAMILY_COUNT: u8 = 32;

/// The outer store of a space: the map from channels to collections.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SpaceOuter {
    HashMap,
    PathMap,
    Array,
    Vector,
    HashSet,
}

impl SpaceOuter {
    /// Every outer store, in family order.
    pub const ALL: [SpaceOuter; 5] = [
        SpaceOuter::HashMap,
        SpaceOuter::PathMap,
        SpaceOuter::Array,
        SpaceOuter::Vector,
        SpaceOuter::HashSet,
    ];

    /// The name of the outer store in a URN and in a space config.
    pub const fn as_str(self) -> &'static str {
        match self {
            SpaceOuter::HashMap => "hashmap",
            SpaceOuter::PathMap => "pathmap",
            SpaceOuter::Array => "array",
            SpaceOuter::Vector => "vector",
            SpaceOuter::HashSet => "hashset",
        }
    }

    /// The outer store with the name `value`.
    pub fn parse(value: &str) -> Option<SpaceOuter> {
        SpaceOuter::ALL
            .into_iter()
            .find(|outer| outer.as_str() == value)
    }

    /// The first family index of the outer store.
    const fn family_base(self) -> u8 {
        match self {
            SpaceOuter::HashMap => 0,
            SpaceOuter::PathMap => 7,
            SpaceOuter::Array => 13,
            SpaceOuter::Vector => 19,
            SpaceOuter::HashSet => 26,
        }
    }
}

/// The inner collection of a space: the discipline of each collection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SpaceInner {
    Bag,
    Queue,
    Stack,
    Set,
    Cell,
    PriorityQueue,
    VectorDb,
}

impl SpaceInner {
    /// Every inner collection, in family order.
    pub const ALL: [SpaceInner; 7] = [
        SpaceInner::Bag,
        SpaceInner::Queue,
        SpaceInner::Stack,
        SpaceInner::Set,
        SpaceInner::Cell,
        SpaceInner::PriorityQueue,
        SpaceInner::VectorDb,
    ];

    /// The name of the inner collection in a URN and in a space config.
    pub const fn as_str(self) -> &'static str {
        match self {
            SpaceInner::Bag => "bag",
            SpaceInner::Queue => "queue",
            SpaceInner::Stack => "stack",
            SpaceInner::Set => "set",
            SpaceInner::Cell => "cell",
            SpaceInner::PriorityQueue => "priorityqueue",
            SpaceInner::VectorDb => "vectordb",
        }
    }

    /// The inner collection with the name `value`.
    pub fn parse(value: &str) -> Option<SpaceInner> {
        SpaceInner::ALL
            .into_iter()
            .find(|inner| inner.as_str() == value)
    }

    /// The offset of the inner collection in the families of an outer store.
    const fn ordinal(self) -> u8 {
        match self {
            SpaceInner::Bag => 0,
            SpaceInner::Queue => 1,
            SpaceInner::Stack => 2,
            SpaceInner::Set => 3,
            SpaceInner::Cell => 4,
            SpaceInner::PriorityQueue => 5,
            SpaceInner::VectorDb => 6,
        }
    }
}

/// The qualifier of a space: its persistence and concurrency.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SpaceQualifier {
    /// Persistent, with concurrent access.
    #[default]
    Default,
    /// Live for one deploy, with concurrent access (P7).
    Temp,
    /// Live for one deploy, with sequential access only (P7).
    Seq,
}

impl SpaceQualifier {
    /// Every qualifier.
    pub const ALL: [SpaceQualifier; 3] = [
        SpaceQualifier::Default,
        SpaceQualifier::Temp,
        SpaceQualifier::Seq,
    ];

    /// The name of the qualifier in a constructor and in a space config.
    pub const fn as_str(self) -> &'static str {
        match self {
            SpaceQualifier::Default => "default",
            SpaceQualifier::Temp => "temp",
            SpaceQualifier::Seq => "seq",
        }
    }

    /// The qualifier with the name `value`.
    pub fn parse(value: &str) -> Option<SpaceQualifier> {
        SpaceQualifier::ALL
            .into_iter()
            .find(|qualifier| qualifier.as_str() == value)
    }
}

impl fmt::Display for SpaceQualifier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A space family: an outer store and an inner collection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SpaceFactoryUrn {
    pub outer: SpaceOuter,
    pub inner: SpaceInner,
}

/// A string that is not the URN of a space family.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SpaceFactoryUrnError {
    #[error("a space factory URN has the form rho:space:<outer>:<inner>")]
    InvalidForm,
    #[error("unknown outer space store: {0}")]
    UnknownOuter(String),
    #[error("unknown inner space collection: {0}")]
    UnknownInner(String),
    #[error("the outer store {outer} does not hold the inner collection {inner}")]
    UnsupportedPair {
        outer: &'static str,
        inner: &'static str,
    },
}

impl SpaceFactoryUrn {
    /// The family `rho:space:hashmap:queue`, the only family that P4 registers.
    pub const HASHMAP_QUEUE: SpaceFactoryUrn = SpaceFactoryUrn {
        outer: SpaceOuter::HashMap,
        inner: SpaceInner::Queue,
    };

    /// Parses an outer-first URN or a FIPS alias.
    pub fn parse(value: &str) -> Result<SpaceFactoryUrn, SpaceFactoryUrnError> {
        match value {
            HASHMAP_BAG_ALIAS => {
                return Ok(SpaceFactoryUrn {
                    outer: SpaceOuter::HashMap,
                    inner: SpaceInner::Bag,
                })
            }
            PATHMAP_BAG_ALIAS => {
                return Ok(SpaceFactoryUrn {
                    outer: SpaceOuter::PathMap,
                    inner: SpaceInner::Bag,
                })
            }
            _ => {}
        }
        let rest = value
            .strip_prefix(SPACE_URN_PREFIX)
            .ok_or(SpaceFactoryUrnError::InvalidForm)?;
        let mut parts = rest.split(':');
        let (Some(outer), Some(inner), None) = (parts.next(), parts.next(), parts.next()) else {
            return Err(SpaceFactoryUrnError::InvalidForm);
        };
        if outer.is_empty() || inner.is_empty() {
            return Err(SpaceFactoryUrnError::InvalidForm);
        }
        let outer = SpaceOuter::parse(outer)
            .ok_or_else(|| SpaceFactoryUrnError::UnknownOuter(outer.to_string()))?;
        let inner = SpaceInner::parse(inner)
            .ok_or_else(|| SpaceFactoryUrnError::UnknownInner(inner.to_string()))?;
        let family = SpaceFactoryUrn { outer, inner };
        match family.is_supported_pair() {
            true => Ok(family),
            false => Err(SpaceFactoryUrnError::UnsupportedPair {
                outer: outer.as_str(),
                inner: inner.as_str(),
            }),
        }
    }

    /// True when the outer store can hold the inner collection. The VectorDB collection needs
    /// the HashMap or the Vector store.
    pub const fn is_supported_pair(self) -> bool {
        match (self.outer, self.inner) {
            (SpaceOuter::HashMap | SpaceOuter::Vector, _) => true,
            (
                SpaceOuter::PathMap | SpaceOuter::Array | SpaceOuter::HashSet,
                SpaceInner::VectorDb,
            ) => false,
            (SpaceOuter::PathMap | SpaceOuter::Array | SpaceOuter::HashSet, _) => true,
        }
    }

    /// True when a space of this family can have the qualifier. The HashSet store takes the
    /// `seq` qualifier only.
    pub const fn supports_qualifier(self, qualifier: SpaceQualifier) -> bool {
        match (self.outer, qualifier) {
            (SpaceOuter::HashSet, SpaceQualifier::Seq) => true,
            (SpaceOuter::HashSet, SpaceQualifier::Default | SpaceQualifier::Temp) => false,
            (
                SpaceOuter::HashMap | SpaceOuter::PathMap | SpaceOuter::Array | SpaceOuter::Vector,
                _,
            ) => true,
        }
    }

    /// The family index, the op of the factory channel `[0xE0, index]`. An unsupported pair
    /// has none.
    pub const fn family_index(self) -> Option<u8> {
        match self.is_supported_pair() {
            true => Some(self.outer.family_base() + self.inner.ordinal()),
            false => None,
        }
    }

    /// The family with the index `index`.
    pub fn from_family_index(index: u8) -> Option<SpaceFactoryUrn> {
        SpaceFactoryUrn::all().find(|family| family.family_index() == Some(index))
    }

    /// The 32 families, in index order.
    pub fn all() -> impl Iterator<Item = SpaceFactoryUrn> {
        SpaceOuter::ALL.into_iter().flat_map(|outer| {
            SpaceInner::ALL
                .into_iter()
                .map(move |inner| SpaceFactoryUrn { outer, inner })
                .filter(|family| family.is_supported_pair())
        })
    }
}

impl fmt::Display for SpaceFactoryUrn {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{SPACE_URN_PREFIX}{}:{}",
            self.outer.as_str(),
            self.inner.as_str()
        )
    }
}

/// The constructor of a factory call: `[q?][FREE?][PARAMS?]` (P4 design C.7, step 2).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SpaceFactoryConstructor {
    /// The qualifier string, if the constructor names one.
    pub qualifier: Option<SpaceQualifier>,
    /// The theory of `free Th(a…)`, if the constructor names one.
    pub theory: Option<FreeTheory>,
    /// The params of the call, if the call has params.
    pub params: Option<SendParams>,
}

/// A constructor that is not the canonical sequence `[q?][FREE?][PARAMS?]`.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SpaceFactoryConstructorError {
    #[error("the constructor is not a list")]
    NotAList,
    #[error("unknown space qualifier: {0}")]
    UnknownQualifier(String),
    #[error("the theory is malformed: {0}")]
    MalformedTheory(ReifiedDecodeError),
    #[error("the params are malformed: {0}")]
    MalformedParams(ReifiedDecodeError),
    #[error("the constructor is not [qualifier?][free Th(…)?][params?]")]
    NonCanonical,
}

impl SpaceFactoryConstructor {
    /// Parses the constructor list of a factory call. The list has no remainder.
    pub fn parse(ctor: &Par) -> Result<SpaceFactoryConstructor, SpaceFactoryConstructorError> {
        match sole_expr(ctor) {
            Some(ExprInstance::EListBody(EList {
                ps,
                remainder: None,
                ..
            })) => SpaceFactoryConstructor::parse_elements(ps),
            _ => Err(SpaceFactoryConstructorError::NotAList),
        }
    }

    /// Parses the elements of a constructor list. The PARAMS tuple is the last element, the
    /// FREE tuple precedes it, and the qualifier string comes first.
    pub fn parse_elements(
        elements: &[Par],
    ) -> Result<SpaceFactoryConstructor, SpaceFactoryConstructorError> {
        let params = split_trailing_params(elements)
            .map_err(SpaceFactoryConstructorError::MalformedParams)?;
        let rest = match params {
            Some(_) => elements.split_last().map_or(elements, |(_, init)| init),
            None => elements,
        };
        let (theory, rest) = match rest.split_last() {
            Some((last, init)) => {
                match decode_free_value(last)
                    .map_err(SpaceFactoryConstructorError::MalformedTheory)?
                {
                    Some(theory) => (Some(theory), init),
                    None => (None, rest),
                }
            }
            None => (None, rest),
        };
        let qualifier = match rest {
            [] => None,
            [value] => Some(qualifier_of(value)?),
            _ => return Err(SpaceFactoryConstructorError::NonCanonical),
        };
        Ok(SpaceFactoryConstructor {
            qualifier,
            theory,
            params,
        })
    }

    /// The qualifier of the space: the named one, or `default`.
    pub fn effective_qualifier(&self) -> SpaceQualifier { self.qualifier.unwrap_or_default() }

    /// The canonical elements of the constructor, the inverse of
    /// [`SpaceFactoryConstructor::parse_elements`]. Empty params have no element.
    pub fn to_elements(&self) -> Result<Vec<Par>, ReifiedEncodeError> {
        let mut elements = Vec::with_capacity(3);
        if let Some(qualifier) = self.qualifier {
            elements.push(string_par(qualifier.as_str()));
        }
        if let Some(theory) = &self.theory {
            elements.push(encode_free(theory)?);
        }
        if let Some(params) = &self.params {
            elements.extend(encode_params(params)?);
        }
        Ok(elements)
    }
}

fn string_par(value: &str) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GString(value.to_owned())),
    }])
}

/// The qualifier of a constructor element: exactly one string that names a qualifier. A string
/// has no free variables, so an element with `locally_free` bits is not canonical.
fn qualifier_of(value: &Par) -> Result<SpaceQualifier, SpaceFactoryConstructorError> {
    match sole_expr(value) {
        Some(ExprInstance::GString(name))
            if !value.connective_used && value.locally_free.is_empty() =>
        {
            SpaceQualifier::parse(name)
                .ok_or_else(|| SpaceFactoryConstructorError::UnknownQualifier(name.clone()))
        }
        _ => Err(SpaceFactoryConstructorError::NonCanonical),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::rust::reified::builders;
    use crate::rust::reified::encoding::Marker;
    use crate::rust::reified::params::SendParam;
    use crate::rust::utils::{new_elist_par, new_gint_par, new_gstring_par};

    fn string(value: &str) -> Par { new_gstring_par(value.to_string(), Vec::new(), false) }

    fn int(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

    fn list(ps: Vec<Par>) -> Par { new_elist_par(ps, Vec::new(), false, None, Vec::new(), false) }

    fn theory(name: &str, args: Vec<Par>) -> Par {
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

    #[test]
    fn space_urn_factory_outer_first_and_aliases() {
        for (input, expected) in [
            ("rho:space:hashmap:queue", "rho:space:hashmap:queue"),
            ("rho:space:hashmap:bag", "rho:space:hashmap:bag"),
            ("rho:space:pathmap:bag", "rho:space:pathmap:bag"),
            ("rho:space:HashMapBagSpace", "rho:space:hashmap:bag"),
            ("rho:space:PathMapSpace", "rho:space:pathmap:bag"),
            ("rho:space:vector:vectordb", "rho:space:vector:vectordb"),
            ("rho:space:hashset:set", "rho:space:hashset:set"),
            ("rho:space:hashset:bag", "rho:space:hashset:bag"),
            (
                "rho:space:array:priorityqueue",
                "rho:space:array:priorityqueue",
            ),
        ] {
            let parsed = SpaceFactoryUrn::parse(input).expect("a valid space URN");
            assert_eq!(parsed.to_string(), expected, "{input}");
        }
        assert_eq!(
            SpaceFactoryUrn::parse("rho:space:hashmap:queue"),
            Ok(SpaceFactoryUrn::HASHMAP_QUEUE)
        );
        assert_eq!(
            SpaceFactoryUrn::parse(HASHMAP_BAG_ALIAS)
                .expect("an alias")
                .family_index(),
            SpaceFactoryUrn::parse("rho:space:hashmap:bag")
                .expect("a family")
                .family_index()
        );
        assert_eq!(
            SpaceFactoryUrn::parse(PATHMAP_BAG_ALIAS)
                .expect("an alias")
                .family_index(),
            Some(7)
        );
    }

    #[test]
    fn space_urn_factory_rejects_invalid_forms_and_pairs() {
        for (input, expected) in [
            (
                "rho:space:bag:pathmap",
                SpaceFactoryUrnError::UnknownOuter("bag".to_string()),
            ),
            (
                "rho:space:bag:hashmap:default",
                SpaceFactoryUrnError::InvalidForm,
            ),
            (
                "rho:space:pathmap:bag:temp",
                SpaceFactoryUrnError::InvalidForm,
            ),
            (
                "rho:space:pathmap:bag[theory=Nat]",
                SpaceFactoryUrnError::UnknownInner("bag[theory=Nat]".to_string()),
            ),
            (
                "rho:space:PathMapSpaceExtra",
                SpaceFactoryUrnError::InvalidForm,
            ),
            ("rho:space:QueueSpace", SpaceFactoryUrnError::InvalidForm),
            (
                "rho:space:pathmap:vectordb",
                SpaceFactoryUrnError::UnsupportedPair {
                    outer: "pathmap",
                    inner: "vectordb",
                },
            ),
            (
                "rho:space:array:vectordb",
                SpaceFactoryUrnError::UnsupportedPair {
                    outer: "array",
                    inner: "vectordb",
                },
            ),
            (
                "rho:space:hashset:vectordb",
                SpaceFactoryUrnError::UnsupportedPair {
                    outer: "hashset",
                    inner: "vectordb",
                },
            ),
            ("rho:space::bag", SpaceFactoryUrnError::InvalidForm),
            ("rho:space:hashmap:", SpaceFactoryUrnError::InvalidForm),
            ("rho:space:metta", SpaceFactoryUrnError::InvalidForm),
            ("space:pathmap:bag", SpaceFactoryUrnError::InvalidForm),
            (
                "rho:space:HashMap:Queue",
                SpaceFactoryUrnError::UnknownOuter("HashMap".to_string()),
            ),
            ("", SpaceFactoryUrnError::InvalidForm),
        ] {
            assert_eq!(SpaceFactoryUrn::parse(input), Err(expected), "{input}");
        }
    }

    #[test]
    fn space_urn_factory_constructor_qualifier_theory_params() {
        assert_eq!(
            SpaceFactoryConstructor::parse(&list(Vec::new())),
            Ok(SpaceFactoryConstructor::default())
        );
        let nat = theory("Nat", vec![int(3)]);
        let named = params(vec![SendParam::Named("size".to_string(), int(4))]);
        let positional = params(vec![SendParam::Positional(int(5))]);
        let elements = vec![string("temp"), nat.clone(), named.clone()];
        let parsed = SpaceFactoryConstructor::parse(&list(elements.clone()))
            .expect("the canonical sequence parses");
        assert_eq!(parsed.qualifier, Some(SpaceQualifier::Temp));
        assert_eq!(parsed.effective_qualifier(), SpaceQualifier::Temp);
        assert_eq!(
            parsed.theory,
            Some(FreeTheory {
                name: "Nat".to_string(),
                args: vec![int(3)],
            })
        );
        assert_eq!(
            parsed.params,
            Some(SendParams {
                positional: Vec::new(),
                named: BTreeMap::from([("size".to_string(), int(4))]),
            })
        );
        assert_eq!(parsed.to_elements(), Ok(elements));
        for elements in [
            vec![string("default")],
            vec![nat.clone()],
            vec![named.clone()],
            vec![positional.clone()],
            vec![string("seq"), named.clone()],
            vec![nat.clone(), positional],
            vec![string("default"), nat],
        ] {
            let parsed = SpaceFactoryConstructor::parse_elements(&elements)
                .expect("a canonical constructor parses");
            assert_eq!(parsed.to_elements(), Ok(elements));
        }
        let only_named = SpaceFactoryConstructor::parse_elements(&[named])
            .expect("params without a qualifier parse");
        assert_eq!(only_named.qualifier, None);
        assert_eq!(only_named.effective_qualifier(), SpaceQualifier::Default);
        assert_eq!(only_named.theory, None);
        let empty_params = SpaceFactoryConstructor {
            params: Some(SendParams::default()),
            ..SpaceFactoryConstructor::default()
        };
        assert_eq!(empty_params.to_elements(), Ok(Vec::new()));
    }

    #[test]
    fn space_urn_factory_rejects_invalid_constructors() {
        let nat = theory("Nat", Vec::new());
        let named = params(vec![SendParam::Named("size".to_string(), int(4))]);
        for elements in [
            vec![nat.clone(), string("default")],
            vec![string("default"), string("default")],
            vec![string("default"), nat.clone(), nat.clone()],
            vec![named.clone(), nat.clone()],
            vec![named.clone(), named.clone()],
            vec![string("default"), named.clone(), nat.clone()],
            vec![int(1)],
            vec![list(vec![string("default")])],
            vec![Marker::Use.par()],
        ] {
            assert_eq!(
                SpaceFactoryConstructor::parse_elements(&elements),
                Err(SpaceFactoryConstructorError::NonCanonical),
                "{elements:?}"
            );
        }
        assert_eq!(
            SpaceFactoryConstructor::parse_elements(&[string("unknown")]),
            Err(SpaceFactoryConstructorError::UnknownQualifier(
                "unknown".to_string()
            ))
        );
        let mut compound = string("default");
        compound.exprs.push(Expr {
            expr_instance: Some(ExprInstance::GInt(1)),
        });
        assert_eq!(
            SpaceFactoryConstructor::parse_elements(&[compound]),
            Err(SpaceFactoryConstructorError::NonCanonical)
        );
        let mut flagged = string("default");
        flagged.connective_used = true;
        assert_eq!(
            SpaceFactoryConstructor::parse_elements(&[flagged]),
            Err(SpaceFactoryConstructorError::NonCanonical)
        );
        let mut stale_theory = nat;
        stale_theory.connective_used = true;
        assert!(matches!(
            SpaceFactoryConstructor::parse_elements(&[stale_theory]),
            Err(SpaceFactoryConstructorError::MalformedTheory(_))
        ));
        let mut stale_params = named;
        stale_params.connective_used = true;
        assert!(matches!(
            SpaceFactoryConstructor::parse_elements(&[stale_params]),
            Err(SpaceFactoryConstructorError::MalformedParams(_))
        ));
        for ctor in [
            string("default"),
            Par::default(),
            new_elist_par(
                Vec::new(),
                Vec::new(),
                false,
                Some(Default::default()),
                Vec::new(),
                false,
            ),
        ] {
            assert_eq!(
                SpaceFactoryConstructor::parse(&ctor),
                Err(SpaceFactoryConstructorError::NotAList)
            );
        }
    }

    #[test]
    fn space_urn_factory_rejects_qualifier_bits() {
        let nat = theory("Nat", vec![int(3)]);
        let named = params(vec![SendParam::Named("size".to_string(), int(4))]);
        let both = vec![nat.clone(), named.clone()];
        let tails = [Vec::new(), vec![nat], vec![named], both];
        for qualifier in SpaceQualifier::ALL {
            let clean = string(qualifier.as_str());
            let stray = Par {
                locally_free: vec![0b0000_0001],
                ..clean.clone()
            };
            for tail in &tails {
                let mut with_bits = Vec::with_capacity(tail.len() + 1);
                with_bits.push(stray.clone());
                with_bits.extend_from_slice(tail);
                assert_eq!(
                    SpaceFactoryConstructor::parse_elements(&with_bits),
                    Err(SpaceFactoryConstructorError::NonCanonical),
                    "{qualifier} {}",
                    tail.len()
                );
                assert_eq!(
                    SpaceFactoryConstructor::parse(&list(with_bits)),
                    Err(SpaceFactoryConstructorError::NonCanonical),
                    "{qualifier} {}",
                    tail.len()
                );
                let mut twin = Vec::with_capacity(tail.len() + 1);
                twin.push(clean.clone());
                twin.extend_from_slice(tail);
                let parsed =
                    SpaceFactoryConstructor::parse_elements(&twin).expect("the clean twin parses");
                assert_eq!(parsed.qualifier, Some(qualifier));
                let canonical = parsed.to_elements().expect("a parsed constructor encodes");
                assert_eq!(canonical, twin);
                assert!(canonical
                    .first()
                    .is_some_and(|element| element.locally_free.is_empty()));
            }
        }
        let unknown = Par {
            locally_free: vec![0b0000_0001],
            ..string("unknown")
        };
        assert_eq!(
            SpaceFactoryConstructor::parse_elements(&[unknown]),
            Err(SpaceFactoryConstructorError::NonCanonical)
        );
    }

    #[test]
    fn space_urn_factory_qualifier_support_matrix() {
        for family in SpaceFactoryUrn::all() {
            for qualifier in SpaceQualifier::ALL {
                let expected = match family.outer {
                    SpaceOuter::HashSet => qualifier == SpaceQualifier::Seq,
                    _ => true,
                };
                assert_eq!(
                    family.supports_qualifier(qualifier),
                    expected,
                    "{family} {qualifier}"
                );
            }
        }
        for qualifier in SpaceQualifier::ALL {
            assert_eq!(SpaceQualifier::parse(qualifier.as_str()), Some(qualifier));
            assert_eq!(qualifier.to_string(), qualifier.as_str());
        }
        assert_eq!(SpaceQualifier::parse("Default"), None);
        assert_eq!(SpaceQualifier::default(), SpaceQualifier::Default);
    }

    #[test]
    fn space_urn_factory_family_indices_stable_and_unique() {
        let mut seen = [false; FACTORY_FAMILY_COUNT as usize];
        let mut supported = 0;
        for outer in SpaceOuter::ALL {
            for inner in SpaceInner::ALL {
                let family = SpaceFactoryUrn { outer, inner };
                match family.family_index() {
                    Some(index) => {
                        let slot = seen
                            .get_mut(usize::from(index))
                            .expect("an index below the family count");
                        assert!(!*slot, "{family}");
                        *slot = true;
                        supported += 1;
                        assert_eq!(SpaceFactoryUrn::parse(&family.to_string()), Ok(family));
                        assert_eq!(SpaceFactoryUrn::from_family_index(index), Some(family));
                    }
                    None => assert!(!family.is_supported_pair(), "{family}"),
                }
            }
        }
        assert!(seen.into_iter().all(|used| used));
        assert_eq!(supported, usize::from(FACTORY_FAMILY_COUNT));
        assert_eq!(
            SpaceFactoryUrn::all()
                .map(|family| family.family_index())
                .collect::<Vec<_>>(),
            (0..FACTORY_FAMILY_COUNT).map(Some).collect::<Vec<_>>()
        );
        assert_eq!(SpaceFactoryUrn::HASHMAP_QUEUE.family_index(), Some(1));
        assert_eq!(
            SpaceFactoryUrn::parse("rho:space:hashset:set")
                .expect("a family")
                .family_index(),
            Some(29)
        );
        assert_eq!(
            SpaceFactoryUrn::from_family_index(FACTORY_FAMILY_COUNT),
            None
        );
        for outer in SpaceOuter::ALL {
            assert_eq!(SpaceOuter::parse(outer.as_str()), Some(outer));
        }
        for inner in SpaceInner::ALL {
            assert_eq!(SpaceInner::parse(inner.as_str()), Some(inner));
        }
    }
}
