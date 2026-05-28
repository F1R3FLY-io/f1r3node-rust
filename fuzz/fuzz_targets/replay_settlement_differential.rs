//! Fuzz replay-field roundtrips together with Casper settlement arithmetic.
//!
//! The oracle ties two production boundaries together: processed-deploy
//! protobuf conversion must preserve the scalar cost and failure flag, and the
//! deploy settlement helper must keep refunds total, bounded, and unable to
//! replenish runtime fuel.

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use models::rust::casper::protocol::casper_message::{DeployData, ProcessedDeploy};

mod cost_accounting_fuzz_support;

#[derive(Arbitrary, Debug)]
struct Input {
    seed: u8,
    cost: u64,
    failed: bool,
    phlo_limit: i64,
    phlo_price: i64,
    token_cost: i64,
}

fuzz_target!(|input: Input| {
    let processed =
        cost_accounting_fuzz_support::processed_deploy(input.seed, input.cost, input.failed);
    let decoded = ProcessedDeploy::from_proto(processed.clone().to_proto())
        .expect("processed deploy protobuf roundtrip");
    assert_eq!(decoded.cost.cost, input.cost);
    assert_eq!(decoded.is_failed, input.failed);

    let deploy = DeployData {
        term: "Nil".to_string(),
        time_stamp: 0,
        phlo_price: input.phlo_price,
        phlo_limit: input.phlo_limit,
        valid_after_block_number: 0,
        shard_id: "root".to_string(),
        expiration_timestamp: None,
    };
    let refund = deploy.refund_amount_for_token_cost(input.token_cost);
    if input.phlo_limit < 0 || input.phlo_price < 0 || input.token_cost < 0 {
        assert!(refund.is_err());
        return;
    }
    let Some(escrow) = input.phlo_limit.checked_mul(input.phlo_price) else {
        assert!(refund.is_err());
        return;
    };
    let refund = refund.expect("valid settlement terms");
    assert!(refund >= 0);
    assert!(refund <= escrow);
    if input.token_cost <= input.phlo_limit {
        assert_eq!(refund + input.token_cost * input.phlo_price, escrow);
    } else {
        assert_eq!(refund, 0);
    }
});
