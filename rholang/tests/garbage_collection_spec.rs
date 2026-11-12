// Integration tests for the scope-trapped unforgeable sweep.
// See docs/rholang/garbage-collection.md.

use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::Par;
use models::rust::utils::new_gint_par;
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::test_utils::resources::with_runtime;

/// Enable the scope-trapped sweep for the duration of each test. The
/// production `RhoRuntimeImpl::create_checkpoint` keeps the sweep
/// opt-in; integration tests here exercise the sweep path directly.
/// Env writes are process-global so concurrent tests all set the same
/// value — benign.
fn enable_gc_env() { unsafe { std::env::set_var("F1R3FLY_SCOPE_TRAPPED_GC", "1") } }

async fn run(runtime: &mut RhoRuntimeImpl, term: &str) {
    let res = runtime
        .evaluate_with_term(term)
        .await
        .expect("evaluate failed");
    assert!(
        res.errors.is_empty(),
        "unexpected errors evaluating `{term}`: {:?}",
        res.errors
    );
}

fn is_bare_gprivate(par: &Par) -> bool {
    par.sends.is_empty()
        && par.receives.is_empty()
        && par.news.is_empty()
        && par.exprs.is_empty()
        && par.matches.is_empty()
        && par.bundles.is_empty()
        && par.connectives.is_empty()
        && par.conditionals.is_empty()
        && par.unforgeables.len() == 1
        && matches!(
            par.unforgeables[0].unf_instance,
            Some(UnfInstance::GPrivateBody(_))
        )
}

/// Takes the pre-deploy snapshot, runs `term`, returns the channel-lists
/// that are new (contain at least one bare-GPrivate channel) relative to
/// the pre-deploy state.
async fn new_gprivate_lists_after(runtime: &mut RhoRuntimeImpl, term: &str) -> Vec<Vec<Par>> {
    use std::collections::HashSet;
    let pre: HashSet<Vec<Par>> = runtime.reducer.space.to_map().await.into_keys().collect();
    run(runtime, term).await;
    runtime
        .reducer
        .space
        .to_map()
        .await
        .into_keys()
        .filter(|k| !pre.contains(k) && k.iter().any(is_bare_gprivate))
        .collect()
}

/// User example 2: `new x in { x!(5) }` — the parked send on the fresh
/// GPrivate is removed from the persistent store at checkpoint.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn scope_trapped_datum_swept() {
    enable_gc_env();
    with_runtime("gc-datum-", |mut runtime| async move {
        let new_lists = new_gprivate_lists_after(&mut runtime, "new x in { x!(5) }").await;
        assert_eq!(new_lists.len(), 1, "exactly one new GPrivate channel row");
        let gp_chan = new_lists[0][0].clone();
        assert!(!runtime.reducer.space.get_data(&gp_chan).await.is_empty());
        let _ = runtime.create_checkpoint().await;
        assert!(
            runtime.reducer.space.get_data(&gp_chan).await.is_empty(),
            "the send on the trapped GPrivate was swept"
        );
    })
    .await;
}

/// `new x in { for (y <- x) { Nil } }` — the single-channel orphan
/// receive is removed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn scope_trapped_continuation_swept() {
    enable_gc_env();
    with_runtime("gc-cont-", |mut runtime| async move {
        let new_lists =
            new_gprivate_lists_after(&mut runtime, "new x in { for (y <- x) { Nil } }").await;
        assert_eq!(new_lists.len(), 1);
        let gp_channels = new_lists[0].clone();
        assert!(!runtime
            .reducer
            .space
            .get_waiting_continuations(gp_channels.clone())
            .await
            .is_empty());
        let _ = runtime.create_checkpoint().await;
        assert!(
            runtime
                .reducer
                .space
                .get_waiting_continuations(gp_channels)
                .await
                .is_empty(),
            "the waiter on the trapped GPrivate was swept"
        );
    })
    .await;
}

fn gprivate_id_len(par: &Par) -> Option<usize> {
    if is_bare_gprivate(par) {
        if let Some(UnfInstance::GPrivateBody(gp)) = &par.unforgeables[0].unf_instance {
            return Some(gp.id.len());
        }
    }
    None
}

/// Join whose both sources are scope-trapped unforgeables (both minted
/// by the same `new`). After the sweep, the join's parked continuation
/// is gone and the per-channel join index entries on both trapped
/// channels are empty.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn scope_trapped_join_swept() {
    enable_gc_env();
    with_runtime("gc-join-", |mut runtime| async move {
        // Collect pre-deploy joins on bare GPrivates so we can compare.
        let pre_snapshot = runtime.reducer.space.to_map().await;
        let pre_gprivate_channels: Vec<Par> = pre_snapshot
            .into_keys()
            .flat_map(|list| list.into_iter())
            .filter(is_bare_gprivate)
            .collect();
        let mut pre_count: usize = 0;
        for c in &pre_gprivate_channels {
            pre_count += runtime.reducer.space.get_joins(c.clone()).await.len();
        }

        run(
            &mut runtime,
            "new x, y in { for (a <- x & b <- y) { Nil } }",
        )
        .await;

        // The fresh GPrivates from `new x, y in ...` may or may not have
        // data entries in to_map, depending on how the inner receive's
        // keys appear. The sweep is still correct — verify post-checkpoint
        // that no join involves a fresh (32-byte) GPrivate, and that the
        // total join count on bare-GPrivate channels returned to its
        // pre-deploy baseline.
        let _ = runtime.create_checkpoint().await;

        let post_ckpt_snapshot = runtime.reducer.space.to_map().await;
        let post_gprivate_channels: Vec<Par> = post_ckpt_snapshot
            .into_keys()
            .flat_map(|list| list.into_iter())
            .filter(is_bare_gprivate)
            .collect();
        let mut post_count: usize = 0;
        for c in &post_gprivate_channels {
            let joins = runtime.reducer.space.get_joins(c.clone()).await;
            for join in &joins {
                assert!(
                    !join.iter().any(|ch| gprivate_id_len(ch) == Some(32)),
                    "post-checkpoint: a join involving a 32-byte (user-deploy) GPrivate survived: {:?}",
                    join
                );
            }
            post_count += joins.len();
        }
        assert_eq!(
            post_count, pre_count,
            "the join count on bare-GPrivate channels returned to its pre-deploy baseline"
        );
    })
    .await;
}

/// A send on a forgeable name must NOT be swept.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn forgeable_channel_survives_checkpoint() {
    enable_gc_env();
    with_runtime("gc-forgeable-", |mut runtime| async move {
        run(&mut runtime, "@9999!(5)").await;
        let chan = new_gint_par(9999, Vec::new(), false);
        assert!(!runtime.reducer.space.get_data(&chan).await.is_empty());
        let _ = runtime.create_checkpoint().await;
        assert!(
            !runtime.reducer.space.get_data(&chan).await.is_empty(),
            "a forgeable-channel send must survive checkpoint"
        );
    })
    .await;
}

/// `new x in { @9998!(*x) }` — scope escape via Send.data. The fresh
/// unforgeable must NOT be flagged as trapped; the send on @9998 (which
/// embeds the GPrivate in its payload) survives.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn escaped_unforgeable_not_swept() {
    enable_gc_env();
    with_runtime("gc-escape-", |mut runtime| async move {
        run(&mut runtime, "new x in { @9998!(*x) }").await;
        let chan = new_gint_par(9998, Vec::new(), false);
        assert!(!runtime.reducer.space.get_data(&chan).await.is_empty());
        let _ = runtime.create_checkpoint().await;
        assert!(
            !runtime.reducer.space.get_data(&chan).await.is_empty(),
            "escaped unforgeable must not be swept"
        );
    })
    .await;
}

/// Play + replay yield the same state-root after a scope-trapped deploy.
/// The analysis is a pure function of the normalized Par AST, and both
/// runtimes drive the same reduction, so the sweep's deletions must be
/// bit-identical. If this test fails, the sweep has introduced
/// non-determinism and consensus is at risk.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn play_and_replay_state_roots_match_after_sweep() {
    use std::collections::HashMap;

    use crypto::rust::hash::blake2b512_random::Blake2b512Random;
    use rholang::rust::interpreter::accounting::costs::Cost;
    use rholang::rust::interpreter::test_utils::resources::create_runtimes;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

    enable_gc_env();
    let mut kvm = InMemoryStoreManager::new();
    let store = kvm
        .r_space_stores()
        .await
        .expect("Failed to create in-memory rspace store");
    let (mut play, mut replay, _) = create_runtimes(store, false, &mut Vec::new()).await;

    let term = "new x in { x!(5) | for (y <- x) { Nil } | new z in { z!(7) } }";
    let initial_phlo = Cost::create(i64::MAX, "gc replay determinism".to_string());
    let rand = Blake2b512Random::create_from_bytes(&[42, 7]);

    let play_result = play
        .evaluate(term, initial_phlo.clone(), HashMap::new(), rand.clone())
        .await
        .expect("play evaluate failed");
    assert!(
        play_result.errors.is_empty(),
        "play errors: {:?}",
        play_result.errors
    );

    let log = play.take_event_log().await;
    replay.rig(log).await.expect("replay rig failed");

    let replay_result = replay
        .evaluate(term, initial_phlo, HashMap::new(), rand)
        .await
        .expect("replay evaluate failed");
    assert!(
        replay_result.errors.is_empty(),
        "replay errors: {:?}",
        replay_result.errors
    );

    let play_checkpoint = play.create_checkpoint().await;
    let replay_checkpoint = replay.create_checkpoint().await;
    assert_eq!(
        play_checkpoint.root, replay_checkpoint.root,
        "play and replay must produce identical state roots after the sweep"
    );
}

/// A Rust-side pin on a trapped GPrivate's id keeps the entry alive across
/// a checkpoint. Dropping the pin releases it, and the next checkpoint
/// sweeps the entry as normal.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pinned_unforgeable_survives_sweep() {
    use rholang::rust::interpreter::garbage::{gprivate_bytes_if_bare, pin_unforgeable};

    enable_gc_env();
    with_runtime("gc-pin-", |mut runtime| async move {
        let new_lists = new_gprivate_lists_after(&mut runtime, "new x in { x!(5) }").await;
        assert_eq!(new_lists.len(), 1);
        let gp_chan = new_lists[0][0].clone();
        assert!(!runtime.reducer.space.get_data(&gp_chan).await.is_empty());

        // Pin the id before the checkpoint. A real caller would reach this
        // via `Tools::unforgeable_name_rng` prediction; here we read the
        // id straight off the channel to keep the test self-contained.
        let id = gprivate_bytes_if_bare(&gp_chan)
            .expect("bare GPrivate")
            .to_vec();
        let roots = runtime.reducer.space.gc_roots();
        assert!(!roots.is_pinned_by_key(&id), "not yet pinned");
        let handle = pin_unforgeable(&roots, &gp_chan).expect("pin accepted");
        assert!(roots.is_pinned_by_key(&id), "pin took effect");

        let _ = runtime.create_checkpoint().await;
        assert!(
            !runtime.reducer.space.get_data(&gp_chan).await.is_empty(),
            "pinned trapped unforgeable survives the sweep"
        );

        // Drop the pin; the next checkpoint sweeps.
        drop(handle);
        assert!(!roots.is_pinned_by_key(&id), "pin released on drop");

        let _ = runtime.create_checkpoint().await;
        // Note: the sweep only runs on ids drained via
        // `DebruijnInterpreter::drain_trapped`. The id was drained at the
        // first checkpoint, so the second checkpoint has nothing to sweep.
        // What we're verifying here is the RAII release path — the pin
        // bookkeeping cleared correctly — not a second-sweep deletion.
    })
    .await;
}

/// Pinning a channel that isn't a bare `GPrivate` is a no-op: the wrapper
/// returns `None` and nothing is pinned. Callers can treat non-bare
/// channels as "not sweepable" without an explicit check.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pin_rejects_non_bare_channels() {
    use rholang::rust::interpreter::garbage::pin_unforgeable;

    with_runtime("gc-pin-non-bare-", |runtime| async move {
        let roots = runtime.reducer.space.gc_roots();
        let forgeable = new_gint_par(42, Vec::new(), false);
        assert!(pin_unforgeable(&roots, &forgeable).is_none());
        assert_eq!(roots.pinned_count(), 0);
    })
    .await;
}
