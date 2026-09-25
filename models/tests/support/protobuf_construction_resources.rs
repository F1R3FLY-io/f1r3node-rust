use std::collections::{BTreeMap, BTreeSet};
use std::panic::{catch_unwind, AssertUnwindSafe};

use proptest::prelude::*;

use super::*;

fn integer(arena: &mut ParConstruction, value: i64) -> PlannedComponent {
    arena
        .expr_component(
            &Expr {
                expr_instance: Some(ExprInstance::GInt(value)),
            },
            &[],
            false,
        )
        .unwrap()
}

fn node_observation(
    node: OccurrenceTreeNode,
) -> (
    usize,
    usize,
    u64,
    Option<usize>,
    Option<usize>,
    usize,
    usize,
    usize,
    usize,
    usize,
) {
    (
        node.owner,
        node.occurrence.component.0,
        node.occurrence.multiplicity.get(),
        node.left(),
        node.right(),
        node.height,
        node.distinct_len,
        node.score_len,
        node.protobuf_len,
        node.protobuf_zero_count,
    )
}

fn inspect(arena: &ParConstruction, root: Option<usize>) -> BTreeMap<Vec<u8>, u64> {
    let mut seen = BTreeSet::new();
    let mut work: Vec<_> = root.into_iter().collect();
    while let Some(index) = work.pop() {
        assert!(
            seen.insert(index),
            "cycle or multiply owned occurrence node"
        );
        let node = arena.occurrence_nodes[index];
        let expected = arena
            .make_occurrence_node(node.occurrence, node.left(), node.right())
            .unwrap();
        assert_eq!(
            (
                node.height,
                node.distinct_len,
                node.score_len,
                node.protobuf_len,
                node.protobuf_zero_count
            ),
            (
                expected.height,
                expected.distinct_len,
                expected.score_len,
                expected.protobuf_len,
                expected.protobuf_zero_count
            )
        );
        assert!(
            arena
                .occurrence_height(node.left())
                .abs_diff(arena.occurrence_height(node.right()))
                <= 1
        );
        for child in [node.left(), node.right()].into_iter().flatten() {
            let owner = arena.occurrence_nodes[child].owner;
            assert!(owner == 0 || owner == node.owner);
            work.push(child);
        }
    }
    let ordered: Vec<_> = OccurrenceIter::new(&arena.occurrence_nodes, root)
        .map(|occurrence| {
            (
                arena.component_key(occurrence.component).unwrap(),
                occurrence.multiplicity.get(),
            )
        })
        .collect();
    assert!(ordered.windows(2).all(|pair| pair[0].0 < pair[1].0));
    ordered.into_iter().collect()
}

fn snapshot(arena: &mut ParConstruction, builder: &ProtobufParBuilder) -> PlannedPar {
    arena
        .freeze_protobuf_par(ProtobufParBuilder {
            occurrence_root: builder.occurrence_root,
            locally_free: builder.locally_free.clone(),
            connective_used: builder.connective_used,
        })
        .unwrap()
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, failure_persistence: None, ..ProptestConfig::default() })]
    #[test]
    fn private_rotations_snapshots_and_interleaved_owners_preserve_bags(
        events in proptest::collection::vec((any::<bool>(), -24i64..24, 0u8..8), 1..100)
    ) {
        let mut arena = ParConstruction::new();
        let mut builders = [ProtobufParBuilder::default(), ProtobufParBuilder::default()];
        let mut expected = [BTreeMap::new(), BTreeMap::new()];
        let mut snapshots = Vec::new();
        for (other, value, observe) in events {
            let index = usize::from(other);
            let component = integer(&mut arena, value);
            let key = arena.component_key(component).unwrap();
            arena.insert_protobuf_component(&mut builders[index], component).unwrap();
            *expected[index].entry(key).or_insert(0u64) += 1;
            for i in 0..2 { prop_assert_eq!(inspect(&arena, builders[i].occurrence_root), expected[i].clone()); }
            if observe == 0 {
                let saved = snapshot(&mut arena, &builders[index]);
                snapshots.push((saved, arena.protobuf_bytes(saved)));
            }
            for (saved, bytes) in &snapshots {
                prop_assert_eq!(arena.protobuf_bytes(*saved), bytes.clone());
                inspect(&arena, arena.pars[saved.0].occurrence_root);
            }
        }
    }
}

#[test]
fn all_four_rotations_and_nested_freezes_retain_one_node_per_distinct_entry() {
    for values in [[3, 2, 1], [1, 2, 3], [3, 1, 2], [1, 3, 2]] {
        let mut arena = ParConstruction::new();
        let mut builder = ProtobufParBuilder::default();
        for value in values {
            let component = integer(&mut arena, value);
            arena
                .insert_protobuf_component(&mut builder, component)
                .unwrap();
            inspect(&arena, builder.occurrence_root);
        }
        assert_eq!(arena.occurrence_nodes.len(), 3);
    }
    let mut arena = ParConstruction::new();
    let mut parent = ProtobufParBuilder::default();
    for value in 0..256 {
        let leaf = integer(&mut arena, value);
        let mut nested = ProtobufParBuilder::default();
        arena.insert_protobuf_component(&mut nested, leaf).unwrap();
        arena.freeze_protobuf_par(nested).unwrap();
        arena.insert_protobuf_component(&mut parent, leaf).unwrap();
    }
    assert_eq!(arena.occurrence_nodes.len(), 512);
    assert_eq!(inspect(&arena, parent.occurrence_root).len(), 256);
    let leaf = integer(&mut arena, 255);
    for _ in 0..256 {
        arena.insert_protobuf_component(&mut parent, leaf).unwrap();
    }
    assert_eq!(arena.occurrence_nodes.len(), 512);
}

#[test]
fn checked_failure_and_unwinding_restore_actual_private_writes() {
    let mut arena = ParConstruction::new();
    let mut parent = ProtobufParBuilder::default();
    for value in [4, 2, 6, 1, 3, 5, 7] {
        let component = integer(&mut arena, value);
        arena
            .insert_protobuf_component(&mut parent, component)
            .unwrap();
    }
    let before: Vec<_> = arena
        .occurrence_nodes
        .iter()
        .copied()
        .map(node_observation)
        .collect();
    let root = parent.occurrence_root;
    let bad = integer(&mut arena, 8);
    let proto = arena.components[bad.0].protobuf;
    arena.protobuf_nodes[proto].encoded_len = usize::MAX - 23;
    assert!(arena.insert_protobuf_component(&mut parent, bad).is_err());
    assert_eq!(parent.occurrence_root, root);
    assert_eq!(
        arena.occurrence_nodes[..before.len()]
            .iter()
            .copied()
            .map(node_observation)
            .collect::<Vec<_>>(),
        before
    );
    inspect(&arena, root);
    let index = root.unwrap();
    let original = arena.occurrence_nodes[index];
    let result = catch_unwind(AssertUnwindSafe(|| {
        let undo = std::mem::take(&mut arena.occurrence_undo);
        let path = std::mem::take(&mut arena.occurrence_path);
        let mut edit = OccurrenceEdit {
            arena: &mut arena,
            owner: original.owner,
            undo,
            path,
            committed: false,
        };
        for count in [2, 3] {
            let occurrence = Occurrence {
                component: original.occurrence.component,
                multiplicity: Multiplicity::new(count).unwrap(),
            };
            edit.store(Some(index), occurrence, original.left(), original.right())
                .unwrap();
        }
        panic!("injected after repeated private writes");
    }));
    assert!(result.is_err());
    assert_eq!(
        node_observation(arena.occurrence_nodes[index]),
        node_observation(original)
    );
    inspect(&arena, root);
}

#[test]
fn candidate_rewind_preserves_search_and_recycles_only_unpublished_handles() {
    let mut arena = ParConstruction::new();
    let mut parent = ProtobufParBuilder::default();
    let first = integer(&mut arena, 1);
    arena.insert_protobuf_component(&mut parent, first).unwrap();
    let prior = snapshot(&mut arena, &parent);
    let observed = arena.protobuf_bytes(prior);
    for _ in 0..64 {
        let checkpoint = arena.construction_checkpoint();
        let duplicate = integer(&mut arena, 1);
        let mut child = ProtobufParBuilder::default();
        arena
            .insert_protobuf_component(&mut child, duplicate)
            .unwrap();
        arena.freeze_protobuf_par(child).unwrap();
        let search = arena.search_protobuf_component(&parent, duplicate);
        let existing = arena.existing_protobuf_component(&search).unwrap();
        arena.rewind_construction(checkpoint);
        arena
            .apply_protobuf_component(&mut parent, existing, search)
            .unwrap();
        assert_eq!(arena.protobuf_bytes(prior), observed);
        assert_eq!(arena.components.len(), 1);
        inspect(&arena, parent.occurrence_root);
    }
    assert_eq!(arena.occurrence_nodes.len(), 2);
    let next = integer(&mut arena, 2);
    arena.insert_protobuf_component(&mut parent, next).unwrap();
    assert_eq!(inspect(&arena, parent.occurrence_root).len(), 2);
}

#[test]
fn terminal_retirement_preserves_order_and_has_geometric_requests() {
    for size in [0, 1, 2, 17, 256, 4096] {
        let mut values: Vec<_> = (0..size).collect();
        let capacity = values.capacity();
        let mut threshold = None;
        let mut moved = 0;
        let mut positive_requests = 0;
        let mut last_target = capacity;
        for remaining in (0..=size).rev() {
            values.truncate(remaining);
            if retire_draining_capacity(&mut values, &mut threshold) && remaining != 0 {
                assert!(remaining * 2 <= last_target);
                last_target = remaining;
                moved += remaining;
                positive_requests += 1;
            }
            assert_eq!(values, (0..remaining).collect::<Vec<_>>());
            assert!(!retire_draining_capacity(&mut values, &mut threshold));
        }
        assert!(moved <= capacity);
        if capacity != 0 {
            assert!((1usize << positive_requests) <= capacity);
        }
        assert_eq!(values.capacity(), 0);
    }
}
