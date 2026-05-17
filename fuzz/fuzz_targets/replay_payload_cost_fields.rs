//! Fuzz cost-field preservation in processed-deploy protobuf payloads.
//!
//! This target checks the public serialization boundary directly: cost-trace
//! digest bytes, event count, scalar cost, and failure flags must survive
//! roundtrip conversion.

#![no_main]

use arbitrary::Arbitrary;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::secp256k1_eth::Secp256k1Eth;
use crypto::rust::signatures::signed::Signed;
use libfuzzer_sys::fuzz_target;
use models::rhoapi::PCost;
use models::rust::casper::protocol::casper_message::{DeployData, ProcessedDeploy};
use prost::bytes::Bytes;

#[derive(Arbitrary, Debug)]
struct Input {
    cost: u64,
    digest: Vec<u8>,
    event_count: u64,
    failed: bool,
    system_error: Option<String>,
}

fn signed_deploy() -> Signed<DeployData> {
    Signed {
        data: DeployData {
            term: "Nil".to_string(),
            time_stamp: 0,
            phlo_price: 1,
            phlo_limit: 100,
            valid_after_block_number: 0,
            shard_id: "root".to_string(),
            expiration_timestamp: None,
        },
        pk: PublicKey::from_bytes(&[1; 65]),
        sig: Bytes::from(vec![2; 64]),
        sig_algorithm: Box::new(Secp256k1Eth),
    }
}

fuzz_target!(|input: Input| {
    let digest = input.digest.into_iter().take(64).collect::<Vec<_>>();
    let deploy = ProcessedDeploy {
        deploy: signed_deploy(),
        cost: PCost { cost: input.cost },
        deploy_log: Vec::new(),
        is_failed: input.failed,
        system_deploy_error: input.system_error.clone().filter(|value| !value.is_empty()),
        cost_trace_digest: digest.clone().into(),
        cost_trace_event_count: input.event_count,
    };

    let proto = deploy.to_proto();
    assert_eq!(proto.cost.expect("cost field").cost, input.cost);
    assert_eq!(proto.errored, input.failed);
    assert_eq!(
        proto.system_deploy_error,
        input
            .system_error
            .filter(|value| !value.is_empty())
            .unwrap_or_default()
    );
    assert_eq!(proto.cost_trace_digest.to_vec(), digest);
    assert_eq!(proto.cost_trace_event_count, input.event_count);
});
