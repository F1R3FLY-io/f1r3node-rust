//! DR-131 P1: stack-safety tests of the cost-signature sort.
//!
//! Each deep test sorts a compound chain that is nested 100,000 levels deep on
//! a thread with a small stack. The expected value comes from a shallow input,
//! so the tests need no deep recursion of their own. The deep inputs are built
//! and dropped without recursion. The tests live in their own file, as the
//! G1-1 tests do (`rholang/.../delta_sigma/stack_safety_tests.rs`).

use proptest::prelude::*;

use super::*;

/// The nesting depth of the deep tests. It is far beyond the depth at which a
/// recursive flatten exhausts a small thread stack.
const DEEP_NESTING: usize = 100_000;

/// The stack of the thread that sorts a deep chain.
const SMALL_STACK_BYTES: usize = 256 * 1024;

fn ground(index: usize) -> CostSignature {
    CostSignature {
        value: Some(Value::Ground(index.to_be_bytes().to_vec())),
    }
}

fn unit() -> CostSignature {
    CostSignature {
        value: Some(Value::Unit(true)),
    }
}

fn compound(elements: Vec<CostSignature>) -> CostSignature {
    CostSignature {
        value: Some(Value::Compound(CostSignatureCompound { elements })),
    }
}

/// `depth` compounds, each holding only the next one. The innermost holds
/// `leaf`.
fn single_element_chain(depth: usize, leaf: CostSignature) -> CostSignature {
    let mut chain = leaf;
    for _ in 0..depth {
        chain = compound(vec![chain]);
    }
    chain
}

/// `depth` compounds. The compound at level `i` holds `ground(i)` and then the
/// compound at level `i + 1`. The innermost holds only `ground(depth - 1)`. In
/// source order the leaves are `ground(0)` to `ground(depth - 1)`.
fn ground_per_level_chain(depth: usize) -> CostSignature {
    let mut chain = compound(vec![ground(depth - 1)]);
    for level in (0..depth - 1).rev() {
        chain = compound(vec![ground(level), chain]);
    }
    chain
}

/// `depth` compounds that each hold a unit and then the next compound. The
/// innermost holds only `ground(0)`.
fn unit_per_level_chain(depth: usize) -> CostSignature {
    let mut chain = compound(vec![ground(0)]);
    for _ in 1..depth {
        chain = compound(vec![unit(), chain]);
    }
    chain
}

/// Drops a signature without recursion. The derived `Drop` of a deep chain
/// would recurse once for each level.
fn dismantle(signature: CostSignature) {
    let mut pending = vec![signature];
    while let Some(mut signature) = pending.pop() {
        if let Some(Value::Compound(compound)) = signature.value.take() {
            pending.extend(compound.elements);
        }
    }
}

/// Runs `work` on a thread whose stack is `SMALL_STACK_BYTES`.
fn on_small_stack<T: Send>(work: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("cost-signature-sort-small-stack".to_string())
            .stack_size(SMALL_STACK_BYTES)
            .spawn_scoped(scope, work)
            .expect("spawn the small-stack thread")
            .join()
            .expect("the sort stays within a small stack")
    })
}

#[test]
fn a_single_element_chain_one_hundred_thousand_deep_sorts_on_a_small_stack() {
    let deep = single_element_chain(DEEP_NESTING, ground(7));
    let sorted = on_small_stack(|| sort_signature(&deep));
    assert_eq!(sorted, sort_signature(&ground(7)));
    dismantle(deep);
}

#[test]
fn a_ground_per_level_chain_sorts_like_its_flat_compound_on_a_small_stack() {
    let deep = ground_per_level_chain(DEEP_NESTING);
    let flat = compound((0..DEEP_NESTING).map(ground).collect());
    let sorted = on_small_stack(|| sort_signature(&deep));
    assert_eq!(sorted, sort_signature(&flat));
    dismantle(deep);
}

#[test]
fn a_unit_per_level_chain_keeps_its_only_ground_on_a_small_stack() {
    let deep = unit_per_level_chain(DEEP_NESTING);
    let sorted = on_small_stack(|| sort_signature(&deep));
    assert_eq!(sorted, sort_signature(&ground(0)));
    dismantle(deep);
}

fn sorted_flatten(signatures: &[CostSignature]) -> Vec<ScoredTerm<CostSignature>> {
    let mut output = Vec::new();
    collect_compound(signatures, &mut output);
    output
}

fn recursive_flatten(signatures: &[CostSignature]) -> Vec<ScoredTerm<CostSignature>> {
    let mut output = Vec::new();
    collect_compound_recursive(signatures, &mut output);
    output
}

#[test]
fn an_empty_compound_sorts_as_the_unit() {
    assert_eq!(
        sort_signature(&compound(Vec::new())),
        sort_signature(&unit())
    );
}

#[test]
fn a_compound_of_units_sorts_as_the_unit() {
    let units = compound(vec![
        CostSignature {
            value: Some(Value::Unit(false)),
        },
        unit(),
    ]);
    assert_eq!(sort_signature(&units), sort_signature(&unit()));
}

#[test]
fn a_singleton_compound_sorts_as_its_element() {
    assert_eq!(
        sort_signature(&compound(vec![ground(3)])),
        sort_signature(&ground(3))
    );
}

#[test]
fn nested_compounds_sort_like_the_flat_compound_of_their_leaves() {
    let nested = compound(vec![
        compound(vec![ground(2), unit(), compound(vec![ground(5)])]),
        ground(0),
        compound(Vec::new()),
    ]);
    let flat = compound(vec![ground(2), ground(5), ground(0)]);
    assert_eq!(sort_signature(&nested), sort_signature(&flat));
}

#[test]
fn equal_leaves_keep_both_occurrences() {
    let sorted = sort_signature(&compound(vec![ground(1), ground(1)]));
    let Some(Value::Compound(ref sorted)) = sorted.term.value else {
        panic!("two equal grounds stay a compound");
    };
    assert_eq!(sorted.elements, vec![ground(1), ground(1)]);
}

#[test]
fn a_missing_value_sorts_as_absent() {
    let sorted = sort_signature(&CostSignature { value: None });
    assert_eq!(sorted.term, CostSignature::default());
    assert_eq!(
        sorted.score,
        Tree::<ScoreAtom>::create_leaf_from_i64(Score::ABSENT as i64)
    );
}

fn leaf_signature() -> impl Strategy<Value = CostSignature> {
    use crate::rust::utils::new_gint_par;
    prop_oneof![
        prop::collection::vec(any::<u8>(), 0..3).prop_map(|bytes| CostSignature {
            value: Some(Value::Ground(bytes)),
        }),
        any::<bool>().prop_map(|value| CostSignature {
            value: Some(Value::Unit(value)),
        }),
        (0..3_i32).prop_map(|level| CostSignature {
            value: Some(Value::BoundLevel(level)),
        }),
        Just(CostSignature { value: None }),
        (0..3_i64).prop_map(|value| CostSignature {
            value: Some(Value::Quote(new_gint_par(value, Vec::new(), false))),
        }),
        (0..3_i64).prop_map(|value| CostSignature {
            value: Some(Value::Name(new_gint_par(value, Vec::new(), false))),
        }),
    ]
}

fn nested_signature() -> impl Strategy<Value = CostSignature> {
    leaf_signature().prop_recursive(6, 64, 4, |inner| {
        prop::collection::vec(inner, 0..4).prop_map(compound)
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn the_work_stack_flatten_equals_the_recursive_flatten(
        signatures in prop::collection::vec(nested_signature(), 0..4),
    ) {
        prop_assert_eq!(sorted_flatten(&signatures), recursive_flatten(&signatures));
    }
}
