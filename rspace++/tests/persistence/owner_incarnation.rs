use std::sync::Arc;

use tokio::sync::RwLock;

use super::checkpoint_wal::fixture_for_profile;
use super::durable_publication::install_checkpoint_image;
use super::durable_recovery::{checkpoint_image_for_profile, limits};
use super::durable_rspace_authority::{
    Profile, Store, poll_once, populate, publication_limits, store_files,
};
use super::support::{PersistenceTrialDir, trial_scratch};
use crate::rspace::bag::BagDelta;
use crate::rspace::hot_store::{DurableHotStoreBootstrapLimits, HotStoreInstances};
use crate::rspace::persistence::durable::{
    DurableAdmissionLimitsV1, DurableAuthorityViewV1, DurableDirectoryV1, DurableRSpaceAuthorityV1,
    DurableRSpaceTransactionError, JournalPublicationPhaseV1, OwnerIncarnationAllocatorV1,
    OwnerIncarnationExhaustedV1, ViewAttachmentRefusalV1, recover_read_authority,
};
use crate::rspace::persistence::{GenerationLeaseKeyV1, LogPackDecodeLimits, SemanticProfile};

fn installed_store(prefix: &str) -> PersistenceTrialDir {
    let scratch = trial_scratch(prefix);
    let image = checkpoint_image_for_profile::<Profile>(fixture_for_profile(Profile::ID));
    install_checkpoint_image(scratch.path(), &image);
    scratch
}

fn recover(path: &std::path::Path) -> DurableAuthorityViewV1<Profile> {
    recover_read_authority::<Profile>(
        path,
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
        DurableAdmissionLimitsV1::UNBOUNDED,
    )
    .expect("recover the durable view")
}

fn hot_store(view: &DurableAuthorityViewV1<Profile>) -> Store {
    Arc::new(
        HotStoreInstances::create_from_durable(view, DurableHotStoreBootstrapLimits::default())
            .expect("bootstrap the hot store from the durable view"),
    )
}

fn open_owner(
    view: DurableAuthorityViewV1<Profile>,
) -> (Store, Arc<RwLock<()>>, DurableRSpaceAuthorityV1<Profile>) {
    let hot = hot_store(&view);
    let gate = Arc::new(RwLock::new(()));
    let owner = view
        .into_rspace_authority(&hot)
        .unwrap_or_else(|failure| panic!("open the durable owner: {:?}", failure.1));
    (hot, gate, owner)
}

pub(super) fn journal_bags(hot: &Store) -> [Option<BagDelta>; 3] {
    let pending = hot.pending_deltas().expect("read the hot store's journals");
    [pending.data, pending.continuations, pending.joins]
}

fn writer_is_free(path: &std::path::Path) -> bool {
    matches!(
        DurableDirectoryV1::open(path)
            .expect("open the durable directory")
            .try_acquire_writer(),
        Ok(_)
    )
}

#[test]
fn close_revokes_old_attachments_without_mutation() {
    let scratch = installed_store("owner-incarnation-close-");
    let (hot, gate, mut owner) = open_owner(recover(scratch.path()));
    let attachment = owner.attach(&hot).expect("attach to the live owner");
    populate(&hot);
    let old = owner.state(&hot).expect("read the live owner's state");
    let journals = journal_bags(&hot);
    let files = store_files(scratch.path());
    let live = (owner.validate_attachment(attachment), writer_is_free(scratch.path()));
    owner.close().expect("close the quiescent owner");
    let refused = (
        owner.validate_attachment(attachment),
        matches!(owner.attach(&hot), Err(DurableRSpaceTransactionError::Closed)),
        matches!(
            poll_once(
                owner.checkpoint_view(
                    Arc::clone(&gate)
                        .try_write_owned()
                        .expect("take the free view gate"),
                    attachment,
                    &gate,
                    &hot,
                    publication_limits(),
                )
            ),
            Err(DurableRSpaceTransactionError::Closed)
        ),
        matches!(owner.reconcile(publication_limits()), Err(DurableRSpaceTransactionError::Closed)),
        matches!(owner.state(&hot), Err(DurableRSpaceTransactionError::Closed)),
        owner.phase(&hot),
        matches!(owner.close(), Err(DurableRSpaceTransactionError::Closed)),
    );
    let gate_free = matches!(gate.try_write(), Ok(_));
    let unchanged = (
        gate_free,
        journal_bags(&hot) == journals,
        store_files(scratch.path()) == files,
        writer_is_free(scratch.path()),
        recover(scratch.path()).state() == old,
    );
    assert_eq!(
        (live, refused, unchanged),
        (
            (Ok(()), false),
            (
                Err(ViewAttachmentRefusalV1::OwnerClosed),
                true,
                true,
                true,
                true,
                JournalPublicationPhaseV1::Closed,
                true,
            ),
            (true, true, true, true, true),
        ),
        "a closed owner must refuse its old attachment and every owner operation, hold neither \
         its view gate nor its writer lease, and leave the journals, the store files and the \
         recovered state unchanged"
    );
}

#[test]
fn reopened_owners_refuse_every_earlier_incarnation() {
    const CYCLES: usize = 3;
    let scratch = installed_store("owner-incarnation-reopen-");
    let mut owners = Vec::with_capacity(CYCLES + 1);
    let mut attachments = Vec::with_capacity(CYCLES + 1);
    let mut observed = Vec::with_capacity(CYCLES);
    let mut expected = Vec::with_capacity(CYCLES);
    let (first_hot, _, first) = open_owner(recover(scratch.path()));
    attachments.push(first.attach(&first_hot).expect("attach to the first owner"));
    owners.push(first);
    for _ in 0..CYCLES {
        owners
            .last_mut()
            .expect("an owner is open")
            .close()
            .expect("close the quiescent owner");
        let (hot, _, reopened) = open_owner(recover(scratch.path()));
        let current = reopened.attach(&hot).expect("attach to the reopened owner");
        observed.push(
            attachments
                .iter()
                .chain([&current])
                .map(|attachment| reopened.validate_attachment(*attachment))
                .collect::<Vec<_>>(),
        );
        expected.push(
            attachments
                .iter()
                .map(|earlier| {
                    Err(ViewAttachmentRefusalV1::StaleIncarnation {
                        attached: earlier.incarnation(),
                        current: current.incarnation(),
                    })
                })
                .chain([Ok(())])
                .collect::<Vec<_>>(),
        );
        attachments.push(current);
        owners.push(reopened);
    }
    let increasing = attachments
        .windows(2)
        .all(|pair| pair[0].incarnation() < pair[1].incarnation());
    let same_base = attachments
        .iter()
        .all(|attachment| attachment.base() == attachments[0].base());
    assert_eq!(
        (observed, increasing, same_base),
        (expected, true, true),
        "every reopened owner must accept only its own attachment and refuse each earlier one as \
         a stale incarnation, although every attachment names the same base"
    );
}

#[test]
fn pinned_reads_survive_owner_close_until_the_last_reader_releases() {
    let scratch = installed_store("owner-incarnation-pinned-");
    let view = recover(scratch.path());
    let registry = view.generation_leases();
    let key = GenerationLeaseKeyV1::from_catalog(view.catalog());
    let roots = view.semantic_roots().expect("read the semantic roots");
    let handle = view.read_handle().expect("take a read handle");
    let (_hot, _gate, mut owner) = open_owner(view);
    let live_leases = registry.active_lease_count_for_test();
    owner.close().expect("close the quiescent owner");
    let closed_leases = registry.active_lease_count_for_test();
    let writer_free = writer_is_free(scratch.path());
    let reads = handle
        .semantic_roots()
        .expect("read through the handle after close") ==
        roots;
    let pinned = registry
        .has_active(key)
        .expect("query the pinned generation");
    let retired_while_read = registry
        .retire_pack_range_if_unleased(key)
        .expect("decide reclamation while the handle reads");
    drop(handle);
    let released = registry.active_lease_count_for_test();
    let retired_after_release = registry
        .retire_pack_range_if_unleased(key)
        .expect("decide reclamation after the last reader releases");
    assert_eq!(
        (
            live_leases,
            closed_leases,
            writer_free,
            reads,
            pinned,
            retired_while_read,
            released,
            retired_after_release,
        ),
        (2, 1, true, true, true, false, 0, true),
        "close must release the owner's writer lease and generation pin while the read handle \
         keeps reading and pinning its generation, and the generation becomes reclaimable only \
         once that last reader releases"
    );
}

#[test]
fn concurrent_incarnation_allocation_is_unique_monotonic_and_never_wraps() {
    const THREADS: u64 = 8;
    const PER_THREAD: u64 = 1024;
    const CAPACITY: u64 = THREADS * PER_THREAD;
    let allocator = &OwnerIncarnationAllocatorV1::starting_at(u64::MAX - CAPACITY);
    let granted = std::thread::scope(|scope| {
        let workers = (0..THREADS)
            .map(move |_| {
                scope.spawn(move || {
                    let mut incarnations = Vec::with_capacity(CAPACITY as usize);
                    loop {
                        match allocator.allocate() {
                            Ok(incarnation) => incarnations.push(incarnation.get()),
                            Err(OwnerIncarnationExhaustedV1) => break incarnations,
                        }
                    }
                })
            })
            .collect::<Vec<_>>();
        workers
            .into_iter()
            .map(|worker| worker.join().expect("join an allocating thread"))
            .collect::<Vec<_>>()
    });
    let monotonic = granted
        .iter()
        .all(|thread| thread.windows(2).all(|pair| pair[0] < pair[1]));
    let mut all = granted.concat();
    all.sort_unstable();
    let exact = all == (u64::MAX - CAPACITY..u64::MAX).collect::<Vec<_>>();
    let still_refused = (0..THREADS)
        .map(|_| allocator.allocate())
        .all(|refusal| matches!(refusal, Err(OwnerIncarnationExhaustedV1)));
    let fresh = OwnerIncarnationAllocatorV1::new();
    let first = (
        fresh.allocate().map(|incarnation| incarnation.get()),
        fresh.allocate().map(|incarnation| incarnation.get()),
    );
    assert_eq!(
        (monotonic, exact, still_refused, first),
        (true, true, true, (Ok(0), Ok(1))),
        "concurrent allocation must grant every remaining incarnation exactly once, in increasing \
         order on each thread, then refuse without wrapping, and a fresh allocator starts at zero"
    );
}

#[test]
fn an_exhausted_allocator_refuses_the_owner_and_returns_its_view() {
    let scratch = installed_store("owner-incarnation-exhausted-");
    let view = recover(scratch.path());
    let state = view.state();
    let hot = hot_store(&view);
    let exhausted = OwnerIncarnationAllocatorV1::starting_at(u64::MAX);
    let (view, error) = match DurableRSpaceAuthorityV1::from_view_with(view, &hot, &exhausted) {
        Ok(_) => panic!("an exhausted incarnation allocator opened an owner"),
        Err(failure) => *failure,
    };
    let refused = matches!(
        error,
        DurableRSpaceTransactionError::IncarnationExhausted(OwnerIncarnationExhaustedV1)
    );
    let returned = view.state();
    let reopened = view
        .into_rspace_authority(&hot)
        .unwrap_or_else(|failure| panic!("open the durable owner: {:?}", failure.1))
        .state(&hot)
        .expect("read the reopened owner's state");
    assert_eq!(
        (refused, returned, reopened),
        (true, state, state),
        "an exhausted allocator must refuse the owner with a typed error and return the view, \
         which still opens an owner over the same state"
    );
}
