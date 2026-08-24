//! `slash_deploy_roundtrip` — proto idempotency for `SystemDeployData::Slash`.
//!
//! Invariant: `from_proto(slash.to_proto()) == slash` for every
//! `arbitrary::Arbitrary`-generated slash payload.
//!
//! Why no input filtering: `PublicKey::from_bytes` accepts arbitrary-length
//! `Bytes` and never panics — even nonsensical key bytes produce a valid
//! in-memory `PublicKey`, so the harness can run on every fuzzer-generated
//! input without an early-return. Likewise `Bytes` for the invalid block
//! hash takes any byte slice. The narrowed surface (Slash only, not the
//! full `ProcessedSystemDeploy` union) lets the fuzzer concentrate on the
//! slash-specific encoding edges (i64 epoch, public-key bytes, hash bytes).
//!
//! Variant scope: this file only exercises `ProcessedSystemDeploy::Succeeded`
//! wrapping a Slash. `Failed` is out of scope here — failed slashes are
//! covered by the lifecycle trace.

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
