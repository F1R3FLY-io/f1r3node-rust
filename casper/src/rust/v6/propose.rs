use crypto::rust::signatures::signed::Signed;
use models::rust::casper::protocol::casper_message::{
    DeployData, ProcessedDeploy, ProcessedUserDeploy,
};
use models::rust::deploy_envelope::{DeployEnvelope, DeployEnvelopeRef};
use prost::bytes::Bytes;

use super::validity::Limits;

#[derive(Clone, Debug, PartialEq)]
pub enum PoolEntry {
    Legacy(Signed<DeployData>),
    Offered(DeployEnvelope),
}

impl From<Signed<DeployData>> for PoolEntry {
    fn from(deploy: Signed<DeployData>) -> Self { PoolEntry::Legacy(deploy) }
}

impl From<DeployEnvelope> for PoolEntry {
    fn from(envelope: DeployEnvelope) -> Self { PoolEntry::Offered(envelope) }
}

impl PoolEntry {
    pub fn time_stamp(&self) -> i64 {
        match self {
            PoolEntry::Legacy(deploy) => deploy.data.time_stamp,
            PoolEntry::Offered(envelope) => envelope.body().time_stamp,
        }
    }

    pub fn id(&self) -> &[u8] {
        match self {
            PoolEntry::Legacy(deploy) => deploy.sig.as_ref(),
            PoolEntry::Offered(envelope) => envelope.identity().as_bytes(),
        }
    }

    fn shard_id(&self) -> &str {
        match self {
            PoolEntry::Legacy(deploy) => &deploy.data.shard_id,
            PoolEntry::Offered(envelope) => &envelope.body().shard_id,
        }
    }

    fn valid_after_block_number(&self) -> i64 {
        match self {
            PoolEntry::Legacy(deploy) => deploy.data.valid_after_block_number,
            PoolEntry::Offered(envelope) => envelope.body().valid_after_block_number,
        }
    }

    fn is_expired_at(&self, now_millis: i64) -> bool {
        match self {
            PoolEntry::Legacy(deploy) => deploy.data.is_expired_at(now_millis),
            PoolEntry::Offered(envelope) => envelope.body().is_expired_at(now_millis),
        }
    }

    fn phlo(&self) -> Option<(i64, i64)> {
        match self {
            PoolEntry::Legacy(deploy) => Some((deploy.data.phlo_price, deploy.data.phlo_limit)),
            PoolEntry::Offered(envelope) => match envelope.view() {
                DeployEnvelopeRef::OfferedFunded(offer) => {
                    Some((offer.data.phlo_price(), offer.data.phlo_limit()))
                }
                _ => None,
            },
        }
    }

    pub fn into_processed(self) -> Result<ProcessedUserDeploy, String> {
        match self {
            PoolEntry::Legacy(deploy) => {
                Ok(ProcessedUserDeploy::from(ProcessedDeploy::empty(deploy)))
            }
            PoolEntry::Offered(envelope) => ProcessedUserDeploy::ordered(envelope),
        }
    }
}

pub struct SelectionContext<'a> {
    pub next_block_number: i64,
    pub deploy_lifespan: i64,
    pub now_millis: i64,
    pub shard_id: &'a str,
    pub min_phlo_price: i64,
    pub in_scope: &'a dyn Fn(&Bytes) -> bool,
    pub limits: &'a Limits,
}

pub fn select_envelopes<I>(pool: I, ctx: &SelectionContext) -> Vec<PoolEntry>
where
    I: IntoIterator,
    I::Item: Into<PoolEntry>,
{
    let earliest = ctx.next_block_number - ctx.deploy_lifespan;
    let mut candidates: Vec<(i64, PoolEntry)> = pool
        .into_iter()
        .map(Into::into)
        .filter(|d: &PoolEntry| d.shard_id() == ctx.shard_id)
        .filter(|d| d.valid_after_block_number() > earliest)
        .filter(|d| d.valid_after_block_number() < ctx.next_block_number)
        .filter(|d| !d.is_expired_at(ctx.now_millis))
        .filter(|d| !(ctx.in_scope)(&Bytes::copy_from_slice(d.id())))
        .filter_map(|d| match d.phlo() {
            Some((price, limit)) if price >= ctx.min_phlo_price && limit > 0 => Some((limit, d)),
            _ => None,
        })
        .collect();
    candidates.sort_by(|(_, a), (_, b)| (a.time_stamp(), a.id()).cmp(&(b.time_stamp(), b.id())));
    let mut demand: i64 = 0;
    let mut out = Vec::new();
    for (phlo_limit, deploy) in candidates {
        if out.len() >= ctx.limits.max_envelopes {
            break;
        }
        match demand.checked_add(phlo_limit) {
            Some(next) if next <= ctx.limits.demand_cap => {
                demand = next;
                out.push(deploy);
            }
            _ => continue,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use crypto::rust::private_key::PrivateKey;
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::signatures_alg::SignaturesAlg;

    use super::*;

    fn deploy(ts: i64, valid_after: i64, phlo_limit: i64) -> Signed<DeployData> {
        let (sk, _) = Secp256k1.new_key_pair();
        let sk: PrivateKey = sk;
        let data = DeployData {
            term: "Nil".to_string(),
            time_stamp: ts,
            phlo_price: 1,
            phlo_limit,
            valid_after_block_number: valid_after,
            shard_id: "root".to_string(),
            expiration_timestamp: None,
            parameters: vec![],
        };
        Signed::create(data, Box::new(Secp256k1), sk).unwrap()
    }

    fn ctx<'a>(limits: &'a Limits, in_scope: &'a dyn Fn(&Bytes) -> bool) -> SelectionContext<'a> {
        SelectionContext {
            next_block_number: 10,
            deploy_lifespan: 5,
            now_millis: 0,
            shard_id: "root",
            min_phlo_price: 1,
            in_scope,
            limits,
        }
    }

    #[test]
    fn selects_in_timestamp_order_within_the_demand_cap() {
        let limits = Limits {
            demand_cap: 250,
            max_envelopes: 10,
            max_records: 1,
            max_record_bytes: 1024,
            ordered_only: false,
        };
        let none = |_: &Bytes| false;
        let picked = select_envelopes(
            vec![deploy(3, 8, 100), deploy(1, 8, 100), deploy(2, 8, 100)],
            &ctx(&limits, &none),
        );
        assert_eq!(
            picked.iter().map(PoolEntry::time_stamp).collect::<Vec<_>>(),
            vec![1, 2]
        );
    }

    #[test]
    fn drops_expired_future_and_in_scope_envelopes() {
        let limits = Limits::default();
        let old = deploy(1, 4, 10);
        let future = deploy(2, 10, 10);
        let scoped = deploy(3, 8, 10);
        let fine = deploy(4, 8, 10);
        let scoped_sig = scoped.sig.clone();
        let in_scope = move |s: &Bytes| *s == scoped_sig;
        let picked = select_envelopes(
            vec![old, future, scoped, fine.clone()],
            &ctx(&limits, &in_scope),
        );
        assert_eq!(picked, vec![PoolEntry::from(fine)]);
    }

    fn offered(ts: i64, valid_after: i64, phlo_limit: u64, price: u64) -> PoolEntry {
        let body = models::rust::cost_deploy_data::DeployData {
            term: "Nil".to_string(),
            language: "rholang".to_string(),
            time_stamp: ts,
            valid_after_block_number: valid_after,
            shard_id: "root".to_string(),
            expiration_timestamp: None,
            authority_presentations: Vec::new(),
        };
        let proto = crate::rust::util::rholang::costacc::owner_offer::owner_direct_offer(
            &Secp256k1.new_key_pair().0,
            body,
            phlo_limit,
            price,
            &crate::rust::util::rholang::costacc::genesis_resource_policy::shard_resource_schedule(
                "root", 8,
            ),
        )
        .unwrap();
        PoolEntry::from(
            DeployEnvelope::from_proto(
                proto,
                models::rust::cost_protocol_limits::offered_funded_v6_limits().envelope,
            )
            .unwrap(),
        )
    }

    #[test]
    fn offers_and_legacy_deploys_share_the_order_and_the_caps() {
        let limits = Limits {
            demand_cap: 250,
            max_envelopes: 10,
            max_records: 1,
            max_record_bytes: 1024,
            ordered_only: false,
        };
        let none = |_: &Bytes| false;
        let pool = vec![
            offered(3, 8, 100, 1),
            PoolEntry::from(deploy(1, 8, 100)),
            offered(2, 8, 100, 1),
            offered(0, 8, 100, 0),
            offered(4, 4, 10, 1),
        ];
        let picked = select_envelopes(pool, &ctx(&limits, &none));
        assert_eq!(
            picked.iter().map(PoolEntry::time_stamp).collect::<Vec<_>>(),
            vec![1, 2]
        );
        let processed: Vec<ProcessedUserDeploy> = picked
            .into_iter()
            .map(|entry| entry.into_processed().unwrap())
            .collect();
        assert!(processed[0].as_legacy().is_some());
        assert!(processed[1].as_ordered().is_some());
    }

    #[test]
    fn an_offer_in_scope_is_not_selected_again() {
        let limits = Limits::default();
        let entry = offered(5, 8, 10, 1);
        let id = Bytes::copy_from_slice(entry.id());
        let in_scope = move |s: &Bytes| *s == id;
        assert!(select_envelopes(vec![entry], &ctx(&limits, &in_scope)).is_empty());
    }
}
