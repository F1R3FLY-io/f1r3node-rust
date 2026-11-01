use std::sync::Arc;

use tokio::sync::RwLock;

use super::checkpoint_wal::fixture_for_profile;
use super::durable_publication::install_checkpoint_image;
use super::durable_recovery::{checkpoint_image_for_profile, limits};
use super::durable_rspace_authority::{Profile, populate, publication_limits};
use super::support::trial_scratch;
use crate::rspace::hot_store::{DurableHotStoreBootstrapLimits, HotStoreInstances};
use crate::rspace::persistence::durable::{DurableAdmissionLimitsV1, recover_read_authority};
use crate::rspace::persistence::{GenerationLeaseKeyV1, LogPackDecodeLimits, SemanticProfile};

#[tokio::test]
async fn read_handle_pins_its_generation_across_a_successor_and_owner_teardown() {
    let scratch = trial_scratch("durable-read-handle-");
    let image = checkpoint_image_for_profile::<Profile>(fixture_for_profile(Profile::ID));
    install_checkpoint_image(scratch.path(), &image);
    let recover = || {
        recover_read_authority::<Profile>(
            scratch.path(),
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .expect("recover the durable view")
    };
    let view = recover();
    let registry = view.generation_leases();
    let old_state = view.state();
    let old_key = GenerationLeaseKeyV1::from_catalog(view.catalog());
    let old_roots = view.semantic_roots().expect("read the old semantic roots");
    let without_handle = registry.active_lease_count_for_test();
    let handle = view.read_handle().expect("take a read handle");
    let handle_leases = registry.active_lease_count_for_test() - without_handle;

    let hot = Arc::new(
        HotStoreInstances::create_from_durable(&view, DurableHotStoreBootstrapLimits::default())
            .expect("bootstrap the hot store from the view"),
    );
    let mut owner = view
        .into_rspace_authority(Arc::clone(&hot), Arc::new(RwLock::new(())))
        .unwrap_or_else(|failure| panic!("{:?}", failure.1));
    populate(&hot);
    let next = owner
        .checkpoint(publication_limits())
        .await
        .expect("publish and acknowledge a successor");
    assert_eq!(next.generation(), old_state.generation() + 1);
    assert_eq!(handle.semantic_roots().expect("read through the handle"), old_roots);
    let old_generation_pinned = registry
        .has_active(old_key)
        .expect("query the old generation's lease");

    drop(owner);
    let successor = recover();
    assert_eq!(successor.state(), next);
    assert_ne!(
        successor
            .semantic_roots()
            .expect("read the successor roots"),
        old_roots
    );
    drop(successor);
    let reclaimable = registry
        .retire_pack_range_if_unleased(old_key)
        .expect("decide whether the old generation is reclaimable");
    assert_eq!(
        (handle_leases, old_generation_pinned, reclaimable),
        (1, true, false),
        "the handle must add exactly one lease, keep its generation pinned after the owner moves \
         to the successor, and keep that generation from reclamation after owner teardown"
    );
    assert_eq!(
        handle
            .semantic_roots()
            .expect("read through the handle after owner teardown"),
        old_roots
    );
    assert_eq!(registry.active_lease_count_for_test(), 1);
    drop(handle);
    assert_eq!(registry.active_lease_count_for_test(), 0);
}
