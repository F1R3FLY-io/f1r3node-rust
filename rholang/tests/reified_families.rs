// P5 integration tests of the space families, their factories and the FIPS alias (P5 design v3,
// A.7, A.8 and A.10, plan decisions 7, 9.4 and 33, I-10 and the P5 choice F7).
//
// A factory is registered exactly when the family dispatch wires its family, and an alias is
// registered exactly when its family is registered (`spaces::factory::urn`). So the tests read
// the registration and check every registered row: the URN binding, the installed factory, the
// space that the factory creates, its config and `getSpaceAgent`. A row whose family is not
// wired yet has no URN binding, and the tests check that it fails closed. The constructor
// checks run for every family with a schema, wired or not, through
// `FactoryConstructor::validate`, and through Rholang deploys for the registered families.
//
// The properties restate the headlines of `SpaceFactory.v` that P5 adds:
// `registered_families_complete` (F26), `aliases_bind_family_capability` (F27),
// `constructor_params_validated` (F28), `priorities_validated` (I-10) and
// `array_size_validated` (F7). Each one compares the code with an independent oracle.
//
// P6 registers the VectorDB families `hashmap:vectordb` and `vector:vectordb` after the rows and
// aliases of P5 (design v3 4.2). Their factories require `capacity` and `dimensions`, and take
// the optional `metric` and `mirror`. Gate 9 then refuses a `mirror` whose metric ChromaDB does
// not index. The oracles of this file restate these rules.

#[path = "support/mod.rs"]
mod support;

use std::collections::{BTreeMap, BTreeSet};

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::Par;
use models::rust::reified::builders;
use models::rust::reified::params::SendParam;
use models::rust::reified::validation::is_param_key;
use models::rust::space_factory_urn::{
    SpaceFactoryUrn, SpaceInner, SpaceOuter, HASHMAP_BAG_ALIAS, PATHMAP_BAG_ALIAS,
};
use models::rust::utils::{
    new_bundle_par, new_elist_par, new_gbool_par, new_gint_par, new_gstring_par,
};
use proptest::collection::{btree_map, vec};
use proptest::prelude::*;
use proptest::sample::select;
use proptest::test_runner::TestCaseError;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::reified::costs::ScanMeter;
use rholang::rust::interpreter::reified::runtime::unavailable;
use rholang::rust::interpreter::reified::system_channels::{
    SystemChannel, P4_SYSTEM_CHANNELS, P5_SYSTEM_CHANNELS, P6_PROCESSES, P6_SYSTEM_CHANNELS,
};
use rholang::rust::interpreter::reified::system_processes::definitions;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::spaces::errors::SpaceError;
use rholang::rust::interpreter::spaces::factory::constructor::{family_of, FactoryConstructor};
use rholang::rust::interpreter::spaces::factory::defaults::{
    constructor_schema, required_params, suggestion, ParamRule, CAPACITY_PARAM, CYCLIC_PARAM,
    DIMENSIONS_PARAM, METRIC_PARAM, MIRROR_CHROMA, MIRROR_PARAM, PRIORITIES_PARAM, SIZE_PARAM,
    SUGGESTION_DISTANCE,
};
use rholang::rust::interpreter::spaces::factory::urn::{
    factory_capability, factory_channel, is_wired, registered_aliases, registered_channel,
    registered_families, registered_p5_families, registered_p6_families, P5_ALIAS_ROWS,
    P5_FAMILY_ROWS, P6_FAMILY_ROWS, REGISTERED_FAMILIES,
};
use rholang::rust::interpreter::spaces::families::{query_params, send_params, Family};
use rholang::rust::interpreter::spaces::namespace::{NativeSpace, SID_LEN};
use rholang::rust::interpreter::spaces::record;
use rholang::rust::interpreter::spaces::types::config::SpaceConfig;
use rholang::rust::interpreter::spaces::types::qualifier::SpaceQualifier;
use support::props::{check, pure_config, runtime_config, tokio_runtime};
use support::session::{config_channel, native, Injections, Session};

// ─── Values and constructors ─────────────────────────────────────────────────────────────────

fn int(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

fn flag(value: bool) -> Par { new_gbool_par(value, Vec::new(), false) }

fn string(value: &str) -> Par { new_gstring_par(value.to_string(), Vec::new(), false) }

fn list(ps: Vec<Par>) -> Par { new_elist_par(ps, Vec::new(), false, None, Vec::new(), false) }

/// The PARAMS tuple of the named params `named` and the positional params `positional`, or
/// `None` when both are empty.
fn params_tuple(positional: &[Par], named: &BTreeMap<String, Par>) -> Option<Par> {
    let mut params = Vec::with_capacity(positional.len() + named.len());
    params.extend(positional.iter().cloned().map(SendParam::Positional));
    params.extend(
        named
            .iter()
            .map(|(key, value)| SendParam::Named(key.clone(), value.clone())),
    );
    builders::send_params(params).expect("valid params")
}

/// The constructor list `[PARAMS?]` of `positional` and `named`.
fn ctor(positional: &[Par], named: &BTreeMap<String, Par>) -> Par {
    list(params_tuple(positional, named).into_iter().collect())
}

/// The named params `pairs` as a map.
fn named(pairs: &[(&str, Par)]) -> BTreeMap<String, Par> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_string(), value.clone()))
        .collect()
}

/// The family `outer:inner`.
fn family(outer: SpaceOuter, inner: SpaceInner) -> SpaceFactoryUrn {
    SpaceFactoryUrn { outer, inner }
}

/// The row of `family` in the tables of P4, P5 and P6, registered or not.
fn row_of(family: SpaceFactoryUrn) -> Option<SystemChannel> {
    REGISTERED_FAMILIES
        .iter()
        .chain(P5_FAMILY_ROWS.iter())
        .chain(P6_FAMILY_ROWS.iter())
        .find(|(candidate, _)| *candidate == family)
        .map(|(_, channel)| *channel)
}

/// The Rholang source of a value of [`int`], [`flag`] or [`string`].
fn rholang_value(value: &Par) -> String {
    match value
        .exprs
        .first()
        .and_then(|expr| expr.expr_instance.as_ref())
    {
        Some(ExprInstance::GInt(number)) => number.to_string(),
        Some(ExprInstance::GBool(flag)) => flag.to_string(),
        Some(ExprInstance::GString(text)) => format!("{text:?}"),
        other => panic!("no Rholang source for {other:?}"),
    }
}

/// The Rholang arguments `"default"; k=v, …` of a factory call with the named params `named`.
fn rholang_ctor(named: &BTreeMap<String, Par>) -> String {
    match named.is_empty() {
        true => "\"default\"".to_string(),
        false => format!(
            "\"default\"; {}",
            named
                .iter()
                .map(|(key, value)| format!("{key}={}", rholang_value(value)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// The Rholang arguments of [`rholang_ctor`] for `named`, with the source `source` as the value
/// of the param `key`. The source can be a computed value, which the normalizer does not check.
fn rholang_ctor_with(named: &BTreeMap<String, Par>, key: &str, source: &str) -> String {
    let mut sources: BTreeMap<&str, String> = named
        .iter()
        .map(|(name, value)| (name.as_str(), rholang_value(value)))
        .collect();
    sources.insert(key, source.to_string());
    format!(
        "\"default\"; {}",
        sources
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// The required params of `family`, each with the value 4.
fn minimal_params(family: SpaceFactoryUrn) -> BTreeMap<String, Par> {
    required_params(family)
        .iter()
        .map(|key| ((*key).to_string(), int(4)))
        .collect()
}

/// The messages of the Reified errors of a deploy.
fn messages(errors: &[InterpreterError]) -> Vec<String> {
    let mut found = Vec::with_capacity(errors.len());
    let mut pending: Vec<&InterpreterError> = errors.iter().collect();
    while let Some(error) = pending.pop() {
        match error {
            InterpreterError::AggregateError { interpreter_errors } => {
                pending.extend(interpreter_errors.iter());
            }
            InterpreterError::ReduceError(message) => found.push(message.clone()),
            other => found.push(format!("{other:?}")),
        }
    }
    found
}

/// The config of `space`, decoded from its record on `C(sid)`.
async fn stored_config(session: &Session, space: NativeSpace) -> SpaceConfig {
    let data = session.data(&config_channel(space)).await;
    let [config] = data.as_slice() else {
        panic!("C(sid) holds {} records", data.len());
    };
    let map = record::config_map(&config.a).expect("the config record decodes");
    SpaceConfig::from_par(map).expect("the config map decodes")
}

/// The families of P5 and P6 that have a constructor schema.
fn schema_families() -> Vec<SpaceFactoryUrn> {
    SpaceFactoryUrn::all()
        .filter(|family| constructor_schema(*family).is_some())
        .collect()
}

// ─── Oracles ─────────────────────────────────────────────────────────────────────────────────

/// The names of the six metrics of a VectorDB space (design v3 4.2).
const METRICS: [&str; 6] = [
    "cosine",
    "dot",
    "euclidean",
    "manhattan",
    "hamming",
    "jaccard",
];

/// The metrics that a ChromaDB mirror indexes (gate 9, design v3 4.2). A VectorDB space without
/// a `metric` param uses `cosine`.
const MIRRORED_METRICS: [&str; 3] = ["cosine", "dot", "euclidean"];

/// The Levenshtein distance of `word` and `candidate`, by the full table.
fn levenshtein(word: &str, candidate: &str) -> usize {
    let source: Vec<char> = word.chars().collect();
    let target: Vec<char> = candidate.chars().collect();
    let mut previous: Vec<usize> = (0..=target.len()).collect();
    for (row, source_char) in source.iter().enumerate() {
        let mut current = Vec::with_capacity(target.len() + 1);
        current.push(row + 1);
        for (column, target_char) in target.iter().enumerate() {
            let substitution = previous[column] + usize::from(source_char != target_char);
            current.push(
                substitution
                    .min(previous[column + 1] + 1)
                    .min(current[column] + 1),
            );
        }
        previous = current;
    }
    previous[target.len()]
}

/// The suggestion of the design: the least `(distance, candidate)` within the bound.
fn expected_suggestion(word: &str, schema: &[&str]) -> Option<String> {
    schema
        .iter()
        .map(|candidate| (levenshtein(word, candidate), *candidate))
        .filter(|(distance, _)| *distance <= SUGGESTION_DISTANCE)
        .min()
        .map(|(_, candidate)| candidate.to_string())
}

/// The canonical value of the param `key` by the rules of F7, I-10 and design v3 4.2 (P6), or
/// `None` when the rule refuses `value`. A value is one ground expression with no connective flag.
/// Its bit sets do not count.
fn expected_value(key: &str, value: &Par) -> Option<Par> {
    let ground = value.sends.is_empty()
        && value.receives.is_empty()
        && value.news.is_empty()
        && value.matches.is_empty()
        && value.unforgeables.is_empty()
        && value.bundles.is_empty()
        && value.connectives.is_empty()
        && value.conditionals.is_empty()
        && value.cost_signed_terms.is_empty()
        && value.cost_stacks.is_empty()
        && value.exprs.len() == 1
        && !value.connective_used;
    let instance = value
        .exprs
        .first()
        .and_then(|expr| expr.expr_instance.as_ref());
    match (ground, key, instance) {
        (true, "size" | "priorities", Some(ExprInstance::GInt(number))) => {
            (*number >= 1).then(|| int(*number))
        }
        (true, "cyclic", Some(ExprInstance::GBool(value))) => Some(flag(*value)),
        (true, "capacity", Some(ExprInstance::GInt(number))) => {
            (1..=1_000_000).contains(number).then(|| int(*number))
        }
        (true, "dimensions", Some(ExprInstance::GInt(number))) => {
            (1..=4096).contains(number).then(|| int(*number))
        }
        (true, "metric", Some(ExprInstance::GString(name))) => {
            METRICS.contains(&name.as_str()).then(|| string(name))
        }
        (true, "mirror", Some(ExprInstance::GString(name))) => {
            (name == "chroma").then(|| string(name))
        }
        _ => None,
    }
}

/// The outcome of the gates 5 to 9 by the design: positional params, then the unknown keys in
/// bytewise order with the suggestion of [`expected_suggestion`], then the missing required
/// keys in bytewise order, then the values in bytewise key order, then the metric of a mirror.
fn expected_params(
    family: SpaceFactoryUrn,
    positional: &[Par],
    named: &BTreeMap<String, Par>,
) -> Result<BTreeMap<String, Par>, SpaceError> {
    let schema = constructor_schema(family).expect("the oracle runs on a family with a schema");
    let required: &[&str] = match (family.outer, family.inner) {
        (SpaceOuter::Array, _) => &["size"],
        (_, SpaceInner::VectorDb) => &["capacity", "dimensions"],
        _ => &[],
    };
    if !positional.is_empty() {
        return Err(SpaceError::PositionalParamsUnsupported);
    }
    for key in named.keys() {
        if !schema.contains(&key.as_str()) {
            return Err(SpaceError::UnknownParam {
                key: key.clone(),
                suggestion: expected_suggestion(key, schema),
            });
        }
    }
    for key in required {
        if !named.contains_key(*key) {
            return Err(SpaceError::MissingParam {
                key: (*key).to_string(),
            });
        }
    }
    let mut canonical = BTreeMap::new();
    for (key, value) in named {
        match expected_value(key, value) {
            Some(value) => {
                canonical.insert(key.clone(), value);
            }
            None => return Err(SpaceError::InvalidParam { key: key.clone() }),
        }
    }
    let metric = canonical
        .get("metric")
        .cloned()
        .unwrap_or_else(|| string("cosine"));
    let mirrored = MIRRORED_METRICS.iter().any(|name| string(name) == metric);
    match canonical.contains_key("mirror") && !mirrored {
        true => Err(SpaceError::InvalidParam {
            key: "mirror".to_string(),
        }),
        false => Ok(canonical),
    }
}

// ─── Strategies ──────────────────────────────────────────────────────────────────────────────

fn arb_any_family() -> BoxedStrategy<SpaceFactoryUrn> {
    select(SpaceFactoryUrn::all().collect::<Vec<_>>()).boxed()
}

fn arb_schema_family() -> BoxedStrategy<SpaceFactoryUrn> { select(schema_families()).boxed() }

/// A param key: a schema key of P5 or P6, a near miss, another send or query key, or a random
/// identifier.
fn arb_key() -> BoxedStrategy<String> {
    prop_oneof![
        4 => select(vec![SIZE_PARAM, CYCLIC_PARAM, PRIORITIES_PARAM]).prop_map(String::from),
        3 => select(vec![CAPACITY_PARAM, DIMENSIONS_PARAM, METRIC_PARAM, MIRROR_PARAM])
            .prop_map(String::from),
        2 => select(vec![
            "siz", "sise", "sizes", "cycli", "cylic", "prioritie", "prioritis", "priority",
            "capacit", "dimension", "metri", "mirro", "k", "levels",
        ])
        .prop_map(String::from),
        1 => "[a-z][a-z_]{0,9}".prop_filter("a param key", |key| is_param_key(key)),
    ]
    .boxed()
}

/// A param value: an integer near the bounds, a flag, a string, a metric or mirror name or a near
/// miss of one, `Nil`, a value with a stale connective flag, a value with stray bits, or two
/// expressions.
fn arb_value() -> BoxedStrategy<Par> {
    prop_oneof![
        4 => (-3i64..7).prop_map(int),
        1 => select(vec![i64::MIN, -1, 0, 1, 4096, 4097, 1_000_000, 1_000_001, i64::MAX])
            .prop_map(int),
        2 => any::<bool>().prop_map(flag),
        1 => "[a-z0-9]{0,3}".prop_map(|text| string(&text)),
        2 => select(
            METRICS
                .iter()
                .copied()
                .chain(["chroma", "Chroma", "Cosine", "l2"])
                .collect::<Vec<_>>()
        )
        .prop_map(string),
        1 => Just(Par::default()),
        1 => (-3i64..7).prop_map(|number| Par {
            connective_used: true,
            ..int(number)
        }),
        1 => (1i64..7, vec(any::<u8>(), 1..3)).prop_map(|(number, bits)| Par {
            locally_free: bits,
            ..int(number)
        }),
        1 => (1i64..7, 1i64..7).prop_map(|(first, second)| {
            let mut pair = int(first);
            pair.exprs.extend(int(second).exprs);
            pair
        }),
    ]
    .boxed()
}

fn arb_named() -> BoxedStrategy<BTreeMap<String, Par>> {
    btree_map(arb_key(), arb_value(), 0..4).boxed()
}

fn arb_positional() -> BoxedStrategy<Vec<Par>> {
    prop_oneof![
        4 => Just(Vec::new()),
        1 => vec(arb_value(), 1..3),
    ]
    .boxed()
}

// ─── Registration ────────────────────────────────────────────────────────────────────────────

/// Every registered family has its row, its op, its body ref, its schemas and its dispatch arm,
/// and nothing else is defined. A runtime binds and installs exactly the registered factories
/// (F26).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_factory_families_registered() {
    for family in SpaceFactoryUrn::all() {
        let row = row_of(family);
        assert_eq!(
            row.is_some(),
            constructor_schema(family).is_some(),
            "{family}: a row exists exactly for a family with a schema"
        );
        assert_eq!(
            registered_channel(family),
            row.filter(|_| is_wired(family)),
            "{family}: a row is registered exactly when its family is wired"
        );
        if let Some(channel) = registered_channel(family) {
            assert_eq!(channel.urn, family.to_string());
            assert_eq!(Some(channel.op), family.family_index());
            assert_eq!(channel.body_ref(), 200 + i64::from(channel.op));
            assert_eq!(channel.arity, 2);
            assert!(constructor_schema(family).is_some(), "{family}");
            assert!(send_params(family).is_some(), "{family}");
            assert!(Family::of_urn(family).is_some(), "{family}");
        }
    }
    // The definitions: the four of P4, then the registered families of P5 in family-index
    // order, then the registered aliases, then the registered families of P6 in family-index
    // order, then the processes of P6 (`rho:lang:vector`).
    let expected: Vec<(String, i64)> = P4_SYSTEM_CHANNELS
        .iter()
        .copied()
        .chain(registered_p5_families().map(|(_, channel)| channel))
        .chain(registered_aliases().map(|(channel, _)| channel))
        .chain(registered_p6_families().map(|(_, channel)| channel))
        .chain(P6_PROCESSES)
        .map(|channel| (channel.urn.to_string(), channel.body_ref()))
        .collect();
    let defined: Vec<(String, i64)> = definitions(&unavailable())
        .iter()
        .map(|definition| (definition.urn.clone(), definition.body_ref))
        .collect();
    assert_eq!(defined, expected);
    let indices: Vec<u8> = registered_p5_families()
        .map(|(_, channel)| channel.op)
        .collect();
    assert!(
        indices.windows(2).all(|pair| pair[0] < pair[1]),
        "the P5 rows are in family-index order"
    );
    let indices: Vec<u8> = registered_p6_families()
        .map(|(_, channel)| channel.op)
        .collect();
    assert!(
        indices.windows(2).all(|pair| pair[0] < pair[1]),
        "the P6 rows are in family-index order"
    );
    // A runtime binds each registered family to its capability and installs one factory
    // continuation on its channel. An unregistered family has neither.
    let session = Session::new().await;
    let urn_map = session.play.reducer.urn_map.clone();
    let table: BTreeSet<i64> = session
        .play
        .reducer
        .dispatcher
        ._dispatch_table
        .read()
        .await
        .keys()
        .copied()
        .collect();
    for family in SpaceFactoryUrn::all() {
        let Some(channel) = factory_channel(family) else {
            continue;
        };
        let installed = session.play.get_continuations(vec![channel]).await;
        match registered_channel(family) {
            Some(registered) => {
                assert_eq!(
                    urn_map.get(&family.to_string()),
                    factory_capability(family).as_ref(),
                    "{family}"
                );
                assert!(table.contains(&registered.body_ref()), "{family}");
                let [install] = installed.as_slice() else {
                    panic!("{family}: {} installed continuations", installed.len());
                };
                assert!(install.persist, "{family}");
                assert_eq!(
                    install.continuation.tagged_cont,
                    Some(TaggedCont::ScalaBodyRef(registered.body_ref())),
                    "{family}"
                );
            }
            None => {
                assert_eq!(urn_map.get(&family.to_string()), None, "{family}");
                assert!(installed.is_empty(), "{family}");
            }
        }
    }
    println!(
        "registered P5 families: {:?}; registered aliases: {:?}; registered P6 families: {:?}",
        registered_p5_families()
            .map(|(family, _)| family.to_string())
            .collect::<Vec<_>>(),
        registered_aliases()
            .map(|(channel, _)| channel.urn)
            .collect::<Vec<_>>(),
        registered_p6_families()
            .map(|(family, _)| family.to_string())
            .collect::<Vec<_>>()
    );
}

/// Each registered factory creates a space whose config names its family, with the canonical
/// required params, and `getSpaceAgent` replies the capability of the family (decision 9.4).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_factory_registered_families_create_their_spaces() {
    let mut session = Session::new().await;
    let urn_map = session.play.reducer.urn_map.clone();
    for (index, (family, _)) in registered_families().enumerate() {
        let params = minimal_params(family);
        let out = format!("rrs-p5-created-{index}");
        let source = format!(
            "new factory(`{family}`), gsa(`rho:space:getSpaceAgent`), \
             equal(`rho:assert:assertEqual`) in {{ for (space <- factory!?({})) {{ \
             for (@capability <- gsa!?(*space)) {{ for (@same <- equal!?(capability, *factory)) \
             {{ @\"{out}\"!(*space, capability, same) }} }} }} }}",
            rholang_ctor(&params)
        );
        session
            .run(&source, &format!("rrs-p5-create {family}"))
            .await;
        let values = session.values(&out).await;
        let [reply] = values.as_slice() else {
            panic!("{family}: the factory replied {} times", values.len());
        };
        let [handle, capability, same] = reply.as_slice() else {
            panic!("{family}: the reply holds {} values", reply.len());
        };
        assert_eq!(Some(capability), urn_map.get(&family.to_string()));
        assert_eq!(Some(capability.clone()), factory_capability(family));
        assert_eq!(same, &flag(true), "{family}");
        let config = stored_config(&session, native(handle)).await;
        assert_eq!(family_of(&config), Ok(family));
        assert_eq!(config.qualifier, SpaceQualifier::Default);
        assert_eq!(config.params, params, "{family}");
    }
}

/// The FIPS alias `HashMapBagSpace` names the factory of `hashmap:bag`. When the alias is
/// registered, `getSpaceAgent` of a space that the alias creates equals the alias and the
/// family URN (decision 33). Until the family is wired, both URNs fail closed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_getspaceagent_alias_equal() {
    let bag = family(SpaceOuter::HashMap, SpaceInner::Bag);
    let mut session = Session::new().await;
    let alias_registered =
        registered_aliases().any(|(channel, _)| channel.urn == HASHMAP_BAG_ALIAS);
    assert_eq!(alias_registered, registered_channel(bag).is_some());
    match alias_registered {
        true => {
            session
                .run(
                    r#"
new alias(`rho:space:HashMapBagSpace`), bag(`rho:space:hashmap:bag`),
    gsa(`rho:space:getSpaceAgent`), equal(`rho:assert:assertEqual`) in {
  for (space <- alias!?("default")) {
    for (@capability <- gsa!?(*space)) {
      for (@sameAlias <- equal!?(capability, *alias)) {
        for (@sameFamily <- equal!?(capability, *bag)) {
          @"rrs-p5-alias"!(*space, sameAlias, sameFamily)
        }
      }
    }
  }
}
"#,
                    "rrs-p5-alias-equal",
                )
                .await;
            let values = session.values("rrs-p5-alias").await;
            let [reply] = values.as_slice() else {
                panic!("the alias replied {} times", values.len());
            };
            let [handle, same_alias, same_family] = reply.as_slice() else {
                panic!("the reply holds {} values", reply.len());
            };
            assert_eq!(same_alias, &flag(true));
            assert_eq!(same_family, &flag(true));
            let config = stored_config(&session, native(handle)).await;
            assert_eq!(family_of(&config), Ok(bag));
        }
        false => {
            for urn in [HASHMAP_BAG_ALIAS, "rho:space:hashmap:bag"] {
                let source = format!(
                    "new factory(`{urn}`) in {{ for (q <- factory!?(\"default\")) {{ Nil }} }}"
                );
                let deploy = session
                    .deploy_with(
                        &source,
                        &format!("rrs-p5-alias-closed {urn}"),
                        &Injections::new(),
                    )
                    .await;
                let errors = format!("{:?}", deploy.result.errors);
                assert!(
                    errors.contains(&format!("No value set for {urn}")),
                    "{urn}: {errors}"
                );
                assert!(deploy.residue.is_empty(), "{urn}");
            }
        }
    }
    // Phase 2 of P5e wires `pathmap:bag`, so its FIPS alias is registered too
    // (`reified_pathmap.rs`, `reified_pathmapspace_alias_equal`).
    assert!(registered_aliases().any(|(channel, _)| channel.urn == PATHMAP_BAG_ALIAS));
}

/// A registered alias adds a definition with the channel, the arity and the body ref of its
/// family, so the second install of the channel changes nothing: the channel holds one
/// installed continuation, and both URNs bind one capability.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_factory_alias_install_idempotent() {
    let defined = definitions(&unavailable());
    for (alias, family) in P5_ALIAS_ROWS {
        assert_eq!(SpaceFactoryUrn::parse(alias.urn), Ok(family));
        assert_eq!(Some(alias.op), family.family_index());
        let alias_definitions: Vec<_> = defined
            .iter()
            .filter(|definition| definition.urn == alias.urn)
            .collect();
        let family_definitions: Vec<_> = defined
            .iter()
            .filter(|definition| definition.urn == family.to_string())
            .collect();
        match registered_channel(family) {
            Some(channel) => {
                let [of_alias] = alias_definitions.as_slice() else {
                    panic!("{} has {} definitions", alias.urn, alias_definitions.len());
                };
                let [of_family] = family_definitions.as_slice() else {
                    panic!("{family} has {} definitions", family_definitions.len());
                };
                assert_eq!(of_alias.fixed_channel, of_family.fixed_channel);
                assert_eq!(of_alias.body_ref, of_family.body_ref);
                assert_eq!(of_alias.arity, of_family.arity);
                assert_eq!(of_alias.to_urn_map().1, of_family.to_urn_map().1);
                assert_eq!(of_family.fixed_channel, channel.channel());
            }
            None => {
                assert!(alias_definitions.is_empty(), "{}", alias.urn);
                assert!(family_definitions.is_empty(), "{family}");
            }
        }
    }
    let mut session = Session::new().await;
    let urn_map = session.play.reducer.urn_map.clone();
    for (alias, family) in P5_ALIAS_ROWS {
        let installed = session.play.get_continuations(vec![alias.channel()]).await;
        match registered_channel(family) {
            Some(_) => {
                assert_eq!(installed.len(), 1, "one install on the shared channel");
                assert_eq!(urn_map.get(alias.urn), urn_map.get(&family.to_string()));
                assert_eq!(urn_map.get(alias.urn), factory_capability(family).as_ref());
                // Both URNs create spaces of the family. Each alias row runs a deploy of its own
                // label, so the factory calls of two rows derive distinct space ids, and replies
                // on channels of its own.
                let (one, two) = (
                    format!("rrs-p5-one-{}", alias.op),
                    format!("rrs-p5-two-{}", alias.op),
                );
                session
                    .run(
                        &format!(
                            "new viaAlias(`{}`), viaFamily(`{family}`) in {{ \
                             for (one <- viaAlias!?(\"default\")) {{ @\"{one}\"!(*one) }} | \
                             for (two <- viaFamily!?(\"default\")) {{ @\"{two}\"!(*two) }} }}",
                            alias.urn
                        ),
                        &format!("rrs-p5-alias-both {}", alias.urn),
                    )
                    .await;
                let one = native(&session.sole(&one).await);
                let two = native(&session.sole(&two).await);
                assert_ne!(one.sid, two.sid);
                for space in [one, two] {
                    assert_eq!(family_of(&stored_config(&session, space).await), Ok(family));
                }
            }
            None => {
                assert!(
                    installed.is_empty(),
                    "no install before the family is wired"
                );
                assert!(!urn_map.contains_key(alias.urn));
            }
        }
    }
}

/// The family agent dispatches by the config: each wired family names its own agent, and a
/// space that a registered HashMap factory creates with its required params runs a send and a
/// receive through the agent of its family.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_family_dispatch_by_config() {
    let meter = ScanMeter::new(None);
    for family in SpaceFactoryUrn::all() {
        let config = SpaceConfig::new(
            [7; SID_LEN],
            family.outer.as_str(),
            family.inner.as_str(),
            SpaceQualifier::Default,
        );
        assert_eq!(
            Family::of_config(&config, &meter),
            Ok(Family::of_urn(family)),
            "{family}"
        );
    }
    let mut session = Session::new().await;
    for (index, (family, _)) in registered_families()
        .filter(|(family, _)| family.outer == SpaceOuter::HashMap)
        .enumerate()
    {
        let out = format!("rrs-p5-dispatch-{index}");
        let source = format!(
            "new factory(`{family}`) in {{ for (space <- factory!?({})) {{ \
             for (a <- space!?(\"gensym\")) {{ a!(7) | for (@x <- a) {{ @\"{out}\"!(x) }} }} }} }}",
            rholang_ctor(&minimal_params(family))
        );
        session
            .run(&source, &format!("rrs-p5-dispatch {family}"))
            .await;
        assert_eq!(session.values(&out).await, vec![vec![int(7)]], "{family}");
    }
}

/// The send schema of a family comes from its dispatch arm (`families::send_params`), and it
/// is separate from the constructor schema: a constructor param is never a send param.
#[test]
fn space_send_params_schema_by_family() {
    for family in SpaceFactoryUrn::all() {
        match Family::of_urn(family) {
            Some(wired) => {
                assert_eq!(send_params(family), Some(wired.send_params()), "{family}");
                assert_eq!(query_params(family), Some(wired.query_params()), "{family}");
                let constructor = constructor_schema(family).expect("a wired family has a schema");
                assert!(
                    constructor
                        .iter()
                        .all(|key| !wired.send_params().contains(key)),
                    "{family}: a constructor param is not a send param"
                );
            }
            None => {
                assert_eq!(send_params(family), None, "{family}");
                assert_eq!(query_params(family), None, "{family}");
                assert_eq!(registered_channel(family), None, "{family}");
            }
        }
    }
    assert_eq!(
        send_params(SpaceFactoryUrn::HASHMAP_QUEUE),
        Some(&[][..]),
        "the queue takes no send params"
    );
}

// ─── Constructor checks ──────────────────────────────────────────────────────────────────────

/// An unknown constructor param fails with `UnknownParam` and the nearest schema key within two
/// edits, in every family with a schema and through every registered factory.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_factory_kwargs_unknown_param_with_suggestion() {
    for family in schema_families() {
        let schema = constructor_schema(family).expect("a schema");
        let required = minimal_params(family);
        for key in schema {
            let near = &key[..key.len() - 1];
            let mut pairs = required.clone();
            pairs.insert(near.to_string(), int(1));
            assert_eq!(
                FactoryConstructor::validate(family, &ctor(&[], &pairs)),
                Err(SpaceError::UnknownParam {
                    key: near.to_string(),
                    suggestion: Some((*key).to_string()),
                }),
                "{family}: {near}"
            );
        }
        let mut far = required.clone();
        far.insert("zeta".to_string(), int(1));
        assert_eq!(
            FactoryConstructor::validate(family, &ctor(&[], &far)),
            Err(SpaceError::UnknownParam {
                key: "zeta".to_string(),
                suggestion: None,
            }),
            "{family}"
        );
    }
    let mut session = Session::new().await;
    for (family, _) in registered_families() {
        let schema = constructor_schema(family).expect("a registered family has a schema");
        let mut cases = vec![("zeta".to_string(), None)];
        cases.extend(
            schema
                .iter()
                .map(|key| (key[..key.len() - 1].to_string(), Some(key.to_string()))),
        );
        for (key, expected) in cases {
            let mut pairs = minimal_params(family);
            pairs.insert(key.clone(), int(1));
            let source = format!(
                "new factory(`{family}`) in {{ for (q <- factory!?({})) {{ Nil }} }}",
                rholang_ctor(&pairs)
            );
            let deploy = session
                .fail_with(
                    &source,
                    &format!("rrs-p5-unknown {family} {key}"),
                    &Injections::new(),
                    &["UnknownParam"],
                )
                .await;
            let text = messages(&deploy.result.errors).join("\n");
            assert!(text.contains(&format!("{key:?} is not a param")), "{text}");
            match expected {
                Some(suggested) => assert!(
                    text.contains(&format!("did you mean {suggested:?}?")),
                    "{text}"
                ),
                None => assert!(!text.contains("did you mean"), "{text}"),
            }
        }
    }
}

/// An Array factory without `size` (F7) and a VectorDB factory without `capacity` or
/// `dimensions` (P6) fail with `MissingParam`, after the unknown keys and before the values.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_factory_kwargs_missing_param() {
    let arrays: Vec<SpaceFactoryUrn> = schema_families()
        .into_iter()
        .filter(|family| family.outer == SpaceOuter::Array)
        .collect();
    assert_eq!(arrays.len(), 6);
    for family in &arrays {
        let missing = Err(SpaceError::MissingParam {
            key: "size".to_string(),
        });
        for pairs in [
            named(&[]),
            named(&[("cyclic", flag(true))]),
            named(&[("cyclic", int(3))]),
        ] {
            assert_eq!(
                FactoryConstructor::validate(*family, &ctor(&[], &pairs)),
                missing,
                "{family}"
            );
        }
        assert_eq!(
            FactoryConstructor::validate(*family, &list(Vec::new())),
            missing
        );
        assert!(
            matches!(
                FactoryConstructor::validate(*family, &ctor(&[], &named(&[("sise", int(4))]))),
                Err(SpaceError::UnknownParam { .. })
            ),
            "{family}: an unknown key comes first"
        );
    }
    let vectordbs: Vec<SpaceFactoryUrn> = schema_families()
        .into_iter()
        .filter(|family| family.inner == SpaceInner::VectorDb)
        .collect();
    assert_eq!(vectordbs.len(), 2);
    let missing = |key: &str| -> Result<FactoryConstructor, SpaceError> {
        Err(SpaceError::MissingParam {
            key: key.to_string(),
        })
    };
    for family in &vectordbs {
        for (pairs, key) in [
            (named(&[]), CAPACITY_PARAM),
            (named(&[("dimensions", int(2))]), CAPACITY_PARAM),
            (
                named(&[("dimensions", int(0)), ("metric", string("l2"))]),
                CAPACITY_PARAM,
            ),
            (named(&[("capacity", int(5))]), DIMENSIONS_PARAM),
            (
                named(&[("capacity", int(0)), ("mirror", string(MIRROR_CHROMA))]),
                DIMENSIONS_PARAM,
            ),
        ] {
            assert_eq!(
                FactoryConstructor::validate(*family, &ctor(&[], &pairs)),
                missing(key),
                "{family}: {pairs:?}"
            );
        }
        assert_eq!(
            FactoryConstructor::validate(*family, &list(Vec::new())),
            missing(CAPACITY_PARAM)
        );
        assert!(
            matches!(
                FactoryConstructor::validate(*family, &ctor(&[], &named(&[("capacit", int(4))]))),
                Err(SpaceError::UnknownParam { .. })
            ),
            "{family}: an unknown key comes first"
        );
    }
    for family in schema_families()
        .into_iter()
        .filter(|family| family.outer != SpaceOuter::Array && family.inner != SpaceInner::VectorDb)
    {
        assert!(required_params(family).is_empty(), "{family}");
        assert!(
            FactoryConstructor::validate(family, &list(Vec::new())).is_ok(),
            "{family}"
        );
    }
    let mut session = Session::new().await;
    for (family, _) in registered_families().filter(|(family, _)| family.outer == SpaceOuter::Array)
    {
        session
            .fail_with(
                &format!(
                    "new factory(`{family}`) in {{ for (q <- factory!?(\"default\"; cyclic=true)) \
                     {{ Nil }} }}"
                ),
                &format!("rrs-p5-missing {family}"),
                &Injections::new(),
                &["MissingParam"],
            )
            .await;
    }
    for (family, _) in
        registered_families().filter(|(family, _)| family.inner == SpaceInner::VectorDb)
    {
        for arguments in [
            r#""default""#,
            r#""default"; capacity=5"#,
            r#""default"; dimensions=2, metric="dot""#,
        ] {
            session
                .fail_with(
                    &format!(
                        "new factory(`{family}`) in {{ for (q <- factory!?({arguments})) {{ Nil }} }}"
                    ),
                    &format!("rrs-p5-missing {family} {arguments}"),
                    &Injections::new(),
                    &["MissingParam"],
                )
                .await;
        }
    }
}

/// A value that its rule refuses fails with `InvalidParam`. `size` and `priorities` take a
/// `GInt` of at least 1, and `cyclic` takes a `GBool` (F7, I-10). `capacity` and `dimensions`
/// take a `GInt` within their bounds, `metric` takes a metric name, and `mirror` takes `"chroma"`
/// (P6). Gate 9 then refuses a mirror whose metric ChromaDB does not index.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_factory_kwargs_invalid_param() {
    let bad_numbers = [int(0), int(-1), int(i64::MIN), flag(true), string("4")];
    let bad_flags = [int(0), int(1), string("true"), Par::default()];
    let bad_capacities = [
        int(0),
        int(-1),
        int(1_000_001),
        int(i64::MAX),
        flag(true),
        string("4"),
    ];
    let bad_dimensions = [
        int(0),
        int(-1),
        int(4097),
        int(i64::MAX),
        flag(true),
        string("4"),
    ];
    let bad_metrics = [
        string("l2"),
        string("Cosine"),
        string(""),
        int(1),
        Par::default(),
    ];
    let bad_mirrors = [
        string("Chroma"),
        string("qdrant"),
        string("cosine"),
        int(1),
        flag(true),
        Par::default(),
    ];
    for family in schema_families() {
        let schema = constructor_schema(family).expect("a schema");
        for key in schema {
            let bad: &[Par] = match *key {
                CYCLIC_PARAM => &bad_flags,
                CAPACITY_PARAM => &bad_capacities,
                DIMENSIONS_PARAM => &bad_dimensions,
                METRIC_PARAM => &bad_metrics,
                MIRROR_PARAM => &bad_mirrors,
                _ => &bad_numbers,
            };
            for value in bad {
                let mut pairs = minimal_params(family);
                pairs.insert((*key).to_string(), value.clone());
                assert_eq!(
                    FactoryConstructor::validate(family, &ctor(&[], &pairs)),
                    Err(SpaceError::InvalidParam {
                        key: (*key).to_string(),
                    }),
                    "{family}: {key}={value:?}"
                );
            }
        }
    }
    // Gate 9: a mirror needs a metric that ChromaDB indexes, and `cosine` is the default metric.
    for family in schema_families()
        .into_iter()
        .filter(|family| family.inner == SpaceInner::VectorDb)
    {
        for metric in METRICS.iter().map(|name| Some(*name)).chain([None]) {
            let mut pairs = minimal_params(family);
            if let Some(metric) = metric {
                pairs.insert(METRIC_PARAM.to_string(), string(metric));
            }
            pairs.insert(MIRROR_PARAM.to_string(), string(MIRROR_CHROMA));
            let validated = FactoryConstructor::validate(family, &ctor(&[], &pairs))
                .map(|validated| validated.params);
            match MIRRORED_METRICS.contains(&metric.unwrap_or("cosine")) {
                true => assert_eq!(validated, Ok(pairs), "{family}: {metric:?}"),
                false => assert_eq!(
                    validated,
                    Err(SpaceError::InvalidParam {
                        key: MIRROR_PARAM.to_string(),
                    }),
                    "{family}: {metric:?}"
                ),
            }
        }
    }
    let mut session = Session::new().await;
    for (family, _) in registered_families() {
        let schema = constructor_schema(family).expect("a registered family has a schema");
        for key in schema {
            // The normalizer refuses a literal metric that is not a metric name, so the bad
            // metric is a computed value, which reaches the rule of the factory.
            let value = match *key {
                CYCLIC_PARAM => "1",
                METRIC_PARAM => r#""l" ++ "2""#,
                MIRROR_PARAM => r#""qdrant""#,
                _ => "0",
            };
            session
                .fail_with(
                    &format!(
                        "new factory(`{family}`) in {{ for (q <- factory!?({})) {{ Nil }} }}",
                        rholang_ctor_with(&minimal_params(family), key, value)
                    ),
                    &format!("rrs-p5-invalid {family} {key}"),
                    &Injections::new(),
                    &["InvalidParam"],
                )
                .await;
        }
    }
    for (family, _) in
        registered_families().filter(|(family, _)| family.inner == SpaceInner::VectorDb)
    {
        let mut pairs = minimal_params(family);
        pairs.insert(METRIC_PARAM.to_string(), string("hamming"));
        pairs.insert(MIRROR_PARAM.to_string(), string(MIRROR_CHROMA));
        session
            .fail_with(
                &format!(
                    "new factory(`{family}`) in {{ for (q <- factory!?({})) {{ Nil }} }}",
                    rholang_ctor(&pairs)
                ),
                &format!("rrs-p5-invalid {family} gate 9"),
                &Injections::new(),
                &["InvalidParam"],
            )
            .await;
    }
}

/// The config of a new space holds the named params in their canonical form, and an absent
/// optional param stays absent: a fixed Array space, priority levels without a bound, or a
/// VectorDB space with the default metric and without a mirror.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_factory_kwargs_are_stored_canonical() {
    for family in schema_families() {
        let schema = constructor_schema(family).expect("a schema");
        let mut given = BTreeMap::new();
        let mut stored = BTreeMap::new();
        for key in schema {
            let (value, canonical) = match *key {
                CYCLIC_PARAM => (
                    Par {
                        locally_free: vec![0b0000_0100],
                        ..flag(true)
                    },
                    flag(true),
                ),
                METRIC_PARAM => (
                    Par {
                        locally_free: vec![0b0000_0010],
                        ..string("dot")
                    },
                    string("dot"),
                ),
                MIRROR_PARAM => (
                    Par {
                        locally_free: vec![0b0000_1000],
                        ..string(MIRROR_CHROMA)
                    },
                    string(MIRROR_CHROMA),
                ),
                _ => (
                    Par {
                        locally_free: vec![0b0000_0001],
                        ..int(3)
                    },
                    int(3),
                ),
            };
            given.insert((*key).to_string(), value);
            stored.insert((*key).to_string(), canonical);
        }
        let validated = FactoryConstructor::validate(family, &ctor(&[], &given))
            .unwrap_or_else(|error| panic!("{family}: {error:?}"));
        assert_eq!(validated.params, stored, "{family}");
        let minimal = FactoryConstructor::validate(family, &ctor(&[], &minimal_params(family)))
            .unwrap_or_else(|error| panic!("{family}: {error:?}"));
        assert_eq!(minimal.params, minimal_params(family), "{family}");
        for optional in [CYCLIC_PARAM, PRIORITIES_PARAM, METRIC_PARAM, MIRROR_PARAM] {
            assert!(
                !minimal.params.contains_key(optional),
                "{family}: {optional}"
            );
        }
    }
    let mut session = Session::new().await;
    for (index, (family, _)) in registered_families().enumerate() {
        let schema = constructor_schema(family).expect("a registered family has a schema");
        let pairs: BTreeMap<String, Par> = schema
            .iter()
            .map(|key| {
                let value = match *key {
                    CYCLIC_PARAM => flag(true),
                    METRIC_PARAM => string("euclidean"),
                    MIRROR_PARAM => string(MIRROR_CHROMA),
                    _ => int(5),
                };
                ((*key).to_string(), value)
            })
            .collect();
        let out = format!("rrs-p5-stored-{index}");
        session
            .run(
                &format!(
                    "new factory(`{family}`) in {{ for (space <- factory!?({})) {{ \
                     @\"{out}\"!(*space) }} }}",
                    rholang_ctor(&pairs)
                ),
                &format!("rrs-p5-stored {family}"),
            )
            .await;
        let space = native(&session.sole(&out).await);
        assert_eq!(
            stored_config(&session, space).await.params,
            pairs,
            "{family}"
        );
    }
}

// ─── Properties (the P5 headlines of SpaceFactory.v) ─────────────────────────────────────────

/// `registered_families_complete` (F26): a family is registered exactly when it has a row and
/// its dispatch arm. A registered family has the op of its index, the body ref `200 + op`, its
/// URN, a constructor schema and a send schema. The definitions are the four of P4, the
/// registered P5 rows in row order, the registered aliases, the registered P6 rows in row order
/// and the processes of P6, and nothing else: each factory definition names a registered family
/// by its op.
#[test]
fn prop_registered_families_complete() {
    let defined: Vec<(String, i64)> = definitions(&unavailable())
        .iter()
        .map(|definition| (definition.urn.clone(), definition.body_ref))
        .collect();
    check(pure_config(), arb_any_family(), |family| {
        let row = row_of(family);
        prop_assert_eq!(row.is_some(), constructor_schema(family).is_some());
        prop_assert_eq!(registered_channel(family), row.filter(|_| is_wired(family)));
        if let Some(channel) = registered_channel(family) {
            prop_assert_eq!(channel.urn.to_string(), family.to_string());
            prop_assert_eq!(Some(channel.op), family.family_index());
            prop_assert_eq!(channel.body_ref(), 200 + i64::from(channel.op));
            prop_assert_eq!(channel.arity, 2);
            prop_assert!(constructor_schema(family).is_some());
            prop_assert!(send_params(family).is_some());
            prop_assert!(Family::of_urn(family).is_some());
            prop_assert!(defined.contains(&(family.to_string(), channel.body_ref())));
        } else {
            prop_assert!(!defined.iter().any(|(urn, _)| *urn == family.to_string()));
        }
        let mut expected: Vec<(String, i64)> = P4_SYSTEM_CHANNELS
            .iter()
            .map(|channel| (channel.urn.to_string(), channel.body_ref()))
            .collect();
        expected.extend(
            P5_SYSTEM_CHANNELS
                .iter()
                .filter(|channel| {
                    registered_p5_families().any(|(_, registered)| registered == **channel)
                        || registered_aliases().any(|(registered, _)| registered == **channel)
                })
                .map(|channel| (channel.urn.to_string(), channel.body_ref())),
        );
        expected.extend(
            P6_SYSTEM_CHANNELS
                .iter()
                .filter(|channel| {
                    registered_p6_families().any(|(_, registered)| registered == **channel)
                })
                .map(|channel| (channel.urn.to_string(), channel.body_ref())),
        );
        expected.extend(
            P6_PROCESSES
                .iter()
                .map(|channel| (channel.urn.to_string(), channel.body_ref())),
        );
        prop_assert_eq!(&defined, &expected);
        let factories = &defined[P4_SYSTEM_CHANNELS.len()..defined.len() - P6_PROCESSES.len()];
        for (urn, body) in factories {
            let named = SpaceFactoryUrn::parse(urn).map_err(|error| {
                TestCaseError::fail(format!("{urn} is not a factory URN: {error}"))
            })?;
            prop_assert_eq!(
                named.family_index().map(|op| 200 + i64::from(op)),
                Some(*body)
            );
            prop_assert!(registered_channel(named).is_some());
        }
        Ok(())
    });
}

/// `aliases_bind_family_capability` (F27): an alias names the op of its family, so its urn map
/// value is the capability of the family, and `getSpaceAgent` of a space of that family
/// replies the same `Par`. An alias is registered exactly when its family is registered.
#[test]
fn prop_aliases_bind_family_capability() {
    check(
        pure_config(),
        (select(P5_ALIAS_ROWS.to_vec()), any::<[u8; SID_LEN]>()),
        |((alias, family), sid)| {
            prop_assert_eq!(SpaceFactoryUrn::parse(alias.urn), Ok(family));
            prop_assert_eq!(Some(alias.op), family.family_index());
            prop_assert_eq!(alias.arity, 2);
            let alias_value = new_bundle_par(alias.channel(), true, false);
            prop_assert_eq!(Some(alias_value.clone()), factory_capability(family));
            let config = SpaceConfig::new(
                sid,
                family.outer.as_str(),
                family.inner.as_str(),
                SpaceQualifier::Default,
            );
            let replied = family_of(&config)
                .ok()
                .and_then(factory_capability)
                .ok_or_else(|| TestCaseError::fail("the config names a family"))?;
            prop_assert_eq!(replied, alias_value);
            prop_assert_eq!(
                registered_aliases().any(|(registered, _)| registered == alias),
                registered_channel(family).is_some()
            );
            Ok(())
        },
    );
}

/// `constructor_params_validated` (F28): the gates 5 to 8 of a factory call equal the oracle of
/// the design. Positional params fail first, then an unknown key with the least `(distance,
/// key)` suggestion within two edits, then a missing required key, then a refused value. The
/// accepted params are a subset of the schema, in canonical form. A family without a schema
/// is unavailable.
#[test]
fn prop_constructor_params_validated() {
    check(
        pure_config(),
        (arb_any_family(), arb_positional(), arb_named()),
        |(family, positional, named)| {
            let validated = FactoryConstructor::validate(family, &ctor(&positional, &named));
            match constructor_schema(family) {
                None => prop_assert_eq!(
                    validated,
                    Err(SpaceError::FamilyUnavailable {
                        outer: family.outer.as_str().to_string(),
                        inner: family.inner.as_str().to_string(),
                    })
                ),
                Some(schema) => {
                    let expected = expected_params(family, &positional, &named);
                    prop_assert_eq!(
                        validated.clone().map(|validated| validated.params),
                        expected
                    );
                    if let Ok(validated) = validated {
                        prop_assert!(validated
                            .params
                            .keys()
                            .all(|key| schema.contains(&key.as_str())));
                        prop_assert_eq!(validated.family, family);
                    }
                    for key in named.keys() {
                        prop_assert_eq!(
                            suggestion(key, schema).map(str::to_string),
                            expected_suggestion(key, schema)
                        );
                    }
                }
            }
            Ok(())
        },
    );
}

/// `priorities_validated` (I-10): every priority queue family takes the optional `priorities`,
/// a `GInt` of at least 1, and no other family takes it. The config keeps the canonical value,
/// and without it the levels have no bound.
#[test]
fn prop_priorities_validated() {
    check(
        pure_config(),
        (arb_schema_family(), proptest::option::of(arb_value())),
        |(family, value)| {
            let mut pairs = minimal_params(family);
            if let Some(value) = &value {
                pairs.insert(PRIORITIES_PARAM.to_string(), value.clone());
            }
            let validated = FactoryConstructor::validate(family, &ctor(&[], &pairs));
            let takes = family.inner == SpaceInner::PriorityQueue;
            prop_assert_eq!(
                constructor_schema(family)
                    .expect("a schema")
                    .contains(&PRIORITIES_PARAM),
                takes
            );
            prop_assert!(!required_params(family).contains(&PRIORITIES_PARAM));
            match (takes, &value) {
                (_, None) => {
                    let validated = validated.expect("the minimal constructor is valid");
                    prop_assert!(!validated.params.contains_key(PRIORITIES_PARAM));
                }
                (false, Some(_)) => prop_assert_eq!(
                    validated,
                    Err(SpaceError::UnknownParam {
                        key: PRIORITIES_PARAM.to_string(),
                        suggestion: None,
                    })
                ),
                (true, Some(value)) => match expected_value(PRIORITIES_PARAM, value) {
                    Some(canonical) => {
                        let validated = validated.expect("a bound of at least 1 is valid");
                        prop_assert_eq!(validated.params.get(PRIORITIES_PARAM), Some(&canonical));
                        prop_assert_eq!(ParamRule::Priorities.canonical(value), Some(canonical));
                    }
                    None => prop_assert_eq!(
                        validated,
                        Err(SpaceError::InvalidParam {
                            key: PRIORITIES_PARAM.to_string(),
                        })
                    ),
                },
            }
            Ok(())
        },
    );
}

/// `array_size_validated` (F7): every Array family requires `size`, a `GInt` in
/// `[1, i64::MAX]`, and takes the optional `cyclic`, a `GBool`. The values are checked in
/// bytewise key order, so a bad `cyclic` fails before a bad `size`. A Vector family takes
/// neither.
#[test]
fn prop_array_size_validated() {
    let stores = select(vec![SpaceOuter::Array, SpaceOuter::Vector]);
    let inners = select(vec![
        SpaceInner::Bag,
        SpaceInner::Queue,
        SpaceInner::Stack,
        SpaceInner::Set,
        SpaceInner::Cell,
        SpaceInner::PriorityQueue,
    ]);
    check(
        pure_config(),
        (
            stores,
            inners,
            proptest::option::of(arb_value()),
            proptest::option::of(arb_value()),
        ),
        |(outer, inner, size, cyclic)| {
            let family = family(outer, inner);
            let mut pairs = BTreeMap::new();
            if let Some(size) = &size {
                pairs.insert(SIZE_PARAM.to_string(), size.clone());
            }
            if let Some(cyclic) = &cyclic {
                pairs.insert(CYCLIC_PARAM.to_string(), cyclic.clone());
            }
            let validated = FactoryConstructor::validate(family, &ctor(&[], &pairs));
            let schema = constructor_schema(family).expect("a schema");
            match outer {
                SpaceOuter::Array => {
                    prop_assert!(schema.contains(&SIZE_PARAM) && schema.contains(&CYCLIC_PARAM));
                    prop_assert_eq!(required_params(family), &[SIZE_PARAM][..]);
                    let cyclic_ok = cyclic
                        .as_ref()
                        .map(|value| expected_value(CYCLIC_PARAM, value));
                    let size_ok = size.as_ref().map(|value| expected_value(SIZE_PARAM, value));
                    let expected = match (size_ok, cyclic_ok) {
                        (None, _) => Err(SpaceError::MissingParam {
                            key: SIZE_PARAM.to_string(),
                        }),
                        (_, Some(None)) => Err(SpaceError::InvalidParam {
                            key: CYCLIC_PARAM.to_string(),
                        }),
                        (Some(None), _) => Err(SpaceError::InvalidParam {
                            key: SIZE_PARAM.to_string(),
                        }),
                        (Some(Some(size)), cyclic) => {
                            let mut params = BTreeMap::from([(SIZE_PARAM.to_string(), size)]);
                            if let Some(Some(cyclic)) = cyclic {
                                params.insert(CYCLIC_PARAM.to_string(), cyclic);
                            }
                            Ok(params)
                        }
                    };
                    prop_assert_eq!(validated.map(|validated| validated.params), expected);
                }
                _ => {
                    prop_assert!(!schema.contains(&SIZE_PARAM) && !schema.contains(&CYCLIC_PARAM));
                    prop_assert!(required_params(family).is_empty());
                    let expected = match (&cyclic, &size) {
                        (Some(_), _) => Err(SpaceError::UnknownParam {
                            key: CYCLIC_PARAM.to_string(),
                            suggestion: None,
                        }),
                        (None, Some(_)) => Err(SpaceError::UnknownParam {
                            key: SIZE_PARAM.to_string(),
                            suggestion: None,
                        }),
                        (None, None) => Ok(BTreeMap::new()),
                    };
                    prop_assert_eq!(validated.map(|validated| validated.params), expected);
                }
            }
            Ok(())
        },
    );
}

/// The factory of a registered family creates a space whose config names the family and holds
/// the canonical params of the constructor, for random valid params of the schema. A VectorDB
/// constructor takes `capacity` and `dimensions`, any metric, and a mirror only with a metric
/// that ChromaDB indexes.
#[test]
fn prop_factory_config_names_its_family() {
    let families: Vec<SpaceFactoryUrn> = registered_families().map(|(family, _)| family).collect();
    check(
        runtime_config(),
        (
            select(families),
            1i64..9,
            any::<bool>(),
            proptest::option::of(any::<bool>()),
            proptest::option::of(1i64..5),
            1i64..5,
            proptest::option::of(select(METRICS.to_vec())),
            any::<bool>(),
        ),
        |(family, size, label, cyclic, priorities, dimensions, metric, mirror)| {
            let schema = constructor_schema(family).expect("a registered family has a schema");
            let mirrored = mirror && MIRRORED_METRICS.contains(&metric.unwrap_or("cosine"));
            let mut pairs = BTreeMap::new();
            for key in schema {
                let value = match *key {
                    SIZE_PARAM | CAPACITY_PARAM => Some(int(size)),
                    DIMENSIONS_PARAM => Some(int(dimensions)),
                    CYCLIC_PARAM => cyclic.map(flag),
                    PRIORITIES_PARAM => priorities.map(int),
                    METRIC_PARAM => metric.map(string),
                    MIRROR_PARAM => mirrored.then(|| string(MIRROR_CHROMA)),
                    _ => None,
                };
                if let Some(value) = value {
                    pairs.insert((*key).to_string(), value);
                }
            }
            tokio_runtime().block_on(async {
                let mut session = Session::new().await;
                session
                    .run(
                        &format!(
                            "new factory(`{family}`) in {{ for (space <- factory!?({})) {{ \
                             @\"rrs-p5-prop\"!(*space) }} }}",
                            rholang_ctor(&pairs)
                        ),
                        &format!("rrs-p5-prop {family} {size} {label}"),
                    )
                    .await;
                let space = native(&session.sole("rrs-p5-prop").await);
                let config = stored_config(&session, space).await;
                prop_assert_eq!(family_of(&config), Ok(family));
                prop_assert_eq!(config.params, pairs);
                prop_assert_eq!(config.space_id, space.sid);
                Ok(())
            })
        },
    );
}
