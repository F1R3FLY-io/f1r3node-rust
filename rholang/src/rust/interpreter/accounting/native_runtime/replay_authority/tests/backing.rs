use std::cell::Cell;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use models::rust::host_work::{HostWorkDimension, HostWorkLimit, HostWorkLimits, HostWorkUnits};
use shared::rust::collection_backing::tree_search_bound;

use super::*;

fn configured(memory: u64) -> (RuntimeBudget, HostWorkBudget) {
    let mut configuration = config(1_000_000, [0, 1, 1, 0]);
    let mut limits = HostWorkLimits::uniform(HostWorkLimit::new(100_000_000));
    limits.set(
        HostWorkDimension::SearchStateBytes,
        HostWorkLimit::new(memory),
    );
    configuration.host_work = HostWorkBudget::new(limits);
    let host = configuration.host_work();
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    budget.reset_for_native_execution(configuration).unwrap();
    (budget, host)
}

#[test]
fn unpaid_authority_preparation_cannot_publish_any_state() {
    let (budget, host) = configured(0);
    let binding = ReplayAuthorityBinding::new(budget.clone(), budget.deploy_id()).unwrap();
    let _scope = budget.enter_comm_accounting_scope();
    assert!(matches!(
        binding.prepare([None, Some(row(0, 3, true, false))]),
        Err(InterpreterError::HostWorkRejected)
    ));
    let state = budget.authority_state.lock().unwrap();
    assert!(state.events.is_empty());
    assert!(state.reserved.0.is_empty());
    assert!(state.realized.0.is_empty());
    assert!(state.pending_replay_events.is_empty());
    assert_eq!(state.pending_replay_rows, 0);
    assert!(state.byte_observations.rows().is_empty());
    assert!(host.is_rejected());
}

#[test]
fn retry_comparison_exhaustion_preserves_published_authority() {
    let (baseline_budget, baseline_host) = configured(100_000_000);
    let baseline_binding =
        ReplayAuthorityBinding::new(baseline_budget.clone(), baseline_budget.deploy_id()).unwrap();
    let _baseline_scope = baseline_budget.enter_comm_accounting_scope();
    baseline_binding
        .prepare([None, Some(row(0, 3, true, false))])
        .unwrap()
        .publish();
    let owner_work = baseline_host
        .usage(HostWorkDimension::VerificationOperations)
        .get();

    let mut configuration = config(1_000_000, [0, 1, 1, 0]);
    let mut limits = HostWorkLimits::uniform(HostWorkLimit::new(100_000_000));
    // Changed by C14 (DR-79): the retry's three identity searches are
    // charged at the largest live size of the three maps, here the one
    // published event.
    // let lookup_work = (u64::from(usize::BITS) + 1) * 11 * 3;
    let lookup_work = 3 * tree_search_bound(1) as u64;
    limits.set(
        HostWorkDimension::VerificationOperations,
        HostWorkLimit::new(owner_work + lookup_work),
    );
    configuration.host_work = HostWorkBudget::new(limits);
    let host = configuration.host_work();
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    budget.reset_for_native_execution(configuration).unwrap();
    let binding = ReplayAuthorityBinding::new(budget.clone(), budget.deploy_id()).unwrap();
    let _scope = budget.enter_comm_accounting_scope();
    binding
        .prepare([None, Some(row(0, 3, true, false))])
        .unwrap()
        .publish();
    let realized = budget.authority_realized();
    assert!(matches!(
        binding.prepare([None, Some(row(0, 3, true, true))]),
        Err(InterpreterError::HostWorkRejected)
    ));
    assert!(host.is_rejected());
    assert_eq!(budget.authority_realized(), realized);
    assert_eq!(budget.authority_events().len(), 1);
    assert_eq!(budget.byte_observations().rows.len(), 1);
    assert!(budget
        .authority_state
        .lock()
        .unwrap()
        .pending_replay_events
        .is_empty());
}

#[test]
fn prepaid_publication_and_cancellation_survive_later_host_rejection() {
    let (budget, host) = configured(100_000_000);
    let binding = ReplayAuthorityBinding::new(budget.clone(), budget.deploy_id()).unwrap();
    let _scope = budget.enter_comm_accounting_scope();
    let mut kept = binding
        .prepare([None, Some(row(0, 3, true, false))])
        .unwrap();
    let cancelled = binding
        .prepare([None, Some(row(1, 3, true, false))])
        .unwrap();
    assert!(host
        .reserve(
            HostWorkDimension::SearchStateBytes,
            HostWorkUnits::new(u64::MAX)
        )
        .is_err());
    let before = host.usages();
    kept.publish();
    drop(cancelled);
    assert_eq!(before, host.usages());
    let expected = authority::authority_demand(&authority(3)).unwrap();
    let state = budget.authority_state.lock().unwrap();
    assert_eq!(state.realized, expected);
    assert_eq!(state.reserved, expected);
    assert_eq!(state.events.len(), 1);
    assert_eq!(state.byte_observations.rows().len(), 1);
    assert!(state.pending_replay_events.is_empty());
    assert_eq!(state.pending_replay_rows, 0);
}

fn populated(memory: u64) -> (RuntimeBudget, HostWorkBudget) {
    let (budget, host) = configured(memory);
    let binding = ReplayAuthorityBinding::new(budget.clone(), budget.deploy_id()).unwrap();
    let scope = budget.enter_comm_accounting_scope();
    binding
        .prepare([None, Some(row(0, 3, true, false))])
        .unwrap()
        .publish();
    drop(scope);
    (budget, host)
}

#[test]
fn checkpoint_and_result_reject_before_unpaid_copies_and_preserve_live_state() {
    let (_, baseline) = populated(100_000_000);
    let exhausted_limit = baseline.usage(HostWorkDimension::SearchStateBytes).get();
    for checkpoint in [true, false] {
        let (budget, host) = populated(exhausted_limit);
        let before = budget.authority_events();
        let realized = budget.authority_realized();
        let result = if checkpoint {
            budget.native_authority_checkpoint().map(|_| ())
        } else {
            budget.reserve_native_result_backing(true)
        };
        assert!(matches!(result, Err(InterpreterError::HostWorkRejected)));
        assert!(host.is_rejected());
        assert_eq!(budget.authority_events(), before);
        assert_eq!(budget.authority_realized(), realized);
        assert_eq!(budget.byte_observations().rows.len(), 1);
    }
}

/// D-E3 (DR-110): the operations, scanned bytes and backing bytes that a
/// budget has charged.
fn walker_usage(host: &HostWorkBudget) -> [u64; 3] {
    [
        HostWorkDimension::VerificationOperations,
        HostWorkDimension::VerificationBytes,
        HostWorkDimension::SearchStateBytes,
    ]
    .map(|dimension| host.usage(dimension).get())
}

fn usage_since(host: &HostWorkBudget, before: [u64; 3]) -> [u64; 3] {
    let after = walker_usage(host);
    [0, 1, 2].map(|index| after[index] - before[index])
}

#[test]
fn authority_checkpoint_prepays_cloned_state_cleanup() {
    let (budget, host) = populated(100_000_000);
    let before = walker_usage(&host);
    let checkpoint = budget.native_authority_checkpoint().unwrap();
    let prepaid = usage_since(&host, before);
    // Changed by D-O1 (DR-110): the checkpoint charges block walks. An
    // unlimited budget that runs the same walks states the exact charge in
    // every dimension: a block copy and a shared-pointer cleanup of the events,
    // the rows and the generation pointer, and a block copy and cleanup of the
    // other state.
    // let before = host.usage(HostWorkDimension::VerificationOperations).get();
    // let introductions = budget.introduction_authorities.lock().unwrap();
    // let state = budget.authority_state.lock().unwrap();
    // clone_backing::reserve(&state.events, &host).unwrap();
    // clone_backing::reserve_slice(state.byte_observations.rows(), &host).unwrap();
    // clone_backing::reserve(&state.realized, &host).unwrap();
    // clone_backing::reserve(&state.reserved, &host).unwrap();
    // clone_backing::reserve(&state.frontier, &host).unwrap();
    // clone_backing::reserve(&state.stack_births, &host).unwrap();
    // clone_backing::reserve(&*introductions, &host).unwrap();
    // let clone_only = host.usage(HostWorkDimension::VerificationOperations).get() - before;
    // assert!(clone_only > 0);
    // assert!(prepaid >= clone_only * 2);
    // drop((state, introductions, checkpoint));
    let mirror = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX)));
    {
        let introductions = budget.introduction_authorities.lock().unwrap();
        let state = budget.authority_state.lock().unwrap();
        let generation = &state.native.as_ref().expect("native state").generation;
        assert!(!state.events.is_empty());
        clone_backing::reserve_blocks(&state.events, &mirror).unwrap();
        clone_backing::inspect_shared_pointers_blocks(&state.events, &mirror).unwrap();
        let rows = state.byte_observations.rows();
        assert!(!rows.is_empty());
        clone_backing::reserve_blocks_slice(rows, &mirror).unwrap();
        clone_backing::inspect_shared_pointer_slice_blocks(rows, &mirror).unwrap();
        clone_backing::reserve_blocks_copy_and_cleanup(&state.realized, &mirror).unwrap();
        clone_backing::reserve_blocks_copy_and_cleanup(&state.reserved, &mirror).unwrap();
        clone_backing::reserve_blocks_copy_and_cleanup(&state.frontier, &mirror).unwrap();
        clone_backing::reserve_blocks_copy_and_cleanup(&state.stack_births, &mirror).unwrap();
        clone_backing::reserve_blocks_copy_and_cleanup(&*introductions, &mirror).unwrap();
        clone_backing::reserve_blocks(generation, &mirror).unwrap();
        clone_backing::inspect_shared_pointers_blocks(generation, &mirror).unwrap();
    }
    assert_eq!(prepaid, walker_usage(&mirror));
    drop(checkpoint);
}

/// D-E3 (DR-110): the byte observations were prepaid at birth (the C5 rule,
/// DR-83), so the cleanup of the copied events visits each shared pointer but
/// not its payload. The checkpoint charge does not change when the events
/// point to a larger observation. The per-level cleanup walked the payloads.
#[test]
fn authority_checkpoint_charge_is_independent_of_observation_payloads() {
    let charge = |owners: Option<usize>| {
        let (budget, host) = populated(100_000_000);
        if let Some(owners) = owners {
            let larger = row(0, owners, true, false).observation;
            let mut state = budget.authority_state.lock().unwrap();
            let mut replaced = 0;
            for event in state.events.values_mut() {
                if event.byte_observation.is_some() {
                    event.byte_observation = Some(Arc::clone(&larger));
                    replaced += 1;
                }
            }
            assert!(replaced > 0);
        }
        let before = walker_usage(&host);
        let checkpoint = budget.native_authority_checkpoint().unwrap();
        let charge = usage_since(&host, before);
        drop(checkpoint);
        charge
    };
    assert_eq!(charge(None), charge(Some(32)));
}

/// D-E3 (DR-110): the backing that the checkpoint reserves covers the bytes
/// that its clones allocate.
#[test]
fn authority_checkpoint_backing_covers_allocations() {
    let (budget, host) = populated(100_000_000);
    let before = walker_usage(&host);
    let (checkpoint, allocated) =
        crate::rust::interpreter::accounting::native_runtime::clone_backing::tests::measured(
            || budget.native_authority_checkpoint().unwrap(),
        );
    let prepaid = usage_since(&host, before);
    assert!(allocated > 0);
    assert!(
        allocated as u64 <= prepaid[2],
        "allocated {allocated} reserved {}",
        prepaid[2]
    );
    drop(checkpoint);
}

#[test]
fn restored_observation_capacity_must_be_paid_again_before_growth() {
    let (budget, host) = populated(100_000_000);
    let checkpoint = budget.native_authority_checkpoint().unwrap();
    budget.restore_native_authority(checkpoint).unwrap();
    let mut state = budget.authority_state.lock().unwrap();
    let before = host.usage(HostWorkDimension::SearchStateBytes).get();
    state.byte_observations.reserve_native(1, &host).unwrap();
    let after = host.usage(HostWorkDimension::SearchStateBytes).get();
    assert!(after > before);
    assert_eq!(state.byte_observations.rows().len(), 1);
}

thread_local! {
    static COMPARISONS: Cell<usize> = const { Cell::new(0) };
}

/// A `[u8; 32]` key with the same layout that counts its comparisons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CountingKey([u8; 32]);

impl PartialOrd for CountingKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}

impl Ord for CountingKey {
    fn cmp(&self, other: &Self) -> Ordering {
        COMPARISONS.with(|count| count.set(count.get() + 1));
        self.0.cmp(&other.0)
    }
}

fn counted<T>(action: impl FnOnce() -> T) -> (T, usize) {
    COMPARISONS.with(|count| count.set(0));
    let value = action();
    (value, COMPARISONS.with(Cell::get))
}

fn key(seed: &mut u64) -> [u8; 32] {
    let mut bytes = [0; 32];
    for chunk in bytes.chunks_mut(8) {
        *seed = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut mixed = *seed;
        mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        chunk.copy_from_slice(&(mixed ^ (mixed >> 31)).to_le_bytes());
    }
    bytes
}

#[derive(Clone, Copy, Debug, Default)]
struct Sizes {
    stack_ids: usize,
    pending: usize,
    events: usize,
    frontier: usize,
    reserved: usize,
    realized: usize,
    lanes: usize,
    shared: usize,
}

/// The live authority maps with counting keys: the same keys and values as
/// the charged state, so the same B-tree shapes.
#[derive(Default)]
struct Mirror {
    stack_ids: BTreeSet<CountingKey>,
    pending: BTreeMap<CountingKey, AuthorityRuntimeEvent>,
    events: BTreeMap<CountingKey, AuthorityRuntimeEvent>,
    frontier: BTreeMap<CountingKey, CostAuthority>,
    reserved: BTreeMap<CountingKey, u64>,
    realized: BTreeMap<CountingKey, u64>,
    allocation: BTreeMap<CountingKey, u64>,
}

/// A live authority state of the given sizes, its counting mirror, a fresh
/// COMM identity, and a debit of `lanes` keys of which up to `shared` are
/// already reserved. An enforced allocation funds every key.
fn build(
    sizes: Sizes,
    enforced: bool,
    mut seed: u64,
) -> (
    AuthorityRuntimeState,
    Mirror,
    [u8; 32],
    BTreeMap<[u8; 32], u64>,
) {
    let mut state = AuthorityRuntimeState {
        enforce_allocation: enforced,
        ..AuthorityRuntimeState::default()
    };
    let mut mirror = Mirror::default();
    for _ in 0..sizes.stack_ids {
        let id = key(&mut seed);
        state.pending_stack_event_ids.insert(id);
        mirror.stack_ids.insert(CountingKey(id));
    }
    for _ in 0..sizes.pending {
        let id = key(&mut seed);
        state
            .pending_replay_events
            .insert(id, AuthorityRuntimeEvent::default());
        mirror
            .pending
            .insert(CountingKey(id), AuthorityRuntimeEvent::default());
    }
    for _ in 0..sizes.events {
        let id = key(&mut seed);
        state.events.insert(id, AuthorityRuntimeEvent::default());
        mirror
            .events
            .insert(CountingKey(id), AuthorityRuntimeEvent::default());
    }
    for _ in 0..sizes.frontier {
        let id = key(&mut seed);
        state.frontier.insert(id, CostAuthority::default());
        mirror
            .frontier
            .insert(CountingKey(id), CostAuthority::default());
    }
    let mut reserved = Vec::with_capacity(sizes.reserved);
    for _ in 0..sizes.reserved {
        let lane = key(&mut seed);
        reserved.push(lane);
        state.reserved.0.insert(lane, 1_000);
        mirror.reserved.insert(CountingKey(lane), 1_000);
    }
    for _ in 0..sizes.realized {
        let lane = key(&mut seed);
        state.realized.0.insert(lane, 500);
        mirror.realized.insert(CountingKey(lane), 500);
    }
    let mut debit = BTreeMap::new();
    for (index, lane) in reserved
        .iter()
        .take(sizes.shared.min(sizes.lanes))
        .enumerate()
    {
        debit.insert(*lane, 1 + index as u64);
    }
    while debit.len() < sizes.lanes {
        debit.insert(key(&mut seed), 7);
    }
    if enforced {
        let funded: Vec<[u8; 32]> = state
            .reserved
            .0
            .keys()
            .chain(debit.keys())
            .copied()
            .collect();
        for lane in funded {
            state.allocation.0.insert(lane, u64::MAX / 4);
            mirror.allocation.insert(CountingKey(lane), u64::MAX / 4);
        }
    }
    (state, mirror, key(&mut seed), debit)
}

/// The charge of one replay publication: the identity lookups and the
/// changes that `ReplayAuthorityBinding::prepare` reserves.
fn charged(
    state: &AuthorityRuntimeState,
    debit: Option<&ResourceMultiset<[u8; 32]>>,
    frontier: bool,
) -> [u64; 3] {
    let host = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(1 << 50)));
    super::super::backing::reserve_event_lookup(state, &host).expect("lookup charge");
    super::super::backing::reserve_changes(state, debit, frontier, &host).expect("change charge");
    [
        HostWorkDimension::VerificationOperations,
        HostWorkDimension::VerificationBytes,
        HostWorkDimension::SearchStateBytes,
    ]
    .map(|dimension| host.usage(dimension).get())
}

/// The real map work of a granted COMM: prepare, then publish or abort.
/// Returns the key comparisons and the bytes allocated.
fn granted_work(
    mirror: &mut Mirror,
    id: [u8; 32],
    debit: &BTreeMap<[u8; 32], u64>,
    enforced: bool,
    publish: bool,
) -> (usize, usize) {
    let id = CountingKey(id);
    let debit: BTreeMap<CountingKey, u64> = debit
        .iter()
        .map(|(lane, amount)| (CountingKey(*lane), *amount))
        .collect();
    let event = AuthorityRuntimeEvent::default();
    let ((_, comparisons), allocated) = clone_backing::tests::measured(|| {
        counted(|| {
            assert!(!mirror.stack_ids.contains(&id));
            assert!(!mirror.pending.contains_key(&id));
            assert!(!mirror.events.contains_key(&id));
            for lane in debit.keys() {
                std::hint::black_box(mirror.reserved.contains_key(lane));
                std::hint::black_box(mirror.realized.contains_key(lane));
            }
            super::super::sparse_ledger::validate_add(
                &mirror.reserved,
                &debit,
                enforced.then_some(&mirror.allocation),
            )
            .expect("funded debit");
            super::super::sparse_ledger::add_assign(&mut mirror.reserved, &debit).expect("reserve");
            mirror.pending.insert(id, event);
            let event = mirror.pending.remove(&id).expect("pending event");
            if publish {
                super::super::sparse_ledger::add_assign(&mut mirror.realized, &debit)
                    .expect("realize");
                mirror.events.insert(id, event);
            } else {
                super::super::sparse_ledger::sub_assign(&mut mirror.reserved, &debit)
                    .expect("cancel");
            }
        })
    });
    (comparisons, allocated)
}

/// The real map work of a denied COMM: prepare, then the frontier insert of
/// its publication. Returns the key comparisons and the bytes allocated.
fn denied_work(mirror: &mut Mirror, id: [u8; 32], enforced: bool) -> (usize, usize) {
    let id = CountingKey(id);
    let authority = CostAuthority::default();
    let ((_, comparisons), allocated) = clone_backing::tests::measured(|| {
        counted(|| {
            assert!(!mirror.stack_ids.contains(&id));
            assert!(!mirror.pending.contains_key(&id));
            assert!(!mirror.events.contains_key(&id));
            if enforced {
                std::hint::black_box(mirror.reserved.iter().all(|(lane, amount)| {
                    mirror.allocation.get(lane).copied().unwrap_or(0) >= *amount
                }));
            }
            mirror.frontier.insert(id, authority);
        })
    });
    (comparisons, allocated)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// C14 (DR-79; `OrderedLookupBound.search_within_bound`): for random
    /// authority states, the charge of one replay publication covers the
    /// comparisons and the allocations of the real `BTreeMap` work of its
    /// prepare and of its publish or abort. Each comparison reads two
    /// 32-byte keys.
    #[test]
    fn authority_update_charge_covers_std_btree_work(
        stack_ids in 0_usize..40,
        pending in 0_usize..4,
        events in 0_usize..700,
        frontier in 0_usize..700,
        reserved in 0_usize..120,
        realized in 0_usize..120,
        lanes in 1_usize..9,
        shared in 0_usize..9,
        enforced in any::<bool>(),
        granted in any::<bool>(),
        publish in any::<bool>(),
        seed in any::<u64>(),
    ) {
        let sizes = Sizes { stack_ids, pending, events, frontier, reserved, realized, lanes, shared };
        let (state, mut mirror, id, debit) = build(sizes, enforced, seed);
        let (charge, (comparisons, allocated)) = if granted {
            let multiset = ResourceMultiset(debit.clone());
            (
                charged(&state, Some(&multiset), false),
                granted_work(&mut mirror, id, &debit, enforced, publish),
            )
        } else {
            (charged(&state, None, true), denied_work(&mut mirror, id, enforced))
        };
        prop_assert!(charge[0] >= comparisons as u64, "{charge:?} < {comparisons} comparisons for {sizes:?}");
        prop_assert!(charge[1] >= 64 * comparisons as u64, "{charge:?} bytes < 64 * {comparisons} for {sizes:?}");
        prop_assert!(charge[2] >= allocated as u64, "{charge:?} < {allocated} allocated bytes for {sizes:?}");
    }
}

/// C14 (DR-79): the publish and abort searches are charged at the sizes after
/// the operation. The ledger maps grow by the debit's lanes and the event
/// maps by one entry (`OrderedLookupBound.pre_operation_size_undercharges`
/// shows why the size before the operation is unsound). The identity lookups
/// run at once, at the live sizes.
#[test]
fn deferred_searches_are_charged_after_the_operation() {
    for (lanes, events) in [
        (1, 0),
        (3, 0),
        (10, 0),
        (11, 0),
        (3, 10),
        (3, 70),
        (12, 430),
    ] {
        let sizes = Sizes {
            events,
            lanes,
            ..Sizes::default()
        };
        let (state, _, _, debit) = build(sizes, false, lanes as u64);
        let charge = charged(&state, Some(&ResourceMultiset(debit)), false);
        let lookups = 3 * tree_search_bound(events);
        let ledger = 15 * lanes * tree_search_bound(lanes);
        let event_maps = 4 * tree_search_bound(events + 1);
        assert_eq!(
            charge[0],
            (lookups + ledger + event_maps) as u64,
            "lanes {lanes}, events {events}"
        );
    }
}

fn usages_of(host: &HostWorkBudget) -> [u64; 3] {
    [
        host.usage(HostWorkDimension::VerificationOperations).get(),
        host.usage(HostWorkDimension::VerificationBytes).get(),
        host.usage(HostWorkDimension::SearchStateBytes).get(),
    ]
}

fn spent(before: [u64; 3], after: [u64; 3]) -> [u64; 3] {
    [
        after[0] - before[0],
        after[1] - before[1],
        after[2] - before[2],
    ]
}

/// The usage of one prepare of `observed` on a fresh budget that already
/// published one granted event with id 0 (so a retry of id 0 finds it).
fn prepare_usage(observed: ReplayAuthorityObservation) -> [u64; 3] {
    let (budget, host) = configured(100_000_000);
    let binding = ReplayAuthorityBinding::new(budget.clone(), budget.deploy_id()).unwrap();
    let _scope = budget.enter_comm_accounting_scope();
    binding
        .prepare([None, Some(row(0, 3, true, false))])
        .unwrap()
        .publish();
    let before = usages_of(&host);
    binding
        .prepare([None, Some(observed)])
        .expect("prepare")
        .publish();
    spent(before, usages_of(&host))
}

/// D-O5 (DR-89): each prepare branch reserves its reads and copies before it
/// performs them: a budget smaller than the measured use in any dimension
/// rejects the prepare and publishes nothing, in every branch.
#[test]
fn prepare_rejects_each_smaller_dimension_without_publication() {
    let rows = [
        ("granted", (7, 4, true, false)),
        ("frontier", (7, 4, false, false)),
        ("retry", (0, 3, true, true)),
    ];
    for (branch, (id, owners, granted, retry)) in rows {
        let required = prepare_usage(row(id, owners, granted, retry));
        assert!(
            required.iter().all(|value| *value > 0),
            "{branch}: {required:?}"
        );
        for dimension in 0..3 {
            let (budget, host) = configured(100_000_000);
            let binding = ReplayAuthorityBinding::new(budget.clone(), budget.deploy_id()).unwrap();
            let _scope = budget.enter_comm_accounting_scope();
            binding
                .prepare([None, Some(row(0, 3, true, false))])
                .unwrap()
                .publish();
            let base = usages_of(&host);
            let dimensions = [
                HostWorkDimension::VerificationOperations,
                HostWorkDimension::VerificationBytes,
                HostWorkDimension::SearchStateBytes,
            ];
            let limit = HostWorkUnits::new(100_000_000 - base[dimension] - required[dimension] + 1);
            assert!(
                host.reserve(dimensions[dimension], limit).is_ok(),
                "{branch}: fill {dimension}"
            );
            let events = budget.authority_events();
            let realized = budget.authority_realized();
            let rows_before = budget.byte_observations().rows.len();
            assert!(
                matches!(
                    binding.prepare([None, Some(row(id, owners, granted, retry))]),
                    Err(InterpreterError::HostWorkRejected)
                ),
                "{branch}: dimension {dimension}"
            );
            assert_eq!(budget.authority_events(), events, "{branch}: {dimension}");
            assert_eq!(
                budget.authority_realized(),
                realized,
                "{branch}: {dimension}"
            );
            assert_eq!(budget.byte_observations().rows.len(), rows_before);
            let state = budget.authority_state.lock().unwrap();
            assert!(state.pending_replay_events.is_empty());
            assert_eq!(state.pending_replay_rows, 0);
        }
    }
}

/// Negative control for D-O5 (DR-89): the legacy prepare also walked the
/// whole recorded observation of every new row, so its charge grew with the
/// observation although the granted branch only copies the authority.
#[test]
fn legacy_prepare_charge_walked_the_recorded_observation() {
    let walk = |observed: &ReplayAuthorityObservation| {
        let (_, host) = configured(100_000_000);
        clone_backing::inspect(observed.observation.as_ref(), &host).unwrap();
        usages_of(&host)
    };
    let small = row(7, 1, true, false);
    let large = row(7, 32, true, false);
    let small_walk = walk(&small);
    let large_walk = walk(&large);
    let small_now = prepare_usage(small);
    let large_now = prepare_usage(large);
    let legacy_growth = (large_now[1] + large_walk[1]) - (small_now[1] + small_walk[1]);
    let growth = large_now[1] - small_now[1];
    assert!(legacy_growth > growth, "{legacy_growth} vs {growth}");
}

fn result_usage(budget: &RuntimeBudget, host: &HostWorkBudget, copies_rows: bool) -> [u64; 3] {
    let before = usages_of(host);
    budget
        .reserve_native_result_backing(copies_rows)
        .expect("result backing");
    spent(before, usages_of(host))
}

fn with_extra_row(owners: usize) -> (RuntimeBudget, HostWorkBudget) {
    let (budget, host) = populated(100_000_000);
    budget
        .authority_state
        .lock()
        .unwrap()
        .byte_observations
        .push(row(9, owners, true, false).observation);
    (budget, host)
}

/// D-O4 (DR-89): a replay with evidence copies no rows, so its result
/// backing does not depend on the rows; play copies one shared pointer per
/// row, so its charge does not depend on the row payloads.
#[test]
fn result_backing_charges_row_pointers_not_payloads() {
    let (budget, host) = populated(100_000_000);
    let replay = result_usage(&budget, &host, false);
    let (small_budget, small_host) = with_extra_row(1);
    let (large_budget, large_host) = with_extra_row(32);
    assert_eq!(result_usage(&small_budget, &small_host, false), replay);
    assert_eq!(result_usage(&large_budget, &large_host, false), replay);
    let small_play = result_usage(&small_budget, &small_host, true);
    let large_play = result_usage(&large_budget, &large_host, true);
    assert_eq!(small_play, large_play);
    assert!(small_play[0] > replay[0]);
}

/// D-O4 (DR-89): the result backing reserves at least the bytes that the
/// result copies allocate in play (events, realized ledger, stack births,
/// row pointers and the result vectors).
#[test]
fn event_result_backing_covers_counted_allocations() {
    use crate::rust::interpreter::accounting::native_runtime::clone_backing::tests::measured;
    let (budget, host) = with_extra_row(32);
    let reserved = result_usage(&budget, &host, true)[2];
    let ((events, realized, births, rows), allocated) = measured(|| {
        (
            budget.authority_events(),
            budget.authority_realized(),
            budget.authority_stack_births(),
            budget.byte_observations(),
        )
    });
    assert_eq!(events.len(), 1);
    assert_eq!(rows.rows.len(), 2);
    drop((realized, births));
    assert!(
        u64::try_from(allocated).expect("allocated fits in u64") <= reserved,
        "allocated {allocated}, reserved {reserved}"
    );
}

/// Negative control for D-O4 (DR-89): the legacy result backing walked the
/// events map and every row payload, also in replay, so it grew with
/// payloads that the result never copies.
#[test]
fn legacy_result_backing_walked_unreturned_payloads() {
    let legacy = |budget: &RuntimeBudget, host: &HostWorkBudget| {
        let now = result_usage(budget, host, false);
        let (_, oracle) = configured(100_000_000);
        let state = budget.authority_state.lock().unwrap();
        clone_backing::reserve_copy_and_cleanup(&state.events, &oracle).unwrap();
        clone_backing::reserve_slice_copy_and_cleanup(state.byte_observations.rows(), &oracle)
            .unwrap();
        let walks = usages_of(&oracle);
        [now[0] + walks[0], now[1] + walks[1], now[2] + walks[2]]
    };
    let (small_budget, small_host) = with_extra_row(1);
    let (large_budget, large_host) = with_extra_row(32);
    let small = legacy(&small_budget, &small_host);
    let large = legacy(&large_budget, &large_host);
    assert!(large[1] > small[1], "{small:?} vs {large:?}");
    assert_eq!(
        result_usage(&small_budget, &small_host, false),
        result_usage(&large_budget, &large_host, false)
    );
}
