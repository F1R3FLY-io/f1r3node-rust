use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use super::durable_rspace_authority::Profile;
use super::durable_rspace_checkpoint::{
    Contents, DurableStore, Observed, Operation, consume, expected, first_poll, hard_checkpoint,
    observe_root, observe_view, open_view, owner_slot, produce, published_root, run,
};
use super::owner_cross_base_checkpoint::{DIRECTORY_FAILPOINTS, directory_image};
use crate::rspace::errors::{HistoryError, RSpaceError, RootError};
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::history::instances::radix_history::RadixHistory;
use crate::rspace::persistence::durable::{
    DurableIoFailpointV1, arm_durable_io_failpoint, armed_durable_io_failpoint,
};
use crate::rspace::persistence::{
    CanonicalDagNode, DurableStateRootV1, GenerationLeaseKeyV1, GenerationLeaseRegistryV1,
    ObjectDigest,
};
use crate::rspace::rspace::RSpace;
use crate::rspace::rspace_interface::ISpace;
use crate::rspace::state::rspace_state_authority::{DurableOwnerSlotV1, RSpaceStateAuthority};

type Space = RSpace<String, String, String, String>;
type Slot = Arc<DurableOwnerSlotV1<Profile>>;
type Outcome = (Result<(), RSpaceError>, Blake2b256Hash, Observed);
type ViewImage = (Blake2b256Hash, Observed, bool);

const INSTALLED_CHANNEL: &str = "i";
const INSTALLED_PATTERN: &str = "installed-pattern";
const INSTALLED_BODY: &str = "installed";
const ABSENT_DIGEST: [u8; 32] = [0xA5; 32];
const LEASE_COUNTER_OVERFLOW: &str =
    "durable generation lease was refused: generation lease counter overflow";
const RESET_WATCHDOG: &str =
    "a durable reset while another task holds the owner mutex did not complete within 30 s";

#[derive(Clone)]
struct Retained {
    root: Blake2b256Hash,
    contents: Contents,
}

fn first_journal() -> [Operation; 2] {
    [produce("a", "v0"), consume(&["b", "c"], &["v0", "v1"], "k0")]
}

fn second_journal() -> [Operation; 2] { [produce("c", "v1"), consume(&["a"], &["v0"], "k1")] }

fn third_journal() -> [Operation; 1] { [produce("b", "v1")] }

fn fourth_journal() -> [Operation; 1] { [produce("b", "v0")] }

fn hash_of(digest: ObjectDigest) -> Blake2b256Hash {
    Blake2b256Hash::from_bytes(digest.as_bytes().to_vec())
}

fn publish(view: &Space, contents: &mut Contents, journal: &[Operation]) -> Retained {
    run(view, contents, journal);
    Retained {
        root: hard_checkpoint(view).root,
        contents: contents.clone(),
    }
}

fn install_a_continuation(view: &Space) {
    assert_eq!(
        first_poll(view.install(
            vec![INSTALLED_CHANNEL.to_owned()],
            vec![INSTALLED_PATTERN.to_owned()],
            INSTALLED_BODY.to_owned(),
        )),
        Ok(None),
        "a continuation is installed on a channel without data"
    );
}

fn holds_the_installed_continuation(view: &Space) -> bool {
    first_poll(view.get_waiting_continuations(vec![INSTALLED_CHANNEL.to_owned()]))
        .iter()
        .any(|stored| stored.persist && stored.continuation.as_str() == INSTALLED_BODY)
}

fn generation_leases(slot: &Slot) -> (GenerationLeaseRegistryV1, GenerationLeaseKeyV1) {
    let owner = slot
        .owner()
        .try_lock()
        .expect("the owner mutex is free between two operations");
    (owner.generation_leases(), owner.generation_key())
}

fn view_image(view: &Space) -> ViewImage {
    (first_poll(view.get_root()), observe_view(view), holds_the_installed_continuation(view))
}

fn reached(target: &Retained) -> Outcome {
    (Ok(()), target.root.clone(), expected(&target.contents))
}

fn outcome_of(view: &Space, result: Result<(), RSpaceError>) -> Outcome {
    (result, first_poll(view.get_root()), observe_view(view))
}

fn is_unknown_root(result: &Result<(), RSpaceError>) -> bool {
    matches!(
        result,
        Err(RSpaceError::HistoryError(HistoryError::RootError(RootError::UnknownRootError(_))))
    )
}

fn reset_to(view: &Space, target: &Retained) {
    assert_eq!(
        outcome_of(view, first_poll(view.reset(&target.root))),
        reached(target),
        "after a reset to a retained root the view names that root and reads that state's contents"
    );
}

fn resets_and_a_clear(
    view: &Space,
    targets: &[&Retained],
    unknown: &Blake2b256Hash,
) -> Vec<Outcome> {
    let mut outcomes = Vec::with_capacity(targets.len() + 2);
    for target in targets {
        outcomes.push(outcome_of(view, first_poll(view.reset(&target.root))));
    }
    outcomes.push(outcome_of(view, first_poll(view.reset(unknown))));
    outcomes.push(outcome_of(view, first_poll(view.clear())));
    outcomes
}

fn the_view_holds_its_generation_leases_until_its_next_reset_or_its_drop(
    store: &DurableStore,
    published: &mut Contents,
    older: &Retained,
) -> [Retained; 2] {
    let (registry, opening_key) = generation_leases(&store.slot);
    let leases_before = registry.active_lease_count_for_test();
    let leaseholder = open_view(&store.authority);
    assert_eq!(
        registry.active_lease_count_for_test(),
        leases_before + 2,
        "a view holds one generation lease for its repository's read handle and one for its \
         store's"
    );
    reset_to(&leaseholder, older);
    assert_eq!(
        (registry.active_lease_count_for_test(), generation_leases(&store.slot).1),
        (leases_before + 2, opening_key),
        "a reset kept a lease of the read handles it replaced or moved the store's generation"
    );

    let third = publish(&store.view, published, &third_journal());
    let later_key = generation_leases(&store.slot).1;
    assert_ne!(later_key, opening_key, "a publication moves the store to another generation");
    assert_eq!(
        registry.has_active(opening_key),
        Ok(true),
        "the view gave up its read handles' generation lease before its next reset"
    );
    reset_to(&leaseholder, &third);
    assert_eq!(
        (registry.has_active(opening_key), registry.has_active(later_key)),
        (Ok(false), Ok(true)),
        "the next reset moves the view's leases to the generation that is current when it runs"
    );

    let fourth = publish(&store.view, published, &fourth_journal());
    let last_key = generation_leases(&store.slot).1;
    assert_ne!(last_key, later_key, "a publication moves the store to another generation");
    assert_eq!(
        registry.has_active(later_key),
        Ok(true),
        "the view gave up its read handles' generation lease before it was dropped"
    );
    drop(leaseholder);
    assert_eq!(
        (registry.has_active(later_key), registry.active_lease_count_for_test()),
        (Ok(false), leases_before),
        "a dropped view kept a generation lease of its read handles"
    );
    [third, fourth]
}

async fn reset_while_another_task_holds_the_owner_mutex(
    authority: Arc<RSpaceStateAuthority>,
    target: Retained,
) {
    let view = open_view(&authority);
    let slot = owner_slot(&authority);
    let (held_sender, held_receiver) = tokio::sync::oneshot::channel::<()>();
    let (release_sender, release_receiver) = tokio::sync::oneshot::channel::<()>();
    let holder = tokio::task::spawn_local({
        let slot = Arc::clone(&slot);
        async move {
            let owner = slot.owner().lock().await;
            held_sender
                .send(())
                .expect("the test awaits the held owner mutex");
            release_receiver
                .await
                .expect("the test releases the held owner mutex");
            drop(owner);
        }
    });
    held_receiver
        .await
        .expect("another task takes the owner mutex");
    assert!(
        slot.owner().try_lock().is_err(),
        "another task holds the owner mutex before the reset"
    );
    assert_eq!(
        view.reset(&target.root).await,
        Ok(()),
        "a reset to a retained root is refused while another task holds the owner mutex"
    );
    assert!(
        slot.owner().try_lock().is_err(),
        "the owner mutex was released before the reset returned"
    );
    assert_eq!(
        (view.get_root().await, observe_view(&view)),
        (target.root.clone(), expected(&target.contents)),
        "a reset under a held owner mutex reads other contents than the state of its target root"
    );
    release_sender
        .send(())
        .expect("the holder awaits its release");
    holder.await.expect("the holder completes");
}

fn finish_under_the_reset_watchdog(trial: impl FnOnce() + Send + 'static) {
    let (finished_sender, finished_receiver) = mpsc::channel();
    let runtime_thread = std::thread::spawn(move || {
        trial();
        finished_sender
            .send(())
            .expect("the test thread awaits the reset");
    });
    match finished_receiver.recv_timeout(Duration::from_secs(30)) {
        Ok(()) => runtime_thread.join().expect("the runtime thread finishes"),
        Err(RecvTimeoutError::Timeout) => panic!("{RESET_WATCHDOG}"),
        Err(RecvTimeoutError::Disconnected) => match runtime_thread.join() {
            Err(panic) => std::panic::resume_unwind(panic),
            Ok(()) => panic!("the runtime thread ended without reporting the reset"),
        },
    }
}

#[test]
fn a_reset_to_each_retained_root_reads_that_state_and_keeps_its_generation_lease() {
    let store = DurableStore::open("durable-rspace-reset-retained-");
    let genesis = Retained {
        root: published_root(&store.slot),
        contents: Contents::default(),
    };
    let mut published = Contents::default();
    let first = publish(&store.view, &mut published, &first_journal());
    let second = publish(&store.view, &mut published, &second_journal());
    let [third, fourth] = the_view_holds_its_generation_leases_until_its_next_reset_or_its_drop(
        &store,
        &mut published,
        &first,
    );

    let follower = open_view(&store.authority);
    install_a_continuation(&follower);
    let mut abandoned = fourth.contents.clone();
    run(&follower, &mut abandoned, &[produce("a", "v1")]);
    assert_eq!(observe_view(&follower), expected(&abandoned));
    for target in [&first, &second, &genesis, &third, &fourth, &fourth, &first] {
        reset_to(&follower, target);
        assert!(
            holds_the_installed_continuation(&follower),
            "a reset lost the view's installed continuation"
        );
        assert_eq!(
            observe_root(&store.slot, &target.root),
            expected(&target.contents),
            "the store retains other contents under a root than its checkpoint published"
        );
    }
    assert!(
        first_poll(follower.take_event_log()).is_empty(),
        "a reset kept the events of the operations it abandoned"
    );
    assert_eq!(published_root(&store.slot), fourth.root, "a reset moved the store's current state");
    reset_to(&store.view, &second);
    reset_to(&store.view, &fourth);

    let mut continued = first.contents.clone();
    run(&follower, &mut continued, &second_journal());
    assert_eq!(
        hard_checkpoint(&follower).root,
        second.root,
        "a view that was reset to a retained root did not continue from that state"
    );
    assert_eq!(observe_view(&follower), expected(&second.contents));
    reset_to(&follower, &fourth);
    let mut advanced = fourth.contents.clone();
    run(&follower, &mut advanced, &[produce("c", "v0")]);
    let advanced_root = hard_checkpoint(&follower).root;
    assert_eq!(
        (observe_root(&store.slot, &advanced_root), published_root(&store.slot)),
        (expected(&advanced), advanced_root),
        "a view that was reset did not publish the successor of the state of its target root"
    );

    let authority = Arc::clone(&store.authority);
    finish_under_the_reset_watchdog(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("build a current-thread runtime");
        tokio::task::LocalSet::new()
            .block_on(&runtime, reset_while_another_task_holds_the_owner_mutex(authority, third));
    });
}

#[test]
fn a_reset_that_does_not_succeed_changes_nothing() {
    let store = DurableStore::open("durable-rspace-reset-refused-");
    install_a_continuation(&store.view);
    let mut published = Contents::default();
    let first = publish(&store.view, &mut published, &first_journal());
    let behind = open_view(&store.authority);
    let second = publish(&store.view, &mut published, &second_journal());
    let ahead_journal = [produce("b", "v1")];
    let behind_journal = [produce("c", "v0"), consume(&["a", "b"], &["v1", "v1"], "k1")];
    let mut ahead_contents = second.contents.clone();
    let mut behind_contents = first.contents.clone();
    run(&store.view, &mut ahead_contents, &ahead_journal);
    run(&behind, &mut behind_contents, &behind_journal);
    let views = [&store.view, &behind];
    let images = [
        (second.root.clone(), expected(&ahead_contents), true),
        (first.root.clone(), expected(&behind_contents), false),
    ];
    let files = directory_image(store.path());
    let unchanged = |case: &str| {
        assert_eq!(
            views.map(view_image),
            images,
            "{case} changed a view's repository, its reads or its installed continuation"
        );
        assert_eq!(directory_image(store.path()), files, "{case} changed a file of the store");
        assert_eq!(
            published_root(&store.slot),
            second.root,
            "{case} moved the store's current state"
        );
    };
    unchanged("the preparation of the views");

    let foreign_root = {
        let foreign = DurableStore::open("durable-rspace-reset-foreign-");
        let mut foreign_contents = Contents::default();
        publish(&foreign.view, &mut foreign_contents, &[produce("c", "v1"); 3]).root
    };
    let retained_data_bag = store
        .slot
        .published_core()
        .read_handle(Some(
            DurableStateRootV1::try_from(&first.root)
                .expect("a checkpoint root is a durable state root"),
        ))
        .expect("the store retains the state of a returned root")
        .0
        .roots()
        .data_bag
        .digest();
    let unknown_roots = [
        ("a digest absent from the store", Blake2b256Hash::from_bytes(ABSENT_DIGEST.to_vec())),
        ("the legacy empty root", RadixHistory::empty_root_node_hash()),
        ("another store's published state root", foreign_root),
        ("the shared empty trie node", hash_of(CanonicalDagNode::empty().object_ref().digest())),
        ("a retained state's data-bag node", hash_of(retained_data_bag)),
    ];
    let malformed_roots =
        [Vec::new(), vec![0x5A; 31], vec![0x5A; 33]].map(Blake2b256Hash::from_bytes);
    for (label, root) in &unknown_roots {
        for view in views {
            let refusal = first_poll(view.reset(root));
            assert!(
                is_unknown_root(&refusal),
                "{label} must be refused with UnknownRootError: {refusal:?}"
            );
        }
    }
    for root in &malformed_roots {
        for view in views {
            let refusal = first_poll(view.reset(root));
            assert!(
                matches!(
                    refusal,
                    Err(RSpaceError::HistoryError(HistoryError::RootError(
                        RootError::InvalidRootHash(_)
                    )))
                ),
                "a hash of {} bytes must be refused as an invalid root: {refusal:?}",
                root.0.len()
            );
        }
    }
    unchanged("a refused root");

    let registry = generation_leases(&store.slot).0;
    let leases = registry.active_lease_count_for_test();
    for (view, target) in [(&store.view, &first), (&behind, &second)] {
        let counter = registry.replace_lease_counter_for_test(u64::MAX - 1);
        let refusal = first_poll(view.reset(&target.root));
        let granted_last = registry.replace_lease_counter_for_test(counter);
        assert_eq!(
            (refusal, granted_last),
            (
                Err(RSpaceError::HistoryError(HistoryError::RootError(RootError::KvStoreError(
                    LEASE_COUNTER_OVERFLOW.to_owned()
                )))),
                u64::MAX,
            ),
            "the repository's read handle takes the last lease, so the target resolved, and the \
             store's read handle is refused by the lease counter"
        );
    }
    assert_eq!(
        registry.active_lease_count_for_test(),
        leases,
        "a reset whose store was not built kept a generation lease"
    );
    unchanged("a reset whose store was not built");

    assert_eq!(
        views.map(|view| first_poll(view.take_event_log()).len()),
        [ahead_journal.len(), behind_journal.len()],
        "a reset that did not succeed changed a view's event log"
    );
    let behind_root = hard_checkpoint(&behind).root;
    let ahead_root = hard_checkpoint(&store.view).root;
    assert_eq!(
        (observe_root(&store.slot, &behind_root), observe_root(&store.slot, &ahead_root)),
        (expected(&behind_contents), expected(&ahead_contents)),
        "a view did not publish its journal from its base after resets that did not succeed"
    );
}

#[test]
fn a_reset_and_a_clear_write_and_sync_nothing_and_a_clear_returns_the_store_genesis() {
    let store = DurableStore::open("durable-rspace-reset-no-write-");
    let genesis = Retained {
        root: Blake2b256Hash::from(DurableStateRootV1::store_genesis::<Profile>()),
        contents: Contents::default(),
    };
    assert_eq!(published_root(&store.slot), genesis.root, "a new durable store is at its genesis");
    install_a_continuation(&store.view);
    let mut published = Contents::default();
    let before_publication = directory_image(store.path());
    let first = publish(&store.view, &mut published, &first_journal());
    assert_ne!(
        directory_image(store.path()),
        before_publication,
        "a publication changes the directory image, so the image shows a write"
    );
    let second = publish(&store.view, &mut published, &second_journal());
    let publisher_image = (second.root.clone(), expected(&second.contents), true);

    let follower = open_view(&store.authority);
    install_a_continuation(&follower);
    let mut abandoned = second.contents.clone();
    run(&follower, &mut abandoned, &third_journal());
    let targets = [&first, &second, &genesis, &second, &first];
    let unknown = Blake2b256Hash::from_bytes(ABSENT_DIGEST.to_vec());
    let files = directory_image(store.path());
    let baseline = resets_and_a_clear(&follower, &targets, &unknown);
    assert!(
        is_unknown_root(&baseline[targets.len()].0),
        "the resets include a refused root: {baseline:?}"
    );
    let mut modelled = Vec::with_capacity(targets.len() + 2);
    modelled.extend(targets.map(reached));
    modelled.push((
        baseline[targets.len()].0.clone(),
        first.root.clone(),
        expected(&first.contents),
    ));
    modelled.push(reached(&genesis));
    assert_eq!(
        baseline, modelled,
        "each reset reads the state of its target, a refused reset reads the state before it, and \
         a clear reads the empty state"
    );

    let mut observations = Vec::with_capacity(DIRECTORY_FAILPOINTS.len());
    for failpoint in DIRECTORY_FAILPOINTS {
        observations.push(std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    arm_durable_io_failpoint(failpoint);
                    let outcomes = resets_and_a_clear(&follower, &targets, &unknown);
                    (failpoint, outcomes, armed_durable_io_failpoint())
                })
                .join()
                .expect("the failpoint thread completes")
        }));
    }
    for (failpoint, outcomes, still_armed) in &observations {
        assert_eq!(
            outcomes, &baseline,
            "a reset or a clear changed its outcome with {failpoint:?} armed"
        );
        assert_eq!(
            *still_armed,
            Some(*failpoint),
            "a reset or a clear reached the {failpoint:?} failpoint and cleared it"
        );
    }

    reset_to(&follower, &second);
    let mut pending = second.contents.clone();
    run(&follower, &mut pending, &third_journal());
    assert_ne!(expected(&pending), expected(&genesis.contents));
    assert_eq!(
        outcome_of(&follower, first_poll(follower.clear())),
        reached(&genesis),
        "after a clear the view names the store genesis and reads no datum, continuation or join"
    );
    assert!(
        holds_the_installed_continuation(&follower),
        "a clear lost the view's installed continuation"
    );
    let unpublished = hard_checkpoint(&follower);
    assert_eq!(
        (unpublished.root, unpublished.log.len()),
        (genesis.root.clone(), 0),
        "a checkpoint with no operation after a clear returns the store-genesis root"
    );
    assert_eq!(
        (
            directory_image(store.path()) == files,
            published_root(&store.slot),
            view_image(&store.view),
        ),
        (true, second.root.clone(), publisher_image),
        "resets, clears and a checkpoint with no operation leave the store directory, the store's \
         current state and the other view as they were"
    );

    let mut restarted = Contents::default();
    run(&follower, &mut restarted, &first_journal());
    assert_eq!(
        hard_checkpoint(&follower).root,
        first.root,
        "a view that was cleared did not continue from the store genesis"
    );

    let fresh = DurableStore::open("durable-rspace-reset-genesis-current-");
    let mut never_published = Contents::default();
    run(&fresh.view, &mut never_published, &first_journal());
    let fresh_files = directory_image(fresh.path());
    assert_eq!(
        outcome_of(&fresh.view, first_poll(fresh.view.clear())),
        reached(&genesis),
        "a clear of a view on the store genesis reads the empty state"
    );
    assert_eq!(
        (
            hard_checkpoint(&fresh.view).root,
            published_root(&fresh.slot),
            directory_image(fresh.path()) == fresh_files,
        ),
        (genesis.root.clone(), genesis.root.clone(), true),
        "a checkpoint with no operation after a clear on the store genesis returns the genesis \
         root and writes nothing"
    );

    run(&store.view, &mut published, &third_journal());
    let (refusal, cleared) = std::thread::scope(|scope| {
        scope
            .spawn(|| {
                arm_durable_io_failpoint(DurableIoFailpointV1::PhysicalWrite);
                let refusal = first_poll(store.view.create_checkpoint())
                    .err()
                    .map(|error| format!("{error:?}"));
                (refusal, armed_durable_io_failpoint().is_none())
            })
            .join()
            .expect("the liveness control thread completes")
    });
    assert!(
        cleared &&
            refusal.as_deref().is_some_and(
                |text| text.contains("injected durable I/O failure at PhysicalWrite")
            ),
        "a publishing hard checkpoint in a failpoint thread trips and clears PhysicalWrite: \
         {refusal:?} {cleared}"
    );
}
