//! Resource and rollback gates for byte-native recursive-family matching.
//!
//! Each fixture is one recursive protobuf family around a terminal capture.
//! The production matcher must retain only borrowed ranges into the two
//! authoritative ParBag endpoints instead of decoding every remaining suffix.
//! The sequence in which this PDA proves child obligations is internal matcher
//! control flow: it establishes no evaluation or scheduling order between the
//! Rholang processes represented by a `Par`.

use std::collections::BTreeMap;
use std::time::Instant;

use models::rhoapi::connective::ConnectiveInstance;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{
    Connective, EList, EMap, EMatches, EMethod, EMinus, ENot, EPlus, ESet, ETuple, Expr,
    KeyValuePair, Match, MatchCase, New, Par, Receive, ReceiveBind, Send,
};
use models::rust::par_construction::ParConstruction;
use models::rust::rholang::{protobuf_decoder, protobuf_encoder};
use models::rust::utils::{
    new_emap_par, new_eset_par, new_freevar_par, new_freevar_var, new_gint_par,
};
use rholang::rust::interpreter::matcher::spatial_matcher::{SpatialMatcher, SpatialMatcherContext};

#[path = "support/counting_alloc.rs"]
mod counting_alloc;

#[global_allocator]
static ALLOCATOR: counting_alloc::Counting = counting_alloc::Counting;

const MALFORMED_CHILD_PROCESS: &str = "RHOLANG_RECURSIVE_CURSOR_MALFORMED_CHILD";

#[derive(Clone, Copy, Debug)]
enum Family {
    New,
    Send,
    Receive,
    Match,
    ENot,
    EPlus,
    EList,
    ETuple,
    EMethod,
    EMatches,
    ESet,
    EMap,
    ConnectiveNot,
    ConnectiveAnd,
    ConnectiveOr,
}

impl Family {
    const ALL: [Self; 15] = [
        Self::New,
        Self::Send,
        Self::Receive,
        Self::Match,
        Self::ENot,
        Self::EPlus,
        Self::EList,
        Self::ETuple,
        Self::EMethod,
        Self::EMatches,
        Self::ESet,
        Self::EMap,
        Self::ConnectiveNot,
        Self::ConnectiveAnd,
        Self::ConnectiveOr,
    ];

    const fn name(self) -> &'static str {
        match self {
            Self::New => "New",
            Self::Send => "Send",
            Self::Receive => "Receive",
            Self::Match => "Match",
            Self::ENot => "ENot",
            Self::EPlus => "EPlus",
            Self::EList => "EList",
            Self::ETuple => "ETuple",
            Self::EMethod => "EMethod",
            Self::EMatches => "EMatches",
            Self::ESet => "ESet",
            Self::EMap => "EMap",
            Self::ConnectiveNot => "Connective::Not",
            Self::ConnectiveAnd => "Connective::And",
            Self::ConnectiveOr => "Connective::Or",
        }
    }
}

fn fixture(family: Family, depth: usize) -> (Par, Par) {
    fixture_with_pattern_terminal(family, depth, new_freevar_par(0, Vec::new()))
}

fn fixture_with_pattern_terminal(
    family: Family,
    depth: usize,
    pattern_terminal: Par,
) -> (Par, Par) {
    match family {
        Family::ESet => return unordered_collection_fixture(false, depth, pattern_terminal),
        Family::EMap => return unordered_collection_fixture(true, depth, pattern_terminal),
        _ => {}
    }

    let mut construction = ParConstruction::new();
    let mut target = construction
        .import_par(&new_gint_par(7, Vec::new(), false))
        .expect("import target terminal");
    let mut pattern = construction
        .import_par(&pattern_terminal)
        .expect("import pattern terminal");
    let empty = construction.empty_par().expect("plan empty Par");

    if matches!(family, Family::ConnectiveNot) {
        let shell = Connective {
            connective_instance: Some(ConnectiveInstance::ConnNotBody(Par::default())),
        };
        for _ in 0..depth {
            let component = construction
                .connective_component(&shell, &[pattern])
                .expect("plan pattern Connective::Not");
            pattern = construction
                .singleton_component(component, Vec::new(), true)
                .expect("plan connective-aware NOT carrier");
        }
        return (
            construction.finish(target).expect("finish target terminal"),
            construction.finish(pattern).expect("finish NOT spine"),
        );
    }

    if matches!(family, Family::ConnectiveAnd | Family::ConnectiveOr) {
        let success = construction
            .import_par(&new_gint_par(7, Vec::new(), false))
            .expect("import AND success terminal");
        let refusal = construction
            .import_par(&new_gint_par(8, Vec::new(), false))
            .expect("import OR refusal terminal");
        let shell = Connective {
            connective_instance: Some(match family {
                Family::ConnectiveAnd => {
                    ConnectiveInstance::ConnAndBody(models::rhoapi::ConnectiveBody {
                        ps: vec![Par::default(), Par::default()],
                    })
                }
                Family::ConnectiveOr => {
                    ConnectiveInstance::ConnOrBody(models::rhoapi::ConnectiveBody {
                        ps: vec![Par::default(), Par::default()],
                    })
                }
                _ => unreachable!("the branching Connective family was selected"),
            }),
        };
        for _ in 0..depth {
            let children = match family {
                Family::ConnectiveAnd => [pattern, success],
                Family::ConnectiveOr => [refusal, pattern],
                _ => unreachable!("the branching Connective family was selected"),
            };
            let component = construction
                .connective_component(&shell, &children)
                .expect("plan branching Connective");
            pattern = construction
                .singleton_component(component, Vec::new(), true)
                .expect("plan branching Connective carrier");
        }
        return (
            construction.finish(target).expect("finish target terminal"),
            construction
                .finish(pattern)
                .expect("finish branching Connective"),
        );
    }

    for _ in 0..depth {
        let (target_component, pattern_component) = match family {
            Family::New => {
                let shell = New {
                    p: Some(Par::default()),
                    injections: BTreeMap::new(),
                    ..New::default()
                };
                (
                    construction
                        .new_component(&shell, &[target])
                        .expect("plan target New"),
                    construction
                        .new_component(&shell, &[pattern])
                        .expect("plan pattern New"),
                )
            }
            Family::Send => {
                let target_shell = Send {
                    chan: Some(Par::default()),
                    ..Send::default()
                };
                let pattern_shell = Send {
                    connective_used: true,
                    ..target_shell.clone()
                };
                (
                    construction
                        .send_component(&target_shell, &[target])
                        .expect("plan target Send"),
                    construction
                        .send_component(&pattern_shell, &[pattern])
                        .expect("plan pattern Send"),
                )
            }
            Family::Receive => {
                let target_shell = Receive {
                    body: Some(Par::default()),
                    ..Receive::default()
                };
                let pattern_shell = Receive {
                    connective_used: true,
                    ..target_shell.clone()
                };
                (
                    construction
                        .receive_component(&target_shell, &[target, empty])
                        .expect("plan target Receive"),
                    construction
                        .receive_component(&pattern_shell, &[pattern, empty])
                        .expect("plan pattern Receive"),
                )
            }
            Family::Match => {
                let target_shell = Match {
                    target: Some(Par::default()),
                    ..Match::default()
                };
                let pattern_shell = Match {
                    connective_used: true,
                    ..target_shell.clone()
                };
                (
                    construction
                        .match_component(&target_shell, &[target])
                        .expect("plan target Match"),
                    construction
                        .match_component(&pattern_shell, &[pattern])
                        .expect("plan pattern Match"),
                )
            }
            Family::ENot => {
                let shell = Expr {
                    expr_instance: Some(ExprInstance::ENotBody(ENot {
                        p: Some(Par::default()),
                    })),
                };
                (
                    construction
                        .expr_component(&shell, &[target], false)
                        .expect("plan target ENot"),
                    construction
                        .expr_component(&shell, &[pattern], false)
                        .expect("plan pattern ENot"),
                )
            }
            Family::EPlus => {
                let shell = Expr {
                    expr_instance: Some(ExprInstance::EPlusBody(EPlus {
                        p1: Some(Par::default()),
                        p2: Some(Par::default()),
                    })),
                };
                (
                    construction
                        .expr_component(&shell, &[target, empty], false)
                        .expect("plan target EPlus"),
                    construction
                        .expr_component(&shell, &[pattern, empty], false)
                        .expect("plan pattern EPlus"),
                )
            }
            Family::EList => {
                let shell = Expr {
                    expr_instance: Some(ExprInstance::EListBody(EList {
                        ps: vec![Par::default()],
                        ..EList::default()
                    })),
                };
                (
                    construction
                        .expr_component(&shell, &[target], false)
                        .expect("plan target EList"),
                    construction
                        .expr_component(&shell, &[pattern], false)
                        .expect("plan pattern EList"),
                )
            }
            Family::ETuple => {
                let shell = Expr {
                    expr_instance: Some(ExprInstance::ETupleBody(ETuple {
                        ps: vec![Par::default()],
                        ..ETuple::default()
                    })),
                };
                (
                    construction
                        .expr_component(&shell, &[target], false)
                        .expect("plan target ETuple"),
                    construction
                        .expr_component(&shell, &[pattern], false)
                        .expect("plan pattern ETuple"),
                )
            }
            Family::EMethod => {
                let shell = Expr {
                    expr_instance: Some(ExprInstance::EMethodBody(EMethod {
                        method_name: "nested".to_owned(),
                        target: Some(Par::default()),
                        arguments: vec![Par::default()],
                        ..EMethod::default()
                    })),
                };
                // The construction API receives sorter order: arguments,
                // then target. The encoded matcher sees protobuf order.
                (
                    construction
                        .expr_component(&shell, &[empty, target], false)
                        .expect("plan target EMethod"),
                    construction
                        .expr_component(&shell, &[empty, pattern], false)
                        .expect("plan pattern EMethod"),
                )
            }
            Family::EMatches => {
                let shell = Expr {
                    expr_instance: Some(ExprInstance::EMatchesBody(EMatches {
                        target: Some(Par::default()),
                        pattern: Some(Par::default()),
                    })),
                };
                (
                    construction
                        .expr_component(&shell, &[target, empty], false)
                        .expect("plan target EMatches"),
                    construction
                        .expr_component(&shell, &[pattern, empty], false)
                        .expect("plan pattern EMatches"),
                )
            }
            Family::ESet | Family::EMap => unreachable!("collections returned above"),
            Family::ConnectiveNot | Family::ConnectiveAnd | Family::ConnectiveOr => {
                unreachable!("Connective families returned above")
            }
        };
        target = construction
            .singleton_component(target_component, Vec::new(), false)
            .expect("plan target carrier");
        pattern = construction
            .singleton_component(pattern_component, Vec::new(), true)
            .expect("plan connective-aware pattern carrier");
    }

    (
        construction.finish(target).expect("finish target spine"),
        construction.finish(pattern).expect("finish pattern spine"),
    )
}

fn assert_matches_terminal(target: Par, pattern: Par) {
    let mut context = SpatialMatcherContext::new();
    assert_eq!(context.spatial_match(target, pattern), Some(()));
    assert_eq!(
        context.free_map.get(&0),
        Some(&new_gint_par(7, Vec::new(), false))
    );
}

fn assert_family_match(family: Family, target: Par, pattern: Par) {
    if matches!(family, Family::ConnectiveNot | Family::ConnectiveOr) {
        let mut context = SpatialMatcherContext::new();
        assert_eq!(context.spatial_match(target, pattern), Some(()));
        assert!(
            context.free_map.is_empty(),
            "NOT and OR must isolate terminal captures"
        );
    } else {
        assert_matches_terminal(target, pattern);
    }
}

fn append_varint(mut value: usize, bytes: &mut Vec<u8>) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        bytes.push(byte);
        if value == 0 {
            return;
        }
    }
}

fn delimited(field_key: u8, body: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(body.len() + 3);
    bytes.push(field_key);
    append_varint(body.len(), &mut bytes);
    bytes.extend_from_slice(body);
    bytes
}

/// Canonical protobuf bytes for an `EMinus` carrier whose required `p1`
/// child is absent. Proto3 accepts this envelope even though it is not a valid
/// Rholang AST node.
fn absent_eminus_par_bytes() -> Vec<u8> {
    let p2 = protobuf_encoder::encode_to_vec(&new_gint_par(2, Vec::new(), false));
    let eminus_body = delimited(0x12, &p2); // EMinus.p2 = field 2
    let expression = delimited(0x52, &eminus_body); // Expr.e_minus_body = field 10
    delimited(0x2a, &expression) // Par.exprs = field 5
}

fn nested_absent_eminus_par_bytes() -> Vec<u8> {
    let inner = absent_eminus_par_bytes();
    let enot_body = delimited(0x0a, &inner); // ENot.p = field 1
    let expression = delimited(0x2a, &enot_body); // Expr.e_not_body = field 5
    delimited(0x2a, &expression) // Par.exprs = field 5
}

fn nested_eminus_pattern(outer_not: bool) -> Par {
    let inner = Par::default()
        .with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EMinusBody(EMinus {
                p1: Some(new_freevar_par(0, Vec::new())),
                p2: Some(new_gint_par(2, Vec::new(), false)),
            })),
        }])
        .with_connective_used(true);
    if outer_not {
        Par::default()
            .with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::ENotBody(ENot { p: Some(inner) })),
            }])
            .with_connective_used(true)
    } else {
        inner
    }
}

#[test]
fn nested_same_tag_absent_child_delegates_to_the_owned_invariant() {
    if std::env::var(MALFORMED_CHILD_PROCESS).is_ok() {
        let well_formed_inner = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EMinusBody(EMinus {
                p1: Some(new_gint_par(7, Vec::new(), false)),
                p2: Some(new_gint_par(2, Vec::new(), false)),
            })),
        }]);
        let well_formed = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::ENotBody(ENot {
                p: Some(well_formed_inner),
            })),
        }]);
        let control =
            SpatialMatcherContext::new().spatial_match(well_formed, nested_eminus_pattern(true));
        println!("MALFORMED_CHILD_CONTROL_MATCHED={}", control.is_some());

        let target = protobuf_decoder::decode_par(nested_absent_eminus_par_bytes().as_slice())
            .expect("proto3 accepts the absent required child");
        let result =
            SpatialMatcherContext::new().spatial_match(target, nested_eminus_pattern(true));
        println!("MALFORMED_CHILD_SURVIVED=true result={result:?}");
        return;
    }

    let output = std::process::Command::new(
        std::env::current_exe().expect("the integration test knows its executable"),
    )
    .env(MALFORMED_CHILD_PROCESS, "1")
    .args([
        "nested_same_tag_absent_child_delegates_to_the_owned_invariant",
        "--exact",
        "--nocapture",
        "--test-threads=1",
    ])
    .output()
    .expect("the malformed-child subprocess starts");
    let text = String::from_utf8_lossy(&output.stdout).into_owned()
        + &String::from_utf8_lossy(&output.stderr);
    assert!(
        text.contains("MALFORMED_CHILD_CONTROL_MATCHED=true"),
        "the control did not prove the nested ENot -> EMinus route\n{text}"
    );
    assert!(
        !text.contains("MALFORMED_CHILD_SURVIVED=true"),
        "the same-tag malformed target was silently classified as a non-match\n{text}"
    );
    assert!(
        !output.status.success(),
        "the malformed child must assert\n{text}"
    );
    assert!(
        text.contains("EMinus.p1 (target)"),
        "the failure did not come from the owned semantic invariant\n{text}"
    );
}

#[test]
fn proven_different_expr_tag_does_not_read_an_absent_child() {
    let target = protobuf_decoder::decode_par(absent_eminus_par_bytes().as_slice())
        .expect("proto3 accepts the absent required child");
    let pattern = Par::default()
        .with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EPlusBody(EPlus {
                p1: Some(new_freevar_par(0, Vec::new())),
                p2: Some(new_gint_par(2, Vec::new(), false)),
            })),
        }])
        .with_connective_used(true);
    let mut context = SpatialMatcherContext::new();
    assert_eq!(context.spatial_match(target, pattern), None);
    assert!(context.free_map.is_empty());
}

#[test]
fn recursive_family_cursors_match_at_depth_4096_on_a_fixed_native_stack() {
    const DEPTH: usize = 4_096;

    for family in Family::ALL {
        std::thread::Builder::new()
            .name(format!("{}-matcher-fixed-stack", family.name()))
            .stack_size(128 * 1024)
            .spawn(move || {
                let (target, pattern) = fixture(family, DEPTH);
                assert_family_match(family, target, pattern);
            })
            .unwrap_or_else(|error| panic!("spawn {} matcher gate: {error}", family.name()))
            .join()
            .unwrap_or_else(|_| {
                panic!(
                    "{} matcher gate must not overflow its fixed stack",
                    family.name()
                )
            });
    }
}

#[test]
fn recursive_family_cursor_allocation_growth_is_subquadratic() {
    const LOW_DEPTH: usize = 128;
    const HIGH_DEPTH: usize = 1_024;

    fn sample(family: Family, depth: usize) -> (usize, usize, usize) {
        let (target, pattern) = fixture(family, depth);
        let input_bytes = protobuf_encoder::encoded_len(&target)
            .checked_add(protobuf_encoder::encoded_len(&pattern))
            .expect("fixture byte length fits usize");
        let ((), allocations, bytes) = counting_alloc::measure(|| {
            assert_family_match(family, target, pattern);
        });
        (allocations, bytes, input_bytes)
    }

    for family in Family::ALL {
        let (low_allocations, low_bytes, low_input) = sample(family, LOW_DEPTH);
        let (high_allocations, high_bytes, high_input) = sample(family, HIGH_DEPTH);
        println!(
            "nested {} matcher allocations: depth {LOW_DEPTH} = {low_allocations} / \
             {low_bytes} B for {low_input} input B; depth {HIGH_DEPTH} = {high_allocations} / \
             {high_bytes} B for {high_input} input B",
            family.name()
        );
        let depth_ratio = HIGH_DEPTH / LOW_DEPTH;
        assert!(
            high_input <= low_input.saturating_mul(depth_ratio + 1),
            "canonical nested-{} input unexpectedly grew superlinearly: {low_input} B at depth \
             {LOW_DEPTH}, {high_input} B at depth {HIGH_DEPTH}",
            family.name()
        );
        counting_alloc::assert_subquadratic(
            &format!("{} allocation count", family.name()),
            LOW_DEPTH,
            low_allocations,
            HIGH_DEPTH,
            high_allocations,
        )
        .expect("allocation count remains subquadratic");
        counting_alloc::assert_subquadratic(
            &format!("{} allocated bytes", family.name()),
            LOW_DEPTH,
            low_bytes,
            HIGH_DEPTH,
            high_bytes,
        )
        .expect("allocated bytes remain subquadratic");
    }

    assert!(
        counting_alloc::assert_subquadratic(
            "synthetic quadratic control",
            LOW_DEPTH,
            LOW_DEPTH * LOW_DEPTH,
            HIGH_DEPTH,
            HIGH_DEPTH * HIGH_DEPTH,
        )
        .is_err(),
        "the allocation-slope classifier must reject a quadratic control"
    );
}

#[test]
#[ignore = "isolated geometric resource probe; select one family and depth by environment"]
fn recursive_family_cursor_resource_probe() {
    let family_name = std::env::var("RHOLANG_RECURSIVE_CURSOR_FAMILY")
        .expect("set RHOLANG_RECURSIVE_CURSOR_FAMILY");
    let family = Family::ALL
        .into_iter()
        .find(|family| family.name() == family_name)
        .expect("select a recursive cursor family");
    let depth = std::env::var("RHOLANG_RECURSIVE_CURSOR_DEPTH")
        .expect("set RHOLANG_RECURSIVE_CURSOR_DEPTH")
        .parse::<usize>()
        .expect("cursor depth is a nonnegative integer");
    let (target, pattern) = fixture(family, depth);
    let input_bytes = protobuf_encoder::encoded_len(&target)
        .checked_add(protobuf_encoder::encoded_len(&pattern))
        .expect("encoded input byte length fits usize");
    let expected_capture = new_gint_par(7, Vec::new(), false);

    for sample in 0..34 {
        let mut context = SpatialMatcherContext::new();
        let target = target.clone();
        let pattern = pattern.clone();
        let start = Instant::now();
        let (result, allocations, allocated_bytes) =
            counting_alloc::measure(|| context.spatial_match(target, pattern));
        let elapsed_ns = start.elapsed().as_nanos();
        assert_eq!(result, Some(()));
        if matches!(family, Family::ConnectiveNot | Family::ConnectiveOr) {
            assert!(context.free_map.is_empty());
        } else {
            assert_eq!(context.free_map.get(&0), Some(&expected_capture));
        }
        if sample >= 3 {
            println!(
                "CURSOR_RESOURCE\t{}\t{depth}\t{input_bytes}\t{}\t{elapsed_ns}\t{allocations}\t{allocated_bytes}",
                family.name(),
                sample - 3
            );
        }
    }
}

#[test]
fn late_recursive_family_failure_rolls_back_earlier_bindings() {
    let integer = |value| new_gint_par(value, Vec::new(), false);
    let hole = || new_freevar_par(0, Vec::new());

    let target_send = Par::default().with_sends(vec![Send {
        chan: Some(integer(7)),
        data: vec![integer(8)],
        ..Send::default()
    }]);
    let pattern_send = Par::default()
        .with_sends(vec![Send {
            chan: Some(hole()),
            data: vec![integer(9)],
            connective_used: true,
            ..Send::default()
        }])
        .with_connective_used(true);

    let target_receive = Par::default().with_receives(vec![Receive {
        binds: vec![ReceiveBind {
            patterns: vec![integer(1)],
            source: Some(integer(7)),
            ..ReceiveBind::default()
        }],
        body: Some(integer(8)),
        ..Receive::default()
    }]);
    let pattern_receive = Par::default()
        .with_receives(vec![Receive {
            binds: vec![ReceiveBind {
                patterns: vec![integer(1)],
                source: Some(hole()),
                ..ReceiveBind::default()
            }],
            body: Some(integer(9)),
            connective_used: true,
            ..Receive::default()
        }])
        .with_connective_used(true);

    let target_match = Par::default().with_matches(vec![Match {
        target: Some(integer(7)),
        cases: vec![MatchCase {
            pattern: Some(integer(1)),
            source: Some(integer(2)),
            ..MatchCase::default()
        }],
        ..Match::default()
    }]);
    let pattern_match = Par::default()
        .with_matches(vec![Match {
            target: Some(hole()),
            cases: vec![MatchCase {
                pattern: Some(integer(99)),
                source: Some(integer(2)),
                ..MatchCase::default()
            }],
            connective_used: true,
            ..Match::default()
        }])
        .with_connective_used(true);

    let target_binary = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EPlusBody(EPlus {
            p1: Some(integer(7)),
            p2: Some(integer(8)),
        })),
    }]);
    let pattern_binary = Par::default()
        .with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EPlusBody(EPlus {
                p1: Some(hole()),
                p2: Some(integer(9)),
            })),
        }])
        .with_connective_used(true);

    let target_map = new_emap_par(
        vec![KeyValuePair {
            key: Some(integer(7)),
            value: Some(integer(8)),
        }],
        Vec::new(),
        false,
        None,
        Vec::new(),
        false,
    );
    let pattern_map = new_emap_par(
        vec![KeyValuePair {
            key: Some(hole()),
            value: Some(integer(9)),
        }],
        Vec::new(),
        true,
        None,
        Vec::new(),
        true,
    );

    for (label, target, pattern) in [
        ("Send", target_send, pattern_send),
        ("Receive", target_receive, pattern_receive),
        ("Match", target_match, pattern_match),
        ("binary Expr", target_binary, pattern_binary),
        ("EMap entry", target_map, pattern_map),
    ] {
        let mut context = SpatialMatcherContext::new();
        assert_eq!(context.spatial_match(target, pattern), None, "{label}");
        assert!(
            context.free_map.is_empty(),
            "{label} leaked a capture from an earlier proof obligation"
        );
    }

    let failing_terminal = Par::default()
        .with_connectives(vec![Connective {
            connective_instance: Some(ConnectiveInstance::ConnAndBody(
                models::rhoapi::ConnectiveBody {
                    ps: vec![hole(), integer(8)],
                },
            )),
        }])
        .with_connective_used(true);
    for family in Family::ALL {
        let depth = if matches!(family, Family::ConnectiveNot) {
            2
        } else {
            3
        };
        let (target, pattern) =
            fixture_with_pattern_terminal(family, depth, failing_terminal.clone());
        let baseline_value = integer(41);
        let mut context = SpatialMatcherContext::new();
        context.free_map.insert(41, baseline_value);
        let baseline = context.free_map.clone();
        assert_eq!(
            context.spatial_match(target, pattern),
            None,
            "{} late terminal failure",
            family.name()
        );
        assert_eq!(
            context.free_map,
            baseline,
            "{} leaked a deepest-terminal capture",
            family.name()
        );
    }
}

#[test]
fn encoded_unordered_collections_preserve_backtracking_and_remainders() {
    let integer = |value| new_gint_par(value, Vec::new(), false);

    // The first free-variable candidate can consume the concrete member that
    // the second row needs. Success therefore witnesses genuine unordered
    // augmenting-path backtracking, not an ordered zip.
    let target_set = new_eset_par(
        vec![integer(7), integer(8)],
        Vec::new(),
        false,
        None,
        Vec::new(),
        false,
    );
    let pattern_set = new_eset_par(
        vec![new_freevar_par(0, Vec::new()), integer(7)],
        Vec::new(),
        true,
        None,
        Vec::new(),
        true,
    );
    let mut context = SpatialMatcherContext::new();
    assert_eq!(context.spatial_match(target_set, pattern_set), Some(()));
    assert_eq!(context.free_map.get(&0), Some(&integer(8)));

    let target_set = new_eset_par(
        vec![integer(1), integer(2), integer(3)],
        Vec::new(),
        false,
        None,
        Vec::new(),
        false,
    );
    let pattern_set = new_eset_par(
        vec![integer(1)],
        Vec::new(),
        true,
        Some(new_freevar_var(1)),
        Vec::new(),
        true,
    );
    let expected_set_remainder = new_eset_par(
        vec![integer(2), integer(3)],
        Vec::new(),
        false,
        None,
        Vec::new(),
        false,
    );
    let mut context = SpatialMatcherContext::new();
    assert_eq!(context.spatial_match(target_set, pattern_set), Some(()));
    assert_eq!(context.free_map.get(&1), Some(&expected_set_remainder));

    let entry = |key, value| KeyValuePair {
        key: Some(integer(key)),
        value: Some(integer(value)),
    };
    let target_map = new_emap_par(
        vec![entry(1, 11), entry(2, 22)],
        Vec::new(),
        false,
        None,
        Vec::new(),
        false,
    );
    let pattern_map = new_emap_par(
        vec![entry(1, 11)],
        Vec::new(),
        true,
        Some(new_freevar_var(2)),
        Vec::new(),
        true,
    );
    let expected_map_remainder = new_emap_par(
        vec![entry(2, 22)],
        Vec::new(),
        false,
        None,
        Vec::new(),
        false,
    );
    let mut context = SpatialMatcherContext::new();
    assert_eq!(context.spatial_match(target_map, pattern_map), Some(()));
    assert_eq!(context.free_map.get(&2), Some(&expected_map_remainder));
}

fn unordered_collection_fixture(map: bool, depth: usize, pattern_terminal: Par) -> (Par, Par) {
    let mut construction = ParConstruction::new();
    let mut target = construction
        .import_par(&new_gint_par(7, Vec::new(), false))
        .expect("import collection target terminal");
    let mut pattern = construction
        .import_par(&pattern_terminal)
        .expect("import collection pattern terminal");
    let key = construction
        .import_par(&new_gint_par(1, Vec::new(), false))
        .expect("import map key");
    for _ in 0..depth {
        let (target_shell, pattern_shell, target_children, pattern_children) = if map {
            let target_shell = Expr {
                expr_instance: Some(ExprInstance::EMapBody(EMap {
                    kvs: vec![KeyValuePair {
                        key: Some(Par::default()),
                        value: Some(Par::default()),
                    }],
                    ..EMap::default()
                })),
            };
            let mut pattern_shell = target_shell.clone();
            let Some(ExprInstance::EMapBody(body)) = pattern_shell.expr_instance.as_mut() else {
                unreachable!()
            };
            body.connective_used = true;
            (target_shell, pattern_shell, vec![key, target], vec![
                key, pattern,
            ])
        } else {
            let target_shell = Expr {
                expr_instance: Some(ExprInstance::ESetBody(ESet {
                    ps: vec![Par::default()],
                    ..ESet::default()
                })),
            };
            let mut pattern_shell = target_shell.clone();
            let Some(ExprInstance::ESetBody(body)) = pattern_shell.expr_instance.as_mut() else {
                unreachable!()
            };
            body.connective_used = true;
            (target_shell, pattern_shell, vec![target], vec![pattern])
        };
        let target_component = construction
            .expr_component(&target_shell, &target_children, false)
            .expect("plan collection target");
        target = construction
            .singleton_component(target_component, Vec::new(), false)
            .expect("plan collection target carrier");
        let pattern_component = construction
            .expr_component(&pattern_shell, &pattern_children, false)
            .expect("plan collection pattern");
        pattern = construction
            .singleton_component(pattern_component, Vec::new(), true)
            .expect("plan collection pattern carrier");
    }
    (
        construction
            .finish(target)
            .expect("finish collection target"),
        construction
            .finish(pattern)
            .expect("finish collection pattern"),
    )
}
