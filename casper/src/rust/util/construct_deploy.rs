// See casper/src/main/scala/coop/rchain/casper/util/ConstructDeploy.scala

use std::time::{SystemTime, UNIX_EPOCH};

use crypto::rust::private_key::PrivateKey;
#[cfg(any(test, feature = "test-utils"))]
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
#[cfg(any(test, feature = "test-utils"))]
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
#[cfg(any(test, feature = "test-utils"))]
use crypto::rust::signatures::signed::Cosigned;
use crypto::rust::signatures::signed::Signed;
#[cfg(any(test, feature = "test-utils"))]
use lazy_static::lazy_static;
use models::rhoapi::PCost;
use models::rust::casper::protocol::casper_message::{DeployData, ProcessedDeploy};

use crate::rust::errors::CasperError;

#[cfg(any(test, feature = "test-utils"))]
lazy_static! {
    pub static ref DEFAULT_SEC: PrivateKey = PrivateKey::from_bytes(
        &hex::decode("a68a6e6cca30f81bd24a719f3145d20e8424bd7b396309b0708a16c7d8000b76")
            .expect("ConstructDeploy: Failed to decode default private key")
    );
    pub static ref DEFAULT_PUB: PublicKey = {
        let secp = Secp256k1;
        secp.to_public(&DEFAULT_SEC)
    };
    pub static ref DEFAULT_KEY_PAIR: (&'static PrivateKey, &'static PublicKey) =
        (&DEFAULT_SEC, &DEFAULT_PUB);
    pub static ref DEFAULT_SEC2: PrivateKey = PrivateKey::from_bytes(
        &hex::decode("5a0bde2f5857124b1379c78535b07a278e3b9cefbcacc02e62ab3294c02765a1")
            .expect("ConstructDeploy: Failed to decode default private key")
    );
    pub static ref DEFAULT_PUB2: PublicKey = {
        let secp = Secp256k1;
        secp.to_public(&DEFAULT_SEC2)
    };
}

// D3 (DR-9, refined by DR-31 and DR-47): `phlo_limit` / `phlo_price` are
// retained as ignored parameters for test-caller signature stability. A deploy
// no longer carries a client-selected escrow limit or price; protocol-4 cost is
// measured under the finite capacity derived from authenticated authority.
pub fn source_deploy(
    source: String,
    timestamp: i64,
    _phlo_limit: Option<i64>,
    _phlo_price: Option<i64>,
    sec: Option<PrivateKey>,
    valid_after_block_number: Option<i64>,
    shard_id: Option<String>,
) -> Result<Signed<DeployData>, CasperError> {
    #[cfg(any(test, feature = "test-utils"))]
    let sec = sec.unwrap_or_else(|| DEFAULT_SEC.clone());
    #[cfg(not(any(test, feature = "test-utils")))]
    let sec = sec.expect("ConstructDeploy: private key is required");
    let valid_after_block_number = valid_after_block_number.unwrap_or(0);
    let shard_id = shard_id.unwrap_or_default();

    let data = DeployData {
        term: source,
        language: "rholang".to_string(),
        time_stamp: timestamp,
        valid_after_block_number,
        shard_id,
        expiration_timestamp: None,
        authority_presentations: Vec::new(),
    };

    Signed::create(data, Box::new(Secp256k1), sec).map_err(|e| CasperError::SigningError(e))
}

pub fn source_deploy_now(
    source: String,
    sec: Option<PrivateKey>,
    valid_after_block_number: Option<i64>,
    shard_id: Option<String>,
) -> Result<Signed<DeployData>, CasperError> {
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as i64;

    source_deploy(
        source,
        timestamp,
        None,
        None,
        sec,
        valid_after_block_number,
        shard_id,
    )
}

pub fn source_deploy_now_full(
    source: String,
    phlo_limit: Option<i64>,
    phlo_price: Option<i64>,
    sec: Option<PrivateKey>,
    valid_after_block_number: Option<i64>,
    shard_id: Option<String>,
) -> Result<Signed<DeployData>, CasperError> {
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as i64;

    source_deploy(
        source,
        timestamp,
        phlo_limit,
        phlo_price,
        sec,
        valid_after_block_number,
        shard_id,
    )
}

pub fn basic_deploy_data(
    id: i32,
    sec: Option<PrivateKey>,
    shard_id: Option<String>,
) -> Result<Signed<DeployData>, CasperError> {
    source_deploy_now(format!("@{}!({})", id, id), sec, None, shard_id)
}

#[cfg(any(test, feature = "test-utils"))]
pub fn envelope_from_deploy_data(
    data: DeployData,
    sec: Option<PrivateKey>,
) -> Result<Cosigned<DeployData>, CasperError> {
    Cosigned::create_single_envelope(
        data,
        Box::new(Secp256k1),
        sec.unwrap_or_else(|| DEFAULT_SEC.clone()),
    )
    .map_err(|error| CasperError::SigningError(error.to_string()))
}

pub fn basic_processed_deploy(
    id: i32,
    shard_id: Option<String>,
) -> Result<ProcessedDeploy, CasperError> {
    basic_deploy_data(id, None, shard_id).map(|deploy| ProcessedDeploy {
        deploy,
        envelope_commitment: prost::bytes::Bytes::new(),
        cost: PCost { cost: 0 },
        deploy_log: Vec::new(),
        is_failed: false,
        system_deploy_error: None,
        cosigners: Vec::new(),
        cosigner_threshold: 0,
        pre_state_hash: prost::bytes::Bytes::new(),
        post_state_hash: prost::bytes::Bytes::new(),
        authority_funding_certificate: None,
        authority_cost_witness: None,
        admission_status: Default::default(),
    })
}
