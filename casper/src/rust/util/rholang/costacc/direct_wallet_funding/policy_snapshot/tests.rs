use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use models::rhoapi::Par;
use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use proptest::prelude::*;
use rholang::rust::interpreter::util::vault_address::VaultAddress;

use super::*;
use crate::rust::util::rholang::acceptance::MonetaryCursorRead;
use crate::rust::util::rholang::supply::PurseInventory;

fn captured_context_result(
    minimum: u64,
    version: u64,
    shard: &[u8],
    adopted: &CasperShardConf,
) -> Result<(), DirectWalletPolicySnapshotError> {
    use rholang::rust::interpreter::accounting::phlo_controls::{
        check_phlo_controls, PhloEnvironment, PhloSchedule, SignedPhloControls,
    };

    let schedule = PhloSchedule {
        commitment: [1; 32],
        environment: PhloEnvironment {
            protocol_version: version,
            network: b"test",
            shard,
            asset: b"REV",
            unit: b"phlo",
            decimal_scale: 8,
        },
        weights: &[1],
        actual_price: u64::MAX,
    };
    let schedules = [schedule];
    let controls = check_phlo_controls(
        schedule.environment,
        minimum,
        u64::MAX,
        SignedPhloControls {
            limit: 0,
            price_ceiling: u64::MAX,
            required_owner_ceilings: &[u64::MAX],
            permitted_schedules: &schedules,
        },
        schedule,
        0,
    )
    .unwrap();
    check_adopted_chain_context(controls, adopted)
}

#[test]
fn adopted_chain_context_rejects_each_mismatch_and_invalid_adoption() {
    for minimum in [0, 1, i64::MAX] {
        for version in [0, 6, i64::MAX] {
            let adopted = CasperShardConf {
                min_phlo_price: minimum,
                casper_version: version,
                shard_name: "root/子".into(),
                ..CasperShardConf::new()
            };
            let (minimum, version) = (minimum as u64, version as u64);
            assert!(captured_context_result(
                minimum,
                version,
                adopted.shard_name.as_bytes(),
                &adopted
            )
            .is_ok());
            assert!(matches!(
                captured_context_result(
                    minimum + 1,
                    version,
                    adopted.shard_name.as_bytes(),
                    &adopted
                ),
                Err(DirectWalletPolicySnapshotError::ChainMinimumMismatch)
            ));
            assert!(matches!(
                captured_context_result(
                    minimum,
                    version + 1,
                    adopted.shard_name.as_bytes(),
                    &adopted
                ),
                Err(DirectWalletPolicySnapshotError::ChainVersionMismatch)
            ));
            assert!(matches!(
                captured_context_result(minimum, version, b"another-shard", &adopted),
                Err(DirectWalletPolicySnapshotError::ChainShardMismatch)
            ));
            for invalid in [
                CasperShardConf {
                    min_phlo_price: -1,
                    ..adopted.clone()
                },
                CasperShardConf {
                    casper_version: -1,
                    ..adopted.clone()
                },
                CasperShardConf {
                    shard_name: String::new(),
                    ..adopted.clone()
                },
            ] {
                assert!(matches!(
                    captured_context_result(
                        minimum,
                        version,
                        adopted.shard_name.as_bytes(),
                        &invalid
                    ),
                    Err(DirectWalletPolicySnapshotError::InvalidChainContext)
                ));
            }
        }
    }
}

proptest! {
    #[test]
    fn genesis_minimum_matches_the_formal_equality(
        genesis_minimum in any::<u64>(), adopted_minimum in any::<i64>(),
        equal in any::<bool>(),
    ) {
        let genesis_minimum = if equal { adopted_minimum as u64 } else { genesis_minimum };
        let adopted = CasperShardConf { min_phlo_price: adopted_minimum, ..CasperShardConf::new() };
        prop_assert_eq!(check_genesis_minimum(genesis_minimum, &adopted).is_ok(),
            i128::from(genesis_minimum) == i128::from(adopted_minimum));
    }

    #[test]
    fn adopted_chain_context_matches_the_formal_equalities(
        adopted_minimum in any::<i64>(),
        adopted_version in any::<i64>(),
        shard in ".{0,32}",
        alternate_minimum in any::<u64>(),
        alternate_version in any::<u64>(),
        alternate_shard in prop::collection::vec(any::<u8>(), 0..64),
        change_minimum in any::<bool>(),
        change_version in any::<bool>(),
        change_shard in any::<bool>(),
    ) {
        let captured_minimum = if change_minimum { alternate_minimum } else { adopted_minimum as u64 };
        let captured_version = if change_version { alternate_version } else { adopted_version as u64 };
        let captured_shard = if change_shard { alternate_shard.as_slice() } else { shard.as_bytes() };
        let adopted = CasperShardConf {
            min_phlo_price: adopted_minimum,
            casper_version: adopted_version,
            shard_name: shard.clone(),
            ..CasperShardConf::new()
        };
        let expected = i128::from(captured_minimum) == i128::from(adopted_minimum)
            && i128::from(captured_version) == i128::from(adopted_version)
            && !shard.is_empty()
            && captured_shard == shard.as_bytes();
        prop_assert_eq!(captured_context_result(captured_minimum, captured_version, captured_shard, &adopted).is_ok(), expected);
    }
}

type ReadResult<'a, T> = Pin<Box<dyn Future<Output = Result<T, CasperError>> + Send + 'a>>;

struct Reader {
    values: [Option<MonetaryCursor>; 2],
    calls: AtomicUsize,
    active: AtomicUsize,
    maximum: AtomicUsize,
    changed: AtomicBool,
    change_after_read: bool,
    failure: Option<usize>,
}

impl Reader {
    fn new(values: [Option<MonetaryCursor>; 2]) -> Self {
        Self {
            values,
            calls: AtomicUsize::new(0),
            active: AtomicUsize::new(0),
            maximum: AtomicUsize::new(0),
            changed: AtomicBool::new(false),
            change_after_read: false,
            failure: None,
        }
    }
}

struct ActiveRead<'a>(&'a AtomicUsize);

impl Drop for ActiveRead<'_> {
    fn drop(&mut self) { self.0.fetch_sub(1, Ordering::SeqCst); }
}

impl SupplyReader for Reader {
    fn pre_state_root(&self) -> [u8; 32] {
        if self.changed.load(Ordering::SeqCst) {
            [2; 32]
        } else {
            [1; 32]
        }
    }

    fn read_monetary_cursor(&self, scope: [u8; 32], _: NonZeroUsize) -> MonetaryCursorRead<'_> {
        Box::pin(async move {
            let index = if scope == [3; 32] {
                0
            } else if scope == [4; 32] {
                1
            } else {
                panic!("unexpected scope")
            };
            self.calls.fetch_add(1, Ordering::SeqCst);
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            let _guard = ActiveRead(&self.active);
            self.maximum.fetch_max(active, Ordering::SeqCst);
            tokio::task::yield_now().await;
            if self.change_after_read {
                self.changed.store(true, Ordering::SeqCst);
            }
            if self.failure == Some(index) {
                return Err(CasperError::InvalidCostSettlement(
                    "cursor read failed".into(),
                ));
            }
            Ok(self.values[index])
        })
    }

    fn urn_map(&self) -> ReadResult<'_, Arc<HashMap<String, Par>>> {
        panic!("unexpected URN query")
    }
    fn read_validator_fuel<'a>(&'a self, _: &'a VaultAddress) -> ReadResult<'a, i64> {
        panic!("unexpected fuel query")
    }
    fn read_purse<'a>(&'a self, _: &'a CostSignature) -> ReadResult<'a, PurseInventory> {
        panic!("unexpected purse query")
    }
}

fn budget() -> HostWorkBudget {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(1_000_000)))
}

#[tokio::test]
async fn cursor_pair_preserves_presence_roles_and_bounded_parallelism() {
    let count = NonZeroUsize::new(3).unwrap();
    for values in [[None, Some(MonetaryCursor::INITIAL)], [
        Some(MonetaryCursor::new(7, 2, count).unwrap()),
        None,
    ]] {
        for parallel in [1, 2, usize::MAX] {
            let reader = Reader::new(values);
            assert_eq!(
                read_cursor_pair(
                    &reader,
                    [1; 32],
                    [[3; 32], [4; 32]],
                    count,
                    NonZeroUsize::new(parallel).unwrap(),
                    &budget()
                )
                .await
                .unwrap(),
                values
            );
            assert_eq!(reader.calls.load(Ordering::SeqCst), 2);
            assert_eq!(reader.maximum.load(Ordering::SeqCst), parallel.min(2));
            assert_eq!(reader.active.load(Ordering::SeqCst), 0);
        }
    }
}

#[tokio::test]
async fn cursor_pair_rejects_wrong_roots_collisions_bad_positions_and_read_failures() {
    let count = NonZeroUsize::new(2).unwrap();
    let reader = Reader::new([None, None]);
    assert!(matches!(
        read_cursor_pair(
            &reader,
            [9; 32],
            [[3; 32], [4; 32]],
            count,
            count,
            &budget()
        )
        .await,
        Err(DirectWalletPolicySnapshotError::Wallet(
            DirectWalletSnapshotError::WrongPreState
        ))
    ));
    assert!(matches!(
        read_cursor_pair(
            &reader,
            [1; 32],
            [[3; 32], [3; 32]],
            count,
            count,
            &budget()
        )
        .await,
        Err(DirectWalletPolicySnapshotError::ScopeCollision)
    ));
    assert_eq!(reader.calls.load(Ordering::SeqCst), 0);
    let mut changed = Reader::new([None, None]);
    changed.change_after_read = true;
    assert!(matches!(
        read_cursor_pair(
            &changed,
            [1; 32],
            [[3; 32], [4; 32]],
            count,
            count,
            &budget()
        )
        .await,
        Err(DirectWalletPolicySnapshotError::Wallet(
            DirectWalletSnapshotError::WrongPreState
        ))
    ));
    for index in 0..2 {
        let mut failed = Reader::new([None, None]);
        failed.failure = Some(index);
        assert!(matches!(
            read_cursor_pair(
                &failed,
                [1; 32],
                [[3; 32], [4; 32]],
                count,
                count,
                &budget()
            )
            .await,
            Err(DirectWalletPolicySnapshotError::Read(_))
        ));
        assert_eq!(failed.active.load(Ordering::SeqCst), 0);
        let mut values = [None, None];
        values[index] = Some(MonetaryCursor::new(0, 2, NonZeroUsize::new(3).unwrap()).unwrap());
        assert!(matches!(
            read_cursor_pair(
                &Reader::new(values),
                [1; 32],
                [[3; 32], [4; 32]],
                count,
                count,
                &budget()
            )
            .await,
            Err(DirectWalletPolicySnapshotError::Cursor(_))
        ));
    }
}

#[tokio::test]
async fn cursor_pair_reserves_both_reads_before_querying() {
    let count = NonZeroUsize::new(2).unwrap();
    for dimension in [
        HostWorkDimension::SearchCandidates,
        HostWorkDimension::VerificationOperations,
    ] {
        for limit in [0, 1, 2] {
            let reader = Reader::new([None, Some(MonetaryCursor::INITIAL)]);
            let mut limits = budget().limits();
            limits.set(dimension, HostWorkLimit::new(limit));
            let result = read_cursor_pair(
                &reader,
                [1; 32],
                [[3; 32], [4; 32]],
                count,
                count,
                &HostWorkBudget::new(limits),
            )
            .await;
            if limit == 2 {
                assert_eq!(result.unwrap(), reader.values);
                assert_eq!(reader.calls.load(Ordering::SeqCst), 2);
            } else {
                assert!(matches!(
                    result,
                    Err(DirectWalletPolicySnapshotError::HostWork(_))
                ));
                assert_eq!(reader.calls.load(Ordering::SeqCst), 0);
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn cursor_pair_keeps_generated_revisions_and_absence(
        count in 1_usize..130, resource in prop::option::of((0_i64..i64::MAX, any::<usize>())),
        fee in prop::option::of((0_i64..i64::MAX, any::<usize>())), parallel in 1_usize..4,
    ) {
        let n = NonZeroUsize::new(count).unwrap();
        let values = [resource, fee].map(|v| v.map(|(revision, position)|
            MonetaryCursor::new(revision, (position % count) as i64, n).unwrap()));
        let reader = Reader::new(values);
        let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let actual = runtime.block_on(read_cursor_pair(&reader, [1; 32], [[3; 32], [4; 32]], n,
            NonZeroUsize::new(parallel).unwrap(), &budget())).unwrap();
        prop_assert_eq!(actual, values);
    }

    #[test]
    fn canonical_scope_hashes_preserve_permutations_and_separate_roles(
        count in 1_usize..130, rotation in any::<usize>(), reverse in any::<bool>(),
    ) {
        let keys: Vec<[u8; 32]> = (0..count).map(|index| {
            let mut key = [0; 32];
            key[..8].copy_from_slice(&(index as u64).to_be_bytes());
            key
        }).collect();
        let mut shuffled: Vec<&[u8]> = keys.iter().map(|key| key.as_slice()).collect();
        shuffled.rotate_left(rotation % count);
        if reverse { shuffled.reverse(); }
        let order = canonical_funding_key_order(&shuffled, &budget()).unwrap();
        let sorted: Vec<&[u8; 32]> = order.iter().map(|index| shuffled[*index].try_into().unwrap()).collect();
        let resource = monetary_scope_for_custodies(&resource_policy_context(), sorted.iter().copied());
        let fee = monetary_scope_for_custodies(&native_fee_policy_context(), sorted.iter().copied());
        prop_assert_ne!(resource, fee);
        prop_assert_eq!(resource, monetary_scope_for_custodies(&resource_policy_context(), keys.iter()));
        prop_assert_eq!(fee, monetary_scope_for_custodies(&native_fee_policy_context(), keys.iter()));
    }
}
