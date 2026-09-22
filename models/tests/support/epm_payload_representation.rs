use std::sync::atomic::{AtomicUsize, Ordering};

use proptest::prelude::*;

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Token(u64);

fn entries<V: Clone + Send + Sync + Unpin + 'static>(map: &PathMap<V>) -> Vec<(Vec<u8>, V)> {
    let mut zipper = map.read_zipper();
    let mut entries = Vec::new();
    if let Some(value) = zipper.val() {
        entries.push((Vec::new(), value.clone()));
    }
    while zipper.to_next_val() {
        entries.push((zipper.path().to_vec(), zipper.val().unwrap().clone()));
    }
    entries
}

fn snapshot<V: Clone + Send + Sync + Unpin + 'static>(repr: &EPathMapRepr<V>) -> Bytes {
    let layout = layout(repr);
    let mut bytes = layout.prefix().to_vec();
    bytes.resize(bytes.len() + layout.value_count(), 0);
    Bytes::from(bytes)
}

fn replay_matches(source: EPathMapRepr<Token>) {
    let expected_prefix = layout(&source).prefix().to_vec();
    let expected = match &source {
        EPathMapRepr::Map(map) => entries(map),
        _ => Vec::new(),
    };
    let mut pending =
        PendingEpmDecode::new_with_scaffold(snapshot(&source), true, Token(u64::MAX)).unwrap();
    for (_, value) in &expected {
        assert!(pending.next_value_bytes().unwrap().unwrap().is_empty());
        pending.push_value(*value).unwrap();
    }
    assert!(pending.next_value_bytes().unwrap().is_none());
    let actual = pending.finish().unwrap();
    assert_eq!(actual.mode(), source.mode());
    assert_eq!(layout(&actual).prefix(), expected_prefix);
    if let EPathMapRepr::Map(map) = actual {
        assert_eq!(entries(&map), expected);
    }
}

#[test]
fn opaque_payloads_preserve_modes_topology_and_remove_scaffolds() {
    replay_matches(EPathMapRepr::Empty);
    let mut set = PathMap::new();
    set.insert(b"set/a", ());
    set.create_path(b"set/topology");
    replay_matches(EPathMapRepr::Set(set));
    let mut map = PathMap::new();
    map.insert([], Token(1));
    map.insert(b"prefix", Token(2));
    map.insert(b"prefix/child", Token(3));
    for byte in 0..=255 {
        map.insert([255, byte], Token(u64::from(byte)));
    }
    map.create_path(b"topology/only");
    replay_matches(EPathMapRepr::Map(map));
    let mut topology = PathMap::new();
    topology.create_path(b"no/values");
    replay_matches(EPathMapRepr::Map(topology));
}

#[test]
fn equal_payload_handles_do_not_collapse_endpoints() {
    let mut map = PathMap::new();
    for key in [b"".as_slice(), b"shared/a", b"shared/b"] {
        map.insert(key, Token(7));
    }
    replay_matches(EPathMapRepr::Map(map));
}

fn root_image(marker: u8, bodies: &[&[u8]], declared: usize) -> Bytes {
    let mut arena = pathmap::arena_compact::COMPACT_TREE_MAGIC.to_vec();
    arena.extend_from_slice(&16u64.to_le_bytes());
    arena.extend_from_slice(&[0x40, marker]);
    arena.extend_from_slice(&[0; 8]);
    let mut image = b"EPM1\x01\x02".to_vec();
    push_varint(&mut image, arena.len() as u64);
    image.extend(arena);
    push_varint(&mut image, declared as u64);
    for body in bodies {
        push_varint(&mut image, body.len() as u64);
        image.extend_from_slice(body);
    }
    Bytes::from(image)
}

fn replay_error(image: Bytes, validate: bool) -> Result<EPathMapRepr<Token>, String> {
    let mut pending = PendingEpmDecode::new_with_scaffold(image, validate, Token(u64::MAX))
        .map_err(|e| e.to_string())?;
    let mut position = 0;
    while let Some(body) = pending.next_value_bytes().map_err(|e| e.to_string())? {
        crate::protobuf_decoder_oracle::legacy(|| protobuf_decoder::decode_par(body.as_ref()))
            .map_err(|error| format!("map value {position} is not a Par: {error}"))?;
        pending
            .push_value(Token(position))
            .map_err(|e| e.to_string())?;
        position += 1;
    }
    pending.finish().map_err(|e| e.to_string())
}

#[test]
fn malformed_frames_and_deferred_precedence_are_pinned() {
    for (image, expected) in [
        (
            root_image(0, &[], 1),
            "map value count 1 exceeds the 0-byte payload lower bound",
        ),
        (
            root_image(0, &[&[]], 0),
            "trailing bytes after EPM1 payload",
        ),
        (
            root_image(1, &[&[0x50, 0]], 1),
            "map ACTree value marker 1 is not zero",
        ),
        (
            root_image(0, &[&[0x50, 0]], 1),
            "map value 0 protobuf body is not canonical",
        ),
    ] {
        assert_eq!(replay_error(image, true).err().unwrap(), expected);
    }
    let child_error =
        crate::protobuf_decoder_oracle::legacy(|| protobuf_decoder::decode_par([0x0f].as_slice()))
            .unwrap_err();
    assert_eq!(
        replay_error(root_image(1, &[&[0x0f]], 1), true)
            .err()
            .unwrap(),
        format!("map value 0 is not a Par: {child_error}")
    );
    assert!(replay_error(root_image(0, &[&[0x50, 0]], 1), false).is_ok());

    let mut map = PathMap::new();
    map.insert(b"a", Token(1));
    map.insert(b"b", Token(2));
    let mut image = layout(&EPathMapRepr::Map(map)).prefix().to_vec();
    image.extend_from_slice(&[2, 0x50, 0, 1, 0x0f]);
    assert_eq!(
        replay_error(Bytes::from(image), true).err().unwrap(),
        format!("map value 1 is not a Par: {child_error}")
    );
}

#[test]
fn pending_protocol_misuse_never_publishes_incomplete_values() {
    let image = root_image(0, &[&[]], 1);
    let new = || PendingEpmDecode::new_with_scaffold(image.clone(), true, Token(99)).unwrap();
    let mut pending = new();
    assert_eq!(
        pending.push_value(Token(1)).unwrap_err().to_string(),
        "map value supplied without a corresponding EPM1 value body"
    );
    assert!(pending.next_value_bytes().unwrap().is_some());
    assert_eq!(
        pending.next_value_bytes().unwrap_err().to_string(),
        "next map value requested before the previous value was decoded"
    );
    pending.push_value(Token(1)).unwrap();
    assert_eq!(
        pending.push_value(Token(2)).unwrap_err().to_string(),
        "more map values were decoded than the EPM1 value count"
    );
    let EPathMapRepr::Map(map) = pending.finish().unwrap() else {
        panic!("expected map")
    };
    assert_eq!(map.get([]), Some(&Token(1)));
    for yielded in [false, true] {
        let mut pending = new();
        if yielded {
            pending.next_value_bytes().unwrap();
        }
        assert_eq!(
            pending.finish().err().unwrap().to_string(),
            "fewer map values were decoded than the EPM1 value count"
        );
    }
}

#[test]
fn scaffold_factory_remains_lazy_and_matches_nonroot_value_free_events() {
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    fn create() -> Token {
        CALLS.fetch_add(1, Ordering::Relaxed);
        Token(u64::MAX)
    }
    let mut root = PathMap::new();
    root.insert([], Token(1));
    let mut branch = PathMap::new();
    branch.insert(b"shared/path/a", Token(1));
    branch.create_path(b"shared/path/empty");
    let mut set = PathMap::new();
    set.insert(b"set/leaf", ());
    let mut saw_scaffold = false;
    for repr in [
        EPathMapRepr::Empty,
        EPathMapRepr::Set(set),
        EPathMapRepr::Map(root),
        EPathMapRepr::Map(branch),
    ] {
        let image = snapshot(&repr);
        let mut expected = 0;
        if repr.mode() == EPathMapMode::Map {
            let framing = parse_pending_framing(&image).unwrap();
            let mut cursor =
                CanonicalCompactTrieTopologyCursor::new(&image[framing.arena]).unwrap();
            let mut depth = 0usize;
            while let Some(event) = cursor.next_event() {
                match event {
                    CanonicalCompactTrieTopologyEvent::Value { value: None, .. } if depth != 0 => {
                        expected += 1
                    }
                    CanonicalCompactTrieTopologyEvent::Edge { .. } => depth += 1,
                    CanonicalCompactTrieTopologyEvent::Leave { .. } => {
                        depth = depth.saturating_sub(1)
                    }
                    _ => {}
                }
            }
        }
        CALLS.store(0, Ordering::Relaxed);
        let mut pending =
            PendingEpmDecode::new_with_provider(image, true, PendingScaffold::New(create)).unwrap();
        assert_eq!(CALLS.load(Ordering::Relaxed), 0);
        while pending.next_value_bytes().unwrap().is_some() {
            pending.push_value(Token(1)).unwrap();
        }
        pending.finish().unwrap();
        assert_eq!(CALLS.load(Ordering::Relaxed), expected);
        saw_scaffold |= expected > 0;
    }
    assert!(saw_scaffold);
    CALLS.store(0, Ordering::Relaxed);
    let mut rejected = PendingEpmDecode::new_with_provider(
        root_image(1, &[&[]], 1),
        true,
        PendingScaffold::New(create),
    )
    .unwrap();
    rejected.next_value_bytes().unwrap();
    rejected.push_value(Token(1)).unwrap();
    assert!(rejected.finish().is_err());
    assert_eq!(CALLS.load(Ordering::Relaxed), 0);
}

#[test]
fn planned_handles_keep_exact_associations_without_resident_emission() {
    use crate::rust::par_construction::{ParConstruction, PlannedPar};
    let mut arena = ParConstruction::new();
    let first = arena.empty_par().unwrap();
    let second = arena.empty_par().unwrap();
    let scaffold = arena.empty_par().unwrap();
    assert_ne!(first, scaffold);
    let mut map = PathMap::new();
    map.insert([], first);
    map.insert(b"prefix/a", second);
    map.insert(b"prefix/b", second);
    map.create_path(b"topology/only");
    let repr = EPathMapRepr::Map(map);
    let expected = entries(repr.as_map().unwrap());
    let image = snapshot(&repr);
    let replay = || {
        let mut pending: OwnedPendingEpmDecode<PlannedPar> =
            PendingEpmDecode::new_with_scaffold(image, true, scaffold).unwrap();
        for (_, value) in &expected {
            pending.next_value_bytes().unwrap().unwrap();
            pending.push_value(*value).unwrap();
        }
        pending.finish().unwrap()
    };
    #[cfg(feature = "protobuf-projection-experiment")]
    let actual = {
        use crate::rust::par_construction::substitution_cost::{capture, Metric};
        let (actual, stats) = capture(replay);
        for metric in [
            Metric::FinishAttempts,
            Metric::FinishSuccesses,
            Metric::ResidentBytes,
            Metric::RootKeys,
            Metric::RootKeyBytes,
        ] {
            assert_eq!(stats.get(metric), 0);
        }
        actual
    };
    #[cfg(not(feature = "protobuf-projection-experiment"))]
    let actual = replay();
    assert_eq!(layout(&actual).prefix(), layout(&repr).prefix());
    assert_eq!(entries(actual.as_map().unwrap()), expected);
}

#[test]
fn generic_topology_replay_and_drop_are_stack_safe() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            let mut map = PathMap::new();
            map.insert(vec![b'a'; 8192], Token(1));
            map.create_path(vec![b'b'; 8192]);
            replay_matches(EPathMapRepr::Map(map));
        })
        .unwrap()
        .join()
        .unwrap();
}

proptest! {
    #[test]
    fn arbitrary_prefix_maps_preserve_payload_association(
        items in prop::collection::vec((prop::collection::vec(any::<u8>(), 0..40), any::<u16>()), 0..40),
        dangling in prop::collection::vec(any::<u8>(), 1..40),
    ) {
        let mut map = PathMap::new();
        for (key, value) in items { map.insert(key, Token(u64::from(value))); }
        map.create_path(dangling);
        replay_matches(EPathMapRepr::Map(map));
    }
}
