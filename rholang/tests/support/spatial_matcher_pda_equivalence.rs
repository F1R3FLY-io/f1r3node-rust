//! Differential between the production PDA and the exact pre-PDA matcher.

#[path = "../../../models/tests/par_corpus/mod.rs"]
mod par_corpus;

use std::collections::{BTreeMap, BTreeSet};

use models::rhoapi::connective::ConnectiveInstance::{
    ConnAndBody, ConnBool, ConnByteArray, ConnInt, ConnNotBody, ConnOrBody, ConnString, ConnUri,
    VarRefBody,
};
use models::rhoapi::expr::ExprInstance::EPathmapBody;
use models::rhoapi::{
    Bundle, Connective, ConnectiveBody, DeployId, DeployerId, EPathMap, ETuple, Expr, GBigRational,
    GFixedPoint, GUnforgeable, If, KeyValuePair, Match, New, PCost, Par, Receive, ReceiveBind,
    Send, VarRef,
};
use models::rust::par_bag::ParBag;
use models::rust::par_construction::ParConstruction;
use models::rust::rholang::match_path::{encode_match_path, encode_par_match_atom};
use models::rust::rholang::match_schema::{RhoPathCursor, RhoPathEvent};
use models::rust::rholang::match_schema_tables::{ParCursor, RHO_MATCH_REGISTRY};
use models::rust::utils::{
    new_elist_par, new_emap_par, new_eset_par, new_etuple_par, new_freevar_expr, new_freevar_par,
    new_gbool_par, new_gbytearray_par, new_gint_par, new_gstring_par, new_guri_par,
    new_wildcard_par,
};
use pathmap::PathMap;
use rspace_plus_plus::rspace::rho_path::{encode_structural_sequence, RhoPathQuery};

use super::r#match::{rho_path_query_atom, rho_path_query_cursor_atom};
use super::recursive_oracle::spatial_matcher::{
    SpatialMatcher as RecursiveSpatialMatcher, SpatialMatcherContext as RecursiveContext,
};
use super::spatial_matcher::{SpatialMatcher, SpatialMatcherContext};
use super::spatial_matcher_pda::{
    connective_candidate_positions_with_stats, encoded_expr_outer_route, if_target_index_shape,
    match_par_list_with_stats, match_par_with_stats, match_send_list_with_remainder_stats,
    match_send_list_with_stats, send_target_index_shape, EncodedExprOuterRoute,
};

#[test]
fn encoded_expr_outer_route_rejects_only_proven_different_tags() {
    use EncodedExprOuterRoute::{Delegate, RejectProvenDifferent};

    for target in [None, Some(10), Some(11)] {
        for pattern in [None, Some(10), Some(11)] {
            let expected = match (target, pattern) {
                (Some(target), Some(pattern)) if target != pattern => RejectProvenDifferent,
                _ => Delegate,
            };
            assert_eq!(encoded_expr_outer_route(target, pattern), expected);
        }
    }
}

fn compare(label: &str, target: Par, pattern: Par) {
    let mut recursive = RecursiveContext::new();
    let recursive_verdict = recursive
        .spatial_match(target.clone(), pattern.clone())
        .is_some();
    let recursive_map = recursive.free_map;

    if recursive_verdict {
        let mut captures = PathMap::new();
        if let Some(atom) = rho_path_query_atom(&pattern, &mut captures) {
            let encoded_target = encode_par_match_atom(&target);
            let target_structure = encode_structural_sequence([encoded_target.as_slice()]);
            let query = RhoPathQuery::structural_sequence([atom], false);
            assert!(
                query.matches_structure(&target_structure),
                "{label}: a compiled RhoPath query rejected a recursive-oracle success"
            );
        }
    }

    let mut pda = SpatialMatcherContext::new();
    let pda_verdict = pda.spatial_match(target, pattern).is_some();
    assert_eq!(pda_verdict, recursive_verdict, "{label}: verdict");
    assert_eq!(pda.free_map, recursive_map, "{label}: FreeMap");
}

fn cover_generated_query_root(
    case: &str,
    root: ParCursor<'_>,
    observed: &mut BTreeSet<&'static str>,
    compiled: &mut usize,
    pending_bag_components: &mut Vec<OwnedGeneratedQueryRoot>,
) {
    for event in RhoPathCursor::new(root) {
        match event {
            RhoPathEvent::EnterNode { cursor, program } => {
                observed.insert(program.type_name);
                let encoded = encode_match_path(cursor);
                let structure = encode_structural_sequence([encoded.as_slice()]);
                let mut captures = PathMap::new();
                let atom = rho_path_query_cursor_atom(cursor, true, &mut captures)
                    .unwrap_or_else(|| panic!("{case}/{}: exact cursor query", program.type_name));
                let query = RhoPathQuery::structural_sequence([atom], false);
                assert!(
                    query.matches_structure(&structure),
                    "{case}/{}: exact generated cursor identity",
                    program.type_name
                );
                *compiled += 1;
            }
            RhoPathEvent::ParBag(bag) => {
                enqueue_par_bag_components(bag, pending_bag_components);
            }
            _ => {}
        }
    }
}

/// Owned roots decoded one endpoint at a time from an opaque ParBag event.
/// RMP1 deliberately treats a ParBag as one compact token in production, but
/// the generated-query coverage proof must still exercise every schema program
/// hidden behind those tokens. This explicit heap queue follows that ownership
/// boundary without reintroducing recursive test traversal or a retained
/// component vector in production.
enum OwnedGeneratedQueryRoot {
    Send(Send),
    Receive(Receive),
    New(New),
    Expr(Expr),
    Match(Match),
    Unforgeable(GUnforgeable),
    Connective(Connective),
    Bundle(Bundle),
    If(If),
}

impl OwnedGeneratedQueryRoot {
    fn cursor(&self) -> ParCursor<'_> {
        match self {
            Self::Send(value) => ParCursor::from(value),
            Self::Receive(value) => ParCursor::from(value),
            Self::New(value) => ParCursor::from(value),
            Self::Expr(value) => ParCursor::from(value),
            Self::Match(value) => ParCursor::from(value),
            Self::Unforgeable(value) => ParCursor::from(value),
            Self::Connective(value) => ParCursor::from(value),
            Self::Bundle(value) => ParCursor::from(value),
            Self::If(value) => ParCursor::from(value),
        }
    }
}

fn enqueue_par_bag_components(bag: &ParBag, pending: &mut Vec<OwnedGeneratedQueryRoot>) {
    macro_rules! enqueue {
        ($ty:ty, $variant:ident) => {
            bag.try_visit_decoded::<$ty, std::convert::Infallible>(|value, _multiplicity| {
                pending.push(OwnedGeneratedQueryRoot::$variant(value));
                Ok(())
            })
            .expect("an authoritative ParBag contains canonical component keys");
        };
    }

    enqueue!(Send, Send);
    enqueue!(Receive, Receive);
    enqueue!(New, New);
    enqueue!(Expr, Expr);
    enqueue!(Match, Match);
    enqueue!(GUnforgeable, Unforgeable);
    enqueue!(Connective, Connective);
    enqueue!(Bundle, Bundle);
    enqueue!(If, If);
}

#[test]
fn compiled_rho_path_query_is_a_superset_of_every_recursive_oracle_success() {
    let expected = RHO_MATCH_REGISTRY
        .iter()
        .map(|(_, program)| program.type_name)
        .collect::<BTreeSet<_>>();
    let mut observed = BTreeSet::new();
    let mut compiled = 0usize;
    let mut pending_bag_components = Vec::new();

    for (case, par) in par_corpus::par_corpus() {
        cover_generated_query_root(
            &format!("Par/{case}"),
            ParCursor::from(&par),
            &mut observed,
            &mut compiled,
            &mut pending_bag_components,
        );
    }
    for (case, root) in par_corpus::list_par_with_random_corpus() {
        cover_generated_query_root(
            &format!("ListParWithRandom/{case}"),
            ParCursor::from(&root),
            &mut observed,
            &mut compiled,
            &mut pending_bag_components,
        );
    }
    for (case, root) in par_corpus::bind_pattern_corpus() {
        cover_generated_query_root(
            &format!("BindPattern/{case}"),
            ParCursor::from(&root),
            &mut observed,
            &mut compiled,
            &mut pending_bag_components,
        );
    }
    for (case, root) in par_corpus::tagged_continuation_corpus() {
        cover_generated_query_root(
            &format!("TaggedContinuation/{case}"),
            ParCursor::from(&root),
            &mut observed,
            &mut compiled,
            &mut pending_bag_components,
        );
    }
    for (case, root) in par_corpus::par_with_random_corpus() {
        cover_generated_query_root(
            &format!("ParWithRandom/{case}"),
            ParCursor::from(&root),
            &mut observed,
            &mut compiled,
            &mut pending_bag_components,
        );
    }
    for (case, root) in par_corpus::list_bind_patterns_corpus() {
        cover_generated_query_root(
            &format!("ListBindPatterns/{case}"),
            ParCursor::from(&root),
            &mut observed,
            &mut compiled,
            &mut pending_bag_components,
        );
    }

    let cost = PCost::default();
    cover_generated_query_root(
        "PCost/default",
        ParCursor::from(&cost),
        &mut observed,
        &mut compiled,
        &mut pending_bag_components,
    );
    let deploy_id = DeployId::default();
    cover_generated_query_root(
        "DeployId/default",
        ParCursor::from(&deploy_id),
        &mut observed,
        &mut compiled,
        &mut pending_bag_components,
    );
    let deployer_id = DeployerId::default();
    cover_generated_query_root(
        "DeployerId/default",
        ParCursor::from(&deployer_id),
        &mut observed,
        &mut compiled,
        &mut pending_bag_components,
    );

    while let Some(root) = pending_bag_components.pop() {
        let case = format!("ParBag/{}", root.cursor().program().type_name);
        cover_generated_query_root(
            &case,
            root.cursor(),
            &mut observed,
            &mut compiled,
            &mut pending_bag_components,
        );
    }

    assert_eq!(
        observed, expected,
        "the query-compiler corpus must cover every generated schema program"
    );
    assert!(
        compiled > expected.len(),
        "the compiler differential must exercise repeated nested programs"
    );

    compare(
        "formal bridge: ordered nested capture",
        list(vec![new_gint_par(11, Vec::new(), false)], false),
        list(vec![new_freevar_par(0, Vec::new())], true),
    );
    compare(
        "formal bridge: generated Send capture",
        send(
            new_gint_par(7, Vec::new(), false),
            vec![new_gint_par(11, Vec::new(), false)],
            false,
        ),
        send(
            new_freevar_par(0, Vec::new()),
            vec![new_freevar_par(1, Vec::new())],
            true,
        ),
    );
}

#[test]
fn declined_nonlinear_query_does_not_publish_speculative_captures() {
    let repeated = list(
        vec![
            new_freevar_par(0, Vec::new()),
            new_freevar_par(0, Vec::new()),
        ],
        true,
    );
    let mut captures = PathMap::new();
    assert!(rho_path_query_atom(&repeated, &mut captures).is_none());
    assert!(
        captures.is_empty(),
        "a declined query must roll back its speculative capture branch"
    );

    assert!(rho_path_query_atom(&new_freevar_par(0, Vec::new()), &mut captures).is_some());
    assert_eq!(captures.val_count(), 1);
}

fn list(ps: Vec<Par>, connective_used: bool) -> Par {
    new_elist_par(
        ps,
        Vec::new(),
        connective_used,
        None,
        Vec::new(),
        connective_used,
    )
}

fn tuple(ps: Vec<Par>, connective_used: bool) -> Par {
    models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(models::rhoapi::expr::ExprInstance::ETupleBody(
                ETuple {
                    ps,
                    locally_free: Vec::new(),
                    connective_used,
                },
            )),
        }],
        connective_used,
        ..Default::default()
    }
}

fn connective(instance: models::rhoapi::connective::ConnectiveInstance) -> Par {
    models::par_from_default! {
        connectives: vec![Connective {
            connective_instance: Some(instance),
        }],
        connective_used: true,
        ..Default::default()
    }
}

fn inactive_connective(instance: models::rhoapi::connective::ConnectiveInstance) -> Par {
    Par::default().with_connectives(vec![Connective {
        connective_instance: Some(instance),
    }])
}

#[derive(Clone, Copy)]
struct NominalFrame {
    pattern: usize,
    next_target: usize,
    pending_target: Option<usize>,
}

/// Exact pre-reuse Kuhn work counter. This oracle owns no relation cache and
/// is intentionally separate from the production PDA.
fn nominal_edge_evaluations(
    pattern_count: usize,
    target_count: usize,
    edge: impl Fn(usize, usize) -> bool,
) -> (bool, usize) {
    let mut assignments = vec![None; target_count];
    let mut evaluations = 0;

    for root_pattern in 0..pattern_count {
        let mut seen_targets = vec![false; target_count];
        let mut search = vec![NominalFrame {
            pattern: root_pattern,
            next_target: 0,
            pending_target: None,
        }];
        let mut augmented = false;

        loop {
            let Some(frame) = search.last_mut() else {
                break;
            };
            let mut descended = false;
            while frame.next_target < target_count {
                let target = frame.next_target;
                frame.next_target += 1;
                if seen_targets[target] {
                    continue;
                }
                evaluations += 1;
                if !edge(frame.pattern, target) {
                    continue;
                }
                seen_targets[target] = true;
                match assignments[target] {
                    None => {
                        assignments[target] = Some(frame.pattern);
                        search.pop();
                        while let Some(parent) = search.pop() {
                            assignments[parent
                                .pending_target
                                .expect("a nominal parent carries its displaced edge")] =
                                Some(parent.pattern);
                        }
                        augmented = true;
                        break;
                    }
                    Some(previous_pattern) => {
                        frame.pending_target = Some(target);
                        search.push(NominalFrame {
                            pattern: previous_pattern,
                            next_target: 0,
                            pending_target: None,
                        });
                        descended = true;
                        break;
                    }
                }
            }
            if augmented {
                break;
            }
            if descended {
                continue;
            }
            search.pop();
            if let Some(parent) = search.last_mut() {
                parent.pending_target = None;
            } else {
                break;
            }
        }
        if !augmented {
            return (false, evaluations);
        }
    }

    (true, evaluations)
}

fn send_value(chan: Par, data: Vec<Par>, connective_used: bool) -> Send {
    Send {
        chan: Some(chan),
        data,
        persistent: false,
        locally_free: Vec::new(),
        connective_used,
    }
}

fn send(chan: Par, data: Vec<Par>, connective_used: bool) -> Par {
    models::par_from_default! {
        sends: vec![send_value(chan, data, connective_used)],
        connective_used,
        ..Default::default()
    }
}

fn receive(source: Par, body: Par, connective_used: bool) -> Par {
    models::par_from_default! {
        receives: vec![Receive {
            binds: vec![ReceiveBind {
                patterns: vec![Par::default()],
                source: Some(source),
                remainder: None,
                free_count: 0,
            }],
            body: Some(body),
            persistent: false,
            peek: false,
            bind_count: 1,
            locally_free: Vec::new(),
            connective_used,
            condition: None,
        }],
        connective_used,
        ..Default::default()
    }
}

fn new_scope(body: Par, connective_used: bool) -> Par {
    models::par_from_default! {
        news: vec![New {
            bind_count: 1,
            p: Some(body),
            uri: Vec::new(),
            injections: BTreeMap::new(),
            locally_free: Vec::new(),
        }],
        connective_used,
        ..Default::default()
    }
}

fn conditional(condition: Par, if_true: Par, if_false: Par, connective_used: bool) -> Par {
    models::par_from_default! {
        conditionals: vec![If {
            condition: Some(condition),
            if_true: Some(if_true),
            if_false: Some(if_false),
            locally_free: Vec::new(),
            connective_used,
        }],
        connective_used,
        ..Default::default()
    }
}

fn pathmap(map: EPathMap, connective_used: bool) -> Par {
    models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(EPathmapBody(map)),
        }],
        connective_used,
        ..Default::default()
    }
}

#[test]
fn recursive_oracle_and_pda_agree_on_the_semantic_corpus() {
    let one = || new_gint_par(1, Vec::new(), false);
    let two = || new_gint_par(2, Vec::new(), false);
    let seven = || new_gint_par(7, Vec::new(), false);
    let free = |level| new_freevar_par(level, Vec::new());

    compare("ground equality", one(), one());
    compare("ground refusal", one(), two());
    compare("free capture", seven(), free(0));
    compare("wildcard", seven(), new_wildcard_par(Vec::new(), true));

    for depth in 0..=8 {
        let mut target = seven();
        let mut pattern = free(0);
        for _ in 0..depth {
            target = list(vec![target], false);
            pattern = list(vec![pattern], true);
        }
        compare(&format!("nested list depth {depth}"), target, pattern);
    }

    compare(
        "tuple fields",
        new_etuple_par(vec![one(), two()]),
        tuple(vec![free(0), free(1)], true),
    );
    compare(
        "nested tuple fields",
        tuple(vec![seven(), tuple(vec![one(), two()], false)], false),
        tuple(vec![free(0), tuple(vec![free(1), free(2)], true)], true),
    );
    compare(
        "send channel and data",
        send(one(), vec![seven()], false),
        send(free(0), vec![free(1)], true),
    );
    let duplicate = send(one(), vec![seven()], false)
        .first::<Send>()
        .unwrap()
        .unwrap()
        .0;
    let duplicate_target = models::par_from_default! {
        sends: vec![duplicate.clone(), duplicate.clone(), duplicate],
        ..Default::default()
    };
    let mut duplicate_pattern = send(one(), vec![free(0)], true);
    let mut expressions = duplicate_pattern.take_occurrences::<Expr>();
    expressions.push(new_freevar_expr(1));
    duplicate_pattern.replace_exprs(expressions);
    duplicate_pattern.connective_used = true;
    compare(
        "Par remainder over byte-identical sends",
        duplicate_target,
        duplicate_pattern,
    );
    compare(
        "receive source and body",
        receive(one(), seven(), false),
        receive(free(0), free(1), true),
    );
    compare(
        "new body",
        new_scope(seven(), false),
        new_scope(free(0), true),
    );
    compare(
        "conditional children",
        conditional(
            new_gint_par(11, Vec::new(), false),
            new_gint_par(12, Vec::new(), false),
            new_gint_par(13, Vec::new(), false),
            false,
        ),
        conditional(free(0), free(1), free(2), true),
    );
    compare(
        "conditional child refusal",
        conditional(
            new_gint_par(11, Vec::new(), false),
            new_gint_par(12, Vec::new(), false),
            new_gint_par(13, Vec::new(), false),
            false,
        ),
        conditional(free(0), new_gint_par(99, Vec::new(), false), free(1), true),
    );

    compare(
        "conjunction",
        seven(),
        connective(ConnAndBody(ConnectiveBody {
            ps: vec![free(0), seven()],
        })),
    );
    compare(
        "failed conjunction rolls back every earlier conjunct binding",
        seven(),
        connective(ConnAndBody(ConnectiveBody {
            ps: vec![free(0), two()],
        })),
    );
    compare(
        "disjunction",
        seven(),
        connective(ConnOrBody(ConnectiveBody {
            ps: vec![two(), free(0)],
        })),
    );
    compare(
        "disjunction isolates a failed binding branch before success",
        seven(),
        connective(ConnOrBody(ConnectiveBody {
            ps: vec![
                connective(ConnAndBody(ConnectiveBody {
                    ps: vec![free(0), two()],
                })),
                seven(),
            ],
        })),
    );
    compare("negation success", seven(), connective(ConnNotBody(two())));
    compare(
        "negation refusal",
        seven(),
        connective(ConnNotBody(free(0))),
    );

    compare(
        "legacy set",
        new_eset_par(
            vec![one(), two()],
            Vec::new(),
            false,
            None,
            Vec::new(),
            false,
        ),
        new_eset_par(
            vec![one(), free(0)],
            Vec::new(),
            true,
            None,
            Vec::new(),
            true,
        ),
    );
    compare(
        "legacy set Bag residual",
        new_eset_par(
            vec![one(), two(), seven()],
            Vec::new(),
            false,
            None,
            Vec::new(),
            false,
        ),
        new_eset_par(
            vec![two()],
            Vec::new(),
            true,
            Some(models::rust::utils::new_freevar_var(4)),
            Vec::new(),
            true,
        ),
    );
    compare(
        "legacy map",
        new_emap_par(
            vec![KeyValuePair {
                key: Some(one()),
                value: Some(seven()),
            }],
            Vec::new(),
            false,
            None,
            Vec::new(),
            false,
        ),
        new_emap_par(
            vec![KeyValuePair {
                key: Some(one()),
                value: Some(free(0)),
            }],
            Vec::new(),
            true,
            None,
            Vec::new(),
            true,
        ),
    );

    let mut target_set = EPathMap::default();
    target_set.insert_entry(one());
    target_set.insert_entry(two());
    let mut pattern_set = EPathMap::default();
    pattern_set.insert_entry(one());
    pattern_set.insert_entry(free(0));
    pattern_set.connective_used = true;
    compare(
        "PathMap set",
        pathmap(target_set, false),
        pathmap(pattern_set, true),
    );

    let mut target_map = EPathMap::default();
    target_map.insert_map_entry(one(), seven()).unwrap();
    let mut pattern_map = EPathMap::default();
    pattern_map.insert_map_entry(one(), free(0)).unwrap();
    pattern_map.connective_used = true;
    compare(
        "PathMap map",
        pathmap(target_map, false),
        pathmap(pattern_map, true),
    );

    compare(
        "empty PathMap",
        pathmap(EPathMap::default(), false),
        pathmap(EPathMap::default(), false),
    );

    let singleton_set = |member: Par| {
        let mut map = EPathMap::default();
        map.insert_entry(member);
        map
    };
    compare(
        "concrete singleton PathMap set",
        pathmap(singleton_set(one()), false),
        pathmap(singleton_set(one()), false),
    );
    compare(
        "concrete singleton PathMap set mismatch",
        pathmap(singleton_set(one()), false),
        pathmap(singleton_set(two()), false),
    );

    let singleton_map = |key: Par, value: Par| {
        let mut map = EPathMap::default();
        map.insert_map_entry(key, value).unwrap();
        map
    };
    compare(
        "concrete singleton PathMap map",
        pathmap(singleton_map(one(), seven()), false),
        pathmap(singleton_map(one(), seven()), false),
    );
    compare(
        "concrete singleton PathMap map key mismatch",
        pathmap(singleton_map(one(), seven()), false),
        pathmap(singleton_map(two(), seven()), false),
    );
    compare(
        "concrete singleton PathMap map value mismatch",
        pathmap(singleton_map(one(), seven()), false),
        pathmap(singleton_map(one(), two()), false),
    );

    compare(
        "string mismatch",
        new_gstring_par("a".into(), Vec::new(), false),
        new_gstring_par("b".into(), Vec::new(), false),
    );
}

#[test]
fn production_list_pda_preserves_the_attempt_baseline_when_committing_deltas() {
    let baseline_value = new_gstring_par("baseline".into(), Vec::new(), false);
    let first = new_gint_par(11, Vec::new(), false);
    let second = new_gint_par(29, Vec::new(), false);
    let mut context = SpatialMatcherContext::new();
    context.free_map.insert(41, baseline_value.clone());

    let (result, _) =
        match_par_list_with_stats(&mut context, vec![first.clone(), second.clone()], vec![
            first,
            new_freevar_par(0, Vec::new()),
        ]);

    assert!(result.is_some());
    assert_eq!(context.free_map.len(), 2);
    assert_eq!(context.free_map.get(&41), Some(&baseline_value));
    assert_eq!(context.free_map.get(&0), Some(&second));
}

#[test]
fn production_list_pda_matches_the_oracle_across_orderings_duplicates_and_nonlinearity() {
    const PERMUTATIONS: [[usize; 3]; 6] =
        [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [
            2, 1, 0,
        ]];

    let integer = |value| new_gint_par(value, Vec::new(), false);
    for target_values in [[1, 2, 3], [1, 1, 2]] {
        for target_order in PERMUTATIONS {
            let targets = target_order
                .map(|index| integer(target_values[index]))
                .into_iter()
                .collect::<Vec<_>>();
            for pattern_order in PERMUTATIONS {
                let base_patterns = [
                    new_freevar_par(0, Vec::new()),
                    integer(2),
                    new_freevar_par(1, Vec::new()),
                ];
                let patterns = pattern_order
                    .map(|index| base_patterns[index].clone())
                    .into_iter()
                    .collect::<Vec<_>>();
                compare(
                    &format!(
                        "AC ordering target={target_order:?} pattern={pattern_order:?} values={target_values:?}"
                    ),
                    list(targets.clone(), false),
                    list(patterns, true),
                );

                let nonlinear_base = [
                    new_freevar_par(0, Vec::new()),
                    integer(2),
                    new_freevar_par(0, Vec::new()),
                ];
                let nonlinear = pattern_order
                    .map(|index| nonlinear_base[index].clone())
                    .into_iter()
                    .collect::<Vec<_>>();
                compare(
                    &format!(
                        "nonlinear AC refusal target={target_order:?} pattern={pattern_order:?} values={target_values:?}"
                    ),
                    list(targets.clone(), false),
                    list(nonlinear, true),
                );
            }
        }
    }
}

#[test]
fn production_list_pda_relational_counters_meet_preregistered_controls() {
    const WIDTH: usize = 128;
    const SAMPLES: usize = 51;

    let integers = || {
        (0..WIDTH)
            .map(|value| new_gint_par(value as i64, Vec::new(), false))
            .collect::<Vec<_>>()
    };

    let diagonal_patterns = integers();
    let (diagonal_verdict, diagonal_control) =
        nominal_edge_evaluations(WIDTH, WIDTH, |pattern, target| pattern == target);
    let mut diagonal_context = SpatialMatcherContext::new();
    let (diagonal_result, diagonal_stats) =
        match_par_list_with_stats(&mut diagonal_context, integers(), diagonal_patterns);
    assert!(diagonal_verdict);
    assert!(diagonal_result.is_some());
    assert_eq!(diagonal_stats.target_index_builds, 1);
    assert_eq!(diagonal_stats.edge_evaluations, WIDTH);
    assert!(diagonal_stats.edge_evaluations < diagonal_control);
    assert_eq!(diagonal_stats.relation_row_reuses, 0);
    assert_eq!(diagonal_stats.cached_edge_visits, 0);

    let chain_edge = |pattern: usize, target: usize| {
        if pattern == 0 {
            target == 1
        } else {
            target == pattern || target == pattern + 1
        }
    };
    let (chain_verdict, chain_control) = nominal_edge_evaluations(WIDTH, WIDTH, chain_edge);
    assert!(
        !chain_verdict,
        "target zero deliberately has no incoming edge"
    );

    let chain_patterns = || {
        (0..WIDTH)
            .map(|pattern| {
                if pattern == 0 {
                    new_gint_par(1, Vec::new(), false)
                } else {
                    connective(ConnOrBody(ConnectiveBody {
                        ps: vec![
                            new_gint_par(pattern as i64, Vec::new(), false),
                            new_gint_par((pattern + 1) as i64, Vec::new(), false),
                        ],
                    }))
                }
            })
            .collect::<Vec<_>>()
    };

    let mut control_samples = Vec::with_capacity(SAMPLES);
    let mut treatment_samples = Vec::with_capacity(SAMPLES);
    // Pattern zero has the singleton domain `{1}`. Each interior disjunction
    // has two exact candidates and the last has one in-range candidate, so
    // PathMap union reduces the complete semantic relation to exactly
    // `1 + 2*(WIDTH-2) + 1` evaluated edges. The nominal Cartesian oracle is
    // retained above as the before-work witness.
    let connective_domain_evaluations = 2 * (WIDTH - 1);
    for _ in 0..SAMPLES {
        let mut context = SpatialMatcherContext::new();
        let (result, stats) = match_par_list_with_stats(&mut context, integers(), chain_patterns());
        assert!(result.is_none());
        assert_eq!(stats.edge_evaluations, connective_domain_evaluations);
        assert!(stats.relation_row_reuses > 0);
        assert!(stats.cached_edge_visits > 0);
        assert!(stats.successful_edge_evaluations <= 2 * WIDTH);
        assert!(stats.edge_evaluations < chain_control);
        control_samples.push(chain_control);
        treatment_samples.push(stats.edge_evaluations);
    }

    println!(
        "D_E4_PRODUCTION_COUNTERS width={WIDTH} control={control_samples:?} treatment={treatment_samples:?} invariant={diagonal_control}"
    );
}

#[test]
fn production_list_pda_connective_candidate_algebra_is_conservative_and_exactly_ordered() {
    let integer = |value| new_gint_par(value, Vec::new(), false);
    let disjunction = |ps| connective(ConnOrBody(ConnectiveBody { ps }));
    let conjunction = |ps| connective(ConnAndBody(ConnectiveBody { ps }));

    let targets = vec![integer(0), integer(1), integer(4)];
    let patterns = vec![
        disjunction(vec![integer(1), integer(3)]),
        conjunction(vec![
            disjunction(vec![integer(2), integer(4)]),
            disjunction(vec![integer(4), integer(5)]),
        ]),
        integer(0),
    ];
    let mut context = SpatialMatcherContext::new();
    let (result, stats) =
        match_par_list_with_stats(&mut context, targets.clone(), patterns.clone());
    assert!(result.is_some());
    assert_eq!(
        stats.edge_evaluations, 3,
        "union produces [1], nested intersection produces [4], and exact zero produces [0]"
    );

    compare(
        "PathMap connective-domain algebra preserves the recursive oracle",
        new_eset_par(targets, Vec::new(), false, None, Vec::new(), false),
        new_eset_par(patterns, Vec::new(), true, None, Vec::new(), true),
    );

    // Empty OR is authoritative refusal, so its empty derived domain may fail
    // before any broader row. Empty AND is true and therefore deliberately
    // compiles to All rather than to an empty set.
    let mut context = SpatialMatcherContext::new();
    let (result, stats) =
        match_par_list_with_stats(&mut context, vec![integer(0), integer(1)], vec![
            integer(0),
            disjunction(Vec::new()),
        ]);
    assert!(result.is_none());
    assert_eq!(stats.edge_evaluations, 0);

    let mut context = SpatialMatcherContext::new();
    let (result, stats) =
        match_par_list_with_stats(&mut context, vec![integer(0), integer(1)], vec![
            integer(0),
            conjunction(Vec::new()),
        ]);
    assert!(result.is_some());
    assert!(stats.edge_evaluations >= 2);

    // NOT cannot safely complement a conservative query. In OR it annihilates
    // pruning (union with All); in AND the exact positive arm remains a sound
    // necessary condition. Both paths still obtain their verdict from the
    // spatial matcher.
    let negated_two = connective(ConnNotBody(integer(2)));
    let mut context = SpatialMatcherContext::new();
    let (result, or_stats) =
        match_par_list_with_stats(&mut context, vec![integer(0), integer(1)], vec![
            disjunction(vec![integer(9), negated_two.clone()]),
            integer(1),
        ]);
    assert!(result.is_some());
    assert!(or_stats.edge_evaluations >= 2);

    let mut context = SpatialMatcherContext::new();
    let (result, and_stats) =
        match_par_list_with_stats(&mut context, vec![integer(0), integer(1)], vec![
            conjunction(vec![integer(0), negated_two]),
            integer(1),
        ]);
    assert!(result.is_some());
    assert_eq!(and_stats.edge_evaluations, 2);

    // A pure connective carrier is evaluated directly against the whole Par.
    // Proper subsets must not be accepted merely because one connective arm
    // matches them; these compound cases prove the fast transition against
    // the pre-PDA oracle in both directions.
    let mut compound = integer(0);
    let second = integer(1);
    compound = compound.append(second);
    compare(
        "pure OR carrier accepts the whole compound target",
        compound.clone(),
        disjunction(vec![integer(9), compound.clone()]),
    );
    compare(
        "pure OR carrier rejects a matching proper subset",
        compound.clone(),
        disjunction(vec![integer(0)]),
    );
    compare(
        "pure AND carrier constrains the whole compound target",
        compound.clone(),
        conjunction(vec![compound, connective(ConnNotBody(integer(9)))]),
    );
}

#[test]
fn byte_native_connective_authority_covers_every_terminal_shape() {
    for payload in [false, true] {
        compare(
            "encoded Bool predicate",
            new_gbool_par(false, Vec::new(), false),
            connective(ConnBool(payload)),
        );
        compare(
            "encoded Int predicate",
            new_gint_par(-7, Vec::new(), false),
            connective(ConnInt(payload)),
        );
        compare(
            "encoded String predicate",
            new_gstring_par("value".to_owned(), Vec::new(), false),
            connective(ConnString(payload)),
        );
        compare(
            "encoded Uri predicate",
            new_guri_par("rho:id:test".to_owned(), Vec::new(), false),
            connective(ConnUri(payload)),
        );
        compare(
            "encoded ByteArray predicate",
            new_gbytearray_par(vec![0, 1, 0xff], Vec::new(), false),
            connective(ConnByteArray(payload)),
        );
    }

    compare(
        "encoded predicate rejects a different scalar family",
        new_gint_par(1, Vec::new(), false),
        connective(ConnBool(true)),
    );
    compare(
        "empty encoded AND is true",
        new_gint_par(1, Vec::new(), false),
        connective(ConnAndBody(ConnectiveBody { ps: Vec::new() })),
    );
    compare(
        "empty encoded OR is false",
        new_gint_par(1, Vec::new(), false),
        connective(ConnOrBody(ConnectiveBody { ps: Vec::new() })),
    );
    compare(
        "encoded VarRef remains unconditional refusal",
        new_gint_par(1, Vec::new(), false),
        connective(VarRefBody(VarRef { index: 0, depth: 0 })),
    );
    compare(
        "absent encoded Connective remains unconditional refusal",
        new_gint_par(1, Vec::new(), false),
        Par::default().with_connectives(vec![Connective::default()]),
    );
}

#[test]
fn byte_native_connective_respects_child_and_root_dispatch_metadata() {
    let inactive_predicate = inactive_connective(ConnInt(true));
    let integer = new_gint_par(7, Vec::new(), false);

    compare(
        "inactive root Connective remains concrete data",
        inactive_predicate.clone(),
        inactive_predicate.clone(),
    );
    compare(
        "inactive root Connective is not an integer predicate",
        integer.clone(),
        inactive_predicate.clone(),
    );

    let unary = connective(ConnAndBody(ConnectiveBody {
        ps: vec![inactive_predicate.clone()],
    }));
    compare(
        "unary spine stops before an inactive Connective child",
        integer.clone(),
        unary.clone(),
    );
    let mut context = SpatialMatcherContext::new();
    let (result, unary_stats) = match_par_with_stats(&mut context, integer.clone(), unary);
    assert!(result.is_none());
    assert_eq!(
        unary_stats.authoritative_connective_evaluations, 0,
        "the unary normalizer must stop before the inactive child"
    );

    let branching = connective(ConnAndBody(ConnectiveBody {
        ps: vec![inactive_predicate.clone(), inactive_predicate.clone()],
    }));
    compare(
        "branching encoded descent delegates inactive Connective children",
        integer.clone(),
        branching.clone(),
    );
    let mut context = SpatialMatcherContext::new();
    let (result, branching_stats) =
        match_par_with_stats(&mut context, integer.clone(), branching.clone());
    assert!(result.is_none());
    assert_eq!(
        branching_stats.authoritative_connective_evaluations, 1,
        "only the active outer AND may be evaluated as logical syntax"
    );
    compare(
        "direct route preserves legacy exclusion of inactive Connective target",
        inactive_predicate.clone(),
        branching.clone(),
    );

    let metadata_sensitive = connective(ConnAndBody(ConnectiveBody {
        ps: vec![new_freevar_par(0, Vec::new()), connective(ConnInt(true))],
    }));
    compare(
        "direct route preserves locally-free projection before capture",
        integer.clone().with_locally_free(vec![0x01]),
        metadata_sensitive.clone(),
    );
    compare(
        "direct route preserves connective-used projection before capture",
        integer.clone().with_connective_used(true),
        metadata_sensitive,
    );

    for encoded_compiler in [false, true] {
        let (positions, stats) = connective_candidate_positions_with_stats(
            vec![inactive_predicate.clone(), integer.clone()],
            branching.clone(),
            encoded_compiler,
        );
        assert_eq!(
            positions.as_ref(),
            &[0],
            "candidate compilation must retain the concrete inactive-Connective target"
        );
        if encoded_compiler {
            assert!(
                stats.candidate_terminal_par_decodes >= 2,
                "both inactive children must delegate to concrete Par compilation: {stats:?}"
            );
        } else {
            assert_eq!(
                stats.candidate_resident_connective_decodes, 1,
                "only the active outer AND is a resident logical connective: {stats:?}"
            );
        }
    }
}

#[test]
fn byte_native_connective_exact_scalar_route_is_complete_and_non_vacuous() {
    use models::rhoapi::expr::ExprInstance;

    fn scalar(instance: ExprInstance) -> Par {
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(instance),
        }])
    }
    fn binary_and(pattern: Par) -> Par {
        connective(ConnAndBody(ConnectiveBody {
            ps: vec![pattern.clone(), pattern],
        }))
    }

    let cases = [
        (
            scalar(ExprInstance::GBool(false)),
            scalar(ExprInstance::GBool(true)),
        ),
        (
            scalar(ExprInstance::GInt(i64::MIN)),
            scalar(ExprInstance::GInt(i64::MAX)),
        ),
        (
            scalar(ExprInstance::GString("λ\0".to_owned())),
            scalar(ExprInstance::GString(String::new())),
        ),
        (
            scalar(ExprInstance::GUri("rho:id:λ".to_owned())),
            scalar(ExprInstance::GUri(String::new())),
        ),
        (
            scalar(ExprInstance::GByteArray(vec![0, 1, 0xff])),
            scalar(ExprInstance::GByteArray(Vec::new())),
        ),
        (
            scalar(ExprInstance::GDouble(0x7ff8_0000_0000_0042)),
            scalar(ExprInstance::GDouble(0x7ff8_0000_0000_0043)),
        ),
        (
            scalar(ExprInstance::GBigInt(vec![0x80, 0])),
            scalar(ExprInstance::GBigInt(Vec::new())),
        ),
        (
            scalar(ExprInstance::GBigRat(GBigRational {
                numerator: vec![1],
                denominator: vec![2],
            })),
            scalar(ExprInstance::GBigRat(GBigRational::default())),
        ),
        (
            scalar(ExprInstance::GFixedPoint(GFixedPoint {
                unscaled: vec![1],
                scale: u32::MAX,
            })),
            scalar(ExprInstance::GFixedPoint(GFixedPoint::default())),
        ),
    ];

    for (equal, unequal) in cases {
        let mut context = SpatialMatcherContext::new();
        let (result, stats) = match_par_with_stats(
            &mut context,
            equal.clone().with_locally_free(vec![0x01]),
            binary_and(equal.clone().with_locally_free(vec![0x80, 0x01])),
        );
        assert!(
            result.is_some(),
            "locally-free summaries do not affect equality"
        );
        assert_eq!(
            stats.authoritative_exact_scalar_attempts, 2,
            "equal scalar did not take the proven route: {equal:?}; {stats:?}"
        );
        assert_eq!(stats.authoritative_exact_scalar_equal, 2);
        assert_eq!(stats.authoritative_exact_scalar_mismatch, 0);
        assert_eq!(stats.authoritative_terminal_par_decodes, 0);

        let mut context = SpatialMatcherContext::new();
        let (result, stats) =
            match_par_with_stats(&mut context, unequal, binary_and(equal.clone()));
        assert!(result.is_none());
        assert_eq!(
            stats.authoritative_exact_scalar_attempts, 1,
            "unequal scalar did not take the proven route: {equal:?}; {stats:?}"
        );
        assert_eq!(stats.authoritative_exact_scalar_equal, 0);
        assert_eq!(stats.authoritative_exact_scalar_mismatch, 1);
        assert_eq!(stats.authoritative_terminal_par_decodes, 0);

        compare(
            "exact scalar accepted domain",
            equal.clone(),
            binary_and(equal),
        );
    }

    let mut context = SpatialMatcherContext::new();
    let (result, stats) = match_par_with_stats(
        &mut context,
        new_gint_par(7, Vec::new(), false),
        binary_and(new_gbool_par(true, Vec::new(), false)),
    );
    assert!(result.is_none(), "different scalar tags are unequal");
    assert_eq!(stats.authoritative_exact_scalar_attempts, 1);
    assert_eq!(stats.authoritative_exact_scalar_mismatch, 1);

    let delegated = new_gint_par(7, Vec::new(), false).with_connective_used(true);
    let mut context = SpatialMatcherContext::new();
    let (_, stats) = match_par_with_stats(
        &mut context,
        new_gint_par(7, Vec::new(), false),
        binary_and(delegated),
    );
    assert_eq!(stats.authoritative_exact_scalar_attempts, 0);
    assert_eq!(stats.authoritative_exact_scalar_unproved, 2);
    assert_eq!(stats.authoritative_terminal_par_decodes, 2);
}

fn planned_skewed_connective(depth: usize, conjunction: bool) -> Par {
    let mut construction = ParConstruction::new();
    let terminal = new_gint_par(1, Vec::new(), false);
    let mut nested = construction
        .import_par(&terminal)
        .expect("import connective terminal");
    let sibling = nested;
    let shell = Connective {
        connective_instance: Some(if conjunction {
            ConnAndBody(ConnectiveBody {
                ps: vec![Par::default(), Par::default()],
            })
        } else {
            ConnOrBody(ConnectiveBody {
                ps: vec![Par::default(), Par::default()],
            })
        }),
    };
    for _ in 0..depth {
        let component = construction
            .connective_component(&shell, &[nested, sibling])
            .expect("plan skewed binary connective");
        nested = construction
            .singleton_component(component, Vec::new(), true)
            .expect("plan connective carrier");
    }
    construction.finish(nested).expect("finish connective plan")
}

fn planned_skewed_failing_or(depth: usize) -> Par {
    let mut construction = ParConstruction::new();
    let mut nested = construction
        .import_par(&new_gint_par(1, Vec::new(), false))
        .expect("import OR success terminal");
    let failure = construction
        .import_par(&new_gint_par(0, Vec::new(), false))
        .expect("import OR refusal terminal");
    let shell = Connective {
        connective_instance: Some(ConnOrBody(ConnectiveBody {
            ps: vec![Par::default(), Par::default()],
        })),
    };
    for _ in 0..depth {
        let component = construction
            .connective_component(&shell, &[failure, nested])
            .expect("plan skewed refusing OR");
        nested = construction
            .singleton_component(component, Vec::new(), true)
            .expect("plan OR carrier");
    }
    construction.finish(nested).expect("finish OR plan")
}

#[test]
fn byte_native_and_resident_candidate_compilers_emit_identical_domains() {
    let targets = (0..8)
        .map(|value| new_gint_par(value, Vec::new(), false))
        .collect::<Vec<_>>();
    for depth in [1, 2, 31, 128] {
        for conjunction in [false, true] {
            let pattern = planned_skewed_connective(depth, conjunction);
            let (encoded, encoded_stats) =
                connective_candidate_positions_with_stats(targets.clone(), pattern.clone(), true);
            let (resident, resident_stats) =
                connective_candidate_positions_with_stats(targets.clone(), pattern, false);
            assert_eq!(encoded, resident);
            assert_eq!(encoded.as_ref(), &[1]);
            assert_eq!(encoded_stats.candidate_encoded_connective_nodes, depth);
            assert_eq!(encoded_stats.candidate_resident_connective_decodes, 0);
            assert_eq!(encoded_stats.candidate_terminal_par_decodes, depth + 1);
            assert_eq!(resident_stats.candidate_encoded_connective_nodes, 0);
            assert_eq!(resident_stats.candidate_resident_connective_decodes, depth);
        }
    }
}

#[test]
fn skewed_branching_candidate_plan_compile_evaluate_and_drop_are_stack_safe_at_depth_twenty_thousand(
) {
    const DEPTH: usize = 20_000;
    let handle = std::thread::Builder::new()
        .name("connective-candidate-plan-depth".into())
        .stack_size(256 * 1024)
        .spawn(|| {
            for conjunction in [false, true] {
                let nested = planned_skewed_connective(DEPTH, conjunction);
                let mut context = SpatialMatcherContext::new();
                let (result, stats) = match_par_list_with_stats(
                    &mut context,
                    vec![
                        new_gint_par(0, Vec::new(), false),
                        new_gint_par(1, Vec::new(), false),
                    ],
                    vec![new_gint_par(0, Vec::new(), false), nested],
                );
                assert!(result.is_some());
                assert_eq!(stats.edge_evaluations, 2);
                assert_eq!(stats.candidate_encoded_connective_nodes, DEPTH);
                assert_eq!(stats.candidate_resident_connective_decodes, 0);
                assert_eq!(stats.candidate_terminal_par_decodes, DEPTH + 1);
                assert!(stats.candidate_plan_nodes >= DEPTH * 2 + 1);
                assert_eq!(stats.authoritative_connective_root_copies, 1);
                assert!(stats.authoritative_connective_root_bytes > DEPTH);
                assert_eq!(stats.authoritative_connective_evaluations, DEPTH);
                assert_eq!(stats.authoritative_encoded_connective_nodes, DEPTH);
                assert_eq!(stats.authoritative_resident_connective_fallbacks, 0);
                if conjunction {
                    assert_eq!(stats.authoritative_terminal_par_decodes, 0);
                    assert_eq!(stats.authoritative_exact_scalar_attempts, DEPTH + 1);
                    assert_eq!(stats.authoritative_exact_scalar_equal, DEPTH + 1);
                    assert_eq!(stats.authoritative_exact_scalar_mismatch, 0);
                    assert_eq!(stats.authoritative_and_frames, DEPTH);
                    assert_eq!(stats.authoritative_or_frames, 0);
                    assert!(stats.authoritative_max_frame_depth >= DEPTH * 2);
                } else {
                    assert_eq!(stats.authoritative_terminal_par_decodes, 0);
                    assert_eq!(stats.authoritative_exact_scalar_attempts, 1);
                    assert_eq!(stats.authoritative_exact_scalar_equal, 1);
                    assert_eq!(stats.authoritative_exact_scalar_mismatch, 0);
                    assert_eq!(stats.authoritative_and_frames, 0);
                    assert_eq!(stats.authoritative_or_frames, DEPTH);
                    assert!(stats.authoritative_max_frame_depth >= DEPTH);
                }
            }
        })
        .expect("the tiny-stack candidate compiler worker starts");
    handle
        .join()
        .expect("the candidate compiler and matcher use constant native stack");
}

#[test]
fn encoded_or_refusal_continuations_are_stack_safe_at_depth_twenty_thousand() {
    const DEPTH: usize = 20_000;
    let handle = std::thread::Builder::new()
        .name("encoded-or-refusal-depth".into())
        .stack_size(256 * 1024)
        .spawn(|| {
            let pattern = planned_skewed_failing_or(DEPTH);
            let mut context = SpatialMatcherContext::new();
            let (result, stats) =
                match_par_with_stats(&mut context, new_gint_par(1, Vec::new(), false), pattern);
            assert!(result.is_some());
            assert!(context.free_map.is_empty());
            assert_eq!(stats.authoritative_connective_root_copies, 1);
            assert_eq!(stats.authoritative_connective_evaluations, DEPTH);
            assert_eq!(stats.authoritative_encoded_connective_nodes, DEPTH);
            assert_eq!(stats.authoritative_terminal_par_decodes, 0);
            assert_eq!(stats.authoritative_resident_connective_fallbacks, 0);
            assert_eq!(stats.authoritative_exact_scalar_attempts, DEPTH + 1);
            assert_eq!(stats.authoritative_exact_scalar_equal, 1);
            assert_eq!(stats.authoritative_exact_scalar_mismatch, DEPTH);
            assert_eq!(stats.authoritative_and_frames, 0);
            assert_eq!(stats.authoritative_or_frames, DEPTH * 2);
            assert!(stats.authoritative_max_frame_depth >= DEPTH);
        })
        .expect("the tiny-stack encoded OR worker starts");
    handle
        .join()
        .expect("encoded OR refusal continuations use constant native stack");
}

#[test]
fn production_list_pda_does_not_build_an_unusable_target_index() {
    const WIDTH: usize = 4;
    let targets = (0..WIDTH)
        .map(|value| new_gint_par(value as i64, Vec::new(), false))
        .collect::<Vec<_>>();
    let patterns = (0..WIDTH)
        .map(|_| connective(ConnInt(true)))
        .collect::<Vec<_>>();

    let mut context = SpatialMatcherContext::new();
    let (result, stats) = match_par_list_with_stats(&mut context, targets, patterns);
    assert!(result.is_some());
    assert_eq!(stats.target_index_builds, 0);
    assert_eq!(stats.target_key_encodes, 0);
}

#[test]
fn production_list_pda_reuses_adjacent_duplicate_bag_identity() {
    const WIDTH: usize = 64;
    let element = send_value(
        new_gint_par(7, Vec::new(), false),
        vec![new_gint_par(11, Vec::new(), false)],
        false,
    );

    let mut context = SpatialMatcherContext::new();
    let (result, stats) =
        match_send_list_with_stats(&mut context, vec![element.clone(); WIDTH], vec![
            element;
            WIDTH
        ]);

    assert!(result.is_some());
    assert_eq!(stats.target_index_builds, 1);
    assert_eq!(stats.target_key_encodes, 1);

    // AlwaysEqual and Par's semantic order deliberately ignore
    // `locally_free`, while RMP1 and bincode identity retain it. The reuse
    // guard must therefore use the generated CoW byte-identity proof or these
    // two distinct trie keys collapse.
    let first = send_value(
        new_gint_par(7, Vec::new(), false),
        vec![new_gint_par(11, Vec::new(), false)],
        false,
    );
    let mut second = first.clone();
    second
        .chan
        .as_mut()
        .expect("the Send channel exists")
        .locally_free = vec![1];
    assert_eq!(first, second);
    assert_eq!(first.cmp(&second), std::cmp::Ordering::Equal);

    assert_eq!(send_target_index_shape(vec![first, second]), (2, 2));
}

#[test]
fn conditional_target_index_retains_distinct_generated_byte_identities() {
    let first = If {
        condition: Some(new_gint_par(7, Vec::new(), false)),
        if_true: Some(new_gint_par(11, Vec::new(), false)),
        if_false: Some(new_gint_par(13, Vec::new(), false)),
        locally_free: Vec::new(),
        connective_used: false,
    };
    let mut second = first.clone();
    second.locally_free = vec![1];

    // Semantic matching deliberately ignores the derived cache, while the
    // generated byte identity used as the PathMap index key must not collapse
    // the two encoded values.
    assert_eq!(first, second, "If AlwaysEqual ignores locally_free");
    assert_eq!(if_target_index_shape(vec![first, second]), (2, 2));
}

#[test]
fn production_list_pda_computes_remainder_as_bag_difference_without_filler_edges() {
    const WIDTH: usize = 128;
    let targets = (0..WIDTH)
        .map(|index| {
            send_value(
                new_gint_par(index as i64, Vec::new(), false),
                vec![new_gint_par((10_000 + index) as i64, Vec::new(), false)],
                false,
            )
        })
        .collect::<Vec<_>>();
    let selected = targets[WIDTH - 1].clone();

    let mut context = SpatialMatcherContext::new();
    let (result, stats) = match_send_list_with_remainder_stats(
        &mut context,
        targets.clone(),
        vec![selected.clone()],
        0,
    );

    assert!(result.is_some());
    assert_eq!(stats.target_index_builds, 1);
    assert_eq!(stats.edge_evaluations, 1);
    assert_eq!(stats.successful_edge_evaluations, 1);
    assert_eq!(
        context.free_map.get(&0),
        Some(&models::par_from_default! {
            sends: targets[..WIDTH - 1].to_vec(),
            ..Default::default()
        })
    );

    compare(
        "Bag-difference remainder preserves the recursive oracle",
        models::par_from_default! {
            sends: targets,
            ..Default::default()
        },
        models::par_from_default! {
            sends: vec![selected],
            exprs: vec![new_freevar_expr(0)],
            connective_used: true,
            ..Default::default()
        },
    );
}

#[test]
fn production_list_pda_bag_residual_is_exhaustively_equivalent_at_small_width() {
    let send_for = |value: usize| {
        send_value(
            new_gint_par(value as i64, Vec::new(), false),
            vec![new_gint_par((100 + value) as i64, Vec::new(), false)],
            false,
        )
    };
    let sequence = |width: usize, bits: usize| {
        (0..width)
            .map(|position| send_for((bits >> position) & 1))
            .collect::<Vec<_>>()
    };

    let mut cases = 0usize;
    for target_width in 0..=4 {
        for target_bits in 0..(1usize << target_width) {
            for pattern_width in 0..=target_width {
                for pattern_bits in 0..(1usize << pattern_width) {
                    let targets = sequence(target_width, target_bits);
                    let patterns = sequence(pattern_width, pattern_bits);
                    compare(
                        &format!(
                            "Bag residual exhaustive target_width={target_width} target_bits={target_bits:#x} pattern_width={pattern_width} pattern_bits={pattern_bits:#x}"
                        ),
                        models::par_from_default! {
                            sends: targets,
                            ..Default::default()
                        },
                        models::par_from_default! {
                            sends: patterns,
                            exprs: vec![new_freevar_expr(0)],
                            connective_used: true,
                            ..Default::default()
                        },
                    );
                    cases += 1;
                }
            }
        }
    }
    assert_eq!(cases, 651, "the bounded model must remain non-vacuous");

    let ground = send_value(
        new_gint_par(7, Vec::new(), false),
        vec![new_gint_par(11, Vec::new(), false)],
        false,
    );
    let open = send_value(new_freevar_par(9, Vec::new()), Vec::new(), true);
    compare(
        "Bag residual retains the predecessor's locally-free refusal",
        models::par_from_default! {
            sends: vec![ground.clone(), open],
            connective_used: true,
            ..Default::default()
        },
        models::par_from_default! {
            sends: vec![ground],
            exprs: vec![new_freevar_expr(0)],
            connective_used: true,
            ..Default::default()
        },
    );
}

#[test]
fn production_list_pda_pathmap_index_prunes_ordered_nested_ac_elements() {
    const WIDTH: usize = 128;
    let targets = (0..WIDTH)
        .map(|index| {
            list(
                vec![
                    new_gint_par(index as i64, Vec::new(), false),
                    new_gint_par((1_000 + index) as i64, Vec::new(), false),
                ],
                false,
            )
        })
        .collect::<Vec<_>>();
    let patterns = (0..WIDTH)
        .rev()
        .map(|index| {
            list(
                vec![
                    new_gint_par(index as i64, Vec::new(), false),
                    new_freevar_par(index as i32, Vec::new()),
                ],
                true,
            )
        })
        .collect::<Vec<_>>();

    let mut context = SpatialMatcherContext::new();
    let (result, stats) =
        match_par_list_with_stats(&mut context, targets.clone(), patterns.clone());
    assert!(result.is_some());
    assert_eq!(stats.target_index_builds, 1);
    assert_eq!(stats.edge_evaluations, WIDTH);
    assert_eq!(stats.successful_edge_evaluations, WIDTH);
    assert_eq!(context.free_map.len(), WIDTH);
    for index in 0..WIDTH {
        assert_eq!(
            context.free_map.get(&(index as i32)),
            Some(&new_gint_par((1_000 + index) as i64, Vec::new(), false))
        );
    }

    compare(
        "PathMap-pruned ordered elements preserve ESet AC semantics",
        new_eset_par(targets, Vec::new(), false, None, Vec::new(), false),
        new_eset_par(patterns, Vec::new(), true, None, Vec::new(), true),
    );
}

#[test]
fn production_list_pda_pathmap_bag_prunes_unordered_send_fields() {
    const WIDTH: usize = 128;
    let targets = (0..WIDTH)
        .map(|index| {
            send_value(
                new_gint_par(index as i64, Vec::new(), false),
                vec![new_gint_par((10_000 + index) as i64, Vec::new(), false)],
                false,
            )
        })
        .collect::<Vec<_>>();
    let patterns = (0..WIDTH)
        .rev()
        .map(|index| {
            send_value(
                new_gint_par(index as i64, Vec::new(), false),
                vec![new_freevar_par(index as i32, Vec::new())],
                true,
            )
        })
        .collect::<Vec<_>>();

    let mut context = SpatialMatcherContext::new();
    let (result, stats) =
        match_send_list_with_stats(&mut context, targets.clone(), patterns.clone());
    assert!(result.is_some());
    assert_eq!(stats.target_index_builds, 1);
    assert_eq!(stats.edge_evaluations, WIDTH);
    assert_eq!(stats.successful_edge_evaluations, WIDTH);
    assert_eq!(context.free_map.len(), WIDTH);
    for index in 0..WIDTH {
        assert_eq!(
            context.free_map.get(&(index as i32)),
            Some(&new_gint_par((10_000 + index) as i64, Vec::new(), false))
        );
    }

    compare(
        "PathMap Bag-pruned sends preserve unordered Par semantics",
        models::par_from_default! {
            sends: targets,
            ..Default::default()
        },
        models::par_from_default! {
            sends: patterns,
            connective_used: true,
            ..Default::default()
        },
    );
}

#[test]
fn the_differential_judge_can_go_red() {
    let target = new_gint_par(7, Vec::new(), false);
    let mut recursive = RecursiveContext::new();
    let recursive_result = recursive
        .spatial_match(target.clone(), new_freevar_par(0, Vec::new()))
        .is_some();
    let mut pda = SpatialMatcherContext::new();
    let mutated_result = pda
        .spatial_match(target, new_gint_par(8, Vec::new(), false))
        .is_some();
    assert_ne!(recursive_result, mutated_result);
}

#[test]
fn oracle_provenance_is_test_only_and_distinct() {
    let recursive = include_str!("spatial_matcher_oracle/spatial_matcher.rs");
    let production = include_str!("../../src/rust/interpreter/matcher/spatial_matcher_pda.rs");
    assert!(recursive.contains("self.spatial_match("));
    assert!(production.contains("enum Job"));
    assert!(!production.contains("fn match_connective_with_bounds"));
    assert_ne!(recursive.as_bytes(), production.as_bytes());
}
