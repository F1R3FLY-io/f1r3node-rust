//! `block_message_roundtrip` — proto idempotency for the full `BlockMessage`.
//!
//! Invariant asserted: `from_proto ∘ to_proto ∘ from_proto = from_proto`.
//! That is, once arbitrary bytes successfully decode into a `BlockMessage`,
//! re-encoding and re-decoding must produce the same in-memory value. A
//! failure here indicates a non-canonical proto encoding (a field that
//! round-trips under one ordering but not another) — exactly the class of
//! bug that silently forks consensus.
//!
//! The two early `return`s filter out malformed bytes: libFuzzer's
//! coverage-guided search steers around these returns toward bytes that
//! successfully decode, so the body downstream runs only on well-formed
//! inputs. We do not assert on malformed bytes — that's the job of
//! `slash_authorization_paths` and the property tests.

#![no_main]

use libfuzzer_sys::fuzz_target;
use models::casper::BlockMessageProto;
use models::rust::casper::protocol::casper_message::BlockMessage;
use prost::Message;

fuzz_target!(|data: &[u8]| {
    let Ok(proto) = BlockMessageProto::decode(data) else {
        return;
    };
    let Ok(block) = BlockMessage::from_proto(proto) else {
        return;
    };

    let normalized = block.to_proto();
    let reparsed = BlockMessage::from_proto(normalized).expect("normalized block reparses");

    assert_eq!(reparsed, block);
});
