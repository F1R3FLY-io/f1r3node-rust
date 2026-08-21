use models::rhoapi::connective::ConnectiveInstance::ConnAndBody;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Connective, ConnectiveBody, ENot, Expr, Par};
use models::rust::par_construction::ParConstruction;
use models::rust::rholang::protobuf_encoder;
use models::rust::utils::{new_freevar_par, new_gint_par};
use rholang::rust::interpreter::matcher::spatial_matcher::{SpatialMatcher, SpatialMatcherContext};

#[path = "support/counting_alloc.rs"]
mod counting_alloc;

#[global_allocator]
static ALLOCATOR: counting_alloc::Counting = counting_alloc::Counting;

fn binding_conjunction(depth: usize, terminal: i64) -> Par {
    let mut construction = ParConstruction::new();
    let mut nested = construction
        .import_par(&new_gint_par(terminal, Vec::new(), false))
        .expect("import binding-journal terminal");
    let shell = Connective {
        connective_instance: Some(ConnAndBody(ConnectiveBody {
            ps: vec![Par::default(), Par::default()],
        })),
    };
    for level in (0..depth).rev() {
        let level = i32::try_from(level).expect("binding-journal depth fits i32");
        let binding = construction
            .import_par(&new_freevar_par(level, Vec::new()))
            .expect("import binding-journal variable");
        let component = construction
            .connective_component(&shell, &[binding, nested])
            .expect("plan binding-journal conjunction");
        nested = construction
            .singleton_component(component, Vec::new(), true)
            .expect("plan binding-journal carrier");
    }
    construction
        .finish(nested)
        .expect("finish binding-journal plan")
}

fn sample(depth: usize, terminal: i64) -> (usize, usize, usize) {
    let target = new_gint_par(1, Vec::new(), false);
    let pattern = binding_conjunction(depth, terminal);
    let input_bytes = protobuf_encoder::encoded_len(&target)
        .checked_add(protobuf_encoder::encoded_len(&pattern))
        .expect("binding-journal input length fits usize");
    let ((matched, bindings), allocations, bytes) = counting_alloc::measure(|| {
        let mut context = SpatialMatcherContext::new();
        let matched = context.spatial_match(target, pattern).is_some();
        (matched, context.free_map.len())
    });
    assert_eq!(matched, terminal == 1);
    assert_eq!(bindings, if matched { depth } else { 0 });
    (allocations, bytes, input_bytes)
}

fn nested_enot_target(depth: usize) -> Par {
    let mut construction = ParConstruction::new();
    let mut nested = construction
        .import_par(&new_gint_par(1, Vec::new(), false))
        .expect("import ENot capture terminal");
    let shell = Expr {
        expr_instance: Some(ExprInstance::ENotBody(ENot {
            p: Some(Par::default()),
        })),
    };
    for _ in 0..depth {
        let component = construction
            .expr_component(&shell, &[nested], false)
            .expect("plan ENot capture component");
        nested = construction
            .singleton_component(component, Vec::new(), false)
            .expect("plan ENot capture carrier");
    }
    construction.finish(nested).expect("finish ENot target")
}

fn sample_deep_target_capture(depth: usize) -> (usize, usize, usize) {
    let target = nested_enot_target(depth);
    let expected = target.clone();
    let pattern = new_freevar_par(0, Vec::new());
    let input_bytes = protobuf_encoder::encoded_len(&target)
        .checked_add(protobuf_encoder::encoded_len(&pattern))
        .expect("deep-target capture input length fits usize");
    let ((matched, captured), allocations, bytes) = counting_alloc::measure(|| {
        let mut context = SpatialMatcherContext::new();
        let matched = context.spatial_match(target, pattern).is_some();
        (matched, context.free_map.remove(&0))
    });
    assert!(matched);
    assert_eq!(captured, Some(expected));
    (allocations, bytes, input_bytes)
}

#[test]
fn binding_journal_allocation_growth_is_subquadratic_on_success_and_deep_refusal() {
    const LOW_DEPTH: usize = 128;
    const HIGH_DEPTH: usize = 1_024;

    for terminal in [1, 0] {
        let (low_allocations, low_bytes, low_input) = sample(LOW_DEPTH, terminal);
        let (high_allocations, high_bytes, high_input) = sample(HIGH_DEPTH, terminal);
        println!(
            "FreeMap journal terminal {terminal}: depth {LOW_DEPTH} = {low_allocations} allocations / {low_bytes} B for {low_input} input B; depth {HIGH_DEPTH} = {high_allocations} allocations / {high_bytes} B for {high_input} input B"
        );
        assert!(high_input <= low_input.saturating_mul(HIGH_DEPTH / LOW_DEPTH + 1));
        counting_alloc::assert_subquadratic(
            "FreeMap journal allocation count",
            LOW_DEPTH,
            low_allocations,
            HIGH_DEPTH,
            high_allocations,
        )
        .expect("FreeMap journal allocation count remains subquadratic");
        counting_alloc::assert_subquadratic(
            "FreeMap journal allocated bytes",
            LOW_DEPTH,
            low_bytes,
            HIGH_DEPTH,
            high_bytes,
        )
        .expect("FreeMap journal allocated bytes remain subquadratic");
    }

    assert!(counting_alloc::assert_subquadratic(
        "synthetic quadratic control",
        LOW_DEPTH,
        LOW_DEPTH * LOW_DEPTH,
        HIGH_DEPTH,
        HIGH_DEPTH * HIGH_DEPTH,
    )
    .is_err());
}

#[test]
fn sole_freevar_captures_a_deep_target_without_decoding_its_suffix() {
    const LOW_DEPTH: usize = 128;
    const HIGH_DEPTH: usize = 1_024;

    let (low_allocations, low_bytes, low_input) = sample_deep_target_capture(LOW_DEPTH);
    let (high_allocations, high_bytes, high_input) = sample_deep_target_capture(HIGH_DEPTH);
    println!(
        "byte-native deep-target capture: depth {LOW_DEPTH} = {low_allocations} allocations / \
         {low_bytes} B for {low_input} input B; depth {HIGH_DEPTH} = {high_allocations} \
         allocations / {high_bytes} B for {high_input} input B"
    );
    assert!(high_input <= low_input.saturating_mul(HIGH_DEPTH / LOW_DEPTH + 1));
    counting_alloc::assert_subquadratic(
        "deep-target capture allocation count",
        LOW_DEPTH,
        low_allocations,
        HIGH_DEPTH,
        high_allocations,
    )
    .expect("deep-target capture allocation count remains subquadratic");
    counting_alloc::assert_subquadratic(
        "deep-target capture allocated bytes",
        LOW_DEPTH,
        low_bytes,
        HIGH_DEPTH,
        high_bytes,
    )
    .expect("deep-target capture allocated bytes remain subquadratic");
}

fn run_profile_probe(depth: usize) {
    let (accept_allocations, accept_bytes, accept_input) = sample(depth, 1);
    let (refuse_allocations, refuse_bytes, refuse_input) = sample(depth, 0);
    assert_eq!(accept_input, refuse_input);
    println!(
        "FreeMap profile probe: depth {depth}, input {accept_input} B, accept \
         {accept_allocations} allocations / {accept_bytes} B, refusal \
         {refuse_allocations} allocations / {refuse_bytes} B"
    );
}

fn run_hotspot_profile(depth: usize, repetitions: usize) {
    let target = new_gint_par(1, Vec::new(), false);
    let accept_pattern = binding_conjunction(depth, 1);
    let refuse_pattern = binding_conjunction(depth, 0);
    for _ in 0..repetitions {
        let mut accept_context = SpatialMatcherContext::new();
        assert!(accept_context
            .spatial_match(target.clone(), accept_pattern.clone())
            .is_some());
        assert_eq!(accept_context.free_map.len(), depth);

        let mut refuse_context = SpatialMatcherContext::new();
        assert!(refuse_context
            .spatial_match(target.clone(), refuse_pattern.clone())
            .is_none());
        assert!(refuse_context.free_map.is_empty());
    }
}

// These exact-name probes keep construction and matching identical at each rung so
// deterministic whole-process instruction and heap profilers can compare depths
// without a runtime parameter changing the ordinary test suite's coverage.
#[test]
#[ignore = "deterministic profiler rung; exercised by the stack-safety measurement protocol"]
fn freemap_journal_profile_depth_128() { run_profile_probe(128); }

#[test]
#[ignore = "deterministic profiler rung; exercised by the stack-safety measurement protocol"]
fn freemap_journal_profile_depth_256() { run_profile_probe(256); }

#[test]
#[ignore = "deterministic profiler rung; exercised by the stack-safety measurement protocol"]
fn freemap_journal_profile_depth_512() { run_profile_probe(512); }

#[test]
#[ignore = "deterministic profiler rung; exercised by the stack-safety measurement protocol"]
fn freemap_journal_profile_depth_1024() { run_profile_probe(1_024); }

#[test]
#[ignore = "deep profiler rung; exercised under an explicit RSS envelope"]
fn freemap_journal_profile_depth_20000() { run_profile_probe(20_000); }

#[test]
#[ignore = "prepared-fixture hotspot probe; exercised by the stack-safety measurement protocol"]
fn freemap_journal_hotspot_profile_depth_1024() { run_hotspot_profile(1_024, 32); }
