use std::num::NonZeroUsize;

use casper::rust::rholang::runtime::RuntimeOps;
use casper::rust::util::rholang::costacc::monetary_cursor;
use casper::rust::util::rholang::costacc::vault_cost_deploy::{
    ApplyCostDeploy, ApplyPhloCostDeploy, VaultAllocation, VaultSettlement,
};
use casper::rust::util::rholang::costacc::vault_payer::balance_query_source;
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use casper::rust::util::rholang::system_deploy_result::SystemDeployResult;
use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rust::block::state_hash::StateHash;
use rholang::rust::interpreter::accounting::monetary_allocation::{
    MonetaryCursor, MonetaryCursorTransition,
};
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::rho_type::RhoNumber;
use rholang::rust::interpreter::util::vault_address::VaultAddress;

use crate::util::rholang::resources::with_runtime_manager;

async fn balance(manager: &RuntimeManager, root: &StateHash, address: &VaultAddress) -> i64 {
    let (values, _) = manager
        .play_exploratory_deploy(balance_query_source(address), root, None)
        .await
        .unwrap();
    assert_eq!(values.len(), 1);
    RhoNumber::unapply(&values[0]).unwrap()
}

async fn cursor(
    manager: &RuntimeManager,
    root: &StateHash,
    scope: [u8; 32],
) -> Option<MonetaryCursor> {
    let (values, _) = manager
        .play_exploratory_deploy(monetary_cursor::query_source(&scope), root, None)
        .await
        .unwrap();
    monetary_cursor::decode_snapshot(&values, NonZeroUsize::new(2).unwrap()).unwrap()
}

fn request(
    payer: &VaultAddress,
    recipient: &VaultAddress,
    revision: i64,
    position: i64,
) -> ApplyPhloCostDeploy {
    let count = NonZeroUsize::new(2).unwrap();
    let expected = MonetaryCursor::new(revision, position, count).unwrap();
    ApplyPhloCostDeploy::new(
        ApplyCostDeploy::new(
            [0xe1; 32],
            vec![VaultAllocation::new(payer.to_base58(), 5).unwrap()],
            vec![VaultSettlement::new(payer.to_base58(), 2, 1).unwrap()],
            recipient.to_base58(),
            Blake2b512Random::create_from_bytes(&[0xe1]),
        )
        .unwrap(),
        Some(MonetaryCursorTransition::new([0xe2; 32], expected, 1, count).unwrap()),
        Some(MonetaryCursorTransition::new([0xe3; 32], expected, 1, count).unwrap()),
        count,
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn general_wallet_cost_settlement_advances_both_cursors_once() {
    with_runtime_manager(|manager, genesis, block| async move {
        let initial = block.body.state.post_state_hash;
        let payer = VaultAddress::from_public_key(&genesis.genesis_vaults[0].1).unwrap();
        let recipient =
            VaultAddress::from_unforgeable(&models::rhoapi::GPrivate { id: vec![0xd1; 32] });
        let before = balance(&manager, &initial, &payer).await;
        assert_eq!(cursor(&manager, &initial, [0xe2; 32]).await, None);
        assert_eq!(cursor(&manager, &initial, [0xe3; 32]).await, None);
        let mut runtime = RuntimeOps::new(manager.spawn_runtime().await.unwrap());
        let first = match runtime
            .play_system_deploy(&initial, &mut request(&payer, &recipient, 0, 0))
            .await
            .unwrap()
        {
            SystemDeployResult::PlaySucceeded { state_hash, .. } => state_hash,
            SystemDeployResult::PlayFailed { .. } => panic!("first settlement failed"),
        };
        assert_eq!(balance(&manager, &first, &payer).await, before - 3);
        assert_eq!(balance(&manager, &first, &recipient).await, 1);
        let advanced = Some(MonetaryCursor::new(1, 1, NonZeroUsize::new(2).unwrap()).unwrap());
        assert_eq!(cursor(&manager, &first, [0xe2; 32]).await, advanced);
        assert_eq!(cursor(&manager, &first, [0xe3; 32]).await, advanced);
        assert!(matches!(
            runtime
                .play_system_deploy(&first, &mut request(&payer, &recipient, 0, 0))
                .await
                .unwrap(),
            SystemDeployResult::PlayFailed { .. }
        ));
        let failed_root = runtime
            .runtime
            .create_checkpoint()
            .await
            .root
            .to_bytes_prost();
        assert_eq!(balance(&manager, &failed_root, &payer).await, before - 3);
        assert_eq!(balance(&manager, &failed_root, &recipient).await, 1);
        assert_eq!(cursor(&manager, &failed_root, [0xe2; 32]).await, advanced);
        assert_eq!(cursor(&manager, &failed_root, [0xe3; 32]).await, advanced);
    })
    .await
    .unwrap();
}

// Added by DR-119 (bug 11004): two settlements that start from one state
// conflict exactly when they share a cursor scope, or when a first use on each
// side falls in one creation bucket.

/// The keccak256 hash that `ensureCostCursor` in `SystemVault.rho` and the
/// cursor TreeHashMap compute for a scope. The input is `scope.toByteArray()`,
/// the protobuf encoding of the scope's ByteArray.
fn cost_cursor_hash(scope: &[u8; 32]) -> Vec<u8> {
    use prost::Message;
    let encoded = rholang::rust::interpreter::rho_type::RhoByteArray::create_par(scope.to_vec())
        .encode_to_vec();
    crypto::rust::hash::keccak256::Keccak256::hash(encoded)
}

/// The creation bucket of a scope: the first byte of its hash.
fn cost_cursor_bucket(scope: &[u8; 32]) -> u8 { cost_cursor_hash(scope)[0] }

/// The TreeHashMap leaf of a scope. The cursor map has depth 2, so a leaf is
/// the first two bytes of the hash (`Registry.rho`, `TreeHashMap`).
fn cost_cursor_leaf(scope: &[u8; 32]) -> [u8; 2] {
    let hash = cost_cursor_hash(scope);
    [hash[0], hash[1]]
}

fn rehash(scope: &[u8; 32]) -> [u8; 32] {
    crypto::rust::hash::blake2b256::Blake2b256::hash(scope.to_vec())
        .try_into()
        .expect("a Blake2b-256 digest has 32 bytes")
}

fn any_scope(_: &[u8; 32]) -> bool { true }

/// The first scope in the rehash chain of `seed` that satisfies `want` and
/// differs from every scope in `avoid`.
fn retarget(seed: [u8; 32], want: impl Fn(&[u8; 32]) -> bool, avoid: &[[u8; 32]]) -> [u8; 32] {
    let mut scope = seed;
    while !want(&scope) || avoid.contains(&scope) {
        scope = rehash(&scope);
    }
    scope
}

/// The two cursor scopes of one settlement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CursorScopes {
    resource: [u8; 32],
    fee: [u8; 32],
}

impl CursorScopes {
    fn both(&self) -> [[u8; 32]; 2] { [self.resource, self.fee] }
}

/// The number of case kinds that `build_case` generates.
const CONFLICT_CASE_KINDS: usize = 7;

/// Two settlements that start from one state, the scopes whose cursors exist
/// in that state, and the kind that `build_case` built.
#[derive(Clone, Debug)]
struct ConflictCase {
    kind: u8,
    first: CursorScopes,
    second: CursorScopes,
    existing: std::collections::BTreeSet<[u8; 32]>,
}

/// The rule of DR-119. A shared scope conflicts on its cursor cells. Two first
/// uses in one bucket conflict on the creation lock of the bucket. The lookup
/// of an existing cursor only peeks, so it takes no lock.
fn expected_conflict(case: &ConflictCase) -> bool {
    let shared = case
        .first
        .both()
        .iter()
        .any(|scope| case.second.both().contains(scope));
    let first_use_buckets = |scopes: &CursorScopes| -> std::collections::BTreeSet<u8> {
        scopes
            .both()
            .iter()
            .filter(|scope| !case.existing.contains(*scope))
            .map(cost_cursor_bucket)
            .collect()
    };
    shared || !first_use_buckets(&case.first).is_disjoint(&first_use_buckets(&case.second))
}

/// Builds a case of one of seven kinds from four seeds:
///
/// 0. Distinct scopes whose cursors exist.
/// 1. First uses in distinct buckets.
/// 2. First uses in one bucket.
/// 3. A shared scope whose cursor exists.
/// 4. A shared first use.
/// 5. A first use in the bucket of an existing cursor of the other side.
/// 6. A first use in the leaf of an existing cursor of the other side.
///
/// Bit 0 of `pairing` selects the scope of the first settlement that the
/// second settlement meets. Bit 1 selects the slot that holds it there.
fn build_case(seeds: [[u8; 32]; 4], kind: u8, pairing: u8) -> ConflictCase {
    let [a, b, c, d] = seeds;
    let first = CursorScopes {
        resource: a,
        fee: retarget(b, any_scope, &[a]),
    };
    let chosen = match pairing & 1 {
        0 => first.resource,
        _ => first.fee,
    };
    let second_with = |scope: [u8; 32], seed: [u8; 32]| {
        let other = retarget(seed, any_scope, &[scope, first.resource, first.fee]);
        match pairing & 2 {
            0 => CursorScopes {
                resource: scope,
                fee: other,
            },
            _ => CursorScopes {
                resource: other,
                fee: scope,
            },
        }
    };
    let taken = first.both().map(|scope| cost_cursor_bucket(&scope));
    let (second, existing) = match kind {
        0 => {
            let resource = retarget(c, any_scope, &first.both());
            let fee = retarget(d, any_scope, &[first.resource, first.fee, resource]);
            let second = CursorScopes { resource, fee };
            (second, [first.both(), second.both()].concat())
        }
        1 => {
            let free = |scope: &[u8; 32]| !taken.contains(&cost_cursor_bucket(scope));
            let resource = retarget(c, free, &first.both());
            let fee = retarget(d, free, &[first.resource, first.fee, resource]);
            (CursorScopes { resource, fee }, Vec::new())
        }
        2 | 5 => {
            let bucket = cost_cursor_bucket(&chosen);
            let neighbour = retarget(
                c,
                |scope| cost_cursor_bucket(scope) == bucket,
                &first.both(),
            );
            let existing = match kind {
                2 => Vec::new(),
                _ => vec![chosen],
            };
            (second_with(neighbour, d), existing)
        }
        3 => {
            let second = second_with(chosen, c);
            (second, [first.both(), second.both()].concat())
        }
        4 => (second_with(chosen, c), Vec::new()),
        _ => {
            let leaf = cost_cursor_leaf(&chosen);
            let neighbour = retarget(c, |scope| cost_cursor_leaf(scope) == leaf, &first.both());
            (second_with(neighbour, d), vec![chosen])
        }
    };
    ConflictCase {
        kind,
        first,
        second,
        existing: existing.into_iter().collect(),
    }
}

fn conflict_case() -> impl proptest::strategy::Strategy<Value = ConflictCase> {
    use proptest::prelude::*;
    let kinds = u8::try_from(CONFLICT_CASE_KINDS).expect("the kind count fits in a byte");
    (any::<[[u8; 32]; 4]>(), 0..kinds, 0u8..4)
        .prop_map(|(seeds, kind, pairing)| build_case(seeds, kind, pairing))
}

/// The settlements that create the existing cursors before the two
/// settlements under test. They take the existing scopes two at a time. A
/// single remaining scope pairs with a filler scope that no settlement under
/// test touches.
fn setup_scopes(case: &ConflictCase) -> Vec<CursorScopes> {
    let existing: Vec<[u8; 32]> = case.existing.iter().copied().collect();
    let mut avoid = Vec::with_capacity(4 + existing.len());
    avoid.extend(case.first.both());
    avoid.extend(case.second.both());
    avoid.extend(existing.iter().copied());
    existing
        .chunks(2)
        .map(|scopes| match scopes {
            [resource, fee] => CursorScopes {
                resource: *resource,
                fee: *fee,
            },
            [single] => CursorScopes {
                resource: *single,
                fee: retarget(rehash(single), any_scope, &avoid),
            },
            _ => panic!("chunks(2) yields one or two scopes"),
        })
        .collect()
}

/// A settlement of `scopes`. It expects each cursor at revision 0, or at
/// revision 1 when the cursor exists. `salt` gives each settlement its own
/// reservation and random state.
fn settlement(
    payer: &VaultAddress,
    recipient: &VaultAddress,
    scopes: &CursorScopes,
    existing: &std::collections::BTreeSet<[u8; 32]>,
    salt: u8,
) -> ApplyPhloCostDeploy {
    let count = NonZeroUsize::new(2).expect("two is not zero");
    let expected = |scope: &[u8; 32]| match existing.contains(scope) {
        true => MonetaryCursor::new(1, 1, count).expect("a cursor at revision 1"),
        false => MonetaryCursor::new(0, 0, count).expect("a cursor at revision 0"),
    };
    ApplyPhloCostDeploy::new(
        ApplyCostDeploy::new(
            [salt; 32],
            vec![VaultAllocation::new(payer.to_base58(), 5).expect("an allocation")],
            vec![VaultSettlement::new(payer.to_base58(), 2, 1).expect("a settlement")],
            recipient.to_base58(),
            Blake2b512Random::create_from_bytes(&[salt, 0x19]),
        )
        .expect("a cost deploy"),
        Some(
            MonetaryCursorTransition::new(scopes.resource, expected(&scopes.resource), 1, count)
                .expect("a resource transition"),
        ),
        Some(
            MonetaryCursorTransition::new(scopes.fee, expected(&scopes.fee), 1, count)
                .expect("a fee transition"),
        ),
        count,
    )
    .expect("a phlo cost deploy")
}

/// Plays `request` from `root`. Returns the post-state and the event-log index
/// of the settlement, built as `block_index::new` builds it for a system
/// deploy.
async fn play_indexed(
    manager: &RuntimeManager,
    runtime: &mut RuntimeOps,
    root: &StateHash,
    request: &mut ApplyPhloCostDeploy,
) -> (
    StateHash,
    rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex,
) {
    use models::rust::casper::protocol::casper_message::ProcessedSystemDeploy;
    use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;

    let SystemDeployResult::PlaySucceeded {
        state_hash,
        processed_system_deploy,
        mergeable_channels,
        ..
    } = runtime
        .play_system_deploy(root, request)
        .await
        .expect("the settlement plays")
    else {
        panic!("the settlement fails");
    };
    let ProcessedSystemDeploy::Succeeded { event_list, .. } = processed_system_deploy else {
        panic!("a succeeded settlement records a succeeded system deploy");
    };
    let pre_state = Blake2b256Hash::from_bytes_prost(root);
    let diff = manager
        .convert_number_channels_to_diff(vec![mergeable_channels], &pre_state)
        .expect("the number channels convert")
        .pop()
        .expect("one diff for one settlement");
    let index = casper::rust::merging::block_index::create_event_log_index(
        &event_list,
        manager.history_repo.clone(),
        &pre_state,
        diff,
    );
    (state_hash, index)
}

/// Creates the existing cursors, plays both settlements from the resulting
/// state, and returns the verdict of dev's conflict test on the two logs.
async fn observe_conflict(
    manager: &RuntimeManager,
    runtime: &tokio::sync::Mutex<RuntimeOps>,
    genesis_root: &StateHash,
    parties: &[(VaultAddress, VaultAddress); 2],
    case: &ConflictCase,
) -> bool {
    let mut runtime = runtime.lock().await;
    let mut root = genesis_root.clone();
    let fresh = std::collections::BTreeSet::new();
    let [(first_payer, first_recipient), (second_payer, second_recipient)] = parties;
    for (index, setup) in setup_scopes(case).iter().enumerate() {
        let salt = 0x40 + u8::try_from(index).expect("at most two setup settlements");
        let mut request = settlement(first_payer, first_recipient, setup, &fresh, salt);
        root = play_indexed(manager, &mut runtime, &root, &mut request)
            .await
            .0;
    }
    let mut first_request = settlement(
        first_payer,
        first_recipient,
        &case.first,
        &case.existing,
        0x11,
    );
    let (_, first) = play_indexed(manager, &mut runtime, &root, &mut first_request).await;
    let mut second_request = settlement(
        second_payer,
        second_recipient,
        &case.second,
        &case.existing,
        0x22,
    );
    let (_, second) = play_indexed(manager, &mut runtime, &root, &mut second_request).await;
    rspace_plus_plus::rspace::merger::merging_logic::are_conflicting(&first, &second)
}

/// DR-119 (bug 11004): dev's conflict test on two settlement event logs agrees
/// with the bucket rule in 256 generated cases. The payers are the DEFAULT_SEC
/// and DEFAULT_SEC2 vaults. The recipients are two other genesis vaults, so no
/// settlement creates a vault. A tally of the verdicts per kind shows that
/// every kind ran.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn settlement_conflict_iff_same_scope_or_same_bucket_first_use() {
    with_runtime_manager(|manager, genesis, block| async move {
        let genesis_root = block.body.state.post_state_hash;
        let vault = |index: usize| {
            VaultAddress::from_public_key(&genesis.genesis_vaults[index].1)
                .expect("a genesis key forms a vault address")
        };
        let parties = [(vault(0), vault(2)), (vault(1), vault(3))];
        let runtime = tokio::sync::Mutex::new(RuntimeOps::new(
            manager.spawn_runtime().await.expect("a runtime spawns"),
        ));
        let handle = tokio::runtime::Handle::current();
        let tally = std::sync::Mutex::new([[0u32; 2]; CONFLICT_CASE_KINDS]);
        let mut runner = proptest::test_runner::TestRunner::new(proptest::test_runner::Config {
            cases: 256,
            failure_persistence: None,
            max_shrink_iters: 64,
            ..proptest::test_runner::Config::default()
        });
        runner
            .run(&conflict_case(), |case| {
                let observed = tokio::task::block_in_place(|| {
                    handle.block_on(observe_conflict(
                        &manager,
                        &runtime,
                        &genesis_root,
                        &parties,
                        &case,
                    ))
                });
                tally.lock().expect("the tally lock is not poisoned")[usize::from(case.kind)]
                    [usize::from(observed)] += 1;
                proptest::prop_assert_eq!(observed, expected_conflict(&case), "case: {:?}", case);
                Ok(())
            })
            .expect("settlements conflict exactly by the DR-119 rule");
        let tally = tally.into_inner().expect("the tally lock is not poisoned");
        println!("DR-119 verdicts per kind as [no conflict, conflict]: {tally:?}");
        for (kind, verdicts) in tally.iter().enumerate() {
            assert!(
                verdicts.iter().sum::<u32>() > 0,
                "case kind {kind} never ran: {tally:?}"
            );
        }
    })
    .await
    .expect("the runtime manager runs");
}
