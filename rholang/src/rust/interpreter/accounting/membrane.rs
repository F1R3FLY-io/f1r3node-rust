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
    pub surface_drawn: RevAmount,
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
    restore: bool,
}

impl StepScope {
    pub fn new(base: Vec<u8>) -> Arc<Self> {
        Arc::new(StepScope {
            base,
            next: AtomicU64::new(0),
            restore: false,
        })
    }

    pub fn restoring(base: Vec<u8>) -> Arc<Self> {
        Arc::new(StepScope {
            base,
            next: AtomicU64::new(0),
            restore: true,
        })
    }

    fn take(&self) -> (Vec<u8>, u64) {
        (self.base.clone(), self.next.fetch_add(1, Ordering::Relaxed))
    }
}

tokio::task_local! {
    static STEP: Arc<StepScope>;
}

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
    STEP.sync_scope(StepScope::restoring(Vec::new()), f)
}

fn restoring() -> bool { STEP.try_with(|scope| scope.restore).unwrap_or(false) }

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Admission {
    Ungated,
    Admitted,
    Refused,
}

#[derive(Default)]
struct MeterState {
    drawn: BTreeMap<Vec<u8>, RevAmount>,
    stopped: BTreeSet<Vec<u8>>,
    refusals: BTreeMap<Vec<u8>, Refusal>,
    reached: BTreeMap<Vec<u8>, RevAmount>,
    work: BTreeMap<Surface, RevAmount>,
}

pub struct ReplayRefusals {
    steps: BTreeSet<Vec<u8>>,
    owners: BTreeSet<Vec<u8>>,
}

pub struct MembraneMeter {
    funding: Arc<dyn Funding>,
    replay: Option<ReplayRefusals>,
    state: Mutex<MeterState>,
}

pub struct Metered {
    pub refusals: Vec<Refusal>,
    pub drawn: BTreeMap<Vec<u8>, RevAmount>,
    pub reached: BTreeMap<Vec<u8>, RevAmount>,
    pub work: BTreeMap<Surface, RevAmount>,
}

impl MembraneMeter {
    pub fn play(funding: Arc<dyn Funding>) -> Arc<Self> {
        Arc::new(MembraneMeter {
            funding,
            replay: None,
            state: Mutex::new(MeterState::default()),
        })
    }

    pub fn replay(funding: Arc<dyn Funding>, logged: &[Refusal]) -> Arc<Self> {
        Arc::new(MembraneMeter {
            funding,
            replay: Some(ReplayRefusals {
                steps: logged.iter().map(|r| r.step.clone()).collect(),
                owners: logged.iter().map(|r| r.surface.owner.clone()).collect(),
            }),
            state: Mutex::new(MeterState::default()),
        })
    }

    pub fn gates(&self, record: &SealRecord) -> bool { self.funding.gates(record) }

    pub fn admit_charge(&self, record: &SealRecord, phlo: i64) -> Admission {
        if phlo <= 0 || !self.funding.gates(record) {
            return Admission::Ungated;
        }
        if restoring() {
            return Admission::Admitted;
        }
        let charge = (phlo as u64).saturating_mul(record.price());
        let (base, position) = next_position(record);
        self.admit(record, charge, RefusalKind::Charge, || {
            step_id(&base, position)
        })
    }

    pub fn admit_cell(&self, record: &SealRecord, step: impl FnOnce() -> Vec<u8>) -> Admission {
        if !self.funding.gates(record) {
            return Admission::Ungated;
        }
        if self.replay.is_some() {
            return Admission::Admitted;
        }
        let charge = record.price();
        let wallets = record.purse();
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(_) => return Admission::Refused,
        };
        match self.binding(&state, &wallets, charge) {
            None => Admission::Admitted,
            Some((wallet, drawn)) => {
                let step = step();
                let surface_drawn = Self::worked(&state, record);
                state.refusals.entry(step.clone()).or_insert(Refusal {
                    surface: Surface::of(record),
                    step,
                    kind: RefusalKind::Cell,
                    drawn,
                    charge,
                    surface_drawn,
                });
                state.stopped.insert(wallet);
                Admission::Refused
            }
        }
    }

    pub fn is_replay(&self) -> bool { self.replay.is_some() }

    pub fn admit_comm(&self, records: &[SealRecord], base: impl FnOnce() -> Vec<u8>) -> bool {
        let mut gated: Vec<&SealRecord> = Vec::new();
        for record in records {
            if self.funding.gates(record)
                && !gated.iter().any(|g| Surface::of(g) == Surface::of(record))
            {
                gated.push(record);
            }
        }
        if gated.is_empty() || self.replay.is_some() {
            return true;
        }
        gated.sort_by_key(|r| Surface::of(r));
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(_) => return false,
        };
        let mut need: BTreeMap<Vec<u8>, RevAmount> = BTreeMap::new();
        for record in &gated {
            for wallet in record.purse() {
                let n = need.entry(wallet.canonical_bytes()).or_insert(0);
                *n = n.saturating_add(record.price());
            }
        }
        let short: Vec<usize> = gated
            .iter()
            .enumerate()
            .filter(|(_, record)| {
                record.purse().iter().any(|wallet| {
                    let key = wallet.canonical_bytes();
                    let drawn = state.drawn.get(&key).copied().unwrap_or(0);
                    state.stopped.contains(&key)
                        || drawn.saturating_add(need.get(&key).copied().unwrap_or(0))
                            > self.funding.limit(wallet)
                })
            })
            .map(|(i, _)| i)
            .collect();
        if short.is_empty() {
            for record in &gated {
                Self::draw(&mut state, &record.purse(), record.price());
            }
            return true;
        }
        let base = base();
        for i in short {
            let record = gated[i];
            let wallet = record
                .purse()
                .into_iter()
                .find(|wallet| {
                    let key = wallet.canonical_bytes();
                    let drawn = state.drawn.get(&key).copied().unwrap_or(0);
                    state.stopped.contains(&key)
                        || drawn.saturating_add(need.get(&key).copied().unwrap_or(0))
                            > self.funding.limit(wallet)
                })
                .map(|w| w.canonical_bytes())
                .unwrap_or_else(|| record.owner.canonical_bytes());
            let drawn = state.drawn.get(&wallet).copied().unwrap_or(0);
            let step = step_id(&base, i as u64);
            let surface_drawn = Self::worked(&state, record);
            state.refusals.entry(step.clone()).or_insert(Refusal {
                surface: Surface::of(record),
                step,
                kind: RefusalKind::Cell,
                drawn,
                charge: record.price(),
                surface_drawn,
            });
            state.stopped.insert(wallet);
        }
        false
    }

    pub fn draw_comm_in_replay(&self, records: &[SealRecord]) {
        if self.replay.is_none() {
            return;
        }
        let mut seen: Vec<Surface> = Vec::new();
        for record in records {
            let key = Surface::of(record);
            if self.funding.gates(record) && !seen.contains(&key) {
                seen.push(key);
                self.draw_cell(record);
            }
        }
    }

    pub fn draw_cell(&self, record: &SealRecord) {
        if !self.funding.gates(record) {
            return;
        }
        let charge = record.price();
        if let Ok(mut state) = self.state.lock() {
            for wallet in record.purse() {
                let drawn = state.drawn.entry(wallet.canonical_bytes()).or_insert(0);
                *drawn = drawn.saturating_add(charge);
            }
        }
    }

    fn binding(
        &self,
        state: &MeterState,
        wallets: &[WalletOwner],
        charge: RevAmount,
    ) -> Option<(Vec<u8>, RevAmount)> {
        wallets.iter().find_map(|wallet| {
            let key = wallet.canonical_bytes();
            let drawn = state.drawn.get(&key).copied().unwrap_or(0);
            let stopped = state.stopped.contains(&key);
            let unpayable = drawn.saturating_add(charge) > self.funding.limit(wallet);
            (stopped || unpayable).then_some((key, drawn))
        })
    }

    fn admit(
        &self,
        record: &SealRecord,
        charge: RevAmount,
        kind: RefusalKind,
        step: impl FnOnce() -> Vec<u8>,
    ) -> Admission {
        let wallets = record.purse();
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(_) => return Admission::Refused,
        };
        match &self.replay {
            Some(logged) => {
                let watched = wallets
                    .iter()
                    .any(|w| logged.owners.contains(&w.canonical_bytes()))
                    || logged.owners.contains(&record.owner.canonical_bytes());
                if watched {
                    let step = step();
                    if logged.steps.contains(&step) {
                        state.reached.insert(step, charge);
                        return Admission::Refused;
                    }
                }
                Self::draw(&mut state, &wallets, charge);
                if kind == RefusalKind::Charge {
                    Self::work(&mut state, record, charge);
                }
                Admission::Admitted
            }
            None => match self.binding(&state, &wallets, charge) {
                None => {
                    Self::draw(&mut state, &wallets, charge);
                    if kind == RefusalKind::Charge {
                        Self::work(&mut state, record, charge);
                    }
                    Admission::Admitted
                }
                Some((wallet, drawn)) => {
                    let step = step();
                    let surface_drawn = Self::worked(&state, record);
                    state.refusals.entry(step.clone()).or_insert(Refusal {
                        surface: Surface::of(record),
                        step,
                        kind,
                        drawn,
                        charge,
                        surface_drawn,
                    });
                    state.stopped.insert(wallet);
                    Admission::Refused
                }
            },
        }
    }

    fn work(state: &mut MeterState, record: &SealRecord, charge: RevAmount) {
        let worked = state.work.entry(Surface::of(record)).or_insert(0);
        *worked = worked.saturating_add(charge);
    }

    fn worked(state: &MeterState, record: &SealRecord) -> RevAmount {
        state.work.get(&Surface::of(record)).copied().unwrap_or(0)
    }

    fn draw(state: &mut MeterState, wallets: &[WalletOwner], charge: RevAmount) {
        for wallet in wallets {
            let drawn = state.drawn.entry(wallet.canonical_bytes()).or_insert(0);
            *drawn = drawn.saturating_add(charge);
        }
    }

    pub fn take(&self) -> Metered {
        let state = std::mem::take(&mut *self.state.lock().expect("membrane meter lock"));
        Metered {
            refusals: state.refusals.into_values().collect(),
            drawn: state.drawn,
            reached: state.reached,
            work: state.work,
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

    #[tokio::test]
    async fn work_under_a_membrane_stops_at_the_first_step_it_cannot_pay() {
        let meter = MembraneMeter::play(funding());
        let alice = record(1, 2);
        let scope = StepScope::new(step_base(StepSource::Evaluation, b"term", b"rand"));
        let seen = stepping(scope, async {
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
        let mut charges: Vec<u64> = metered.refusals.iter().map(|r| r.charge).collect();
        charges.sort();
        assert_eq!(charges, vec![2, 4]);
        assert!(metered.refusals.iter().any(|r| r.drawn + r.charge > 10));
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
    async fn restoring_a_stored_item_is_never_refused_and_draws_nothing() {
        let meter = MembraneMeter::play(funding());
        let alice = record(1, 2);
        let restore = StepScope::restoring(step_base(StepSource::Restore, b"", b"item"));
        let seen = stepping(restore, async {
            [
                meter.admit_charge(&alice, 100),
                meter.admit_charge(&alice, 100),
            ]
        })
        .await;
        assert_eq!(seen, [Admission::Admitted, Admission::Admitted]);
        let metered = meter.take();
        assert!(metered.drawn.is_empty() && metered.refusals.is_empty());
    }

    #[tokio::test]
    async fn replay_refuses_exactly_the_logged_steps_and_counts_the_rest() {
        let alice = record(1, 2);
        let play = MembraneMeter::play(funding());
        let base = step_base(StepSource::Evaluation, b"term", b"rand");
        stepping(StepScope::new(base.clone()), async {
            for phlo in [3, 1, 2, 1] {
                play.admit_charge(&alice, phlo);
            }
        })
        .await;
        let logged = play.take().refusals;
        let replay = MembraneMeter::replay(funding(), &logged);
        let seen = stepping(StepScope::new(base), async {
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
        assert_eq!(metered.drawn.get(&owner(1).canonical_bytes()), Some(&8));
        assert_eq!(
            metered.reached,
            logged
                .iter()
                .map(|r| (r.step.clone(), r.charge))
                .collect::<BTreeMap<_, _>>()
        );
    }

    #[test]
    fn the_cells_of_a_comm_are_admitted_together_or_the_comm_is_vetoed() {
        let meter = MembraneMeter::play(funding());
        let alice = record(1, 4);
        let bob = record(2, 30);
        let free = record(9, 1_000);
        assert!(meter.admit_comm(&[alice.clone(), bob.clone(), free.clone()], || vec![1]));
        assert!(meter.admit_comm(&[alice.clone(), alice.clone()], || vec![2]));
        assert!(!meter.admit_comm(&[alice.clone(), bob.clone()], || vec![3]));
        assert!(meter.admit_comm(&[bob.clone()], || vec![4]));
        assert!(!meter.admit_comm(&[alice.clone()], || vec![5]));
        let metered = meter.take();
        assert_eq!(metered.drawn.get(&owner(1).canonical_bytes()), Some(&8));
        assert_eq!(metered.drawn.get(&owner(2).canonical_bytes()), Some(&60));
        assert_eq!(metered.refusals.len(), 2);
        assert!(metered
            .refusals
            .iter()
            .all(|r| r.surface == Surface::of(&alice)
                && (r.drawn, r.charge, r.kind) == (8, 4, RefusalKind::Cell)));
        let replay = MembraneMeter::replay(funding(), &metered.refusals);
        assert!(replay.admit_comm(&[alice.clone()], || vec![3]));
        replay.draw_comm_in_replay(&[alice.clone(), alice.clone(), free]);
        assert_eq!(
            replay.take().drawn.get(&owner(1).canonical_bytes()),
            Some(&4)
        );
    }

    #[test]
    fn a_cell_is_refused_once_its_wallet_cannot_pay_and_the_refusal_is_logged_once() {
        let meter = MembraneMeter::play(funding());
        let alice = record(1, 4);
        assert_eq!(meter.admit_cell(&alice, || vec![1]), Admission::Admitted);
        meter.draw_cell(&alice);
        meter.draw_cell(&alice);
        assert_eq!(meter.admit_cell(&alice, || vec![1]), Admission::Refused);
        assert_eq!(meter.admit_cell(&alice, || vec![1]), Admission::Refused);
        assert_eq!(
            meter.admit_cell(&record(9, 4), || vec![2]),
            Admission::Ungated
        );
        let metered = meter.take();
        assert_eq!(metered.refusals.len(), 1);
        assert_eq!(
            (
                metered.refusals[0].kind,
                metered.refusals[0].drawn,
                metered.refusals[0].charge
            ),
            (RefusalKind::Cell, 8, 4)
        );
        let replay = MembraneMeter::replay(funding(), &metered.refusals);
        assert_eq!(replay.admit_cell(&alice, || vec![1]), Admission::Admitted);
    }
}
