// See casper/src/test/scala/coop/rchain/casper/api/PreviewPrivateNameTest.scala

use casper::rust::api::block_api::BlockAPI;
use casper::rust::casper::{CURRENT_CASPER_PROTOCOL_VERSION, LEGACY_CASPER_PROTOCOL_VERSION};
use shared::rust::ByteString;

fn preview_legacy_id(pk_hex: &str, timestamp: i64, nth: i32) -> String {
    let deployer_bytes: ByteString = if pk_hex.is_empty() {
        vec![]
    } else {
        hex::decode(pk_hex).expect("Failed to decode hex")
    };

    let preview = BlockAPI::preview_private_names(
        &deployer_bytes,
        timestamp,
        nth + 1,
        LEGACY_CASPER_PROTOCOL_VERSION,
    )
    .expect("Failed to preview private names");

    hex::encode(&preview[nth as usize])
}

const MY_NODE_PK: &str = "464f6780d71b724525be14348b59c53dc8795346dfd7576c9f01c397ee7523e6";

#[test]
fn legacy_private_name_preview_should_match_the_first_golden_vector() {
    // Scala comments:
    // When we deploy `new x ...` code from a javascript gRPC client,
    // we get this private name id in the log:
    // 16:41:08.995 [node-runner-15] INFO  c.r.casper.MultiParentCasperImpl - Received Deploy #1542308065454 -- new x0, x1 in {
    //   @{x1}!(...
    // [Unforgeable(0xb5630d1bfb836635126ee7f2770873937933679e38146b1ddfbfcc14d7d8a787), bundle+ {   Unforgeable(0x00) }]
    // 2018-11-15T18:54:25.454Z
    assert_eq!(
        preview_legacy_id(MY_NODE_PK, 1542308065454, 0),
        "b5630d1bfb836635126ee7f2770873937933679e38146b1ddfbfcc14d7d8a787"
    );
}

#[test]
fn legacy_private_name_preview_should_match_another_timestamp() {
    assert_eq!(
        preview_legacy_id(MY_NODE_PK, 1542315551822, 0),
        "d472acf9c61e276e460de567a2b709bc9b97ff6135a812abcbaa60106d2744f9"
    );
}

#[test]
fn legacy_private_name_preview_should_handle_empty_user_public_key() {
    assert_eq!(
        preview_legacy_id("", 1542308065454, 0),
        "a249b81b82572b32e9a8adc9d708be08bc85fdf19e4aca3c316e51d30b97c993"
    );
}

#[test]
fn legacy_private_name_preview_should_match_the_second_name() {
    assert_eq!(
        preview_legacy_id(MY_NODE_PK, 1542308065454, 1),
        "cdaba23ba96f28c7f443a84086e260b839cc33068d0f685648ba2ae08fd7f9da"
    );
}

#[test]
fn preview_private_names_should_fail_closed_for_protocol_v6() {
    let deployer = hex::decode(MY_NODE_PK).expect("valid public key fixture");
    let error = BlockAPI::preview_private_names(
        &deployer,
        1542308065454,
        1,
        CURRENT_CASPER_PROTOCOL_VERSION,
    )
    .expect_err("protocol-v6 preview must fail closed");

    assert!(error
        .to_string()
        .contains("names are bound to the authenticated deploy envelope"));
}
