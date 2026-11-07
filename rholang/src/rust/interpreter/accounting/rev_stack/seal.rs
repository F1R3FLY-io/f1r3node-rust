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
    pub phlo_limit: u64,
}

pub fn origin_digest(origin: &SealOrigin) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend((origin.envelope.len() as u64).to_le_bytes());
    bytes.extend(&origin.envelope);
    bytes.extend(origin.phlo_price.to_le_bytes());
    for signer in &origin.signers {
        bytes.extend((signer.len() as u64).to_le_bytes());
        bytes.extend(signer);
    }
    crypto::rust::hash::blake2b256::Blake2b256::hash(bytes)
}

impl Origin {
    pub fn digest(&self) -> Vec<u8> {
        origin_digest(&SealOrigin {
            envelope: self.envelope.clone(),
            signers: self.signers.iter().map(WalletOwner::canonical_bytes).collect(),
            phlo_price: self.phlo_price,
            phlo_limit: self.phlo_limit,
        })
    }

    pub fn same_envelope(&self, other: &Origin) -> bool {
        self.envelope == other.envelope
            && self.signers == other.signers
            && self.phlo_price == other.phlo_price
    }

    pub fn matches_proto(&self, proto: &SealOrigin) -> bool {
        self.envelope == proto.envelope
            && self.phlo_price == proto.phlo_price
            && self.signers.len() == proto.signers.len()
            && self
                .signers
                .iter()
                .zip(proto.signers.iter())
                .all(|(signer, bytes)| signer.canonical_bytes() == *bytes)
    }
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

pub use crate::rust::interpreter::accounting::membrane::{Funding, MembraneMeter};

tokio::task_local! {
    static SEAL: Option<SealRecord>;
    static FUNDING: Option<Arc<MembraneMeter>>;
}

pub fn meter() -> Option<Arc<MembraneMeter>> { FUNDING.try_with(Clone::clone).ok().flatten() }

fn comm_records(
    continuation: &TaggedContinuation,
    data: impl IntoIterator<Item = Option<SealRecordProto>>,
) -> Option<(Option<SealRecord>, Vec<SealRecord>)> {
    let receive = of_continuation(continuation).ok()?;
    let sends = data
        .into_iter()
        .flatten()
        .map(|proto| SealRecord::from_proto(&proto).ok())
        .collect::<Option<Vec<_>>>()?;
    Some((receive, sends))
}

pub fn holds_comm(
    meter: &MembraneMeter,
    continuation: &TaggedContinuation,
    data: impl IntoIterator<Item = Option<SealRecordProto>>,
    channels: usize,
) -> bool {
    match comm_records(continuation, data) {
        Some((receive, sends)) => {
            let named = receive
                .as_ref()
                .map(|r| firing_owners(continuation, r))
                .unwrap_or_default();
            meter.hold(&meter.comm_needs(
                receive.as_ref().map(|r| (r, named.as_slice())),
                &sends,
                channels,
            ))
        }
        None => false,
    }
}

pub fn own_owner(continuation: &TaggedContinuation, record: &SealRecord) -> Option<WalletOwner> {
    use models::rhoapi::tagged_continuation::TaggedCont;
    match &continuation.tagged_cont {
        Some(TaggedCont::ParBody(body)) => {
            crate::rust::interpreter::reduce::own_seal_owner(body.body.as_ref(), record)
        }
        _ => None,
    }
}

pub fn firing_owners(continuation: &TaggedContinuation, record: &SealRecord) -> Vec<WalletOwner> {
    let mut named = body_owners(continuation);
    named.extend(own_owner(continuation, record));
    named
}

pub fn body_owners(continuation: &TaggedContinuation) -> Vec<WalletOwner> {
    use models::rhoapi::tagged_continuation::TaggedCont;
    match &continuation.tagged_cont {
        Some(TaggedCont::ParBody(body)) => body
            .body
            .as_ref()
            .map(super::region::resolved_owners)
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

pub fn admits_comm(
    continuation: &TaggedContinuation,
    matched: &[models::rhoapi::ListParWithRandom],
) -> bool {
    let Some(meter) = meter() else {
        return true;
    };
    if meter.is_replay() {
        return true;
    }
    holds_comm(
        &meter,
        continuation,
        matched.iter().map(|datum| datum.seal.clone()),
        matched.len(),
    )
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
    SEAL.scope(
        current(),
        keep_funding(crate::rust::interpreter::accounting::membrane::inherit_firing(future)),
    )
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

    pub(crate) fn from_tag(tag: i32) -> Option<Self> {
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
        self.owner == other.owner && self.origin.same_envelope(&other.origin)
    }

    pub(crate) fn with_limit(mut self, phlo_limit: u64) -> Self {
        self.origin.phlo_limit = phlo_limit;
        self
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
                phlo_limit: self.origin.phlo_limit,
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
                phlo_limit: origin.phlo_limit,
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
                phlo_limit: 11,
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

    fn admits(continuation: &TaggedContinuation) -> bool { admits_comm(continuation, &[]) }

    #[tokio::test]
    async fn the_meter_is_scoped_and_spawned_tasks_keep_it() {
        let stored = |r: &SealRecord| TaggedContinuation {
            seal: Some(r.to_proto()),
            ..TaggedContinuation::default()
        };
        let poor = stored(&record(5, SealKind::Process));
        let rich = stored(&record(7, SealKind::Process));
        assert!(admits(&poor));
        let seen = funded_by(only(7), async {
            let here = (admits(&poor), admits(&rich));
            let p = poor.clone();
            let inherited = tokio::spawn(inherit(async move { admits(&p) }))
                .await
                .unwrap();
            let p = poor.clone();
            let kept = tokio::spawn(keep_funding(async move { admits(&p) }))
                .await
                .unwrap();
            let p = poor.clone();
            let bare = tokio::spawn(async move { admits(&p) }).await.unwrap();
            (
                here,
                inherited,
                kept,
                bare,
                admits(&TaggedContinuation::default()),
            )
        })
        .await;
        assert_eq!(seen, ((false, true), false, false, true, true));
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
