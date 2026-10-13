use std::path::{Path, PathBuf};

fn crate_path(relative: impl AsRef<Path>) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn source(relative: impl AsRef<Path>) -> String {
    let path = crate_path(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()))
}

#[test]
fn concurrent_rspace_architecture_repro_hot_store_must_not_wrap_dashmaps_in_global_mutex() {
    let hot_store = source("src/rspace/hot_store.rs");

    assert!(
        !hot_store.contains("Arc<Mutex<HotStoreState") &&
            !hot_store.contains("Mutex<HotStoreState"),
        "HotStoreState is still protected by a global mutex, defeating the DashMap shard-level \
         concurrency"
    );
}

#[test]
fn concurrent_rspace_architecture_repro_ispace_mutations_must_use_shared_access() {
    let interface = source("src/rspace/rspace_interface.rs");

    for method in ["create_checkpoint", "clear", "reset", "consume", "produce", "install"] {
        let old_signature = format!("fn {method}(\n        &mut self");
        assert!(
            !interface.contains(&old_signature),
            "ISpace::{method} still requires &mut self; the interpreter must not need an \
             exclusive global RSpace lock"
        );
    }
}

// Superseded by DR-129: restored dev's randomized candidate order (user
// decision 2026-10-12). This guard read only rspace.rs, while the shuffle
// lives in rspace/ops_produce.rs, so it passed without checking anything.
// Native replay follows the recorded pick in its own canonical order, so play
// needs no deterministic order. The guard below requires dev's shuffle instead.
// #[test]
// fn concurrent_rspace_architecture_repro_candidate_ordering_must_be_content_hash_deterministic() {
//     let rspace = source("src/rspace/rspace.rs");
//
//     assert!(
//         !rspace.contains("thread_rng()") && !rspace.contains(".shuffle("),
//         "RSpace production matching still uses random shuffle ordering;
// consensus requires \          deterministic content-hash ordering"
//     );
// }

/// Added by DR-129: play shuffles the candidates of an operation at random,
/// as dev does (`RSpace::shuffle_with_index`).
#[test]
fn concurrent_rspace_architecture_repro_play_candidate_order_is_devs_random_shuffle() {
    let produce = source("src/rspace/rspace/ops_produce.rs");

    assert!(
        produce.contains("indexed_vec.shuffle(&mut rand::rng());"),
        "play no longer shuffles its candidates at random as dev does (DR-129)"
    );
}

#[test]
fn concurrent_rspace_architecture_repro_join_matching_must_have_channel_group_locks() {
    let rspace = source("src/rspace/rspace.rs");

    assert!(
        rspace.contains("phase_a_locks") &&
            rspace.contains("phase_b_locks") &&
            rspace.contains("DashMap") &&
            rspace.contains("Mutex<()>"),
        "RSpace has no two-phase per-channel-group lock surface, so join matching cannot safely \
         maximize independent-channel parallelism"
    );
}
