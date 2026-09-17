use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use casper::rust::errors::CasperError;
use casper::rust::util::rholang::acceptance::{MonetaryCursorRead, SupplyReader};
use casper::rust::util::rholang::costacc::direct_wallet_funding::DirectWalletSnapshotError;
use casper::rust::util::rholang::supply::{PurseInventory, PurseStack};
use models::rhoapi::{CostStack, Par};
use rholang::rust::interpreter::util::vault_address::VaultAddress;

use super::*;

type ReadResult<'a, T> = Pin<Box<dyn Future<Output = Result<T, CasperError>> + Send + 'a>>;

pub(super) struct Reader {
    balances: Vec<Option<i64>>,
    calls: AtomicUsize,
    active: AtomicUsize,
    maximum: AtomicUsize,
    changed: AtomicBool,
    change_after_read: bool,
    failure: Option<usize>,
    stacks: Vec<PurseStack>,
}

struct ActiveRead<'a>(&'a AtomicUsize);
impl Drop for ActiveRead<'_> {
    fn drop(&mut self) { self.0.fetch_sub(1, Ordering::SeqCst); }
}

impl Reader {
    pub(super) fn new(balances: Vec<Option<i64>>) -> Self {
        Self {
            balances,
            calls: AtomicUsize::new(0),
            active: AtomicUsize::new(0),
            maximum: AtomicUsize::new(0),
            changed: AtomicBool::new(false),
            change_after_read: false,
            failure: None,
            stacks: vec![],
        }
    }
}

impl SupplyReader for Reader {
    fn pre_state_root(&self) -> [u8; 32] {
        if self.changed.load(Ordering::SeqCst) {
            [2; 32]
        } else {
            [1; 32]
        }
    }
    fn read_monetary_cursor(&self, _: [u8; 32], _: NonZeroUsize) -> MonetaryCursorRead<'_> {
        panic!("wallet snapshot must not read a monetary cursor")
    }
    fn urn_map(&self) -> ReadResult<'_, Arc<HashMap<String, Par>>> {
        panic!("wallet snapshot must not read the URN map")
    }
    fn read_validator_fuel<'a>(&'a self, _: &'a VaultAddress) -> ReadResult<'a, i64> {
        panic!("general wallet snapshot must not read validator fuel")
    }
    fn read_purse<'a>(&'a self, signature: &'a CostSignature) -> ReadResult<'a, PurseInventory> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            let _guard = ActiveRead(&self.active);
            self.maximum.fetch_max(active, Ordering::SeqCst);
            let payer = vault_payer(signature).unwrap();
            let index = (0..self.balances.len())
                .find(|index| custody(*index).as_slice() == payer.custody_key)
                .expect("only authorized wallets are queried");
            for _ in 0..(self.balances.len() - index) {
                tokio::task::yield_now().await;
            }
            if self.change_after_read {
                self.changed.store(true, Ordering::SeqCst);
            }
            if self.failure == Some(index) {
                return Err(CasperError::InvalidCostSettlement(
                    "test read failure".to_string(),
                ));
            }
            Ok(PurseInventory {
                balance: self.balances[index],
                stacks: self.stacks.clone(),
            })
        })
    }
}

#[tokio::test]
async fn bounded_parallel_reads_preserve_source_identity_and_consent() {
    let balances = vec![Some(3), None, Some(i64::MAX), Some(17)];
    let signed = envelope(&[true; 4], &(0..4).map(custody).collect::<Vec<_>>());
    for parallel in [1, 2, usize::MAX] {
        let reader = Reader::new(balances.clone());
        let authorized = authorize_direct_wallet_funding(&signed, authorization_limits()).unwrap();
        let snapshot = authorized
            .read_snapshot(&reader, [1; 32], NonZeroUsize::new(parallel).unwrap())
            .await
            .unwrap();
        assert_eq!(snapshot.pre_state_root(), [1; 32]);
        assert_eq!(snapshot.sources().len(), 4);
        assert!(std::ptr::eq(snapshot.authorization().envelope(), &signed));
        assert_eq!(reader.calls.load(Ordering::SeqCst), 4);
        assert_eq!(reader.active.load(Ordering::SeqCst), 0);
        assert_eq!(reader.maximum.load(Ordering::SeqCst), parallel.min(4));
        let offered = offered_envelope(&[true; 4], &(0..4).map(custody).collect::<Vec<_>>(), 10, 1);
        let offered_reader = Reader::new(balances.clone());
        let offered_authorized =
            authorize_offered_direct_wallet_funding(&offered, authorization_limits()).unwrap();
        let offered_snapshot = offered_authorized
            .read_snapshot(
                &offered_reader,
                [1; 32],
                NonZeroUsize::new(parallel).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(offered_snapshot.sources(), snapshot.sources());
        assert_eq!(
            offered_reader.maximum.load(Ordering::SeqCst),
            parallel.min(4)
        );
        assert!(std::ptr::eq(
            offered_snapshot.authorization().envelope(),
            &offered
        ));
        for (index, source) in snapshot.sources().iter().enumerate() {
            assert_eq!(source.source().custody, custody(index));
            assert_eq!(
                source.source().capacity,
                balances[index].unwrap_or(0) as u64
            );
            assert_eq!(source.source().exposure_limit, 100);
            assert_eq!(source.source().debit_limit, 100);
            assert_eq!(source.inventory().balance, balances[index]);
        }
    }
}

#[tokio::test]
async fn root_mismatch_and_failed_reads_return_no_snapshot() {
    let signed = envelope(&[true; 3], &(0..3).map(custody).collect::<Vec<_>>());
    let reader = Reader::new(vec![Some(7); 3]);
    let authorized = authorize_direct_wallet_funding(&signed, authorization_limits()).unwrap();
    assert!(matches!(
        authorized
            .read_snapshot(&reader, [2; 32], NonZeroUsize::new(2).unwrap())
            .await,
        Err(DirectWalletSnapshotError::WrongPreState)
    ));
    assert_eq!(reader.calls.load(Ordering::SeqCst), 0);
    for failure in 0..3 {
        let mut reader = Reader::new(vec![Some(7); 3]);
        reader.failure = Some(failure);
        let authorized = authorize_direct_wallet_funding(&signed, authorization_limits()).unwrap();
        assert!(matches!(
            authorized
                .read_snapshot(&reader, [1; 32], NonZeroUsize::new(3).unwrap())
                .await,
            Err(DirectWalletSnapshotError::Read(_))
        ));
        assert_eq!(reader.active.load(Ordering::SeqCst), 0);
    }
    let mut changed = Reader::new(vec![Some(7); 3]);
    changed.change_after_read = true;
    let authorized = authorize_direct_wallet_funding(&signed, authorization_limits()).unwrap();
    assert!(matches!(
        authorized
            .read_snapshot(&changed, [1; 32], NonZeroUsize::new(2).unwrap())
            .await,
        Err(DirectWalletSnapshotError::WrongPreState)
    ));
    let negative = Reader::new(vec![Some(7), Some(-1), Some(3)]);
    let authorized = authorize_direct_wallet_funding(&signed, authorization_limits()).unwrap();
    assert!(matches!(
        authorized
            .read_snapshot(&negative, [1; 32], NonZeroUsize::new(2).unwrap())
            .await,
        Err(DirectWalletSnapshotError::NegativeBalance)
    ));
}

#[tokio::test]
async fn offered_snapshot_rejects_inconsistent_roots_read_failures_and_negative_balances() {
    let signed = offered_envelope(&[true; 3], &(0..3).map(custody).collect::<Vec<_>>(), 10, 1);
    let authorize =
        || authorize_offered_direct_wallet_funding(&signed, authorization_limits()).unwrap();
    let reader = Reader::new(vec![Some(7); 3]);
    assert!(matches!(
        authorize()
            .read_snapshot(&reader, [2; 32], NonZeroUsize::new(2).unwrap())
            .await,
        Err(DirectWalletSnapshotError::WrongPreState)
    ));
    assert_eq!(reader.calls.load(Ordering::SeqCst), 0);
    for index in 0..3 {
        let mut reader = Reader::new(vec![Some(7); 3]);
        reader.failure = Some(index);
        assert!(matches!(
            authorize()
                .read_snapshot(&reader, [1; 32], NonZeroUsize::new(3).unwrap())
                .await,
            Err(DirectWalletSnapshotError::Read(_))
        ));
        assert_eq!(reader.active.load(Ordering::SeqCst), 0);
        let mut balances = vec![Some(7); 3];
        balances[index] = Some(-1);
        let reader = Reader::new(balances);
        assert!(matches!(
            authorize()
                .read_snapshot(&reader, [1; 32], NonZeroUsize::new(3).unwrap())
                .await,
            Err(DirectWalletSnapshotError::NegativeBalance)
        ));
        assert_eq!(reader.active.load(Ordering::SeqCst), 0);
    }
    let mut reader = Reader::new(vec![Some(7); 3]);
    reader.change_after_read = true;
    assert!(matches!(
        authorize()
            .read_snapshot(&reader, [1; 32], NonZeroUsize::new(2).unwrap())
            .await,
        Err(DirectWalletSnapshotError::WrongPreState)
    ));
}

#[tokio::test]
async fn stored_resources_remain_distinct_from_monetary_capacity() {
    let signed = envelope(&[true], &[custody(0)]);
    let mut reader = Reader::new(vec![Some(3)]);
    reader.stacks.push(PurseStack {
        instance_id: [4; 32],
        source_hash: [5; 32],
        channel: Par::default(),
        datum_index: 6,
        random_state: vec![7; 32],
        persistent: true,
        stack: CostStack {
            cells: vec![CostSignature {
                value: Some(Value::Ground(Secp256k1.to_public(&key(0)).bytes.to_vec())),
            }],
        },
    });
    let authorized = authorize_direct_wallet_funding(&signed, authorization_limits()).unwrap();
    let snapshot = authorized
        .read_snapshot(&reader, [1; 32], NonZeroUsize::new(2).unwrap())
        .await
        .unwrap();
    assert_eq!(snapshot.sources()[0].source().capacity, 3);
    assert_eq!(snapshot.sources()[0].inventory().stacks, reader.stacks);
}

#[tokio::test]
async fn an_empty_source_set_does_not_query_or_invent_capacity() {
    let signed = envelope(&[true], &[]);
    let reader = Reader::new(vec![]);
    let authorized = authorize_direct_wallet_funding(&signed, authorization_limits()).unwrap();
    let snapshot = authorized
        .read_snapshot(&reader, [1; 32], NonZeroUsize::new(usize::MAX).unwrap())
        .await
        .unwrap();
    assert!(snapshot.sources().is_empty());
    assert_eq!(reader.calls.load(Ordering::SeqCst), 0);
    assert_eq!(reader.maximum.load(Ordering::SeqCst), 0);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn generated_snapshots_match_the_fixed_state_and_parallel_bound(
        balances in prop::collection::vec(prop::option::of(0_i64..1000), 1..=12),
        concurrency in 1usize..=16,
        reverse in any::<bool>(),
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let mut order: Vec<_> = (0..balances.len()).collect();
        if reverse { order.reverse(); }
        let requested: Vec<_> = order.iter().copied().map(custody).collect();
        let signed = envelope(&vec![true; balances.len()], &requested);
        let reader = Reader::new(balances.clone());
        let authorized = authorize_direct_wallet_funding(&signed, authorization_limits()).unwrap();
        let snapshot = runtime.block_on(authorized.read_snapshot(&reader, [1; 32], NonZeroUsize::new(concurrency).unwrap())).unwrap();
        prop_assert_eq!(reader.calls.load(Ordering::SeqCst), balances.len());
        prop_assert!(reader.maximum.load(Ordering::SeqCst) <= concurrency.min(balances.len()));
        prop_assert_eq!(reader.active.load(Ordering::SeqCst), 0);
        for (source, index) in snapshot.sources().iter().zip(order) {
            prop_assert_eq!(source.source().custody, custody(index));
            prop_assert_eq!(source.source().capacity, balances[index].unwrap_or(0) as u64);
        }
    }
}
