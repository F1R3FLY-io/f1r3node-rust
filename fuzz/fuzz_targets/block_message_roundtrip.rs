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
