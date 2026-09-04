//! `slash_deploy_roundtrip` — proto idempotency for `SystemDeployData::Slash`.
//!
//! Invariant: `from_proto(slash.to_proto()) == slash` for every slash payload
//! in the protobuf semantic domain.
//!
//! Why no input filtering: `PublicKey::from_bytes` accepts arbitrary-length
//! `Bytes` and never panics — even nonsensical key bytes produce a valid
//! in-memory `PublicKey`, so the harness can run on every fuzzer-generated
//! input without an early-return. Likewise `Bytes` for the invalid block
//! hash takes any byte slice. The optional equivocation hash is either absent
//! or a full block hash because proto3 reserves empty bytes as the legacy
//! absence sentinel. The narrowed surface (Slash only, not the full
//! `ProcessedSystemDeploy` union) lets the fuzzer concentrate on the
//! slash-specific encoding edges (i64 epoch, public-key bytes, hash bytes).
//!
//! Variant scope: this file only exercises `ProcessedSystemDeploy::Succeeded`
//! wrapping a Slash. `Failed` is out of scope here — failed slashes are
//! covered by the lifecycle trace.

#![no_main]

use crypto::rust::public_key::PublicKey;
use libfuzzer_sys::fuzz_target;
use models::rust::bond_generation::BondGeneration;
use models::rust::casper::protocol::casper_message::{ProcessedSystemDeploy, SystemDeployData};
use prost::bytes::Bytes;

#[derive(arbitrary::Arbitrary, Debug)]
struct Input {
    invalid_block_hash: Vec<u8>,
    equivocation_block_hash: Option<[u8; models::rust::block_hash::LENGTH]>,
    issuer_public_key: Vec<u8>,
    target_activation_epoch: i64,
    target_bond_generation: u64,
}

fuzz_target!(|input: Input| {
    let slash = SystemDeployData::Slash {
        invalid_block_hash: Bytes::from(input.invalid_block_hash),
        equivocation_block_hash: input
            .equivocation_block_hash
            .map(|hash| Bytes::copy_from_slice(&hash)),
        issuer_public_key: PublicKey::from_bytes(&Bytes::from(input.issuer_public_key)),
        target_activation_epoch: input.target_activation_epoch,
        target_bond_generation: BondGeneration::new(
            i64::try_from(input.target_bond_generation).unwrap_or(i64::MAX),
        )
        .expect("nonnegative generation"),
    };
    let processed = ProcessedSystemDeploy::Succeeded {
        event_list: Vec::new(),
        system_deploy: slash,
        pre_state_hash: Vec::<u8>::new().into(),
        post_state_hash: Vec::<u8>::new().into(),
    };

    let proto = processed.clone().to_proto();
    let decoded = ProcessedSystemDeploy::from_proto(proto).expect("slash deploy roundtrip");

    assert_eq!(decoded, processed);
});
