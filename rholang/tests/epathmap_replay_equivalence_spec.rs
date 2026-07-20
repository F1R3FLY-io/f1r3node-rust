//! EPathMap fix P4.3 — REPLAY EQUIVALENCE over an EPathMap-heavy program
//! (plan v1 §1-P4 gate: "record a run, replay, assert log equality").
//!
//! Replay matching is driven ENTIRELY by event hashes: `rig` indexes the
//! recorded log by produce/consume identity, and every replayed
//! produce/consume must re-derive the SAME hash to find its COMM partner —
//! `check_replay_data` then asserts the rigged multiset was fully consumed
//! (every recorded event was reproduced, none left over: log equality). A
//! single diverging event hash (e.g. a spliced datum encoding differing
//! from the recorded direct encoding) fails the rig lookup and surfaces
//! here as an unreplayable COMM or leftover replay data.
//!
//! The program is EPathMap-heavy on every hash leg: map-bearing PRODUCE
//! datums (the map travels in send payloads, twice — original and relay),
//! map-bearing CONSUME patterns are exercised structurally via the bound
//! `@idx` (free-var binds re-transport the map), and the query-method
//! chains drive the intern store so the play and replay runs see DIFFERENT
//! intern-cell warmth (replay runs second in the same process) — the
//! spliced/direct paths must agree on bytes regardless (pinned exhaustively
//! by the models differential suite; sealed end-to-end here).
//!
//! Checkpoint roots are compared too: play and replay must materialize the
//! same history (the cold-store serializer twins at work).

use std::collections::HashMap;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::trace::event::Event;

/// Map-heavy source: a pathmap index is produced, consumed, queried through
/// zipper chains, relayed (a SECOND map-bearing produce of the SAME value),
/// re-consumed, and re-queried. Every COMM either transports the map or
/// fires a continuation whose body mentions it.
const EPATHMAP_HEAVY_PROGRAM: &str = r#"
    new idxCh, relayCh in {
        idxCh!({| ["a", "x"], ["a", "y"], ["b"], ["c", "z"] |}) |
        for (@idx <- idxCh) {
            @"existsA"!( idx.readZipperAt(["a"]).pathExists() ) |
            @"countA"!( idx.readZipperAt(["a"]).childCount() ) |
            @"firstLeaf"!( idx.readZipperAt(["a"]).descendFirst().getLeaf() == ["a", "x"] ) |
            relayCh!(idx) |
            for (@idx2 <- relayCh) {
                @"existsCZ"!( idx2.readZipperAt(["c", "z"]).pathExists() ) |
                @"missing"!( idx2.readZipperAt(["zz"]).pathExists() == false ) |
                @"subtrie"!( idx2.readZipperAt(["a"]).getSubtrie().childCount() )
            }
        }
    }
"#;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn epathmap_heavy_record_then_replay_reproduces_the_log() {
    let mut kvm = InMemoryStoreManager::new();
    let store = kvm
        .r_space_stores()
        .await
        .expect("in-memory rspace store");
    let mut additional_system_processes = Vec::new();
    let (mut runtime, mut replay_runtime, _history) =
        create_runtimes(store, false, &mut additional_system_processes).await;

    let initial_phlo = Cost::create(i64::MAX, "epathmap replay equivalence".to_string());
    let rand = Blake2b512Random::create_from_bytes(&[0x51; 32]);

    // ── RECORD ───────────────────────────────────────────────────────────
    let play_result = runtime
        .evaluate(
            EPATHMAP_HEAVY_PROGRAM,
            initial_phlo.clone(),
            HashMap::new(),
            rand.clone(),
        )
        .await
        .expect("play evaluation must run");
    assert!(
        play_result.errors.is_empty(),
        "play evaluation errors: {:?}",
        play_result.errors
    );

    let log = runtime.take_event_log().await;
    let comm_count = log
        .iter()
        .filter(|event| matches!(event, Event::Comm(_)))
        .count();
    assert!(
        comm_count >= 2,
        "the program must fire at least the idx and relay COMMs (got {comm_count} in {} events)",
        log.len()
    );

    // ── REPLAY ───────────────────────────────────────────────────────────
    // `rig` indexes the recorded log by EVENT HASH; the replayed run must
    // re-derive every hash to consume it.
    replay_runtime
        .rig(log.clone())
        .await
        .expect("replay rig must accept the recorded log");

    let replay_result = replay_runtime
        .evaluate(EPATHMAP_HEAVY_PROGRAM, initial_phlo, HashMap::new(), rand)
        .await
        .expect("replay evaluation must run");
    assert!(
        replay_result.errors.is_empty(),
        "replay evaluation errors: {:?}",
        replay_result.errors
    );

    // LOG EQUALITY: every recorded event was reproduced by the replay (the
    // rigged multiset is empty — nothing missing, nothing left over).
    replay_runtime
        .check_replay_data()
        .await
        .expect("replay must consume the ENTIRE recorded log (event-hash equality)");

    // Same results (play vs replay charge the same total too).
    assert_eq!(
        play_result.cost, replay_result.cost,
        "play and replay must charge identically"
    );

    // HISTORY EQUALITY: both sides checkpoint to the same root (the
    // cold-store serializer twins produce identical leaves).
    let play_checkpoint = runtime.create_checkpoint().await;
    let replay_checkpoint = replay_runtime.create_checkpoint().await;
    assert_eq!(
        play_checkpoint.root, replay_checkpoint.root,
        "play and replay checkpoint roots must be identical"
    );
}
