use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signed::Signed;
use models::rust::casper::protocol::casper_message::DeployData;
use prost::Message;

#[test]
fn legacy_casper_accepts_legacy_wire_and_rejects_cost_authorization() {
    let body = DeployData {
        term: "Nil".to_string(),
        time_stamp: 1,
        phlo_price: 1,
        phlo_limit: 100,
        valid_after_block_number: 0,
        shard_id: "root".to_string(),
        expiration_timestamp: None,
    };
    let signed = Signed::create(
        body.clone(),
        Box::new(Secp256k1),
        PrivateKey::from_bytes(&[1; 32]),
    )
    .unwrap();
    let legacy = DeployData::to_proto(signed);
    assert_eq!(DeployData::from_proto(legacy.clone()).unwrap().data, body);

    let mut funded = legacy.clone();
    funded.funding_intent = Some(vec![1].into());
    assert!(DeployData::from_proto(funded.clone()).is_err());
    assert!(DeployData::decode(funded.encode_to_vec()).is_err());

    let mut bound = legacy;
    bound.deploy_id = vec![1; 32].into();
    assert!(DeployData::from_proto(bound).is_err());
}
