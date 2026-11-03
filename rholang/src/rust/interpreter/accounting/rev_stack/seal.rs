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

pub trait Funding: Send + Sync {
    fn funds(&self, record: &SealRecord) -> bool;
}

tokio::task_local! {
    static SEAL: Option<SealRecord>;
    static FUNDING: Option<Arc<dyn Funding>>;
}

fn current_funding() -> Option<Arc<dyn Funding>> { FUNDING.try_with(Clone::clone).ok().flatten() }

pub fn funds(record: &SealRecord) -> bool {
    current_funding().is_none_or(|funding| funding.funds(record))
}

pub fn funds_continuation(continuation: &TaggedContinuation) -> bool {
    match of_continuation(continuation) {
        Ok(Some(record)) => funds(&record),
        Ok(None) => true,
        Err(_) => false,
    }
}

pub(crate) async fn funded_by<F: Future>(
    funding: Option<Arc<dyn Funding>>,
    future: F,
) -> F::Output {
    FUNDING.scope(funding, future).await
}

pub(crate) fn keep_funding<F: Future>(future: F) -> impl Future<Output = F::Output> {
    FUNDING.scope(current_funding(), future)
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
        fn funds(&self, record: &SealRecord) -> bool {
            record.owner == WalletOwner::Unforgeable(vec![self.0; 32])
        }
    }

    #[tokio::test]
    async fn funding_is_scoped_and_spawned_tasks_keep_it() {
        let poor = record(5, SealKind::Process);
        let rich = record(7, SealKind::Process);
        assert!(funds(&poor));
        let funding: Arc<dyn Funding> = Arc::new(Only(7));
        let seen = funded_by(Some(funding), async {
            let here = (funds(&poor), funds(&rich));
            let p = poor.clone();
            let inherited = tokio::spawn(inherit(async move { funds(&p) }))
                .await
                .unwrap();
            let p = poor.clone();
            let kept = tokio::spawn(keep_funding(async move { funds(&p) }))
                .await
                .unwrap();
            let p = poor.clone();
            let bare = tokio::spawn(async move { funds(&p) }).await.unwrap();
            let sealed = within(Some(rich.clone()), async { funds(&poor) }).await;
            (here, inherited, kept, bare, sealed)
        })
        .await;
        assert_eq!(seen, ((false, true), false, false, true, false));
        let stored = TaggedContinuation {
            seal: Some(poor.to_proto()),
            ..TaggedContinuation::default()
        };
        assert!(funds_continuation(&stored));
        let unfunded = funded_by(Some(Arc::new(Only(7)) as Arc<dyn Funding>), async {
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
