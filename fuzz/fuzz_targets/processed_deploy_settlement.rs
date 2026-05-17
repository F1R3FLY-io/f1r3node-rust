//! Fuzz deploy settlement arithmetic.
//!
//! Valid settlement terms must produce a refund bounded by escrow. Invalid
//! negative or overflowing terms must be rejected by the production helper.

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use models::rust::casper::protocol::casper_message::DeployData;

#[derive(Arbitrary, Debug)]
struct Input {
    phlo_limit: i64,
    phlo_price: i64,
    token_cost: i64,
}

fn deploy_data(input: &Input) -> DeployData {
    DeployData {
        term: String::new(),
        time_stamp: 0,
        phlo_price: input.phlo_price,
        phlo_limit: input.phlo_limit,
        valid_after_block_number: 0,
        shard_id: "root".to_string(),
        expiration_timestamp: None,
    }
}

fuzz_target!(|input: Input| {
    let deploy = deploy_data(&input);
    let result = deploy.refund_amount_for_token_cost(input.token_cost);

    if input.phlo_limit < 0 || input.phlo_price < 0 || input.token_cost < 0 {
        assert!(result.is_err());
        return;
    }

    let Some(escrow) = input.phlo_limit.checked_mul(input.phlo_price) else {
        assert!(result.is_err());
        return;
    };

    let refund = result.expect("valid bounded refund");
    assert!(refund >= 0);
    assert!(refund <= escrow);
    if input.token_cost <= input.phlo_limit {
        assert_eq!(refund + input.token_cost * input.phlo_price, escrow);
    } else {
        assert_eq!(refund, 0);
    }
});
