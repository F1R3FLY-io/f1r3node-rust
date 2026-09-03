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

#[test]
fn concurrent_rspace_architecture_repro_candidate_ordering_must_be_content_hash_deterministic() {
    let rspace = source("src/rspace/rspace.rs");

    assert!(
        !rspace.contains("thread_rng()") && !rspace.contains(".shuffle("),
        "RSpace production matching still uses random shuffle ordering; consensus requires \
         deterministic content-hash ordering"
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

#[test]
fn concurrent_rspace_architecture_repro_checkpoint_gate_must_exclude_transitions() {
    let rspace = source("src/rspace/rspace.rs");

    for method in
        [
            "create_checkpoint",
            "reset",
            "create_soft_checkpoint",
            "revert_to_soft_checkpoint",
            "take_event_log",
        ]
    {
        let marker = format!("async fn {method}(");
        let start = rspace
            .find(&marker)
            .unwrap_or_else(|| panic!("RSpace::{method} is missing"));
        let body = &rspace[start..rspace.len().min(start + 320)];
        assert!(
            body.contains("operation_gate.write().await"),
            "RSpace::{method} must hold the exclusive checkpoint operation gate"
        );
    }

    assert_eq!(
        rspace
            .matches("let _operation_guard = self.operation_gate.read().await;")
            .count(),
        5,
        "consume, produce, install, and both remove-all mutation paths must hold shared permits"
    );
}

#[test]
fn concurrent_rspace_architecture_repro_replay_gate_must_match_live_authority_boundary() {
    let replay = source("src/rspace/replay_rspace.rs");

    for method in [
        "create_checkpoint",
        "reset",
        "create_soft_checkpoint",
        "revert_to_soft_checkpoint",
        "clear",
        "take_event_log",
    ] {
        let marker = format!("async fn {method}(");
        let start = replay
            .find(&marker)
            .unwrap_or_else(|| panic!("ReplayRSpace::{method} is missing"));
        let body = &replay[start..replay.len().min(start + 420)];
        assert!(
            body.contains("operation_gate.write().await"),
            "ReplayRSpace::{method} must hold the exclusive authority-transition gate"
        );
    }

    for method in ["consume", "produce", "install"] {
        let marker = format!("async fn {method}(");
        let start = replay
            .find(&marker)
            .unwrap_or_else(|| panic!("ReplayRSpace::{method} is missing"));
        let body = &replay[start..replay.len().min(start + 2_500)];
        assert!(
            body.contains("operation_gate.read().await"),
            "ReplayRSpace::{method} must hold a shared mutation permit"
        );
    }
}
