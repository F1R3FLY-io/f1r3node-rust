//! `If` is the ninth authoritative `ParBag` family and must remain visible at
//! every layer of concrete and connective-aware spatial matching.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use models::rhoapi::connective::ConnectiveInstance;
use models::rhoapi::{Connective, Expr, If, Par};
use models::rust::par_construction::ParConstruction;
use models::rust::rholang::implicits::vector_par;
use models::rust::utils::{new_freevar_par, new_gbool_par, new_gint_par, union};
use rholang::rust::interpreter::matcher::match_pars::match_pars;
use rholang::rust::interpreter::matcher::par_count::ParCount;
use rholang::rust::interpreter::matcher::spatial_matcher::{SpatialMatcher, SpatialMatcherContext};
use rholang::rust::interpreter::matcher::sub_pars::sub_pars;

mod counting_alloc {
    use super::*;

    thread_local! {
        static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
        static BYTES: Cell<usize> = const { Cell::new(0) };
    }

    fn bump(bytes: usize) {
        let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
        let _ = BYTES.try_with(|count| count.set(count.get() + bytes));
    }

    fn read() -> (usize, usize) {
        (
            ALLOCATIONS.try_with(Cell::get).unwrap_or(0),
            BYTES.try_with(Cell::get).unwrap_or(0),
        )
    }

    pub struct Counting;

    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            bump(layout.size());
            unsafe { System.alloc(layout) }
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }

        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            bump(new_size);
            unsafe { System.realloc(ptr, layout, new_size) }
        }
    }

    pub fn measure<R>(body: impl FnOnce() -> R) -> (R, usize, usize) {
        let (allocations_before, bytes_before) = read();
        let result = body();
        let (allocations_after, bytes_after) = read();
        (
            result,
            allocations_after - allocations_before,
            bytes_after - bytes_before,
        )
    }
}

#[global_allocator]
static ALLOCATOR: counting_alloc::Counting = counting_alloc::Counting;

fn integer(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

fn boolean(value: bool) -> Par { new_gbool_par(value, Vec::new(), false) }

fn conditional(condition: Par, if_true: Par, if_false: Par) -> If {
    let locally_free = union(
        condition.locally_free().to_vec(),
        union(
            if_true.locally_free().to_vec(),
            if_false.locally_free().to_vec(),
        ),
    );
    let connective_used =
        condition.connective_used() || if_true.connective_used() || if_false.connective_used();
    If {
        condition: Some(condition),
        if_true: Some(if_true),
        if_false: Some(if_false),
        locally_free,
        connective_used,
    }
}

fn conditional_par(condition: Par, if_true: Par, if_false: Par) -> Par {
    Par::default().prepend_if(conditional(condition, if_true, if_false))
}

fn nested_conditional_fixture(depth: usize) -> (Par, Par) {
    let mut construction = ParConstruction::new();
    let mut target = construction
        .import_par(&integer(7))
        .expect("import target terminal");
    let mut pattern = construction
        .import_par(&new_freevar_par(0, Vec::new()))
        .expect("import pattern terminal");
    let if_true = construction
        .import_par(&integer(12))
        .expect("import true branch");
    let if_false = construction
        .import_par(&integer(13))
        .expect("import false branch");
    let target_shell = If {
        condition: Some(Par::default()),
        if_true: Some(Par::default()),
        if_false: Some(Par::default()),
        locally_free: Vec::new(),
        connective_used: false,
    };
    let pattern_shell = If {
        locally_free: Vec::new(),
        connective_used: true,
        ..target_shell.clone()
    };

    for _ in 0..depth {
        let target_component = construction
            .if_component(&target_shell, &[target, if_true, if_false])
            .expect("plan target conditional");
        target = construction
            .singleton_component(target_component, Vec::new(), false)
            .expect("plan target carrier");

        let pattern_component = construction
            .if_component(&pattern_shell, &[pattern, if_true, if_false])
            .expect("plan pattern conditional");
        pattern = construction
            .singleton_component(pattern_component, Vec::new(), true)
            .expect("plan pattern carrier");
    }

    (
        construction.finish(target).expect("finish target spine"),
        construction.finish(pattern).expect("finish pattern spine"),
    )
}

#[test]
fn concrete_matching_observes_if_and_connective_cardinality() {
    let first = conditional(boolean(true), integer(1), integer(2));
    let different = conditional(boolean(false), integer(1), integer(2));
    let one_if = Par::default().with_conditionals(vec![first.clone()]);

    assert!(match_pars(&one_if, &one_if));
    assert!(!match_pars(
        &one_if,
        &Par::default().with_conditionals(vec![different])
    ));
    assert!(!match_pars(&one_if, &Par::default()));
    assert!(!match_pars(
        &one_if,
        &Par::default().with_conditionals(vec![first.clone(), first])
    ));

    let connective = Connective {
        connective_instance: Some(ConnectiveInstance::ConnBool(true)),
    };
    assert!(!match_pars(
        &Par::default().with_connectives(vec![connective.clone()]),
        &Par::default().with_connectives(vec![connective.clone(), connective])
    ));
}

#[test]
fn concrete_if_matching_ignores_only_derived_locally_free_metadata() {
    let semantic = conditional(boolean(true), integer(1), integer(2));
    let mut stale_cache = semantic.clone();
    stale_cache.locally_free = vec![1, 0, 1];

    assert!(match_pars(
        &Par::default().with_conditionals(vec![semantic]),
        &Par::default().with_conditionals(vec![stale_cache]),
    ));
}

#[test]
fn conditional_children_match_on_the_pda_and_bind_in_schema_order() {
    let cases = [
        (
            "condition",
            conditional_par(integer(11), integer(12), integer(13)),
            conditional_par(new_freevar_par(0, Vec::new()), integer(12), integer(13)),
            integer(11),
        ),
        (
            "if_true",
            conditional_par(integer(11), integer(12), integer(13)),
            conditional_par(integer(11), new_freevar_par(0, Vec::new()), integer(13)),
            integer(12),
        ),
        (
            "if_false",
            conditional_par(integer(11), integer(12), integer(13)),
            conditional_par(integer(11), integer(12), new_freevar_par(0, Vec::new())),
            integer(13),
        ),
    ];

    for (field, target, pattern, expected) in cases {
        let mut context = SpatialMatcherContext::new();
        assert_eq!(
            context.spatial_match(target, pattern),
            Some(()),
            "the conditional PDA failed to match `{field}`"
        );
        assert_eq!(
            context.free_map.get(&0),
            Some(&expected),
            "the conditional PDA bound the wrong `{field}` child"
        );
    }
}

#[test]
fn a_later_conditional_child_failure_rolls_back_an_earlier_binding() {
    let target = conditional_par(integer(11), integer(12), integer(13));
    let pattern = conditional_par(new_freevar_par(0, Vec::new()), integer(99), integer(13));
    let mut context = SpatialMatcherContext::new();

    assert_eq!(context.spatial_match(target, pattern), None);
    assert!(
        context.free_map.is_empty(),
        "the condition binding escaped after the true branch refused"
    );
}

#[test]
fn trie_native_sub_pars_partitions_the_if_family_without_dropping_it() {
    let target = Par::default().with_conditionals(vec![
        conditional(boolean(true), integer(1), integer(2)),
        conditional(boolean(false), integer(3), integer(4)),
    ]);
    let zero = ParCount::new(&Par::default());
    let mut one_if = zero.clone();
    one_if.conditionals = 1;

    let candidates =
        sub_pars(&target, &one_if, &one_if, &zero, &ParCount::unbounded()).collect::<Vec<_>>();
    assert_eq!(candidates.len(), 2);
    assert!(candidates.iter().all(|(selected, complement)| {
        selected.family_cardinality::<If>() == 1
            && complement.family_cardinality::<If>() == 1
            && selected.family_cardinality::<Expr>() == 0
            && complement.family_cardinality::<Expr>() == 0
    }));
}

#[test]
fn the_fixture_really_places_if_in_the_authoritative_bag() {
    let term = conditional_par(boolean(true), integer(1), integer(2));
    assert_eq!(term.family_cardinality::<If>(), 1);
    assert_eq!(term.cardinality(), 1);
    assert_eq!(term.decode_occurrences::<Expr>(), Vec::<Expr>::new());
    assert_eq!(vector_par(Vec::new(), false).cardinality(), 0);
}

#[test]
fn deeply_nested_conditional_matching_uses_constant_native_stack() {
    // Deep relative to the deliberately tiny worker stack, while keeping the
    // canonical inline child encoding of this unary fixture bounded in a
    // routine debug test. Construction/decode growth is measured separately;
    // this gate isolates native-stack usage.
    const DEPTH: usize = 4_096;

    std::thread::Builder::new()
        .name("conditional-matcher-fixed-stack".into())
        .stack_size(128 * 1024)
        .spawn(|| {
            let (target, pattern) = nested_conditional_fixture(DEPTH);
            let mut context = SpatialMatcherContext::new();
            assert_eq!(context.spatial_match(target, pattern), Some(()));
            assert_eq!(context.free_map.get(&0), Some(&integer(7)));
        })
        .expect("spawn conditional matcher gate")
        .join()
        .expect("conditional matcher gate must not overflow its fixed stack");
}

#[test]
fn nested_conditional_matcher_allocation_growth_is_linear() {
    fn sample(depth: usize) -> (usize, usize, usize) {
        let (target, pattern) = nested_conditional_fixture(depth);
        let mut input_bytes = 0usize;
        target.visit_encoded::<If>(|entry| input_bytes += entry.payload().len());
        pattern.visit_encoded::<If>(|entry| input_bytes += entry.payload().len());
        let ((), allocations, bytes) = counting_alloc::measure(|| {
            let mut context = SpatialMatcherContext::new();
            assert_eq!(context.spatial_match(target, pattern), Some(()));
            assert_eq!(context.free_map.get(&0), Some(&integer(7)));
        });
        (allocations, bytes, input_bytes)
    }

    fn subquadratic_verdict(
        label: &str,
        low_depth: usize,
        low: usize,
        high_depth: usize,
        high: usize,
    ) -> Result<(), String> {
        let depth_ratio = high_depth / low_depth;
        (high <= low.saturating_mul(depth_ratio + depth_ratio / 2))
            .then_some(())
            .ok_or_else(|| {
                format!(
                    "{label} grew from {low} at depth {low_depth} to {high} at depth \
                     {high_depth}; this exceeds a 1.5x envelope around linear encoded-input growth"
                )
            })
    }

    let low_depth = 128;
    let high_depth = 1_024;
    let (low_allocations, low_bytes, low_input) = sample(low_depth);
    let (high_allocations, high_bytes, high_input) = sample(high_depth);
    println!(
        "nested If matcher allocations: depth {low_depth} = {low_allocations} / {low_bytes} B \
         for {low_input} input B; depth {high_depth} = {high_allocations} / {high_bytes} B for \
         {high_input} input B"
    );
    assert_linear_input_growth(low_depth, low_input, high_depth, high_input);
    subquadratic_verdict(
        "allocation count",
        low_depth,
        low_allocations,
        high_depth,
        high_allocations,
    )
    .expect("allocation count remains subquadratic");
    subquadratic_verdict(
        "allocated bytes",
        low_depth,
        low_bytes,
        high_depth,
        high_bytes,
    )
    .expect("allocated bytes remain subquadratic");

    let synthetic = subquadratic_verdict(
        "synthetic quadratic control",
        low_depth,
        low_depth * low_depth,
        high_depth,
        high_depth * high_depth,
    );
    assert!(
        synthetic.is_err(),
        "the allocation-slope classifier must reject a quadratic control"
    );

    fn assert_linear_input_growth(low_depth: usize, low: usize, high_depth: usize, high: usize) {
        let depth_ratio = high_depth / low_depth;
        assert!(
            high <= low.saturating_mul(depth_ratio + 1),
            "canonical nested-If input itself unexpectedly grew superlinearly: {low} B at \
             depth {low_depth}, {high} B at depth {high_depth}"
        );
    }
}
