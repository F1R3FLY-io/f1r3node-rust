#![no_main]

use crypto::rust::public_key::PublicKey;
use libfuzzer_sys::fuzz_target;
use models::rust::casper::protocol::casper_message::{ProcessedSystemDeploy, SystemDeployData};
use prost::bytes::Bytes;

#[derive(arbitrary::Arbitrary, Debug)]
struct Input {
    invalid_block_hash: Vec<u8>,
    issuer_public_key: Vec<u8>,
    target_activation_epoch: i64,
}

fuzz_target!(|input: Input| {
    let slash = SystemDeployData::Slash {
        invalid_block_hash: Bytes::from(input.invalid_block_hash),
        issuer_public_key: PublicKey::from_bytes(&Bytes::from(input.issuer_public_key)),
        target_activation_epoch: input.target_activation_epoch,
    };
    let processed = ProcessedSystemDeploy::Succeeded {
        event_list: Vec::new(),
        system_deploy: slash,
    };

    let proto = processed.clone().to_proto();
    let decoded = ProcessedSystemDeploy::from_proto(proto).expect("slash deploy roundtrip");

    assert_eq!(decoded, processed);
});
