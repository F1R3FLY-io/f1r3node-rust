use std::collections::BTreeMap;
use std::sync::Arc;

use proptest::prelude::*;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::history::cold_store::{
    ContinuationsLeaf, DataLeaf, JoinsLeaf, PersistedData,
};
use rspace_plus_plus::rspace::history::history_repository::{
    PREFIX_DATUM, PREFIX_JOINS, PREFIX_KONT,
};
use rspace_plus_plus::rspace::history::radix_tree::{Item, empty_node, hash_node};
use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
use rspace_plus_plus::rspace::shared::trie_importer::TrieImporter;
use rspace_plus_plus::rspace::state::exporters::rspace_exporter_items::RSpaceExporterItems;
use rspace_plus_plus::rspace::state::instances::rspace_exporter_store::RSpaceExporterStore;
use rspace_plus_plus::rspace::state::instances::rspace_importer_store::RSpaceImporterImpl;
use rspace_plus_plus::rspace::state::rspace_exporter::RSpaceExporter;
use rspace_plus_plus::rspace::state::rspace_importer::{RSpaceImporter, RSpaceImporterInstance};
use rspace_plus_plus::rspace::state::state_walk::{StateWalk, WALK_PAGE_SIZE};
use shared::rust::store::key_value_store::KeyValueStore;

type Items = BTreeMap<Vec<u8>, Vec<u8>>;

type Keys = Vec<(Vec<u8>, Vec<u8>)>;

#[derive(Default, Clone)]
struct State {
    history: Items,
    data: Items,
}

struct Stores {
    importer: Arc<RSpaceImporterImpl>,
    exporter: Arc<dyn RSpaceExporter>,
}

fn stores(state: &State) -> Stores {
    let store = |items: &Items| -> Arc<dyn KeyValueStore> {
        let store = InMemoryKeyValueStore::new();
        store.put(items.clone().into_iter().collect()).unwrap();
        Arc::new(store)
    };
    let (history, data, roots) = (store(&state.history), store(&state.data), store(&Items::new()));
    Stores {
        exporter: Arc::new(RSpaceExporterStore::create(
            history.clone(),
            data.clone(),
            roots.clone(),
        )),
        importer: Arc::new(RSpaceImporterImpl {
            history_store: history,
            value_store: data,
            roots_store: roots,
        }),
    }
}

fn leaf(id: u32, seed: u8) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let bytes = [id.to_be_bytes().as_slice(), &[seed]].concat();
    let (kind, data) = match id % 3 {
        0 => (PREFIX_DATUM, PersistedData::Data(DataLeaf { bytes })),
        1 => (PREFIX_KONT, PersistedData::Continuations(ContinuationsLeaf { bytes })),
        _ => (PREFIX_JOINS, PersistedData::Joins(JoinsLeaf { bytes })),
    };
    let inner = match &data {
        PersistedData::Data(leaf) => bincode::serialize(leaf),
        PersistedData::Continuations(leaf) => bincode::serialize(leaf),
        PersistedData::Joins(leaf) => bincode::serialize(leaf),
    }
    .unwrap();
    let mut key = vec![kind];
    let mut tail = Blake2b256Hash::new(&id.to_be_bytes()).bytes();
    tail[0] %= 8;
    key.extend(tail);
    (key, Blake2b256Hash::new(&inner).bytes(), bincode::serialize(&data).unwrap())
}

fn build(keys: &[(Vec<u8>, Vec<u8>)], depth: usize, state: &mut State) -> Vec<u8> {
    let mut node = empty_node();
    let mut slots: BTreeMap<u8, Keys> = BTreeMap::new();
    for (key, value) in keys {
        slots
            .entry(key[depth])
            .or_default()
            .push((key.clone(), value.clone()));
    }
    for (slot, group) in slots {
        node[slot as usize] = match group.as_slice() {
            [(key, value)] => Item::Leaf {
                prefix: key[depth + 1..].to_vec(),
                value: value.clone(),
            },
            _ => {
                let first = &group[0].0;
                let common = (depth + 1..first.len())
                    .take_while(|i| group.iter().all(|(key, _)| key[*i] == first[*i]))
                    .count();
                Item::NodePtr {
                    prefix: first[depth + 1..depth + 1 + common].to_vec(),
                    ptr: build(&group, depth + 1 + common, state),
                }
            }
        };
    }
    let (hash, bytes) = hash_node(&node);
    state.history.insert(hash.clone(), bytes);
    hash
}

fn state_of(leaves: &BTreeMap<u32, u8>) -> (Blake2b256Hash, State) {
    let mut state = State::default();
    let keys: Vec<_> = leaves
        .iter()
        .map(|(id, seed)| {
            let (key, hash, data) = leaf(*id, *seed);
            state.data.insert(hash.clone(), data);
            (key, hash)
        })
        .collect();
    let root = build(&keys, 0, &mut state);
    (Blake2b256Hash::from_bytes(root), state)
}

#[derive(Default, Debug)]
struct Traffic {
    requests: usize,
    history: usize,
    data: usize,
}

fn fetch(
    server: &Stores,
    client: &Stores,
    path: &[(Blake2b256Hash, Option<u8>)],
    kind: Option<u8>,
    traffic: &mut Traffic,
) -> Vec<Blake2b256Hash> {
    let (history, data) = RSpaceExporterItems::get_history_and_data(
        server.exporter.clone(),
        path.to_vec(),
        0,
        WALK_PAGE_SIZE,
    )
    .expect("the server exports the page");
    RSpaceImporterInstance::validate_subtree_items(
        &history.items,
        &data.items,
        path.to_vec(),
        &history.last_path,
        WALK_PAGE_SIZE,
        0,
        kind,
        client.importer.clone(),
    )
    .expect("the page is valid");
    traffic.requests += 1;
    traffic.history += history.items.len();
    traffic.data += data.items.len();
    let keys = data.items.iter().map(|(key, _)| key.clone()).collect();
    client.importer.set_history_items(history.items);
    client.importer.set_data_items(data.items);
    keys
}

fn sync(server: &Stores, client: &Stores, roots: &[Blake2b256Hash]) -> Traffic {
    let mut traffic = Traffic::default();
    let mut walk = StateWalk::new(client.importer.clone());
    let mut pending: Vec<Blake2b256Hash> = Vec::new();
    for root in roots {
        pending.extend(walk.add_root(root).unwrap());
    }
    while let Some(node) = pending.pop() {
        let kind = walk.leaf_kind(&node);
        fetch(server, client, &[(node.clone(), None)], kind, &mut traffic);
        pending.extend(walk.node_imported(&node).unwrap());
    }
    for missing in walk.take_missing_data().unwrap() {
        let received = fetch(server, client, &missing.path, missing.kind, &mut traffic);
        for hash in &missing.data {
            assert!(received.contains(hash), "the page does not carry the leaf it was asked for");
        }
    }
    traffic
}

fn assert_holds(client: &Stores, state: &State) {
    for (hash, bytes) in &state.history {
        assert_eq!(
            client
                .importer
                .get_history_item(Blake2b256Hash::from_bytes(hash.clone())),
            Some(bytes.clone())
        );
    }
    let hashes: Vec<_> = state
        .data
        .keys()
        .cloned()
        .map(Blake2b256Hash::from_bytes)
        .collect();
    assert!(
        client
            .importer
            .has_data_items(&hashes)
            .into_iter()
            .all(|held| held)
    );
}

fn leaves(ids: std::ops::Range<u32>) -> BTreeMap<u32, u8> { ids.map(|id| (id, 0)).collect() }

#[test]
fn an_unchanged_state_needs_no_request() {
    let (root, state) = state_of(&leaves(0..2000));
    let traffic = sync(&stores(&state), &stores(&state), &[root]);
    assert_eq!(traffic.requests, 0);
}

#[test]
fn only_the_changed_part_of_a_state_is_requested() {
    let mut changed = leaves(0..20_000);
    let (_, base) = state_of(&changed);
    for id in 0..20 {
        changed.insert(id * 997, 1);
    }
    let (root, state) = state_of(&changed);
    let client = stores(&base);
    let traffic = sync(&stores(&state), &client, &[root]);

    assert_holds(&client, &state);
    let total = state.history.len() + state.data.len();
    assert!(
        (traffic.history + traffic.data) * 50 < total,
        "{traffic:?} of {total} items were transferred"
    );
}

#[test]
fn a_node_shared_by_several_roots_is_requested_once() {
    let mut current = leaves(0..5000);
    let mut server = State::default();
    let mut roots = Vec::new();
    for step in 0..20u32 {
        current.insert(step, 1);
        let (root, state) = state_of(&current);
        server.history.extend(state.history);
        server.data.extend(state.data);
        roots.push(root);
    }
    let client = stores(&State::default());
    let traffic = sync(&stores(&server), &client, &roots);
    assert_holds(&client, &server);
    assert!(traffic.requests <= server.history.len() + server.data.len(), "{traffic:?}");
}

#[test]
fn a_held_node_does_not_hide_its_missing_subtree() {
    let (root, state) = state_of(&leaves(0..3000));
    let mut held = State::default();
    held.history
        .insert(root.bytes(), state.history[&root.bytes()].clone());
    let client = stores(&held);
    sync(&stores(&state), &client, &[root]);
    assert_holds(&client, &state);
}

#[test]
fn a_node_that_is_still_missing_after_its_import_is_an_error() {
    let (root, _) = state_of(&leaves(0..10));
    let mut walk = StateWalk::new(stores(&State::default()).importer);
    assert_eq!(walk.add_root(&root).unwrap(), vec![root.clone()]);
    assert!(walk.node_imported(&root).is_err());
}

#[test]
fn missing_data_of_leaves_of_several_kinds_in_a_root_is_requested() {
    let (root, state) = state_of(&[(44, 0), (253, 0)].into());
    let held = State {
        history: state.history.clone(),
        data: Items::new(),
    };
    let client = stores(&held);
    sync(&stores(&state), &client, &[root]);
    assert_holds(&client, &state);
}

#[test]
fn data_stored_by_a_checkpoint_counts_as_held() {
    let (root, mut state) = state_of(&leaves(0..100));
    state.data = state
        .data
        .into_iter()
        .map(|(hash, data)| (bincode::serialize(&Blake2b256Hash::from_bytes(hash)).unwrap(), data))
        .collect();
    let client = stores(&state);
    let traffic = sync(&stores(&state), &client, &[root]);
    assert_eq!(traffic.requests, 0);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn any_partial_store_is_completed(
        ids in prop::collection::btree_set(0u32..600, 1..250),
        kept in prop::collection::vec(any::<bool>(), 512),
    ) {
        let (root, state) = state_of(&ids.into_iter().map(|id| (id, 0)).collect());
        let keep = |items: &Items, offset: usize| -> Items {
            items
                .iter()
                .enumerate()
                .filter(|(i, _)| kept[(i * 7 + offset) % kept.len()])
                .map(|(_, (key, value))| (key.clone(), value.clone()))
                .collect()
        };
        let held = State { history: keep(&state.history, 0), data: keep(&state.data, 3) };
        let client = stores(&held);
        sync(&stores(&state), &client, &[root]);
        assert_holds(&client, &state);
    }
}
