// P4 integration tests of the space factory URNs and of the factory constructor (P4 design A.5
// and C.7, plan decisions 7, 18 and 21).
//
// The unit tests of `models/src/rust/space_factory_urn.rs` check single cases. These tests use
// only the public API of the crate, as the runtime does. They check every outer store against
// every inner collection, every family index against a pinned table, and a matrix of canonical
// constructors that the P2 builders make.

#[path = "support/reified_strategies.rs"]
mod reified_strategies;

use std::collections::{BTreeMap, BTreeSet};

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, Par, Var};
use models::rust::reified::builders;
use models::rust::reified::encoding::{encode_free, FreeTheory, Marker, SendParams};
use models::rust::reified::params::SendParam;
use models::rust::space_factory_urn::{
    SpaceFactoryConstructor, SpaceFactoryConstructorError, SpaceFactoryUrn, SpaceFactoryUrnError,
    SpaceInner, SpaceOuter, SpaceQualifier, FACTORY_FAMILY_COUNT, HASHMAP_BAG_ALIAS,
    PATHMAP_BAG_ALIAS, SPACE_URN_PREFIX,
};
use reified_strategies::{expr_par, gint, gstring, list, tuple};

/// The 32 space families in the order of their family indices (P4 design A.5). The factory of
/// family `i` listens on the op channel `[0xE0, i]`, so this table pins the channel of every
/// family.
const FAMILY_TABLE: [&str; 32] = [
    "rho:space:hashmap:bag",
    "rho:space:hashmap:queue",
    "rho:space:hashmap:stack",
    "rho:space:hashmap:set",
    "rho:space:hashmap:cell",
    "rho:space:hashmap:priorityqueue",
    "rho:space:hashmap:vectordb",
    "rho:space:pathmap:bag",
    "rho:space:pathmap:queue",
    "rho:space:pathmap:stack",
    "rho:space:pathmap:set",
    "rho:space:pathmap:cell",
    "rho:space:pathmap:priorityqueue",
    "rho:space:array:bag",
    "rho:space:array:queue",
    "rho:space:array:stack",
    "rho:space:array:set",
    "rho:space:array:cell",
    "rho:space:array:priorityqueue",
    "rho:space:vector:bag",
    "rho:space:vector:queue",
    "rho:space:vector:stack",
    "rho:space:vector:set",
    "rho:space:vector:cell",
    "rho:space:vector:priorityqueue",
    "rho:space:vector:vectordb",
    "rho:space:hashset:bag",
    "rho:space:hashset:queue",
    "rho:space:hashset:stack",
    "rho:space:hashset:set",
    "rho:space:hashset:cell",
    "rho:space:hashset:priorityqueue",
];

/// The last op of the factory channels `[0xE0, 0x00]` to `[0xE0, 0x1F]`.
const FACTORY_OP_LAST: u8 = 0x1F;

fn family(urn: &str) -> SpaceFactoryUrn {
    SpaceFactoryUrn::parse(urn).unwrap_or_else(|error| panic!("{urn} is a family: {error}"))
}

fn named(key: &str, value: Par) -> SendParam { SendParam::Named(key.to_owned(), value) }

/// The PARAMS tuple of `params`, as the normalizer builds it for a params section.
fn params_tuple(params: Vec<SendParam>) -> Par {
    builders::send_params(params)
        .expect("the params are valid")
        .expect("the params are not empty")
}

/// The FREE tuple of `free name(args…)`.
fn theory(name: &str, args: Vec<Par>) -> Par {
    encode_free(&FreeTheory {
        name: name.to_owned(),
        args,
    })
    .expect("the theory is valid")
}

/// A list `[ps…...@rest]` with a remainder variable.
fn list_with_remainder(ps: Vec<Par>) -> Par {
    expr_par(
        ExprInstance::EListBody(EList {
            ps,
            locally_free: Vec::new(),
            connective_used: true,
            remainder: Some(Var {
                var_instance: Some(models::rhoapi::var::VarInstance::FreeVar(0)),
            }),
        }),
        Vec::new(),
        true,
    )
}

#[test]
fn space_urn_factory_outer_first_and_aliases() {
    for (index, urn) in FAMILY_TABLE.into_iter().enumerate() {
        let parsed = family(urn);
        assert_eq!(
            parsed.to_string(),
            urn,
            "the display of a family is its URN"
        );
        assert!(urn.starts_with(SPACE_URN_PREFIX), "{urn}");
        let segments: Vec<&str> = urn
            .strip_prefix(SPACE_URN_PREFIX)
            .expect("every URN has the space prefix")
            .split(':')
            .collect();
        assert_eq!(
            segments,
            vec![parsed.outer.as_str(), parsed.inner.as_str()],
            "the outer store comes first in {urn}"
        );
        assert_eq!(parsed.family_index().map(usize::from), Some(index), "{urn}");
        // The inner-first spelling names an inner collection where the outer store must be.
        let inner_first = format!(
            "{SPACE_URN_PREFIX}{}:{}",
            parsed.inner.as_str(),
            parsed.outer.as_str()
        );
        assert_eq!(
            SpaceFactoryUrn::parse(&inner_first),
            Err(SpaceFactoryUrnError::UnknownOuter(
                parsed.inner.as_str().to_owned()
            )),
            "{inner_first}"
        );
    }
    for (alias, canonical, index) in [
        (HASHMAP_BAG_ALIAS, "rho:space:hashmap:bag", 0),
        (PATHMAP_BAG_ALIAS, "rho:space:pathmap:bag", 7),
    ] {
        let parsed = family(alias);
        assert_eq!(parsed, family(canonical), "{alias} names {canonical}");
        assert_eq!(
            parsed.to_string(),
            canonical,
            "an alias displays as its outer-first URN"
        );
        assert_eq!(parsed.family_index(), Some(index), "{alias}");
        assert_eq!(family(&parsed.to_string()), parsed, "{alias}");
    }
    assert_eq!(
        family("rho:space:hashmap:queue"),
        SpaceFactoryUrn::HASHMAP_QUEUE
    );
    assert_eq!(SpaceFactoryUrn::HASHMAP_QUEUE.family_index(), Some(1));
    assert_eq!(
        SpaceFactoryUrn::HASHMAP_QUEUE.to_string(),
        "rho:space:hashmap:queue"
    );
}

#[test]
fn space_urn_factory_rejects_invalid_forms_and_pairs() {
    let mut unsupported = Vec::new();
    for outer in SpaceOuter::ALL {
        for inner in SpaceInner::ALL {
            let urn = format!("{SPACE_URN_PREFIX}{}:{}", outer.as_str(), inner.as_str());
            let candidate = SpaceFactoryUrn { outer, inner };
            match (SpaceFactoryUrn::parse(&urn), candidate.is_supported_pair()) {
                (Ok(parsed), true) => assert_eq!(parsed, candidate, "{urn}"),
                (Err(error), false) => {
                    assert_eq!(
                        error,
                        SpaceFactoryUrnError::UnsupportedPair {
                            outer: outer.as_str(),
                            inner: inner.as_str(),
                        },
                        "{urn}"
                    );
                    assert_eq!(candidate.family_index(), None, "{urn}");
                    unsupported.push(urn);
                }
                (result, supported) => {
                    panic!("{urn}: parse gave {result:?}, but the pair support is {supported}")
                }
            }
        }
    }
    assert_eq!(
        unsupported,
        vec![
            "rho:space:pathmap:vectordb".to_owned(),
            "rho:space:array:vectordb".to_owned(),
            "rho:space:hashset:vectordb".to_owned(),
        ],
        "only the VectorDB collection outside the HashMap and Vector stores is unsupported"
    );
    for (input, expected) in [
        ("", SpaceFactoryUrnError::InvalidForm),
        ("rho:space:", SpaceFactoryUrnError::InvalidForm),
        ("rho:space:hashmap", SpaceFactoryUrnError::InvalidForm),
        ("rho:space:hashmap:", SpaceFactoryUrnError::InvalidForm),
        ("rho:space::queue", SpaceFactoryUrnError::InvalidForm),
        (
            "rho:space:hashmap:queue:default",
            SpaceFactoryUrnError::InvalidForm,
        ),
        (
            "rho:space:hashmap:queue:",
            SpaceFactoryUrnError::InvalidForm,
        ),
        ("space:hashmap:queue", SpaceFactoryUrnError::InvalidForm),
        (
            "rho:spaces:hashmap:queue",
            SpaceFactoryUrnError::InvalidForm,
        ),
        (
            "rho:space:HashMapBagSpace2",
            SpaceFactoryUrnError::InvalidForm,
        ),
        ("rho:space:QueueSpace", SpaceFactoryUrnError::InvalidForm),
        ("rho:space:metta", SpaceFactoryUrnError::InvalidForm),
        (
            "rho:space:HashMap:queue",
            SpaceFactoryUrnError::UnknownOuter("HashMap".to_owned()),
        ),
        (
            "rho:space: hashmap:queue",
            SpaceFactoryUrnError::UnknownOuter(" hashmap".to_owned()),
        ),
        (
            "rho:space:hashmap:Queue",
            SpaceFactoryUrnError::UnknownInner("Queue".to_owned()),
        ),
        (
            "rho:space:hashmap:queue[theory=Nat]",
            SpaceFactoryUrnError::UnknownInner("queue[theory=Nat]".to_owned()),
        ),
        (
            "rho:space:queue:hashmap",
            SpaceFactoryUrnError::UnknownOuter("queue".to_owned()),
        ),
    ] {
        assert_eq!(SpaceFactoryUrn::parse(input), Err(expected), "{input:?}");
    }
    assert!(
        FAMILY_TABLE
            .iter()
            .all(|urn| !unsupported.iter().any(|bad| bad == urn)),
        "no unsupported pair has a family index"
    );
}

#[test]
fn space_urn_factory_constructor_qualifier_theory_params() {
    let qualifiers: [Option<SpaceQualifier>; 4] = [
        None,
        Some(SpaceQualifier::Default),
        Some(SpaceQualifier::Temp),
        Some(SpaceQualifier::Seq),
    ];
    let theories: [Option<FreeTheory>; 3] = [
        None,
        Some(FreeTheory {
            name: "Nat".to_owned(),
            args: Vec::new(),
        }),
        Some(FreeTheory {
            name: "Bounded".to_owned(),
            args: vec![gint(3), gstring("three")],
        }),
    ];
    let params: [Option<SendParams>; 4] = [
        None,
        Some(SendParams {
            positional: Vec::new(),
            named: BTreeMap::from([("capacity".to_owned(), gint(4))]),
        }),
        Some(SendParams {
            positional: vec![gint(5)],
            named: BTreeMap::new(),
        }),
        Some(SendParams {
            positional: vec![gint(5)],
            named: BTreeMap::from([
                ("capacity".to_owned(), gint(4)),
                ("label".to_owned(), gstring("box")),
            ]),
        }),
    ];
    let mut forms = BTreeSet::new();
    for qualifier in qualifiers {
        for theory in &theories {
            for param in &params {
                let constructor = SpaceFactoryConstructor {
                    qualifier,
                    theory: theory.clone(),
                    params: param.clone(),
                };
                let elements = constructor
                    .to_elements()
                    .expect("a canonical constructor encodes");
                let expected_length = usize::from(qualifier.is_some())
                    + usize::from(theory.is_some())
                    + usize::from(param.is_some());
                assert_eq!(elements.len(), expected_length, "{constructor:?}");
                assert_eq!(
                    SpaceFactoryConstructor::parse_elements(&elements),
                    Ok(constructor.clone()),
                    "the elements parse back: {constructor:?}"
                );
                assert_eq!(
                    SpaceFactoryConstructor::parse(&list(elements.clone())),
                    Ok(constructor.clone()),
                    "the constructor list parses back: {constructor:?}"
                );
                assert_eq!(
                    constructor.effective_qualifier(),
                    qualifier.unwrap_or(SpaceQualifier::Default),
                    "{constructor:?}"
                );
                assert!(
                    forms.insert(format!("{elements:?}")),
                    "two constructors encode alike: {constructor:?}"
                );
            }
        }
    }
    assert_eq!(
        forms.len(),
        4 * 3 * 4,
        "every constructor has its own encoding"
    );
    // The tuples of the P2 builders are the canonical constructor elements.
    let built = vec![
        gstring("default"),
        theory("Nat", Vec::new()),
        params_tuple(vec![named("capacity", gint(4))]),
    ];
    assert_eq!(
        SpaceFactoryConstructor::parse(&list(built.clone())),
        Ok(SpaceFactoryConstructor {
            qualifier: Some(SpaceQualifier::Default),
            theory: Some(FreeTheory {
                name: "Nat".to_owned(),
                args: Vec::new(),
            }),
            params: Some(SendParams {
                positional: Vec::new(),
                named: BTreeMap::from([("capacity".to_owned(), gint(4))]),
            }),
        })
    );
    // Empty params have no element, so a send with an empty params section makes the
    // constructor of a send without one.
    let empty = SpaceFactoryConstructor {
        params: Some(SendParams::default()),
        ..SpaceFactoryConstructor::default()
    };
    assert_eq!(empty.to_elements(), Ok(Vec::new()));
    assert_eq!(
        SpaceFactoryConstructor::parse(&list(Vec::new())),
        Ok(SpaceFactoryConstructor::default())
    );
}

#[test]
fn space_urn_factory_rejects_invalid_constructors() {
    let qualifier = gstring("default");
    let nat = theory("Nat", Vec::new());
    let capacity = params_tuple(vec![named("capacity", gint(4))]);
    for elements in [
        vec![nat.clone(), qualifier.clone()],
        vec![capacity.clone(), qualifier.clone()],
        vec![capacity.clone(), nat.clone()],
        vec![qualifier.clone(), qualifier.clone()],
        vec![nat.clone(), nat.clone()],
        vec![qualifier.clone(), nat.clone(), nat.clone()],
        vec![qualifier.clone(), capacity.clone(), nat.clone()],
        vec![qualifier.clone(), nat.clone(), capacity.clone(), gint(1)],
        vec![gint(1)],
        vec![gstring("default"), gint(1)],
        vec![list(vec![qualifier.clone()])],
        vec![tuple(vec![qualifier.clone()])],
        vec![Marker::Use.par()],
        vec![Marker::Params.par()],
    ] {
        assert_eq!(
            SpaceFactoryConstructor::parse_elements(&elements),
            Err(SpaceFactoryConstructorError::NonCanonical),
            "{elements:?}"
        );
    }
    for name in ["Default", "DEFAULT", "persistent", "", "temp "] {
        assert_eq!(
            SpaceFactoryConstructor::parse_elements(&[gstring(name)]),
            Err(SpaceFactoryConstructorError::UnknownQualifier(
                name.to_owned()
            )),
            "{name:?}"
        );
    }
    let mut compound = gstring("default");
    compound.exprs.extend(gint(1).exprs);
    let mut flagged = gstring("default");
    flagged.connective_used = true;
    for element in [compound, flagged] {
        assert_eq!(
            SpaceFactoryConstructor::parse_elements(&[element.clone()]),
            Err(SpaceFactoryConstructorError::NonCanonical),
            "{element:?}"
        );
    }
    let mut stale_theory = nat;
    stale_theory.connective_used = true;
    assert!(matches!(
        SpaceFactoryConstructor::parse_elements(&[qualifier.clone(), stale_theory]),
        Err(SpaceFactoryConstructorError::MalformedTheory(_))
    ));
    let mut stale_params = capacity;
    stale_params.connective_used = true;
    assert!(matches!(
        SpaceFactoryConstructor::parse_elements(&[qualifier.clone(), stale_params]),
        Err(SpaceFactoryConstructorError::MalformedParams(_))
    ));
    for ctor in [
        qualifier.clone(),
        Par::default(),
        gint(0),
        tuple(vec![qualifier.clone()]),
        list_with_remainder(vec![qualifier.clone()]),
        {
            let mut two = list(vec![qualifier.clone()]);
            two.exprs.extend(list(Vec::new()).exprs);
            two
        },
    ] {
        assert_eq!(
            SpaceFactoryConstructor::parse(&ctor),
            Err(SpaceFactoryConstructorError::NotAList),
            "{ctor:?}"
        );
    }
}

#[test]
fn space_urn_factory_qualifier_support_matrix() {
    let mut supported = 0;
    for urn in FAMILY_TABLE {
        let parsed = family(urn);
        for qualifier in SpaceQualifier::ALL {
            let expected = match parsed.outer {
                SpaceOuter::HashSet => qualifier == SpaceQualifier::Seq,
                SpaceOuter::HashMap
                | SpaceOuter::PathMap
                | SpaceOuter::Array
                | SpaceOuter::Vector => true,
            };
            assert_eq!(
                parsed.supports_qualifier(qualifier),
                expected,
                "{urn} with the {qualifier} qualifier"
            );
            supported += usize::from(expected);
        }
    }
    assert_eq!(
        supported,
        26 * 3 + 6,
        "26 families take every qualifier and the 6 HashSet families take seq only"
    );
    let names: Vec<&str> = SpaceQualifier::ALL.iter().map(|q| q.as_str()).collect();
    assert_eq!(names, vec!["default", "temp", "seq"]);
    for qualifier in SpaceQualifier::ALL {
        assert_eq!(SpaceQualifier::parse(qualifier.as_str()), Some(qualifier));
        assert_eq!(qualifier.to_string(), qualifier.as_str());
        assert_eq!(
            SpaceQualifier::parse(&qualifier.as_str().to_uppercase()),
            None,
            "qualifier names are lowercase only"
        );
    }
    assert_eq!(SpaceQualifier::default(), SpaceQualifier::Default);
    for outer in SpaceOuter::ALL {
        assert_eq!(SpaceOuter::parse(outer.as_str()), Some(outer));
    }
    for inner in SpaceInner::ALL {
        assert_eq!(SpaceInner::parse(inner.as_str()), Some(inner));
    }
    assert_eq!(SpaceOuter::parse("bag"), None);
    assert_eq!(SpaceInner::parse("hashmap"), None);
}

#[test]
fn space_urn_factory_family_indices_stable_and_unique() {
    assert_eq!(usize::from(FACTORY_FAMILY_COUNT), FAMILY_TABLE.len());
    assert_eq!(
        FACTORY_FAMILY_COUNT,
        FACTORY_OP_LAST + 1,
        "the families fill the factory op range exactly"
    );
    let all: Vec<SpaceFactoryUrn> = SpaceFactoryUrn::all().collect();
    let pinned: Vec<SpaceFactoryUrn> = FAMILY_TABLE.into_iter().map(family).collect();
    assert_eq!(all, pinned, "all() lists the families in index order");
    let mut indices = BTreeSet::new();
    for (index, parsed) in pinned.iter().enumerate() {
        let op = u8::try_from(index).expect("a family index fits in a byte");
        assert_eq!(parsed.family_index(), Some(op), "{parsed}");
        assert_eq!(SpaceFactoryUrn::from_family_index(op), Some(*parsed));
        assert!(op <= FACTORY_OP_LAST, "{parsed} is a factory op");
        assert!(indices.insert(op), "{parsed} repeats an index");
    }
    for index in FACTORY_FAMILY_COUNT..=u8::MAX {
        assert_eq!(SpaceFactoryUrn::from_family_index(index), None, "{index}");
    }
    let mut seen_pairs = BTreeSet::new();
    for outer in SpaceOuter::ALL {
        for inner in SpaceInner::ALL {
            let candidate = SpaceFactoryUrn { outer, inner };
            if let Some(index) = candidate.family_index() {
                assert!(seen_pairs.insert(index), "{candidate} repeats an index");
                assert_eq!(
                    FAMILY_TABLE[usize::from(index)],
                    candidate.to_string(),
                    "the pinned table holds {candidate} at {index}"
                );
            }
        }
    }
    assert_eq!(seen_pairs, indices, "the indices are 0 to 31 without a gap");
}
