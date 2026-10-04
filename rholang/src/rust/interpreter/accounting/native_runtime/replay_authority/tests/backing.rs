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
            budget.reserve_native_result_backing()
        };
        assert!(matches!(result, Err(InterpreterError::HostWorkRejected)));
        assert!(host.is_rejected());
        assert_eq!(budget.authority_events(), before);
        assert_eq!(budget.authority_realized(), realized);
        assert_eq!(budget.byte_observations().rows.len(), 1);
    }
}

#[test]
fn authority_checkpoint_prepays_cloned_state_cleanup() {
    let (budget, host) = populated(100_000_000);
    let before = host.usage(HostWorkDimension::VerificationOperations).get();
    let checkpoint = budget.native_authority_checkpoint().unwrap();
    let prepaid = host.usage(HostWorkDimension::VerificationOperations).get() - before;
    let before = host.usage(HostWorkDimension::VerificationOperations).get();
    let introductions = budget.introduction_authorities.lock().unwrap();
    let state = budget.authority_state.lock().unwrap();
    clone_backing::reserve(&state.events, &host).unwrap();
    clone_backing::reserve_slice(state.byte_observations.rows(), &host).unwrap();
    clone_backing::reserve(&state.realized, &host).unwrap();
    clone_backing::reserve(&state.reserved, &host).unwrap();
    clone_backing::reserve(&state.frontier, &host).unwrap();
    clone_backing::reserve(&state.stack_births, &host).unwrap();
    clone_backing::reserve(&*introductions, &host).unwrap();
    let clone_only = host.usage(HostWorkDimension::VerificationOperations).get() - before;
    assert!(clone_only > 0);
    assert!(prepaid >= clone_only * 2);
    drop((state, introductions, checkpoint));
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
