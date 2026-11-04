use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crypto::rust::hash::blake2b256::Blake2b256;

use super::comm_ledger::Surface;
use super::rev_stack::seal::SealRecord;
use super::rev_stack::{RevAmount, WalletOwner};

pub const STEP_DOMAIN: &[u8] = b"f1r3fly/step/v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StepSource {
    Evaluation = 1,
    Restore = 2,
    Comm = 3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RefusalKind {
    Charge = 1,
    Cell = 2,
}

impl RefusalKind {
    pub fn tag(self) -> u32 { self as u32 }

    pub fn from_tag(tag: u32) -> Option<Self> {
        match tag {
            1 => Some(RefusalKind::Charge),
            2 => Some(RefusalKind::Cell),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Refusal {
    pub surface: Surface,
    pub step: Vec<u8>,
    pub kind: RefusalKind,
    pub drawn: RevAmount,
    pub charge: RevAmount,
}

pub trait Funding: Send + Sync {
    fn gates(&self, record: &SealRecord) -> bool;

    fn limit(&self, wallet: &WalletOwner) -> RevAmount;
}

pub fn step_base(source: StepSource, content: &[u8], random_state: &[u8]) -> Vec<u8> {
    let mut input = Vec::with_capacity(STEP_DOMAIN.len() + 9 + content.len() + random_state.len());
    input.extend_from_slice(STEP_DOMAIN);
    input.push(source as u8);
    input.extend_from_slice(&(content.len() as u32).to_be_bytes());
    input.extend_from_slice(content);
    input.extend_from_slice(&(random_state.len() as u32).to_be_bytes());
    input.extend_from_slice(random_state);
    Blake2b256::hash(input)
}

pub fn step_id(base: &[u8], position: u64) -> Vec<u8> {
    let mut input = Vec::with_capacity(base.len() + 8);
    input.extend_from_slice(base);
    input.extend_from_slice(&position.to_be_bytes());
    Blake2b256::hash(input)
}

pub struct StepScope {
    base: Vec<u8>,
    next: AtomicU64,
    prepaid: bool,
}

impl StepScope {
    pub fn new(base: Vec<u8>) -> Arc<Self> {
        Arc::new(StepScope {
            base,
            next: AtomicU64::new(0),
            prepaid: false,
        })
    }

    pub fn prepaid(base: Vec<u8>) -> Arc<Self> {
        Arc::new(StepScope {
            base,
            next: AtomicU64::new(0),
            prepaid: true,
        })
    }

    fn take(&self) -> (Vec<u8>, u64) {
        (self.base.clone(), self.next.fetch_add(1, Ordering::Relaxed))
    }
}

tokio::task_local! {
    static STEP: Arc<StepScope>;
    static FOLLOW_UP: i64;
}

pub async fn following<F: Future>(phlo: i64, future: F) -> F::Output {
    FOLLOW_UP.scope(phlo, future).await
}

pub fn follow_up() -> i64 { FOLLOW_UP.try_with(|phlo| *phlo).unwrap_or(0) }

pub async fn stepping<F: Future>(scope: Arc<StepScope>, future: F) -> F::Output {
    STEP.scope(scope, future).await
}

fn next_position(record: &SealRecord) -> (Vec<u8>, u64) {
    STEP.try_with(|scope| scope.take()).unwrap_or_else(|_| {
        (
            step_base(
                StepSource::Evaluation,
                &record.owner.canonical_bytes(),
                b"unscoped",
            ),
            0,
        )
    })
}

#[cfg(test)]
fn next_step(record: &SealRecord) -> Vec<u8> {
    let (base, position) = next_position(record);
    step_id(&base, position)
}

pub fn in_step_scope() -> bool { STEP.try_with(|_| ()).is_ok() }

pub fn settling<R>(f: impl FnOnce() -> R) -> R {
    STEP.sync_scope(StepScope::prepaid(Vec::new()), f)
}

fn prepaid() -> bool { STEP.try_with(|scope| scope.prepaid).unwrap_or(false) }

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Admission {
    Ungated,
    Admitted,
    Refused,
}

#[derive(Default)]
struct LedgerState {
    drawn: BTreeMap<Vec<u8>, RevAmount>,
    stopped: BTreeSet<Vec<u8>>,
}

#[derive(Default)]
pub struct BlockLedger {
    state: Mutex<LedgerState>,
}

impl BlockLedger {
    pub fn new() -> Arc<Self> { Arc::new(BlockLedger::default()) }

    pub fn drawn(&self) -> BTreeMap<Vec<u8>, RevAmount> {
        self.state
            .lock()
            .map(|s| s.drawn.clone())
            .unwrap_or_default()
    }
}

#[derive(Default)]
struct RunState {
    drawn: BTreeMap<Vec<u8>, RevAmount>,
    refusals: BTreeMap<Vec<u8>, Refusal>,
    reached: BTreeMap<Vec<u8>, RevAmount>,
}

pub struct ReplayRefusals {
    steps: BTreeSet<Vec<u8>>,
    owners: BTreeSet<Vec<u8>>,
}

pub struct MembraneMeter {
    funding: Arc<dyn Funding>,
    ledger: Arc<BlockLedger>,
    replay: Option<ReplayRefusals>,
    run: Mutex<RunState>,
}

pub struct Metered {
    pub refusals: Vec<Refusal>,
    pub drawn: BTreeMap<Vec<u8>, RevAmount>,
    pub reached: BTreeMap<Vec<u8>, RevAmount>,
}

fn add(map: &mut BTreeMap<Vec<u8>, RevAmount>, key: &[u8], amount: RevAmount) {
    let entry = map.entry(key.to_vec()).or_insert(0);
    *entry = entry.saturating_add(amount);
}

impl MembraneMeter {
    pub fn play(funding: Arc<dyn Funding>, ledger: Arc<BlockLedger>) -> Arc<Self> {
        Arc::new(MembraneMeter {
            funding,
            ledger,
            replay: None,
            run: Mutex::new(RunState::default()),
        })
    }

    pub fn replay(
        funding: Arc<dyn Funding>,
        ledger: Arc<BlockLedger>,
        logged: &[Refusal],
    ) -> Arc<Self> {
        Arc::new(MembraneMeter {
            funding,
            ledger,
            replay: Some(ReplayRefusals {
                steps: logged.iter().map(|r| r.step.clone()).collect(),
                owners: logged
                    .iter()
                    .flat_map(|r| {
                        let mut keys = vec![r.surface.owner.clone()];
                        if let Some(owner) = WalletOwner::from_canonical_bytes(&r.surface.owner) {
                            keys.extend(
                                super::rev_stack::seal::purse_of(&owner)
                                    .iter()
                                    .map(WalletOwner::canonical_bytes),
                            );
                        }
                        keys
                    })
                    .collect(),
            }),
            run: Mutex::new(RunState::default()),
        })
    }

    pub fn gates(&self, record: &SealRecord) -> bool { self.funding.gates(record) }

    pub fn is_replay(&self) -> bool { self.replay.is_some() }

    pub fn admit_charge(&self, record: &SealRecord, phlo: i64) -> Admission {
        if phlo <= 0 || !self.funding.gates(record) {
            return Admission::Ungated;
        }
        if prepaid() {
            return Admission::Admitted;
        }
        let charge = (phlo as u64).saturating_mul(record.price());
        let (base, position) = next_position(record);
        let step = || step_id(&base, position);
        let mut ledger = match self.ledger.state.lock() {
            Ok(ledger) => ledger,
            Err(_) => return Admission::Refused,
        };
        let mut run = match self.run.lock() {
            Ok(run) => run,
            Err(_) => return Admission::Refused,
        };
        let wallets = record.purse();
        if let Some(logged) = &self.replay {
            let watched = wallets
                .iter()
                .any(|w| logged.owners.contains(&w.canonical_bytes()));
            if watched {
                let step = step();
                if logged.steps.contains(&step) {
                    run.reached.insert(step, charge);
                    return Admission::Refused;
                }
            }
        } else if let Some((wallet, drawn)) = self.short(
            &ledger,
            &wallets
                .iter()
                .map(|w| (w.clone(), charge))
                .collect::<Vec<_>>(),
        ) {
            let step = step();
            run.refusals.entry(step.clone()).or_insert(Refusal {
                surface: Surface::of(record),
                step,
                kind: RefusalKind::Charge,
                drawn,
                charge,
            });
            ledger.stopped.insert(wallet);
            return Admission::Refused;
        }
        for wallet in &wallets {
            add(&mut ledger.drawn, &wallet.canonical_bytes(), charge);
            add(&mut run.drawn, &wallet.canonical_bytes(), charge);
        }
        Admission::Admitted
    }

    fn short(
        &self,
        ledger: &LedgerState,
        needs: &[(WalletOwner, RevAmount)],
    ) -> Option<(Vec<u8>, RevAmount)> {
        let mut total: BTreeMap<Vec<u8>, RevAmount> = BTreeMap::new();
        for (wallet, need) in needs {
            add(&mut total, &wallet.canonical_bytes(), *need);
        }
        needs.iter().find_map(|(wallet, _)| {
            let key = wallet.canonical_bytes();
            let drawn = ledger.drawn.get(&key).copied().unwrap_or(0);
            let need = total.get(&key).copied().unwrap_or(0);
            (ledger.stopped.contains(&key)
                || drawn.saturating_add(need) > self.funding.limit(wallet))
            .then_some((key, drawn))
        })
    }

    fn comm_needs(
        &self,
        records: &[SealRecord],
        trigger: Option<&SealRecord>,
        follow_up: i64,
    ) -> Vec<(SealRecord, RevAmount)> {
        let mut needs: Vec<(SealRecord, RevAmount)> = Vec::new();
        for record in records {
            if self.funding.gates(record)
                && !needs
                    .iter()
                    .any(|(r, _)| Surface::of(r) == Surface::of(record))
            {
                needs.push((record.clone(), record.price()));
            }
        }
        if let Some(trigger) = trigger.filter(|t| self.funding.gates(t) && follow_up > 0) {
            let extra = (follow_up as u64).saturating_mul(trigger.price());
            match needs
                .iter_mut()
                .find(|(r, _)| Surface::of(r) == Surface::of(trigger))
            {
                Some((_, need)) => *need = need.saturating_add(extra),
                None => needs.push((trigger.clone(), extra)),
            }
        }
        needs.sort_by_key(|(r, _)| Surface::of(r));
        needs
    }

    pub fn admit_comm(
        &self,
        records: &[SealRecord],
        trigger: Option<&SealRecord>,
        follow_up: i64,
        base: impl FnOnce() -> Vec<u8>,
    ) -> bool {
        if self.replay.is_some() {
            return true;
        }
        let needs = self.comm_needs(records, trigger, follow_up);
        if needs.is_empty() {
            return true;
        }
        let mut ledger = match self.ledger.state.lock() {
            Ok(ledger) => ledger,
            Err(_) => return false,
        };
        let mut run = match self.run.lock() {
            Ok(run) => run,
            Err(_) => return false,
        };
        let per_wallet: Vec<(WalletOwner, RevAmount)> = needs
            .iter()
            .flat_map(|(r, need)| r.purse().into_iter().map(move |w| (w, *need)))
            .collect();
        if self.short(&ledger, &per_wallet).is_none() {
            for (record, need) in &needs {
                for wallet in record.purse() {
                    add(&mut ledger.drawn, &wallet.canonical_bytes(), *need);
                    add(&mut run.drawn, &wallet.canonical_bytes(), *need);
                }
            }
            return true;
        }
        let base = base();
        for (i, (record, need)) in needs.iter().enumerate() {
            let wallets: Vec<(WalletOwner, RevAmount)> =
                record.purse().into_iter().map(|w| (w, *need)).collect();
            let Some((wallet, drawn)) = self.short(&ledger, &wallets).or_else(|| {
                let shared: Vec<(WalletOwner, RevAmount)> = per_wallet
                    .iter()
                    .filter(|(w, _)| wallets.iter().any(|(x, _)| x == w))
                    .cloned()
                    .collect();
                self.short(&ledger, &shared)
            }) else {
                continue;
            };
            let step = step_id(&base, i as u64);
            run.refusals.entry(step.clone()).or_insert(Refusal {
                surface: Surface::of(record),
                step,
                kind: RefusalKind::Cell,
                drawn,
                charge: *need,
            });
            ledger.stopped.insert(wallet);
        }
        false
    }

    pub fn draw_comm_in_replay(
        &self,
        records: &[SealRecord],
        trigger: Option<&SealRecord>,
        follow_up: i64,
    ) {
        if self.replay.is_none() {
            return;
        }
        let needs = self.comm_needs(records, trigger, follow_up);
        if let (Ok(mut ledger), Ok(mut run)) = (self.ledger.state.lock(), self.run.lock()) {
            for (record, need) in &needs {
                for wallet in record.purse() {
                    add(&mut ledger.drawn, &wallet.canonical_bytes(), *need);
                    add(&mut run.drawn, &wallet.canonical_bytes(), *need);
                }
            }
        }
    }

    pub fn take(&self) -> Metered {
        let run = std::mem::take(&mut *self.run.lock().expect("membrane meter lock"));
        Metered {
            refusals: run.refusals.into_values().collect(),
            drawn: run.drawn,
            reached: run.reached,
        }
    }
}

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
                .map_or(0, |(_, a)| *a)
        }
    }

    fn owner(n: u8) -> WalletOwner { WalletOwner::Unforgeable(vec![n; 32]) }

    fn record(n: u8, price: u64) -> SealRecord {
        SealRecord {
            owner: owner(n),
            kind: SealKind::Process,
            origin: Origin {
                envelope: vec![n; 64],
                signers: Vec::new(),
                phlo_price: price,
            },
        }
    }

    fn funding() -> Arc<dyn Funding> { Arc::new(Fixed(vec![(owner(1), 10), (owner(2), 100)])) }

    fn scope() -> Arc<StepScope> {
        StepScope::new(step_base(StepSource::Evaluation, b"term", b"rand"))
    }

    #[tokio::test]
    async fn work_under_a_membrane_stops_at_the_first_step_it_cannot_pay() {
        let meter = MembraneMeter::play(funding(), BlockLedger::new());
        let alice = record(1, 2);
        let seen = stepping(scope(), async {
            vec![
                meter.admit_charge(&alice, 3),
                meter.admit_charge(&alice, 1),
                meter.admit_charge(&alice, 2),
                meter.admit_charge(&alice, 1),
                meter.admit_charge(&record(2, 1), 50),
                meter.admit_charge(&record(9, 1), 1_000),
                meter.admit_charge(&alice, 0),
            ]
        })
        .await;
        assert_eq!(
            seen,
            vec![
                Admission::Admitted,
                Admission::Admitted,
                Admission::Refused,
                Admission::Refused,
                Admission::Admitted,
                Admission::Ungated,
                Admission::Ungated,
            ]
        );
        let metered = meter.take();
        assert_eq!(metered.drawn.get(&owner(1).canonical_bytes()), Some(&8));
        assert_eq!(metered.drawn.get(&owner(2).canonical_bytes()), Some(&50));
        assert_eq!(metered.refusals.len(), 2);
        assert!(metered
            .refusals
            .iter()
            .all(|r| r.drawn == 8 && r.kind == RefusalKind::Charge));
        assert!(metered.refusals.iter().any(|r| r.drawn + r.charge > 10));
    }

    #[tokio::test]
    async fn one_ledger_is_shared_by_the_runs_of_a_block() {
        let ledger = BlockLedger::new();
        let first = MembraneMeter::play(funding(), ledger.clone());
        let second = MembraneMeter::play(funding(), ledger.clone());
        let alice = record(1, 1);
        let seen = stepping(scope(), async {
            [
                first.admit_charge(&alice, 6),
                second.admit_charge(&alice, 4),
                second.admit_charge(&alice, 1),
                first.admit_charge(&alice, 0),
                first.admit_charge(&alice, 1),
            ]
        })
        .await;
        assert_eq!(
            seen,
            [
                Admission::Admitted,
                Admission::Admitted,
                Admission::Refused,
                Admission::Ungated,
                Admission::Refused,
            ]
        );
        assert_eq!(ledger.drawn().get(&owner(1).canonical_bytes()), Some(&10));
        let (a, b) = (first.take(), second.take());
        assert_eq!(a.drawn.get(&owner(1).canonical_bytes()), Some(&6));
        assert_eq!(b.drawn.get(&owner(1).canonical_bytes()), Some(&4));
        assert!(a.refusals.iter().chain(&b.refusals).all(|r| r.drawn == 10));
    }

    #[tokio::test]
    async fn the_step_identity_is_the_scope_and_the_position_in_it() {
        let base = step_base(StepSource::Evaluation, b"term", b"rand");
        let ids = |scope: Arc<StepScope>| async move {
            stepping(scope, async {
                (next_step(&record(1, 1)), next_step(&record(1, 1)))
            })
            .await
        };
        let (a0, a1) = ids(StepScope::new(base.clone())).await;
        let (b0, b1) = ids(StepScope::new(base.clone())).await;
        assert_eq!((a0.clone(), a1.clone()), (b0, b1));
        assert_ne!(a0, a1);
        assert_eq!(a0, step_id(&base, 0));
        assert_ne!(base, step_base(StepSource::Restore, b"term", b"rand"));
        assert_ne!(base, step_base(StepSource::Evaluation, b"term", b"other"));
        assert_ne!(base, step_base(StepSource::Evaluation, b"terms", b"rand"));
    }

    #[tokio::test]
    async fn replay_refuses_exactly_the_logged_steps_and_counts_the_rest() {
        let alice = record(1, 2);
        let play = MembraneMeter::play(funding(), BlockLedger::new());
        stepping(scope(), async {
            for phlo in [3, 1, 2, 1] {
                play.admit_charge(&alice, phlo);
            }
        })
        .await;
        let logged = play.take().refusals;
        let ledger = BlockLedger::new();
        let replay = MembraneMeter::replay(funding(), ledger.clone(), &logged);
        let seen = stepping(scope(), async {
            [3, 1, 2, 1].map(|phlo| replay.admit_charge(&alice, phlo))
        })
        .await;
        assert_eq!(
            seen,
            [
                Admission::Admitted,
                Admission::Admitted,
                Admission::Refused,
                Admission::Refused
            ]
        );
        let metered = replay.take();
        assert!(metered.refusals.is_empty());
        assert_eq!(ledger.drawn().get(&owner(1).canonical_bytes()), Some(&8));
        assert_eq!(
            metered.reached,
            logged
                .iter()
                .map(|r| (r.step.clone(), r.charge))
                .collect::<BTreeMap<_, _>>()
        );
    }

    #[test]
    fn a_comm_is_admitted_with_its_cells_and_the_storage_it_causes() {
        let meter = MembraneMeter::play(funding(), BlockLedger::new());
        let alice = record(1, 1);
        let bob = record(2, 30);
        let free = record(9, 1_000);
        assert!(meter.admit_comm(
            &[alice.clone(), bob.clone(), free.clone()],
            None,
            0,
            || vec![1]
        ));
        assert!(meter.admit_comm(&[bob.clone()], Some(&alice), 4, || vec![2]));
        assert!(!meter.admit_comm(&[bob.clone()], Some(&alice), 6, || vec![3]));
        assert!(meter.admit_comm(&[bob.clone()], None, 0, || vec![4]));
        assert!(!meter.admit_comm(&[alice.clone()], None, 0, || vec![5]));
        let metered = meter.take();
        assert_eq!(metered.drawn.get(&owner(1).canonical_bytes()), Some(&5));
        assert_eq!(metered.drawn.get(&owner(2).canonical_bytes()), Some(&90));
        assert_eq!(metered.refusals.len(), 2);
        assert!(metered
            .refusals
            .iter()
            .all(|r| r.surface == Surface::of(&alice)
                && r.kind == RefusalKind::Cell
                && r.drawn == 5));
        let ledger = BlockLedger::new();
        let replay = MembraneMeter::replay(funding(), ledger.clone(), &metered.refusals);
        assert!(replay.admit_comm(&[bob.clone()], Some(&alice), 6, || vec![3]));
        replay.draw_comm_in_replay(&[alice.clone(), alice.clone(), free], Some(&alice), 4);
        assert_eq!(ledger.drawn().get(&owner(1).canonical_bytes()), Some(&5));
    }

    #[tokio::test]
    async fn prepaid_storage_is_never_refused_and_draws_nothing() {
        let meter = MembraneMeter::play(funding(), BlockLedger::new());
        let alice = record(1, 2);
        let restore = StepScope::prepaid(step_base(StepSource::Restore, b"", b"item"));
        let seen = stepping(restore, async {
            [
                meter.admit_charge(&alice, 100),
                meter.admit_charge(&alice, 100),
            ]
        })
        .await;
        assert_eq!(seen, [Admission::Admitted, Admission::Admitted]);
        let settled = settling(|| meter.admit_charge(&alice, 100));
        assert_eq!(settled, Admission::Admitted);
        let metered = meter.take();
        assert!(metered.drawn.is_empty() && metered.refusals.is_empty());
    }

    #[tokio::test]
    async fn the_follow_up_is_scoped() {
        assert_eq!(follow_up(), 0);
        assert_eq!(following(7, async { follow_up() }).await, 7);
    }
}
