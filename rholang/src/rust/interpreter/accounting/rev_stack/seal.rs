use std::future::Future;
use std::sync::Arc;

use models::rhoapi::{SealOrigin, SealRecordProto, TaggedContinuation};

use super::owner::WalletOwner;
use super::RevStackError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Origin {
    pub envelope: Vec<u8>,
    pub signers: Vec<WalletOwner>,
    pub phlo_price: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SealKind {
    Deployment,
    Signed,
    Process,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SealRecord {
    pub owner: WalletOwner,
    pub kind: SealKind,
    pub origin: Origin,
}

pub use crate::rust::interpreter::accounting::membrane::{Admission, Funding, MembraneMeter};

tokio::task_local! {
    static SEAL: Option<SealRecord>;
    static FUNDING: Option<Arc<MembraneMeter>>;
}

pub fn meter() -> Option<Arc<MembraneMeter>> { FUNDING.try_with(Clone::clone).ok().flatten() }

pub fn funds(record: &SealRecord, step: impl FnOnce() -> Vec<u8>) -> bool {
    meter().is_none_or(|meter| meter.admit_cell(record, step) != Admission::Refused)
}

pub fn funds_continuation(continuation: &TaggedContinuation) -> bool {
    match of_continuation(continuation) {
        Ok(Some(record)) => funds(&record, || cell_step(continuation)),
        Ok(None) => true,
        Err(_) => false,
    }
}

pub fn admits_comm(
    continuation: &TaggedContinuation,
    matched: &[models::rhoapi::ListParWithRandom],
) -> bool {
    let Some(meter) = meter() else {
        return true;
    };
    let mut records = Vec::new();
    match of_continuation(continuation) {
        Ok(Some(record)) => records.push(record),
        Ok(None) => {}
        Err(_) => return false,
    }
    for datum in matched {
        match datum.seal.as_ref().map(SealRecord::from_proto) {
            Some(Ok(record)) => records.push(record),
            Some(Err(_)) => return false,
            None => {}
        }
    }
    meter.admit_comm(&records, || comm_step(continuation, matched))
}

pub fn records_of_comm(
    continuation: Option<&SealRecord>,
    sends: &[Option<SealRecord>],
) -> Vec<SealRecord> {
    continuation
        .into_iter()
        .chain(sends.iter().flatten())
        .cloned()
        .collect()
}

fn comm_step(
    continuation: &TaggedContinuation,
    matched: &[models::rhoapi::ListParWithRandom],
) -> Vec<u8> {
    use crate::rust::interpreter::accounting::membrane::{step_base, StepSource};
    use crypto::rust::hash::blake2b512_random::Blake2b512Random;
    use models::rhoapi::tagged_continuation::TaggedCont;
    let body = match &continuation.tagged_cont {
        Some(TaggedCont::ParBody(body)) => body.random_state.clone(),
        _ => Vec::new(),
    };
    let mut randoms = vec![Blake2b512Random::from_bytes(&body)];
    randoms.extend(
        matched
            .iter()
            .map(|d| Blake2b512Random::from_bytes(&d.random_state)),
    );
    step_base(
        StepSource::Comm,
        &[],
        &Blake2b512Random::merge(randoms).to_bytes(),
    )
}

fn cell_step(continuation: &TaggedContinuation) -> Vec<u8> {
    use crate::rust::interpreter::accounting::membrane::{step_base, step_id, StepSource};
    use models::rhoapi::tagged_continuation::TaggedCont;
    let random_state = match &continuation.tagged_cont {
        Some(TaggedCont::ParBody(body)) => body.random_state.clone(),
        _ => Vec::new(),
    };
    step_id(&step_base(StepSource::Comm, &[], &random_state), 0)
}

pub(crate) async fn funded_by<F: Future>(
    meter: Option<Arc<MembraneMeter>>,
    future: F,
) -> F::Output {
    FUNDING.scope(meter, future).await
}

pub(crate) fn keep_funding<F: Future>(future: F) -> impl Future<Output = F::Output> {
    FUNDING.scope(meter(), future)
}

pub fn current() -> Option<SealRecord> { SEAL.try_with(Clone::clone).ok().flatten() }

pub fn current_owner() -> Option<WalletOwner> { current().map(|record| record.owner) }

pub fn of_continuation(
    continuation: &TaggedContinuation,
) -> Result<Option<SealRecord>, RevStackError> {
    continuation
        .seal
        .as_ref()
        .map(SealRecord::from_proto)
        .transpose()
}

pub fn purse_of(owner: &WalletOwner) -> Vec<WalletOwner> {
    match owner {
        WalletOwner::Compound(components) => components.clone(),
        single => vec![single.clone()],
    }
}

pub(crate) async fn within<F: Future>(record: Option<SealRecord>, future: F) -> F::Output {
    SEAL.scope(record, future).await
}

pub(crate) fn within_now<R>(record: Option<SealRecord>, f: impl FnOnce() -> R) -> R {
    SEAL.sync_scope(record, f)
}

pub(crate) fn inherit<F: Future>(future: F) -> impl Future<Output = F::Output> {
    SEAL.scope(current(), keep_funding(future))
}

const KIND_DEPLOYMENT: i32 = 1;
const KIND_SIGNED: i32 = 2;
const KIND_PROCESS: i32 = 3;

impl SealKind {
    fn tag(self) -> i32 {
        match self {
            SealKind::Deployment => KIND_DEPLOYMENT,
            SealKind::Signed => KIND_SIGNED,
            SealKind::Process => KIND_PROCESS,
        }
    }

    fn from_tag(tag: i32) -> Option<Self> {
        match tag {
            KIND_DEPLOYMENT => Some(SealKind::Deployment),
            KIND_SIGNED => Some(SealKind::Signed),
            KIND_PROCESS => Some(SealKind::Process),
            _ => None,
        }
    }
}

impl SealRecord {
    pub(crate) fn deployment(owner: WalletOwner, origin: Origin) -> Self {
        SealRecord {
            owner,
            kind: SealKind::Deployment,
            origin,
        }
    }

    pub(crate) fn enter(&self, owner: WalletOwner, kind: SealKind) -> Self {
        SealRecord {
            owner,
            kind,
            origin: self.origin.clone(),
        }
    }

    pub fn purse(&self) -> Vec<WalletOwner> { purse_of(&self.owner) }

    pub fn price(&self) -> u64 { self.origin.phlo_price }

    pub fn shares_membrane(&self, other: &SealRecord) -> bool {
        self.owner == other.owner && self.origin == other.origin
    }

    pub fn to_proto(&self) -> SealRecordProto {
        SealRecordProto {
            owner: self.owner.canonical_bytes(),
            kind: self.kind.tag(),
            origin: Some(SealOrigin {
                envelope: self.origin.envelope.clone(),
                signers: self
                    .origin
                    .signers
                    .iter()
                    .map(WalletOwner::canonical_bytes)
                    .collect(),
                phlo_price: self.origin.phlo_price,
            }),
        }
    }

    pub fn from_proto(proto: &SealRecordProto) -> Result<Self, RevStackError> {
        let corrupt = |what: &str| RevStackError::CorruptWallet(format!("a seal record {what}"));
        let owner = WalletOwner::from_canonical_bytes(&proto.owner)
            .ok_or_else(|| corrupt("names no owner"))?;
        let kind = SealKind::from_tag(proto.kind).ok_or_else(|| corrupt("has no kind"))?;
        let origin = proto
            .origin
            .as_ref()
            .ok_or_else(|| corrupt("has no origin"))?;
        let signers = origin
            .signers
            .iter()
            .map(|bytes| WalletOwner::from_canonical_bytes(bytes))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| corrupt("names a signer that is no owner"))?;
        Ok(SealRecord {
            owner,
            kind,
            origin: Origin {
                envelope: origin.envelope.clone(),
                signers,
                phlo_price: origin.phlo_price,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(owner: u8, kind: SealKind) -> SealRecord {
        SealRecord {
            owner: WalletOwner::Unforgeable(vec![owner; 32]),
            kind,
            origin: Origin {
                envelope: vec![9; 64],
                signers: vec![
                    WalletOwner::Unforgeable(vec![1; 32]),
                    WalletOwner::Unforgeable(vec![2; 32]),
                ],
                phlo_price: 3,
            },
        }
    }

    #[test]
    fn a_record_round_trips_through_its_proto() {
        for kind in [SealKind::Deployment, SealKind::Signed, SealKind::Process] {
            let r = record(5, kind);
            assert_eq!(SealRecord::from_proto(&r.to_proto()), Ok(r));
        }
        let mut bad = record(5, SealKind::Signed).to_proto();
        bad.kind = 0;
        assert!(SealRecord::from_proto(&bad).is_err());
        let mut bad = record(5, SealKind::Signed).to_proto();
        bad.origin = None;
        assert!(SealRecord::from_proto(&bad).is_err());
    }

    #[test]
    fn the_purse_is_the_owners_wallet_and_the_price_is_the_origins() {
        let r = record(5, SealKind::Signed);
        assert_eq!(r.purse(), vec![WalletOwner::Unforgeable(vec![5; 32])]);
        assert_eq!(r.price(), 3);
        let pair = vec![
            WalletOwner::Unforgeable(vec![1; 32]),
            WalletOwner::Unforgeable(vec![2; 32]),
        ];
        let joint = r.enter(WalletOwner::Compound(pair.clone()), SealKind::Process);
        assert_eq!(joint.purse(), pair);
        assert_eq!(joint.price(), 3);
        let mut dearer = r.clone();
        dearer.origin.phlo_price = 4;
        assert_eq!(dearer.purse(), r.purse());
        assert_eq!(dearer.price(), 4);
    }

    #[test]
    fn a_membrane_is_one_owner_under_one_origin() {
        let r = record(5, SealKind::Deployment);
        assert!(r.shares_membrane(&r.enter(r.owner.clone(), SealKind::Signed)));
        assert!(!r.shares_membrane(&record(6, SealKind::Deployment)));
        let mut elsewhere = r.clone();
        elsewhere.origin.envelope = vec![8; 64];
        assert!(!r.shares_membrane(&elsewhere));
        let mut dearer = r.clone();
        dearer.origin.phlo_price = 4;
        assert!(!r.shares_membrane(&dearer));
    }

    #[tokio::test]
    async fn the_record_is_scoped_and_spawned_tasks_inherit_it() {
        assert_eq!(current(), None);
        let outer = record(5, SealKind::Deployment);
        let inner = outer.enter(WalletOwner::Unforgeable(vec![6; 32]), SealKind::Signed);
        let seen = within(Some(outer.clone()), async {
            let before = current();
            let nested = within(Some(inner.clone()), async { current() }).await;
            let cleared = within(None, async { current() }).await;
            let spawned = tokio::spawn(inherit(async { current() })).await.unwrap();
            let bare = tokio::spawn(async { current() }).await.unwrap();
            (before, nested, cleared, spawned, bare, current())
        })
        .await;
        assert_eq!(
            seen,
            (
                Some(outer.clone()),
                Some(inner.clone()),
                None,
                Some(outer.clone()),
                None,
                Some(outer)
            )
        );
        assert_eq!(inner.origin.phlo_price, 3);
        assert_eq!(current(), None);
    }

    struct Only(u8);

    impl Funding for Only {
        fn gates(&self, _record: &SealRecord) -> bool { true }

        fn limit(&self, wallet: &WalletOwner) -> u64 {
            if *wallet == WalletOwner::Unforgeable(vec![self.0; 32]) {
                1_000
            } else {
                0
            }
        }
    }

    fn only(n: u8) -> Option<Arc<MembraneMeter>> { Some(MembraneMeter::play(Arc::new(Only(n)))) }

    #[tokio::test]
    async fn funding_is_scoped_and_spawned_tasks_keep_it() {
        let poor = record(5, SealKind::Process);
        let rich = record(7, SealKind::Process);
        let step = || vec![1];
        assert!(funds(&poor, step));
        let seen = funded_by(only(7), async {
            let here = (funds(&poor, step), funds(&rich, step));
            let p = poor.clone();
            let inherited = tokio::spawn(inherit(async move { funds(&p, || vec![2]) }))
                .await
                .unwrap();
            let p = poor.clone();
            let kept = tokio::spawn(keep_funding(async move { funds(&p, || vec![3]) }))
                .await
                .unwrap();
            let p = poor.clone();
            let bare = tokio::spawn(async move { funds(&p, || vec![4]) })
                .await
                .unwrap();
            let sealed = within(Some(rich.clone()), async { funds(&poor, step) }).await;
            (here, inherited, kept, bare, sealed)
        })
        .await;
        assert_eq!(seen, ((false, true), false, false, true, false));
        let stored = TaggedContinuation {
            seal: Some(poor.to_proto()),
            ..TaggedContinuation::default()
        };
        assert!(funds_continuation(&stored));
        let unfunded = funded_by(only(7), async {
            (
                funds_continuation(&stored),
                funds_continuation(&TaggedContinuation::default()),
            )
        })
        .await;
        assert_eq!(unfunded, (false, true));
    }

    #[tokio::test]
    async fn a_charge_made_for_another_record_sees_that_record_only_while_it_runs() {
        let running = record(5, SealKind::Deployment);
        let payer = record(7, SealKind::Signed);
        let seen = within(Some(running.clone()), async {
            let during = within_now(Some(payer.clone()), current);
            let cleared = within_now(None, current);
            (during, cleared, current())
        })
        .await;
        assert_eq!(seen, (Some(payer), None, Some(running)));
    }
}
