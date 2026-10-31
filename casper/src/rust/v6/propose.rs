use crypto::rust::signatures::signed::Signed;
use models::rust::casper::protocol::casper_message::DeployData;
use prost::bytes::Bytes;

use super::validity::Limits;

pub struct SelectionContext<'a> {
    pub next_block_number: i64,
    pub deploy_lifespan: i64,
    pub now_millis: i64,
    pub shard_id: &'a str,
    pub min_phlo_price: i64,
    pub in_scope: &'a dyn Fn(&Bytes) -> bool,
    pub limits: &'a Limits,
}

pub fn select_envelopes<I>(pool: I, ctx: &SelectionContext) -> Vec<Signed<DeployData>>
where
    I: IntoIterator<Item = Signed<DeployData>>,
{
    let earliest = ctx.next_block_number - ctx.deploy_lifespan;
    let mut candidates: Vec<Signed<DeployData>> = pool
        .into_iter()
        .filter(|d| d.data.shard_id == ctx.shard_id)
        .filter(|d| d.data.valid_after_block_number > earliest)
        .filter(|d| d.data.valid_after_block_number < ctx.next_block_number)
        .filter(|d| !d.data.is_expired_at(ctx.now_millis))
        .filter(|d| d.data.phlo_price >= ctx.min_phlo_price)
        .filter(|d| d.data.phlo_limit > 0)
        .filter(|d| !(ctx.in_scope)(&d.sig))
        .collect();
    candidates.sort_by(|a, b| (a.data.time_stamp, &a.sig).cmp(&(b.data.time_stamp, &b.sig)));
    let mut demand: i64 = 0;
    let mut out = Vec::new();
    for deploy in candidates {
        if out.len() >= ctx.limits.max_envelopes {
            break;
        }
        match demand.checked_add(deploy.data.phlo_limit) {
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
    use super::*;
    use crypto::rust::private_key::PrivateKey;
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::signatures_alg::SignaturesAlg;

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
            cosignatures: Vec::new(),
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
            ..Limits::default()
        };
        let none = |_: &Bytes| false;
        let picked = select_envelopes(
            vec![deploy(3, 8, 100), deploy(1, 8, 100), deploy(2, 8, 100)],
            &ctx(&limits, &none),
        );
        assert_eq!(
            picked.iter().map(|d| d.data.time_stamp).collect::<Vec<_>>(),
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
        assert_eq!(picked, vec![fine]);
    }
}
