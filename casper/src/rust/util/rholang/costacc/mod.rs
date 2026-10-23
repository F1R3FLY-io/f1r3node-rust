pub mod check_balance;
pub mod close_block_deploy;
pub mod conversion;
#[allow(dead_code)]
pub mod direct_wallet_funding;
pub mod genesis_resource_policy;
pub mod monetary_cursor;
pub mod offered_acceptance;
pub mod offered_candidate;
pub mod offered_context;
pub mod offered_evidence;
pub mod offered_grants;
pub mod offered_only;
pub mod owner_offer;
pub mod pre_charge_deploy;
pub mod prepaid_receipts;
pub mod production_limits;
pub mod refund_deploy;
pub mod slash_deploy;
pub mod supply;
pub mod supply_reader;
pub mod vault_cost_deploy;
pub mod vault_payer;

#[cfg(test)]
pub(crate) fn cost_encoding() -> models::rust::record_encoding::RecordEncodingScope {
    models::rust::record_encoding::scoped_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    )
}

#[cfg(test)]
pub(crate) fn cost_encoded_runtime(workers: usize) -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(workers)
        .enable_all()
        .on_thread_start(|| std::mem::forget(cost_encoding()))
        .build()
        .expect("a cost-encoded test runtime")
}
