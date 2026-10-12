use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{GPrivate, GUnforgeable, New, Par};
use rholang::rust::interpreter::accounting::cost_accounting::CostAccounting;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::env::Env;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::io::{FS_NATIVE_URN_PREFIX, FS_NATIVE_URN_PREFIX_VERSIONED};
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::reduce::DebruijnInterpreter;
use rholang::rust::interpreter::rho_runtime::RhoISpace;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

fn rand() -> Blake2b512Random { Blake2b512Random::create_from_bytes(&Vec::new()) }

/// Build a `New`-wrapped Par that binds `uris` and otherwise does
/// nothing in its body (empty `Par::default()`).  Used across every
/// filter test to exercise the exact `eval_new` code path.
fn new_par_binding_uris(uris: Vec<String>) -> Par {
    Par::default().with_news(vec![New {
        bind_count: uris.len() as i32,
        p: Some(Par::default()),
        uri: uris,
        injections: BTreeMap::new(),
        locally_free: Vec::new(),
    }])
}

/// Minimal reducer + a `urn_map` entry for the given URN mapping to
/// a dummy private channel so the non-filter success paths resolve.
async fn reducer_with_urn_map(entries: Vec<String>) -> Arc<DebruijnInterpreter> {
    let cost = CostAccounting::empty_cost();
    cost.set(Cost::unsafe_max());

    let mut urn_map = HashMap::new();
    for (idx, urn) in entries.into_iter().enumerate() {
        urn_map.insert(
            urn,
            Par::default().with_unforgeables(vec![GUnforgeable {
                unf_instance: Some(UnfInstance::GPrivateBody(GPrivate {
                    id: vec![idx as u8],
                })),
            }]),
        );
    }

    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.unwrap();
    let space = RSpace::create(store, Arc::new(Box::new(Matcher))).unwrap();
    let rspace: RhoISpace = Arc::new(Box::new(space));

    DebruijnInterpreter::new(
        rspace,
        Arc::new(urn_map),
        Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        Arc::new(HashMap::new()),
        cost,
    )
}

/// Default (post-genesis / state-execution) posture: `filter_fs_native_urns`
/// defaults to TRUE.  A `New` binding any URN under
/// `FS_NATIVE_URN_PREFIX` must fail with `ReduceError` carrying the
/// phase-scope rejection text, not silently fall through to urn-map
/// resolution.
#[tokio::test]
async fn state_execution_rejects_fs_native_urn() {
    let fs_urn = format!("{FS_NATIVE_URN_PREFIX_VERSIONED}open");
    let reducer = reducer_with_urn_map(vec![fs_urn.clone()]).await;
    // Confirm the default posture — the structural pin lives at the
    // `DebruijnInterpreter::new` call site but is reflected here so a
    // regression that flipped the default to FALSE would trip this
    // test before any filter semantics apply.
    assert!(
        reducer
            .filter_fs_native_urns
            .load(std::sync::atomic::Ordering::Acquire),
        "DebruijnInterpreter::new must default filter_fs_native_urns \
         to TRUE (state-execution posture).  A regression flipping \
         the default would silently let user deploys bind FS native \
         URNs."
    );

    let par = new_par_binding_uris(vec![fs_urn.clone()]);
    let err = reducer
        .eval(par, &Env::new(), rand())
        .await
        .expect_err("eval_new must reject fs native URN under default filter");

    match err {
        InterpreterError::ReduceError(msg) => {
            assert!(
                msg.contains(&fs_urn),
                "rejection message must name the offending URN; got: {msg}"
            );
            assert!(
                msg.contains("not resolvable in this phase"),
                "rejection message must flag phase-scope rejection; got: {msg}"
            );
        }
        other => panic!("expected ReduceError, got {other:?}"),
    }
}

/// A non-fs URN under the default filter posture must still resolve
/// normally — the filter is scoped to the FS native prefix, not a
/// blanket URN block.
#[tokio::test]
async fn state_execution_accepts_non_fs_urn() {
    let non_fs_urn = "rho:test:foo".to_string();
    assert!(
        !non_fs_urn.starts_with(FS_NATIVE_URN_PREFIX),
        "fixture URN must not match filter prefix"
    );
    let reducer = reducer_with_urn_map(vec![non_fs_urn.clone()]).await;
    let par = new_par_binding_uris(vec![non_fs_urn]);
    reducer
        .eval(par, &Env::new(), rand())
        .await
        .expect("non-fs URN under default filter must resolve");
}

/// Genesis-posture (filter toggled OFF) must let FS native URNs bind
/// — otherwise FsGenesis cannot wire the raw fs primitives into its
/// outer new-scope.  Simulates the toggle performed by
/// `casper::play_deploys_for_genesis` / `replay_compute_state`
/// (slices 5.33 / 5.35).
#[tokio::test]
async fn genesis_posture_accepts_fs_native_urn() {
    let fs_urn = format!("{FS_NATIVE_URN_PREFIX_VERSIONED}open");
    let reducer = reducer_with_urn_map(vec![fs_urn.clone()]).await;
    reducer
        .filter_fs_native_urns
        .store(false, std::sync::atomic::Ordering::Release);

    let par = new_par_binding_uris(vec![fs_urn]);
    reducer
        .eval(par, &Env::new(), rand())
        .await
        .expect("FS native URN under toggled-off filter must resolve");
}

/// Even one FS native URN in a mixed `New` scope rejects the whole
/// binding — the filter is any-URN-matches, not all-URNs-match.
/// Guards against a regression that only inspected the first uri or
/// used `all` instead of `any` semantics.
#[tokio::test]
async fn mixed_urns_reject_if_any_fs_native() {
    let non_fs_urn = "rho:test:foo".to_string();
    let fs_urn = format!("{FS_NATIVE_URN_PREFIX_VERSIONED}open");
    let reducer = reducer_with_urn_map(vec![non_fs_urn.clone(), fs_urn.clone()]).await;

    let par = new_par_binding_uris(vec![non_fs_urn.clone(), fs_urn.clone()]);
    let err = reducer
        .eval(par, &Env::new(), rand())
        .await
        .expect_err("mixed URNs with any fs native entry must reject");

    match err {
        InterpreterError::ReduceError(msg) => {
            assert!(
                msg.contains(&fs_urn),
                "rejection must name the fs native URN, not the \
                 non-fs sibling.  Got: {msg}"
            );
            assert!(
                !msg.contains(&non_fs_urn),
                "rejection must not name the non-fs URN (that one is \
                 resolvable).  Got: {msg}"
            );
        }
        other => panic!("expected ReduceError, got {other:?}"),
    }
}

/// A URN under the unversioned filter prefix but NOT the versioned
/// one (e.g., a hypothetical `rho:io:fs:native:2.0.0/open` or a
/// malformed `rho:io:fs:native:evil`) must still be rejected — the
/// reducer's filter uses the shorter unversioned prefix so a future
/// version bump keeps the gate in effect without a code change.
#[tokio::test]
async fn filter_catches_unknown_fs_native_prefix() {
    let unknown_fs_urn = format!("{FS_NATIVE_URN_PREFIX}evil");
    assert!(
        !unknown_fs_urn.starts_with(FS_NATIVE_URN_PREFIX_VERSIONED),
        "fixture URN must not match the versioned prefix — the whole \
         point is that the filter catches it via the shorter prefix"
    );
    let reducer = reducer_with_urn_map(vec![unknown_fs_urn.clone()]).await;
    let err = reducer
        .eval(
            new_par_binding_uris(vec![unknown_fs_urn.clone()]),
            &Env::new(),
            rand(),
        )
        .await
        .expect_err("fs native prefix match must reject regardless of version suffix");

    match err {
        InterpreterError::ReduceError(msg) => {
            assert!(
                msg.contains(&unknown_fs_urn),
                "rejection must name the offending URN; got: {msg}"
            );
        }
        other => panic!("expected ReduceError, got {other:?}"),
    }
}
