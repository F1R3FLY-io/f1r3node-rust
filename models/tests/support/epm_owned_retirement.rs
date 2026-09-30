use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use pathmap::zipper::Zipper;
use proptest::prelude::*;

use super::*;

#[derive(Default)]
struct Counts {
    created: AtomicUsize,
    dropped: AtomicUsize,
}

struct Tracked(Arc<Counts>);

impl Tracked {
    fn new(counts: &Arc<Counts>) -> Self {
        counts.created.fetch_add(1, Ordering::Relaxed);
        Self(Arc::clone(counts))
    }
}

impl Clone for Tracked {
    fn clone(&self) -> Self { Self::new(&self.0) }
}

impl Drop for Tracked {
    fn drop(&mut self) { self.0.dropped.fetch_add(1, Ordering::Relaxed); }
}

impl payload_seal::Sealed for Tracked {}

impl EpmPayload for Tracked {
    fn dispose(self) { drop(self); }
}

fn balanced(counts: &Counts) {
    assert_eq!(
        counts.created.load(Ordering::Relaxed),
        counts.dropped.load(Ordering::Relaxed)
    );
}

fn comb_image(depth: usize, mode: EPathMapMode, values: bool, bad_last: bool) -> (Bytes, usize) {
    let mut owner = EpmMapOwner {
        owned: Some((PathMap::new(), PendingScaffold::Repeat(()))),
    };
    let map = &mut owner.owned.as_mut().unwrap().0;
    let mut key = Vec::with_capacity(depth + 1);
    for _ in 0..depth {
        key.push(1);
        if values {
            map.set_val_at(&key, ());
        } else {
            map.create_path(&key);
        }
        *key.last_mut().unwrap() = 0;
    }
    let arena = ArenaCompactTree::from_zipper(map.read_zipper(), |_| 0);
    let count = if mode == EPathMapMode::Map && values {
        depth
    } else {
        0
    };
    let mut bytes = b"EPM1\x01".to_vec();
    bytes.push(mode as u8);
    push_varint(&mut bytes, arena.get_data().len() as u64);
    bytes.extend_from_slice(arena.get_data());
    push_varint(&mut bytes, count as u64);
    for index in 0..count {
        if bad_last && index + 1 == count {
            bytes.extend_from_slice(&[2, 0x10, 1]);
        } else {
            bytes.push(0);
        }
    }
    (Bytes::from(bytes), count)
}

fn small_stack(f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn branching_pending_map_abandonment_retires_all_suspension_states() {
    small_stack(|| {
        let (image, count) = comb_image(8192, EPathMapMode::Map, true, false);
        for stop in [0, 1, count / 2, count] {
            for after_attachment in [false, true] {
                let counts = Arc::new(Counts::default());
                let mut pending =
                    PendingEpmDecode::new_with_scaffold(image.clone(), true, Tracked::new(&counts))
                        .unwrap();
                for index in 0..stop {
                    assert!(pending.next_value_bytes().unwrap().is_some());
                    if after_attachment || index + 1 < stop {
                        pending.push_value(Tracked::new(&counts)).unwrap();
                    }
                }
                assert!(pending.first_semantic_error.is_none());
                assert_eq!(pending.mode, Some(EPathMapMode::Map));
                assert_eq!(
                    pending.decoded_values,
                    stop.saturating_sub(usize::from(!after_attachment))
                );
                let PendingWriterState::Map(writer, _) = &mut pending.writer.state else {
                    panic!("abandonment must exercise a retained map, not Discard");
                };
                if stop > 0 {
                    assert_eq!(writer.path().len(), pending.depth);
                    if after_attachment {
                        assert!(writer.path_exists());
                    }
                    assert_eq!(pending.current_value_range.is_some(), !after_attachment);
                    assert_eq!(writer.val().is_some(), after_attachment);
                }
                if stop == 1 {
                    assert_eq!(pending.depth, 8192);
                    assert!(pending.scaffold_frames.iter().any(|present| *present));
                }
                drop(pending);
                balanced(&counts);
            }
        }
    });
}

#[test]
fn branching_set_and_topology_only_pending_frames_retire_without_publication() {
    small_stack(|| {
        for (mode, values) in [
            (EPathMapMode::Set, true),
            (EPathMapMode::Set, false),
            (EPathMapMode::Map, false),
        ] {
            let (image, _) = comb_image(8192, mode, values, false);
            let counts = Arc::new(Counts::default());
            let mut pending =
                PendingEpmDecode::new_with_scaffold(image, true, Tracked::new(&counts)).unwrap();
            assert!(pending.first_semantic_error.is_none());
            assert!(!pending.advance_to_map_value());
            assert!(pending.first_semantic_error.is_none());
            match &mut pending.writer.state {
                PendingWriterState::Set(writer) => assert!(writer.child_count() > 0),
                PendingWriterState::Map(writer, _) => assert!(writer.child_count() > 0),
                _ => panic!("retirement must exercise retained topology, not Discard"),
            }
            drop(pending);
            balanced(&counts);
        }
    });
}

#[test]
fn branching_final_refusal_preserves_selected_error_and_retires_values() {
    small_stack(|| {
        for semantic in [false, true] {
            let (image, count) = comb_image(8192, EPathMapMode::Map, true, true);
            let counts = Arc::new(Counts::default());
            let mut pending =
                PendingEpmDecode::new_with_scaffold(image, true, Tracked::new(&counts)).unwrap();
            for _ in 0..count {
                pending.next_value_bytes().unwrap().unwrap();
                pending.push_value(Tracked::new(&counts)).unwrap();
            }
            assert_eq!(pending.first_noncanonical_value, Some(count - 1));
            if semantic {
                pending.latch_semantic(TrieCodecError::new("selected semantic error"));
                pending.latch_semantic(TrieCodecError::new("later error must not replace it"));
            }
            let error = match pending.finish() {
                Err(error) => error,
                Ok(_) => panic!("noncanonical pending state must not publish"),
            };
            let expected = if semantic {
                "selected semantic error".to_owned()
            } else {
                format!("map value {} protobuf body is not canonical", count - 1)
            };
            assert_eq!(error.to_string(), expected);
            balanced(&counts);
        }
    });
}

#[test]
fn map_writer_transfer_and_replacement_keep_exact_payload_ownership() {
    let counts = Arc::new(Counts::default());
    let mut state = PendingWriter {
        state: PendingWriterState::Map(
            WriteZipperOwned::new(),
            PendingScaffold::Repeat(Tracked::new(&counts)),
        ),
    };
    if let PendingWriterState::Map(writer, _) = &mut state.state {
        writer.set_val(Tracked::new(&counts));
    }
    state.descend_first_byte(1);
    state.install_scaffold_value();
    let PendingWriterState::Map(writer, scaffold) =
        std::mem::replace(&mut state.state, PendingWriterState::Discard)
    else {
        unreachable!();
    };
    let owner = EpmMapOwner::from_writer(writer, scaffold);
    let (writer, scaffold) = owner.into_writer_parts();
    state.state = PendingWriterState::Map(writer, scaffold);
    assert_eq!(counts.dropped.load(Ordering::Relaxed), 0);
    state = PendingWriter::discard();
    balanced(&counts);
    state.state = PendingWriterState::Map(
        WriteZipperOwned::new(),
        PendingScaffold::Repeat(Tracked::new(&counts)),
    );
    assert_eq!(
        counts.created.load(Ordering::Relaxed) - counts.dropped.load(Ordering::Relaxed),
        1
    );
    drop(state);
    balanced(&counts);
}

#[test]
fn unwinding_a_partially_built_frame_retires_its_owned_map() {
    let scratch = shared::rust::test_scratch::acquire("epm-owned-retirement-unwind-");
    let marker = scratch.path().join("balanced");
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--ignored",
            "--exact",
            "rust::epathmap_trie_codec::owned_retirement_tests::partially_built_frame_unwind_child",
            "--nocapture",
        ])
        .env("EPM_OWNED_RETIREMENT_MARKER", &marker)
        .env_remove(shared::rust::test_process::ALLOW_EXPECTED_PROBE_CORE);
    let outcome = shared::rust::test_process::run_child_probe(
        &mut command,
        std::time::Duration::from_secs(30),
    )
    .expect("run the partially built frame child");
    match outcome {
        shared::rust::test_process::ProbeOutcome::Failed(failure) => {
            assert_eq!(
                failure.kind,
                shared::rust::test_process::ProbeFailureKind::Panicked
            );
            assert!(failure.stderr.contains("injected frame failure"));
        }
        shared::rust::test_process::ProbeOutcome::Passed => {
            panic!("the partially built frame child did not fail")
        }
    }
    assert_eq!(std::fs::read(marker).unwrap(), b"balanced");
}

#[test]
#[ignore = "isolated partially built frame child"]
fn partially_built_frame_unwind_child() {
    let Some(marker) = std::env::var_os("EPM_OWNED_RETIREMENT_MARKER") else {
        return;
    };
    struct BalanceOnDrop {
        counts: Arc<Counts>,
        marker: std::path::PathBuf,
    }
    impl Drop for BalanceOnDrop {
        fn drop(&mut self) {
            balanced(&self.counts);
            std::fs::write(&self.marker, b"balanced").unwrap();
        }
    }
    let counts = Arc::new(Counts::default());
    let _balance = BalanceOnDrop {
        counts: Arc::clone(&counts),
        marker: marker.into(),
    };
    let mut owner = EpmMapOwner {
        owned: Some((
            PathMap::new(),
            PendingScaffold::Repeat(Tracked::new(&counts)),
        )),
    };
    owner
        .owned
        .as_mut()
        .unwrap()
        .0
        .set_val_at([], Tracked::new(&counts));
    owner
        .owned
        .as_mut()
        .unwrap()
        .0
        .create_path(b"unpublished/topology");
    panic!("injected frame failure");
}

#[test]
fn writer_guard_reuses_existing_root_and_provider_storage() {
    #[allow(dead_code)]
    enum PriorWriter<V: EpmPayload> {
        Empty,
        Set(WriteZipperOwned<()>),
        Map(WriteZipperOwned<V>, PendingScaffold<V>),
        Discard,
    }

    fn same_storage<V: EpmPayload>() {
        assert_eq!(
            std::mem::size_of::<PendingWriter<V>>(),
            std::mem::size_of::<PriorWriter<V>>()
        );
        assert_eq!(
            std::mem::align_of::<PendingWriter<V>>(),
            std::mem::align_of::<PriorWriter<V>>()
        );
    }
    same_storage::<()>();
    same_storage::<Par>();
    same_storage::<crate::rust::par_construction::PlannedPar>();
}

proptest! {
    #[test]
    fn generated_abandonment_and_rejection_retire_every_created_payload(
        depth in 1usize..128,
        stop in 0usize..128,
        attach_last in any::<bool>(),
        reject in any::<bool>(),
    ) {
        let (image, count) = comb_image(depth, EPathMapMode::Map, true, false);
        let counts = Arc::new(Counts::default());
        let mut pending = PendingEpmDecode::new_with_scaffold(
            image, true, Tracked::new(&counts),
        ).unwrap();
        let stop = stop.min(count);
        for index in 0..stop {
            pending.next_value_bytes().unwrap().unwrap();
            if attach_last || index + 1 < stop {
                pending.push_value(Tracked::new(&counts)).unwrap();
            }
        }
        if reject {
            pending.latch_semantic(TrieCodecError::new("selected rejection"));
            let error = match pending.finish() {
                Err(error) => error,
                Ok(_) => panic!("a rejected writer must not publish"),
            };
            prop_assert_eq!(error.to_string(), "selected rejection");
        } else {
            drop(pending);
        }
        prop_assert_eq!(counts.created.load(Ordering::Relaxed), counts.dropped.load(Ordering::Relaxed));
    }
}
