use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use super::comm_ledger::{CommLedger, Surface};
use super::costs::comm_event_storage_cost;
use super::rev_stack::seal::{purse_of, SealRecord};
use super::rev_stack::{RevAmount, WalletOwner};
use crate::rust::interpreter::errors::InterpreterError;

pub trait Funding: Send + Sync {
    fn gates(&self, record: &SealRecord) -> bool;

    fn limit(&self, wallet: &WalletOwner) -> RevAmount;
}

pub struct MembraneMeter {
    funding: Arc<dyn Funding>,
    replay: bool,
    held: Mutex<BTreeMap<Vec<u8>, RevAmount>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Metered {
    pub held: BTreeMap<Vec<u8>, RevAmount>,
}

fn add(map: &mut BTreeMap<Vec<u8>, RevAmount>, key: &[u8], amount: RevAmount) {
    let entry = map.entry(key.to_vec()).or_insert(0);
    *entry = entry.saturating_add(amount);
}

fn priced(phlo: i64, price: u64) -> RevAmount { (phlo.max(0) as u64).saturating_mul(price) }

fn limit_of(record: &SealRecord) -> i64 { record.origin.phlo_limit.min(i64::MAX as u64) as i64 }

pub fn firing_wallets(record: &SealRecord, named: &[WalletOwner]) -> BTreeSet<Vec<u8>> {
    record
        .purse()
        .iter()
        .chain(named.iter())
        .flat_map(purse_of)
        .map(|wallet| wallet.canonical_bytes())
        .collect()
}

impl MembraneMeter {
    pub fn play(funding: Arc<dyn Funding>) -> Arc<Self> { Self::new(funding, false) }

    pub fn replay(funding: Arc<dyn Funding>) -> Arc<Self> { Self::new(funding, true) }

    fn new(funding: Arc<dyn Funding>, replay: bool) -> Arc<Self> {
        Arc::new(MembraneMeter {
            funding,
            replay,
            held: Mutex::new(BTreeMap::new()),
        })
    }

    pub fn gates(&self, record: &SealRecord) -> bool { self.funding.gates(record) }

    pub fn is_replay(&self) -> bool { self.replay }

    pub fn comm_needs(
        &self,
        continuation: Option<(&SealRecord, &[WalletOwner])>,
        data: &[SealRecord],
        channels: usize,
    ) -> BTreeMap<Vec<u8>, RevAmount> {
        let mut needs = BTreeMap::new();
        if let Some((record, named)) = continuation.filter(|(r, _)| self.gates(r)) {
            let bound = priced(limit_of(record), record.price());
            for wallet in firing_wallets(record, named) {
                add(&mut needs, &wallet, bound);
            }
            let follow = 1 + comm_event_storage_cost(channels as i64).value;
            for wallet in record.purse() {
                add(
                    &mut needs,
                    &wallet.canonical_bytes(),
                    priced(follow, record.price()),
                );
            }
        }
        for record in data.iter().filter(|r| self.gates(r)) {
            for wallet in record.purse() {
                add(&mut needs, &wallet.canonical_bytes(), record.price());
            }
        }
        needs
    }

    pub fn hold(&self, needs: &BTreeMap<Vec<u8>, RevAmount>) -> bool {
        if needs.is_empty() {
            return true;
        }
        let Ok(mut held) = self.held.lock() else {
            return false;
        };
        let fits = needs.iter().all(|(wallet, need)| {
            let Some(owner) = WalletOwner::from_canonical_bytes(wallet) else {
                return false;
            };
            held.get(wallet)
                .copied()
                .unwrap_or(0)
                .checked_add(*need)
                .is_some_and(|total| total <= self.funding.limit(&owner))
        });
        if fits {
            for (wallet, need) in needs {
                add(&mut held, wallet, *need);
            }
        }
        fits
    }

    pub fn take(&self) -> Metered {
        Metered {
            held: std::mem::take(&mut *self.held.lock().expect("membrane meter lock")),
        }
    }
}

pub type Deferred = Pin<Box<dyn Future<Output = Result<(), InterpreterError>> + Send>>;

#[derive(Default)]
struct FiringState {
    cost: i64,
    exhausted: bool,
    work: Vec<(Surface, i64)>,
    ops: Vec<Deferred>,
}

pub struct Firing {
    bound: SealRecord,
    wallets: BTreeSet<Vec<u8>>,
    limit: i64,
    state: Mutex<FiringState>,
}

pub enum Finished {
    Exhausted,
    Within(Vec<Deferred>),
}

impl Firing {
    pub fn new(payer: SealRecord, named: &[WalletOwner]) -> Arc<Self> {
        Self::bounded_by(payer, named, None)
    }

    pub fn bounded_by(
        payer: SealRecord,
        named: &[WalletOwner],
        own: Option<WalletOwner>,
    ) -> Arc<Self> {
        let bound = match own {
            Some(owner) => payer.enter(owner, payer.kind),
            None => payer.clone(),
        };
        let mut named = named.to_vec();
        named.push(bound.owner.clone());
        Arc::new(Firing {
            wallets: firing_wallets(&payer, &named),
            limit: limit_of(&payer),
            bound,
            state: Mutex::new(FiringState::default()),
        })
    }

    pub fn charge(
        &self,
        record: &SealRecord,
        phlo: i64,
        reserve: i64,
    ) -> Result<(), InterpreterError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| InterpreterError::FiringExhausted)?;
        if state.exhausted {
            return Err(InterpreterError::FiringExhausted);
        }
        let covered = record
            .purse()
            .iter()
            .all(|wallet| self.wallets.contains(&wallet.canonical_bytes()));
        state.cost = state
            .cost
            .saturating_add(phlo.max(0))
            .saturating_add(reserve.max(0));
        if !covered || state.cost > self.limit {
            state.exhausted = true;
            state.ops.clear();
            return Err(InterpreterError::FiringExhausted);
        }
        state.work.push((Surface::of(record), phlo));
        Ok(())
    }

    pub fn defer(&self, op: Deferred) {
        if let Ok(mut state) = self.state.lock() {
            if !state.exhausted {
                state.ops.push(op);
            }
        }
    }

    pub fn exhausted(&self) -> bool { self.state.lock().map(|s| s.exhausted).unwrap_or(true) }

    pub fn finish(&self, ledger: &CommLedger) -> Finished {
        let state = std::mem::take(&mut *self.state.lock().expect("firing lock"));
        if state.exhausted {
            ledger.on_work(Some(Surface::of(&self.bound)), self.limit);
            return Finished::Exhausted;
        }
        for (surface, phlo) in state.work {
            ledger.on_work(Some(surface), phlo);
        }
        Finished::Within(state.ops)
    }
}

tokio::task_local! {
    static FIRING: Arc<Firing>;
    static PRECHARGED: Arc<AtomicBool>;
    static SETTLING: ();
}

pub fn firing() -> Option<Arc<Firing>> { FIRING.try_with(Arc::clone).ok() }

pub async fn firing_scope<F: Future>(firing: Arc<Firing>, future: F) -> F::Output {
    FIRING.scope(firing, future).await
}

pub fn inherit_firing<F: Future>(future: F) -> impl Future<Output = F::Output> {
    let firing = firing();
    async move {
        match firing {
            Some(firing) => FIRING.scope(firing, future).await,
            None => future.await,
        }
    }
}

pub fn precharged<F: Future>(future: F) -> impl Future<Output = F::Output> {
    PRECHARGED.scope(Arc::new(AtomicBool::new(true)), future)
}

pub fn take_precharged() -> bool {
    PRECHARGED
        .try_with(|flag| flag.swap(false, Ordering::Relaxed))
        .unwrap_or(false)
}

pub fn settling<R>(f: impl FnOnce() -> R) -> R { SETTLING.sync_scope((), f) }

pub fn settled<F: Future>(future: F) -> impl Future<Output = F::Output> {
    SETTLING.scope((), future)
}

pub fn is_settling() -> bool { SETTLING.try_with(|_| ()).is_ok() }

static UNBOUNDED: AtomicU64 = AtomicU64::new(0);

pub fn unbounded_charges() -> u64 { UNBOUNDED.load(Ordering::Relaxed) }

pub(crate) fn count_unbounded() { UNBOUNDED.fetch_add(1, Ordering::Relaxed); }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::interpreter::accounting::rev_stack::seal::{Origin, SealKind};

    struct Fixed(Vec<(WalletOwner, RevAmount)>);

    impl Funding for Fixed {
        fn gates(&self, record: &SealRecord) -> bool {
            record.owner != WalletOwner::Unforgeable(vec![9; 32])
        }

        fn limit(&self, wallet: &WalletOwner) -> RevAmount {
            self.0
                .iter()
                .find(|(w, _)| w == wallet)
                .map(|(_, l)| *l)
                .unwrap_or(0)
        }
    }

    fn owner(n: u8) -> WalletOwner { WalletOwner::Unforgeable(vec![n; 32]) }

    fn record(n: u8, price: u64, limit: u64) -> SealRecord {
        SealRecord {
            owner: owner(n),
            kind: SealKind::Process,
            origin: Origin {
                envelope: vec![n],
                signers: vec![owner(n)],
                phlo_price: price,
                phlo_limit: limit,
            },
        }
    }

    fn follow(channels: i64) -> u64 { (1 + comm_event_storage_cost(channels).value) as u64 }

    #[test]
    fn a_firing_holds_its_bound_and_the_comm_follow_up_on_the_installer() {
        let meter = MembraneMeter::play(Arc::new(Fixed(vec![(owner(1), 1_000_000)])));
        let needs = meter.comm_needs(Some((&record(1, 2, 100), &[])), &[], 1);
        assert_eq!(
            needs.get(&owner(1).canonical_bytes()),
            Some(&(2 * (100 + follow(1))))
        );
        assert!(meter.hold(&needs));
        assert_eq!(meter.take().held, needs);
    }

    #[test]
    fn the_holds_of_a_run_never_pass_its_share_whatever_their_order() {
        let share = 3 * 2 * (100 + follow(1));
        let meter = MembraneMeter::play(Arc::new(Fixed(vec![(owner(1), share)])));
        let needs = meter.comm_needs(Some((&record(1, 2, 100), &[])), &[], 1);
        assert!(meter.hold(&needs));
        assert!(meter.hold(&needs));
        assert!(meter.hold(&needs));
        assert!(!meter.hold(&needs), "a fourth firing passes the share");
        assert_eq!(
            meter.take().held.get(&owner(1).canonical_bytes()),
            Some(&share)
        );
    }

    #[test]
    fn an_ungated_record_holds_nothing_and_a_gated_datum_holds_its_cell() {
        let meter = MembraneMeter::play(Arc::new(Fixed(vec![(owner(2), 5)])));
        let needs = meter.comm_needs(Some((&record(9, 2, 100), &[])), &[record(2, 5, 0)], 1);
        assert_eq!(needs, BTreeMap::from([(owner(2).canonical_bytes(), 5)]));
        assert!(meter.hold(&needs));
        assert!(!meter.hold(&needs));
    }

    #[test]
    fn a_wallet_that_the_stored_body_names_holds_the_bound_too() {
        let r = record(1, 1, 50);
        let meter = MembraneMeter::play(Arc::new(Fixed(vec![(owner(1), 1_000), (owner(3), 49)])));
        let needs = meter.comm_needs(Some((&r, &[owner(3)])), &[], 1);
        assert_eq!(needs.get(&owner(3).canonical_bytes()), Some(&50));
        assert!(
            !meter.hold(&needs),
            "the named wallet cannot hold the bound"
        );
        assert!(meter.take().held.is_empty());
    }

    #[test]
    fn a_firing_within_its_bound_pays_what_it_worked() {
        let firing = Firing::new(record(1, 2, 100), &[]);
        firing.charge(&record(1, 2, 100), 30, 10).unwrap();
        firing.charge(&record(1, 2, 100), 20, 0).unwrap();
        let ledger = CommLedger::default();
        assert!(matches!(firing.finish(&ledger), Finished::Within(ops) if ops.is_empty()));
        let work = ledger.take_work();
        assert_eq!(work.get(&Some(Surface::of(&record(1, 2, 100)))), Some(&50));
    }

    #[test]
    fn a_firing_that_passes_its_bound_pays_the_bound_and_keeps_nothing() {
        let firing = Firing::new(record(1, 2, 100), &[]);
        firing.charge(&record(1, 2, 100), 60, 0).unwrap();
        firing.defer(Box::pin(async { Ok(()) }));
        assert_eq!(
            firing.charge(&record(1, 2, 100), 30, 20),
            Err(InterpreterError::FiringExhausted)
        );
        assert_eq!(
            firing.charge(&record(1, 2, 100), 1, 0),
            Err(InterpreterError::FiringExhausted)
        );
        let ledger = CommLedger::default();
        assert!(matches!(firing.finish(&ledger), Finished::Exhausted));
        let work = ledger.take_work();
        assert_eq!(work.get(&Some(Surface::of(&record(1, 2, 100)))), Some(&100));
        assert_eq!(work.len(), 1);
    }

    #[test]
    fn a_firing_over_its_bound_charges_the_owner_of_the_continuations_own_seal() {
        let firing = Firing::bounded_by(record(2, 3, 100), &[], Some(owner(1)));
        let inner = record(2, 3, 100).enter(owner(1), SealKind::Signed);
        firing.charge(&inner, 80, 0).unwrap();
        assert_eq!(
            firing.charge(&inner, 30, 0),
            Err(InterpreterError::FiringExhausted)
        );
        let ledger = CommLedger::default();
        assert!(matches!(firing.finish(&ledger), Finished::Exhausted));
        let work = ledger.take_work();
        assert_eq!(work.len(), 1);
        let (surface, phlo) = work.into_iter().next().unwrap();
        assert_eq!(
            (surface.unwrap().owner, phlo),
            (owner(1).canonical_bytes(), 100)
        );
    }

    #[test]
    fn a_charge_to_a_wallet_that_the_firing_does_not_hold_exhausts_it() {
        let firing = Firing::new(record(1, 2, 100), &[]);
        assert_eq!(
            firing.charge(&record(4, 2, 100), 1, 0),
            Err(InterpreterError::FiringExhausted)
        );
        assert!(firing.exhausted());
    }

    #[test]
    fn the_exhaustion_does_not_depend_on_the_order_of_the_charges() {
        let charges = [(40, 5), (30, 0), (25, 10)];
        let exhausted = |limit: u64, order: &[usize]| {
            let firing = Firing::new(record(1, 1, limit), &[]);
            for i in order {
                let (phlo, reserve) = charges[*i];
                let _ = firing.charge(&record(1, 1, limit), phlo, reserve);
            }
            firing.exhausted()
        };
        let all = [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [
            2, 1, 0,
        ]];
        assert!(all.iter().all(|o| exhausted(100, o)));
        assert!(all.iter().all(|o| !exhausted(110, o)));
    }

    #[tokio::test]
    async fn the_firing_scope_is_inherited_and_the_precharge_is_taken_once() {
        let firing = Firing::new(record(1, 1, 10), &[]);
        let seen = firing_scope(firing.clone(), async {
            let here = super::firing().is_some();
            let inherited = tokio::spawn(inherit_firing(async { super::firing().is_some() }))
                .await
                .unwrap();
            let bare = tokio::spawn(async { super::firing().is_some() })
                .await
                .unwrap();
            (here, inherited, bare)
        })
        .await;
        assert_eq!(seen, (true, true, false));
        let taken = precharged(async { (take_precharged(), take_precharged()) }).await;
        assert_eq!(taken, (true, false));
        assert!(!take_precharged());
    }
}
