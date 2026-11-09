// The concrete Rholang hosts implement the Reified RSpaces substrate
// (`SpaceTxnHost`) over `Par, BindPattern, ListParWithRandom,
// TaggedContinuation`: the bound the runtime constructors carry from P4.
// A handle over each host begins and commits an empty transaction.

use std::sync::Arc;

use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use models::rust::utils::new_gstring_par;
use rholang::rust::interpreter::matcher::r#match::Matcher;
// Retired by decision 90 (C6, step D): a rigged replay space no longer implements `SpaceTxnHost`.
// The text before:
// use rspace_plus_plus::rspace::reporting_rspace::ReportingRspace;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::space_store_handle::{SpaceMode, SpaceStoreHandle, SpaceTxnHost};

type ParHandle = SpaceStoreHandle<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

fn assert_host<H>(_host: &H)
where H: SpaceTxnHost<Par, BindPattern, ListParWithRandom, TaggedContinuation> + 'static {
}

async fn begin_and_commit_empty(handle: ParHandle, mode: SpaceMode) {
    assert_eq!(handle.mode(), mode);
    let lock_keys = vec![new_gstring_par(
        "reified-space-txn-host-bounds".to_string(),
        Vec::new(),
        false,
    )];
    let mut txn = handle
        .begin(&lock_keys)
        .await
        .expect("begin an empty transaction");
    assert_eq!(txn.mode(), mode);
    let receipt = txn.commit().await.expect("commit the empty transaction");
    assert!(receipt.ops.is_empty());
}

#[tokio::test]
async fn reified_space_txn_host_bounds_hold_for_par_hosts() {
    let mut stores = InMemoryStoreManager::new();
    // Changed by decision 90 (C6, step D): the play space is the only host left of the pair. The
    // text before:
    // let (play, replay) =
    let (play, _replay) =
        RSpace::<Par, BindPattern, ListParWithRandom, TaggedContinuation>::create_with_replay(
            stores
                .r_space_stores()
                .await
                .expect("create the play and replay stores"),
            Arc::new(Box::new(Matcher)),
        )
        .expect("create the play and replay spaces");
    // Retired by decision 90 (C6, step D): a rigged replay space no longer implements
    // `SpaceTxnHost`, so the reporting half of this test retires with it. The text before:
    // ```
    // let mut reporting_stores = InMemoryStoreManager::new();
    // let reporting =
    //     ReportingRspace::<Par, BindPattern, ListParWithRandom, TaggedContinuation>::create(
    //         reporting_stores
    //             .r_space_stores()
    //             .await
    //             .expect("create the reporting stores"),
    //         Arc::new(Box::new(Matcher)),
    //     )
    //     .expect("create the reporting space");
    // ```

    assert_host(&play);
    // Retired by decision 90 (C6, step D): the rigged spaces no longer implement `SpaceTxnHost`.
    // The text before:
    // assert_host(&replay);
    // assert_host(&reporting);
    begin_and_commit_empty(SpaceStoreHandle::new(play), SpaceMode::Play).await;
    // Retired by decision 90 (C6, step D): the modes retire with the rigged spaces. The text
    // before:
    // begin_and_commit_empty(SpaceStoreHandle::new(replay), SpaceMode::Replay).await;
    // begin_and_commit_empty(SpaceStoreHandle::new(reporting), SpaceMode::Reporting).await;
}
