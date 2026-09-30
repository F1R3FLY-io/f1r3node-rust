use std::collections::{BTreeMap, BTreeSet};

use proptest::prelude::*;
use prost::Message;

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

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, failure_persistence: None, ..ProptestConfig::default() })]
    #[test]
    fn root_stream_preserves_unsorted_duplicates_and_metadata(
        input in proptest::collection::vec(-40i64..40, 0..100),
        locally_free in proptest::collection::vec(any::<u8>(), 0..8),
        connective_used in any::<bool>(),
    ) {
        let mut arena = ParConstruction::new();
        let mut control = ParConstruction::new();
        let mut stream = ProtobufRootStream::default();
        let mut expected = ProtobufParBuilder::default();
        for value in input {
            let checkpoint = arena.construction_checkpoint();
            let component = integer(&mut arena, value);
            prop_assert!(stream.try_insert(&arena, component, usize::MAX));
            arena.rewind_construction(checkpoint);
            let component = integer(&mut control, value);
            control.insert_protobuf_component(&mut expected, component).unwrap();
        }
        expected.locally_free = locally_free.clone();
        expected.connective_used = connective_used;
        let root = control.freeze_protobuf_par(expected).unwrap();
        let expected = control.finish(root).unwrap();
        let observed = stream.finish(&mut arena, ProtobufParBuilder {
            locally_free, connective_used, ..Default::default()
        }).unwrap();
        prop_assert_eq!(observed.encode_to_vec(), expected.encode_to_vec());
        prop_assert_eq!(crate::rust::rholang::bincode_encoder::encode(&observed),
            crate::rust::rholang::bincode_encoder::encode(&expected));
        prop_assert_eq!(observed.components().compact_bytes(), expected.components().compact_bytes());
        prop_assert_eq!(super::super::canonical_path::encode_trie_path(&observed),
            super::super::canonical_path::encode_trie_path(&expected));
    }

    #[test]
    fn root_stream_decline_replays_first_insertion_topology(
        input in proptest::collection::vec(-40i64..40, 1..100),
        limit in 16usize..2000,
    ) {
        let mut arena = ParConstruction::new();
        let mut control = ParConstruction::new();
        let mut stream = Some(ProtobufRootStream::default());
        let mut actual = ProtobufParBuilder::default();
        let mut expected = ProtobufParBuilder::default();
        for value in input {
            let checkpoint = arena.construction_checkpoint();
            let component = integer(&mut arena, value);
            if stream.as_mut().is_some_and(|stream| stream.try_insert(&arena, component, limit)) {
                arena.rewind_construction(checkpoint);
            } else {
                if let Some(stream) = stream.take() { stream.replay(&mut arena, &mut actual).unwrap(); }
                arena.insert_protobuf_component(&mut actual, component).unwrap();
            }
            let component = integer(&mut control, value);
            control.insert_protobuf_component(&mut expected, component).unwrap();
            if stream.is_none() {
                prop_assert_eq!(root_topology(&arena, actual.occurrence_root),
                    root_topology(&control, expected.occurrence_root));
            }
        }
        let observed = if let Some(stream) = stream {
            stream.finish(&mut arena, actual).unwrap()
        } else {
            let root = arena.freeze_protobuf_par(actual).unwrap();
            arena.finish(root).unwrap()
        };
        let root = control.freeze_protobuf_par(expected).unwrap();
        let expected = control.finish(root).unwrap();
        prop_assert_eq!(observed.encode_to_vec(), expected.encode_to_vec());
        prop_assert_eq!(crate::rust::rholang::bincode_encoder::encode(&observed),
            crate::rust::rholang::bincode_encoder::encode(&expected));
    }
}

#[test]
fn root_stream_key_overflow_is_deferred_without_publication() {
    let mut arena = ParConstruction::new();
    let component = integer(&mut arena, 1);
    let node = &arena.components[component.0];
    arena.scores[node.score.0].encoded_len = usize::MAX / 2;
    arena.protobuf_nodes[node.protobuf].encoded_len = usize::MAX / 2;
    let mut stream = ProtobufRootStream::default();
    assert!(!stream.try_insert(&arena, component, usize::MAX));
    assert!(stream.entries.is_empty());
    assert!(stream.occurrences.is_empty());
    assert!(stream.key.is_empty());
    assert_eq!(stream.score_len, 0);
}

#[test]
fn root_stream_compact_counts_cross_byte_boundary() {
    for repeats in [254, 255, 256, 4096] {
        let mut arena = ParConstruction::new();
        let mut control = ParConstruction::new();
        let mut stream = ProtobufRootStream::default();
        let mut expected = ProtobufParBuilder::default();
        for value in std::iter::once(7)
            .chain(std::iter::once(8))
            .chain(std::iter::repeat_n(7, repeats))
        {
            let checkpoint = arena.construction_checkpoint();
            let component = integer(&mut arena, value);
            assert!(stream.try_insert(&arena, component, usize::MAX));
            arena.rewind_construction(checkpoint);
            let component = integer(&mut control, value);
            control
                .insert_protobuf_component(&mut expected, component)
                .unwrap();
        }
        assert_eq!(stream.occurrences.len(), 2);
        assert_eq!(stream.multiplicity_at(0).get(), (repeats + 1) as u64);
        assert_eq!(stream.multiplicity_at(1), Multiplicity::ONE);
        assert_eq!(stream.overflow_counts.is_empty(), repeats < 255);
        let observed = stream
            .finish(&mut arena, ProtobufParBuilder::default())
            .unwrap();
        let root = control.freeze_protobuf_par(expected).unwrap();
        let expected = control.finish(root).unwrap();
        assert_eq!(observed.encode_to_vec(), expected.encode_to_vec());
        assert_eq!(
            observed.components().compact_bytes(),
            expected.components().compact_bytes()
        );
    }
}

#[test]
fn root_stream_late_changes_preserve_count_and_replay_topology() {
    for suffix_len in [1, 65] {
        let input: Vec<i64> = (0..256)
            .map(i64::from)
            .chain(std::iter::once(0))
            .chain((0..suffix_len).map(|index| -i64::from(index) - 1))
            .collect();
        for replay in [false, true] {
            let mut arena = ParConstruction::new();
            let mut control = ParConstruction::new();
            let mut stream = ProtobufRootStream::default();
            let mut expected = ProtobufParBuilder::default();
            for value in input.iter().copied() {
                let checkpoint = arena.construction_checkpoint();
                let component = integer(&mut arena, value);
                assert!(stream.try_insert(&arena, component, usize::MAX));
                arena.rewind_construction(checkpoint);
                let component = integer(&mut control, value);
                control
                    .insert_protobuf_component(&mut expected, component)
                    .unwrap();
            }
            let observed = if replay {
                let mut parent = ProtobufParBuilder::default();
                stream.replay(&mut arena, &mut parent).unwrap();
                assert_eq!(
                    root_topology(&arena, parent.occurrence_root),
                    root_topology(&control, expected.occurrence_root)
                );
                let root = arena.freeze_protobuf_par(parent).unwrap();
                arena.finish(root).unwrap()
            } else {
                stream
                    .finish(&mut arena, ProtobufParBuilder::default())
                    .unwrap()
            };
            let root = control.freeze_protobuf_par(expected).unwrap();
            let expected = control.finish(root).unwrap();
            assert_eq!(observed.encode_to_vec(), expected.encode_to_vec());
            assert_eq!(
                observed.components().compact_bytes(),
                expected.components().compact_bytes()
            );
        }
    }
}

fn root_topology(
    arena: &ParConstruction,
    root: Option<usize>,
) -> BTreeMap<
    Vec<u8>,
    (
        Option<Vec<u8>>,
        Option<Vec<u8>>,
        usize,
        u64,
        usize,
        usize,
        usize,
    ),
> {
    let mut topology = BTreeMap::new();
    let mut work: Vec<_> = root.into_iter().collect();
    while let Some(index) = work.pop() {
        let node = arena.occurrence_nodes[index];
        let key_at = |index: usize| {
            arena
                .component_key(arena.occurrence_nodes[index].occurrence.component)
                .unwrap()
        };
        topology.insert(
            key_at(index),
            (
                node.left().map(key_at),
                node.right().map(key_at),
                node.height,
                node.occurrence.multiplicity.get(),
                node.score_len,
                node.protobuf_len,
                node.protobuf_zero_count,
            ),
        );
        work.extend([node.left(), node.right()].into_iter().flatten());
    }
    topology
}

#[test]
fn root_stream_retains_only_fixed_records_after_rewind() {
    assert_eq!(
        std::mem::size_of::<Multiplicity>(),
        std::mem::size_of::<u64>()
    );
    let mut arena = ParConstruction::new();
    let mut stream = ProtobufRootStream::default();
    let mut capacities = None;
    for value in 0..4096 {
        let checkpoint = arena.construction_checkpoint();
        let component = integer(&mut arena, value);
        assert!(stream.try_insert(&arena, component, usize::MAX));
        arena.rewind_construction(checkpoint);
        assert!(arena.components.is_empty());
        assert!(arena.scores.is_empty());
        assert!(arena.protobuf_nodes.is_empty());
        assert!(arena.byte_pool.is_empty());
        assert!(arena.occurrence_nodes.is_empty());
        let current = (
            arena.components.capacity(),
            arena.scores.capacity(),
            arena.protobuf_nodes.capacity(),
            arena.byte_pool.capacity(),
        );
        assert_eq!(*capacities.get_or_insert(current), current);
    }
    assert_eq!(stream.occurrences.len(), 4096);
    let value = stream
        .finish(&mut arena, ProtobufParBuilder::default())
        .unwrap();
    assert_eq!(value.components().distinct_len(), 4096);
}

#[test]
fn root_stream_replay_preserves_the_original_checked_failure() {
    let mut arena = ParConstruction::new();
    let mut stream = ProtobufRootStream::default();
    for value in [4, 2, 6, 1, 3, 5, 7, 4, 2] {
        let checkpoint = arena.construction_checkpoint();
        let component = integer(&mut arena, value);
        assert!(stream.try_insert(&arena, component, usize::MAX));
        arena.rewind_construction(checkpoint);
    }
    let mut parent = ProtobufParBuilder::default();
    stream.replay(&mut arena, &mut parent).unwrap();
    let component = integer(&mut arena, 8);
    let score = arena.components[component.0].score;
    arena.scores[score.0].encoded_len = usize::MAX;
    let before = inspect(&arena, parent.occurrence_root);
    let error = arena
        .insert_protobuf_component(&mut parent, component)
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Par score body exceeds addressable memory"
    );
    assert_eq!(inspect(&arena, parent.occurrence_root), before);
}

#[test]
fn root_stream_teardown_keeps_deep_keys_on_a_small_stack() {
    std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(|| {
            let mut arena = ParConstruction::new();
            let mut stream = ProtobufRootStream::default();
            let leaf = integer(&mut arena, 1);
            let mut child = arena
                .par([(leaf, Multiplicity::ONE)], Vec::new(), false)
                .unwrap();
            for _ in 0..4096 {
                let shell = Expr {
                    expr_instance: Some(ExprInstance::ENotBody(crate::rhoapi::ENot {
                        p: Some(Par::default()),
                    })),
                };
                let component = arena.expr_component(&shell, &[child], false).unwrap();
                child = arena
                    .par([(component, Multiplicity::ONE)], Vec::new(), false)
                    .unwrap();
            }
            let component = arena.occurrence_nodes[arena.pars[child.0].occurrence_root.unwrap()]
                .occurrence
                .component;
            assert!(stream.try_insert(&arena, component, usize::MAX));
            drop(arena);
            drop(stream);
        })
        .unwrap()
        .join()
        .unwrap();
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
    let scratch = shared::rust::test_scratch::acquire("occurrence-edit-unwind-");
    let marker = scratch.path().join("restored");
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--ignored",
            "--exact",
            "rust::par_construction::protobuf_construction_resource_tests::occurrence_edit_unwind_child",
            "--nocapture",
        ])
        .env("OCCURRENCE_EDIT_UNWIND_MARKER", &marker)
        .env_remove(shared::rust::test_process::ALLOW_EXPECTED_PROBE_CORE);
    let outcome = shared::rust::test_process::run_child_probe(
        &mut command,
        std::time::Duration::from_secs(30),
    )
    .expect("run occurrence edit unwind child");
    match outcome {
        shared::rust::test_process::ProbeOutcome::Failed(failure) => {
            assert_eq!(
                failure.kind,
                shared::rust::test_process::ProbeFailureKind::Panicked
            );
            assert!(
                failure
                    .stderr
                    .contains("injected after repeated private writes"),
                "{failure:?}"
            );
        }
        shared::rust::test_process::ProbeOutcome::Passed => {
            panic!("occurrence edit unwind child did not fail")
        }
    }
    assert_eq!(std::fs::read(marker).unwrap(), b"restored");
}

#[test]
#[ignore = "isolated occurrence edit unwind child"]
fn occurrence_edit_unwind_child() {
    let Some(marker) = std::env::var_os("OCCURRENCE_EDIT_UNWIND_MARKER") else {
        return;
    };
    struct VerifyOnDrop {
        arena: ParConstruction,
        index: usize,
        original: OccurrenceTreeNode,
        root: Option<usize>,
        marker: std::path::PathBuf,
    }
    impl Drop for VerifyOnDrop {
        fn drop(&mut self) {
            assert!(std::thread::panicking());
            assert_eq!(
                node_observation(self.arena.occurrence_nodes[self.index]),
                node_observation(self.original)
            );
            inspect(&self.arena, self.root);
            std::fs::write(&self.marker, b"restored").unwrap();
        }
    }
    let mut arena = ParConstruction::new();
    let mut parent = ProtobufParBuilder::default();
    for value in [4, 2, 6, 1, 3, 5, 7] {
        let component = integer(&mut arena, value);
        arena
            .insert_protobuf_component(&mut parent, component)
            .unwrap();
    }
    let root = parent.occurrence_root;
    let index = root.unwrap();
    let original = arena.occurrence_nodes[index];
    let mut verify = VerifyOnDrop {
        arena,
        index,
        original,
        root,
        marker: marker.into(),
    };
    let undo = std::mem::take(&mut verify.arena.occurrence_undo);
    let path = std::mem::take(&mut verify.arena.occurrence_path);
    let mut edit = OccurrenceEdit {
        arena: &mut verify.arena,
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
