// The Reified RSpaces substrate over real `Par` channels. `Par` derives
// `Ord` over every field, but its `PartialEq`, `Hash` and serde ignore
// `locally_free`. The substrate keys every view map and set by store
// identity (`stable_hash_provider::hash`), so `Par` values that differ only
// in `locally_free` are one channel for every substrate decision: reads,
// staging, the installed guard, lock coverage, commit and checkpoint.

use std::collections::BTreeSet;
use std::sync::Arc;

use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::{BindPattern, ListParWithRandom, Par, ParWithRandom, TaggedContinuation};
use models::rust::utils::new_gstring_par;
use proptest::prelude::*;
use proptest::test_runner::RngSeed;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::hash;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::space_store_handle::{
    bind_candidate, BoxedSpaceTxn, ConsumeOutcome, ProduceOutcome, SpaceConsume, SpaceProduce,
    SpaceStoreHandle, SpaceTxnError,
};
use rspace_plus_plus::rspace::trace::event::{Event, IOEvent};

type ParSpace = RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;
type ParHandle = SpaceStoreHandle<Par, BindPattern, ListParWithRandom, TaggedContinuation>;
type ParTxn = BoxedSpaceTxn<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

const SEED: u64 = 0x0E0_5EED_2026_1004;
const NAMES: [&str; 3] = ["p0", "p1", "p2"];
/// `locally_free` variants; variant 0 is the empty set.
const VARIANTS: usize = 4;

fn locally_free(variant: usize) -> Vec<u8> {
    match variant % VARIANTS {
        0 => Vec::new(),
        1 => vec![1],
        2 => vec![0, 2],
        _ => vec![3],
    }
}

fn channel(name: &str, variant: usize) -> Par {
    new_gstring_par(name.to_string(), locally_free(variant), false)
}

fn datum(value: &str) -> ListParWithRandom {
    ListParWithRandom {
        pars: vec![new_gstring_par(value.to_string(), Vec::new(), false)],
        random_state: Vec::new(),
        ..ListParWithRandom::default()
    }
}

fn continuation(tag: &str) -> TaggedContinuation {
    TaggedContinuation {
        tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
            body: Some(new_gstring_par(tag.to_string(), Vec::new(), false)),
            random_state: Vec::new(),
        })),
        ..TaggedContinuation::default()
    }
}

async fn par_space() -> ParSpace {
    let mut stores = InMemoryStoreManager::new();
    let store = stores
        .r_space_stores()
        .await
        .expect("create the in-memory RSpace stores");
    RSpace::create(store, Arc::new(Box::new(Matcher))).expect("create the RSpace")
}

fn lock_keys() -> Vec<Par> {
    let mut keys: Vec<Par> = NAMES.iter().map(|name| channel(name, 0)).collect();
    keys.push(channel("sys", 0));
    keys
}

fn store_op(
    channel: Par,
    value: &str,
) -> SpaceProduce<Par, BindPattern, ListParWithRandom, TaggedContinuation> {
    SpaceProduce {
        channel,
        data: datum(value),
        persist: false,
        outcome: ProduceOutcome::Store,
        before_observe: None,
        own: false,
        // Added by decision 123 (plan v4, V3-9): no row search.
        search: None,
    }
}

fn park_op(
    channels: Vec<Par>,
    tag: &str,
) -> SpaceConsume<Par, BindPattern, ListParWithRandom, TaggedContinuation> {
    SpaceConsume {
        patterns: vec![BindPattern::default(); channels.len()],
        channels,
        continuation: continuation(tag),
        persist: false,
        peeks: BTreeSet::new(),
        outcome: ConsumeOutcome::Park,
        before_observe: None,
        own: false,
        // Added by decision 123 (plan v4, V3-9): no row search.
        search: None,
    }
}

// One operation on `Par` channels; each channel reference names a
// `locally_free` variant.
#[derive(Clone, Debug)]
enum ParOp {
    Store {
        name: usize,
        variant: usize,
        value: usize,
    },
    Park {
        name: usize,
        variant: usize,
    },
    ParkJoin {
        first: usize,
        second: usize,
        variants: (usize, usize),
    },
    Fire {
        name: usize,
        variant: usize,
        bind_variant: usize,
    },
}

fn par_op() -> impl Strategy<Value = ParOp> {
    prop_oneof![
        (0usize..3, 0..VARIANTS, 0usize..2).prop_map(|(name, variant, value)| ParOp::Store {
            name,
            variant,
            value
        }),
        (0usize..3, 0..VARIANTS).prop_map(|(name, variant)| ParOp::Park { name, variant }),
        (0usize..3, 1usize..3, (0..VARIANTS, 0..VARIANTS)).prop_map(|(first, offset, variants)| {
            ParOp::ParkJoin {
                first,
                second: (first + offset) % 3,
                variants,
            }
        }),
        (0usize..3, 0..VARIANTS, 0..VARIANTS).prop_map(|(name, variant, bind_variant)| {
            ParOp::Fire {
                name,
                variant,
                bind_variant,
            }
        }),
    ]
}

// Stages `op` with the variants `variant_of` gives; `None` when the
// operation is skipped (a fire on an empty channel). Returns the staged
// identity as bytes.
fn stage_par(
    txn: &mut ParTxn,
    op: &ParOp,
    variant_of: &dyn Fn(usize) -> usize,
) -> Result<Option<Vec<u8>>, SpaceTxnError> {
    let staged = match op {
        ParOp::Store {
            name,
            variant,
            value,
        } => txn.produce(store_op(
            channel(NAMES[*name], variant_of(*variant)),
            ["x", "y"][*value],
        ))?,
        ParOp::Park { name, variant } => txn.consume(park_op(
            vec![channel(NAMES[*name], variant_of(*variant))],
            &format!("k{}", name),
        ))?,
        ParOp::ParkJoin {
            first,
            second,
            variants,
        } => {
            let mut channels = vec![
                channel(NAMES[*first], variant_of(variants.0)),
                channel(NAMES[*second], variant_of(variants.1)),
            ];
            channels.sort_by_key(hash);
            txn.consume(park_op(
                channels,
                &format!("j{}{}", first.min(second), first.max(second)),
            ))?
        }
        ParOp::Fire {
            name,
            variant,
            bind_variant,
        } => {
            let bound = channel(NAMES[*name], variant_of(*bind_variant));
            let Some(record) = txn.data(&bound)?.first().cloned() else {
                return Ok(None);
            };
            let matched = record.a.clone();
            txn.consume(SpaceConsume {
                channels: vec![channel(NAMES[*name], variant_of(*variant))],
                patterns: vec![BindPattern::default()],
                continuation: continuation("fire"),
                persist: false,
                peeks: BTreeSet::new(),
                outcome: ConsumeOutcome::Fire {
                    data: vec![bind_candidate(bound, &record, matched, 0)],
                },
                before_observe: None,
                own: false,
                // Added by decision 123 (plan v4, V3-9): no row search.
                search: None,
            })?
        }
    };
    Ok(Some(
        bincode::serialize(&(format!("{:?}", staged.source), &staged.comm))
            .expect("serialize the staged identity"),
    ))
}

// The view of every name through the variant `variant`, as bytes.
fn par_view(txn: &mut ParTxn, variant: usize) -> Result<Vec<Vec<u8>>, SpaceTxnError> {
    let mut view = Vec::with_capacity(NAMES.len());
    for name in NAMES {
        let channel = channel(name, variant);
        let data = txn.data(&channel)?.to_vec();
        let continuations = txn.continuations(std::slice::from_ref(&channel))?.to_vec();
        let joins = txn.joins(&channel)?.to_vec();
        view.push(bincode::serialize(&(data, continuations, joins)).expect("serialize the view"));
    }
    Ok(view)
}

async fn par_store_view(space: &ParSpace, variant: usize) -> Vec<Vec<u8>> {
    let mut view = Vec::with_capacity(NAMES.len());
    for name in NAMES {
        let channel = channel(name, variant);
        let data = space.get_data(&channel).await;
        let continuations = space.get_waiting_continuations(vec![channel.clone()]).await;
        let joins = space.get_joins(channel).await;
        view.push(bincode::serialize(&(data, continuations, joins)).expect("serialize the view"));
    }
    view
}

fn event_bytes(log: &[Event]) -> Vec<Vec<u8>> {
    log.iter()
        .map(|event| match event {
            Event::IoEvent(io_event) => {
                let mut bytes = vec![0u8];
                bytes.extend(bincode::serialize(io_event).expect("serialize an IO event"));
                bytes
            }
            Event::Comm(comm) => {
                let mut bytes = vec![1u8];
                bytes.extend(bincode::serialize(comm).expect("serialize a COMM"));
                bytes
            }
        })
        .collect()
}

// H21 with `C = Par`: a run that names channels with arbitrary
// `locally_free` sets stages, reads, commits and checkpoints exactly like
// the run with every set empty; the staged view through any variant equals
// the committed store through any variant; and the installed guard holds
// through any variant.
async fn par_identity_ignores_locally_free(groups: Vec<Vec<ParOp>>) -> Result<(), TestCaseError> {
    let varied = par_space().await;
    let plain = par_space().await;
    for (space, variant) in [(&varied, 1usize), (&plain, 0usize)] {
        space
            .install(
                vec![channel("sys", variant)],
                vec![BindPattern::default()],
                continuation("system"),
            )
            .await
            .expect("install a system continuation");
    }
    let varied_handle: ParHandle = SpaceStoreHandle::new(varied.clone());
    let plain_handle: ParHandle = SpaceStoreHandle::new(plain.clone());
    let keys = lock_keys();
    let keep = |variant: usize| variant;
    let empty = |_: usize| 0usize;
    for group in &groups {
        let mut varied_txn = varied_handle.begin(&keys).await.expect("begin");
        let mut plain_txn = plain_handle.begin(&keys).await.expect("begin");
        for op in group {
            let varied_result = stage_par(&mut varied_txn, op, &keep);
            let plain_result = stage_par(&mut plain_txn, op, &empty);
            prop_assert_eq!(varied_result, plain_result);
            let plain_view = par_view(&mut plain_txn, 0);
            for variant in 0..VARIANTS {
                prop_assert_eq!(&par_view(&mut varied_txn, variant), &plain_view);
            }
            prop_assert_eq!(
                varied_txn.data(&channel("sys", 2)).err(),
                Some(SpaceTxnError::InstalledChannel)
            );
        }
        let mut staged_views = Vec::with_capacity(VARIANTS);
        for variant in 0..VARIANTS {
            staged_views.push(par_view(&mut varied_txn, variant));
        }
        let committed = varied_txn.commit().await.map(|receipt| receipt.ops.len());
        prop_assert_eq!(
            committed,
            plain_txn.commit().await.map(|receipt| receipt.ops.len())
        );
        let plain_store = par_store_view(&plain, 0).await;
        for variant in 0..VARIANTS {
            let varied_store = par_store_view(&varied, variant).await;
            prop_assert_eq!(&varied_store, &plain_store);
            for staged_view in &staged_views {
                prop_assert_eq!(staged_view, &Ok(varied_store.clone()));
            }
        }
    }
    match (
        varied.create_checkpoint().await,
        plain.create_checkpoint().await,
    ) {
        (Ok(varied_checkpoint), Ok(plain_checkpoint)) => {
            prop_assert_eq!(
                event_bytes(&varied_checkpoint.log),
                event_bytes(&plain_checkpoint.log)
            );
            prop_assert_eq!(varied_checkpoint.root, plain_checkpoint.root);
        }
        (varied, plain) => {
            return Err(TestCaseError::fail(format!(
                "checkpoints: {:?} / {:?}",
                varied.map(|checkpoint| checkpoint.root),
                plain.map(|checkpoint| checkpoint.root)
            )));
        }
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 64,
        rng_seed: RngSeed::Fixed(SEED),
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    #[test]
    fn prop_space_txn_par_identity_ignores_locally_free(
        groups in proptest::collection::vec(proptest::collection::vec(par_op(), 1..=5), 1..=4),
    ) {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build a test runtime")
            .block_on(par_identity_ignores_locally_free(groups))?;
    }
}

// The same `TaggedContinuation` parked through two `Par` values that differ
// only in `locally_free` is one continuation, as in the host: the view sees
// the first park through either value and deduplicates the second, the
// store keeps one, and both consume events are logged.
#[tokio::test]
async fn space_store_handle_par_tagged_continuation_dedup() {
    let p = channel("p0", 0);
    let q = channel("p0", 1);
    assert_eq!(p, q);
    assert_ne!(p.cmp(&q), std::cmp::Ordering::Equal);

    let host = par_space().await;
    for parked in [&p, &q] {
        host.consume(
            vec![parked.clone()],
            vec![BindPattern::default()],
            continuation("k"),
            false,
            BTreeSet::new(),
        )
        .await
        .expect("the host parks");
    }

    let space = par_space().await;
    let handle: ParHandle = SpaceStoreHandle::new(space.clone());
    let mut txn = handle.begin(&lock_keys()).await.expect("begin");
    txn.consume(park_op(vec![p.clone()], "k"))
        .expect("park through p");
    assert_eq!(
        txn.continuations(std::slice::from_ref(&q))
            .expect("read through q")
            .len(),
        1
    );
    assert_eq!(txn.joins(&q).expect("read the joins through q"), &[vec![
        p.clone()
    ]]);
    txn.consume(park_op(vec![q.clone()], "k"))
        .expect("park through q");
    for parked in [&p, &q] {
        assert_eq!(
            txn.continuations(std::slice::from_ref(parked))
                .expect("read the continuations")
                .len(),
            1
        );
        assert_eq!(txn.joins(parked).expect("read the joins").len(), 1);
    }
    assert_eq!(txn.commit().await.expect("commit").ops.len(), 2);

    for parked in [&p, &q] {
        assert_eq!(
            space
                .get_waiting_continuations(vec![parked.clone()])
                .await
                .len(),
            1
        );
        assert_eq!(
            space.get_waiting_continuations(vec![parked.clone()]).await,
            host.get_waiting_continuations(vec![parked.clone()]).await
        );
    }
    let (space_checkpoint, host_checkpoint) = (
        space
            .create_checkpoint()
            .await
            .expect("checkpoint the space"),
        host.create_checkpoint().await.expect("checkpoint the host"),
    );
    let consumes = space_checkpoint
        .log
        .iter()
        .filter(|event| matches!(event, Event::IoEvent(IOEvent::Consume(_))))
        .count();
    assert_eq!(consumes, 2);
    assert_eq!(
        event_bytes(&space_checkpoint.log),
        event_bytes(&host_checkpoint.log)
    );
    assert_eq!(space_checkpoint.root, host_checkpoint.root);
}

// Review regression (`Par` identity): a datum staged through `p` is visible
// through `q`, so a space never parks beside matching data it cannot see;
// and the installed guard holds through either value.
#[tokio::test]
async fn space_store_handle_par_locally_free_is_one_channel() {
    let p = channel("p0", 0);
    let q = channel("p0", 1);
    let space = par_space().await;
    let handle: ParHandle = SpaceStoreHandle::new(space.clone());
    let mut txn = handle.begin(std::slice::from_ref(&p)).await.expect("begin");
    txn.produce(store_op(p.clone(), "x"))
        .expect("stage a store through p");
    assert_eq!(txn.data(&p).expect("read p").len(), 1);
    assert_eq!(txn.data(&q).expect("read q").len(), 1);
    txn.produce(store_op(q.clone(), "y"))
        .expect("q is covered by the lock key p");
    assert_eq!(txn.data(&p).expect("read p").len(), 2);
    txn.commit().await.expect("commit");
    assert_eq!(space.get_data(&q).await.len(), 2);

    let system = par_space().await;
    system
        .install(
            vec![p.clone()],
            vec![BindPattern::default()],
            continuation("system"),
        )
        .await
        .expect("install on p");
    let handle: ParHandle = SpaceStoreHandle::new(system.clone());
    let mut txn = handle.begin(std::slice::from_ref(&p)).await.expect("begin");
    assert_eq!(txn.data(&q).err(), Some(SpaceTxnError::InstalledChannel));
    assert_eq!(
        txn.continuations(std::slice::from_ref(&q)).err(),
        Some(SpaceTxnError::InstalledChannel)
    );
    assert_eq!(
        txn.produce(store_op(q.clone(), "z")).err(),
        Some(SpaceTxnError::InstalledChannel)
    );
    // A sweep inside the open transaction fails with `NestedTransaction`
    // (FR-9'), so the transaction ends first.
    drop(txn);
    assert_eq!(
        handle.sweep_volatile(std::slice::from_ref(&q)).await,
        Err(SpaceTxnError::InstalledChannel)
    );
}
