// Regression test for slash-deploy authorization error routing.
//
// Maps to: docs/theory/slashing/design/09-bug-fixes-and-rationale.md §9.14
// (subsection "Error routing").
//
// What this pins:
//   `Validate::slash_deploy_authorization` (validate.rs:934) splits the
//   outcome of `validate_received_slash_deploys` into two arms:
//     * `CasperError::SlashAuth(_)` — authorization-predicate failure;
//        block author is Byzantine → InvalidBlock::UnauthorizedSlashDeploy
//        (slashable, per the T-9.3 catch-all dispatcher).
//     * any other CasperError variant — local-infrastructure failure
//        (KvStoreError, BlockStoreError, HistoryError, RuntimeError)
//        → BlockError::BlockException, propagated WITHOUT slashing the
//        block sender.
//
// Pre-fix behavior (before commit landing this test): the
// `slash_deploy_authorization` match arm was `Err(_) => Invalid(...)`,
// collapsing every CasperError variant into a slashable verdict. A
// transient storage I/O hiccup during slash validation would wrongly
// slash the otherwise-honest block sender — a security-adjacent
// false-positive slashing path.
//
// We test the routing helper directly with synthesized CasperError
// values, because the storage-fault path is not naturally reachable
// from the DetectorFixture used by neighboring tests (the in-memory
// KV store doesn't fail under normal operation).

use casper::rust::block_status::{BlockError, InvalidBlock};
use casper::rust::errors::CasperError;
use casper::rust::slashing_authorization::SlashAuthError;
use casper::rust::validate::Validate;
use models::rust::casper::protocol::casper_message::{Body, BlockMessage, F1r3flyState, Header};
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::history::Either;
use shared::rust::store::key_value_store::KvStoreError;

fn fixture_block() -> BlockMessage {
    BlockMessage {
        block_hash: Bytes::from(vec![0xAB; 32]),
        header: Header {
            parents_hash_list: vec![],
            timestamp: 0,
            version: 0,
            extra_bytes: Bytes::new(),
        },
        body: Body {
            state: F1r3flyState {
                pre_state_hash: Bytes::new(),
                post_state_hash: Bytes::new(),
                bonds: vec![],
                block_number: 0,
            },
            deploys: vec![],
            rejected_deploys: vec![],
            system_deploys: vec![],
            extra_bytes: Bytes::new(),
        },
        justifications: vec![],
        sender: Bytes::from(vec![0xCD; 33]),
        seq_num: 1,
        sig: Bytes::from(vec![0xEF; 64]),
        sig_algorithm: "secp256k1".to_string(),
        shard_id: "root".to_string(),
        extra_bytes: Bytes::new(),
    }
}

#[test]
fn slash_auth_error_routes_to_unauthorized_slash_deploy() {
    let block = fixture_block();
    let auth_err = CasperError::SlashAuth(SlashAuthError::IssuerMismatch {
        block_hash: hex::encode(&block.block_hash),
        issuer: "01abcd".to_string(),
        sender: "02ef01".to_string(),
    });

    let outcome = Validate::route_slash_validation_outcome(&block, Err(auth_err));

    assert!(
        matches!(
            outcome,
            Either::Left(BlockError::Invalid(InvalidBlock::UnauthorizedSlashDeploy))
        ),
        "SlashAuth error must produce slashable UnauthorizedSlashDeploy verdict; got {:?}",
        outcome
    );
}

#[test]
fn kv_store_error_routes_to_block_exception_not_slashable() {
    let block = fixture_block();
    let kv_err = CasperError::KvStoreError(KvStoreError::IoError(
        "simulated transient storage failure during slash validation".to_string(),
    ));

    let outcome = Validate::route_slash_validation_outcome(&block, Err(kv_err));

    match outcome {
        Either::Left(BlockError::BlockException(CasperError::KvStoreError(KvStoreError::IoError(msg)))) => {
            assert!(
                msg.contains("simulated transient storage failure"),
                "BlockException must preserve the underlying KvStoreError payload; got msg={msg:?}"
            );
        }
        other => panic!(
            "KvStoreError must propagate as BlockException(KvStoreError::IoError), NOT as \
             Invalid(UnauthorizedSlashDeploy). Honest block senders must not be slashed for \
             local infrastructure failures. Got: {other:?}"
        ),
    }
}

#[test]
fn runtime_error_routes_to_block_exception_not_slashable() {
    let block = fixture_block();
    let runtime_err = CasperError::RuntimeError(
        "simulated runtime fault during slash validation".to_string(),
    );

    let outcome = Validate::route_slash_validation_outcome(&block, Err(runtime_err));

    assert!(
        matches!(
            outcome,
            Either::Left(BlockError::BlockException(CasperError::RuntimeError(_)))
        ),
        "RuntimeError must propagate as BlockException; got {:?}",
        outcome
    );
    // Negative assertion: must NOT be a slashable verdict.
    assert!(
        !matches!(
            outcome,
            Either::Left(BlockError::Invalid(InvalidBlock::UnauthorizedSlashDeploy))
        ),
        "RuntimeError must NOT be misrouted as UnauthorizedSlashDeploy"
    );
}

#[test]
fn ok_routes_to_valid() {
    let block = fixture_block();
    let outcome = Validate::route_slash_validation_outcome(&block, Ok(()));
    assert!(
        matches!(outcome, Either::Right(_)),
        "Ok must produce ValidBlock; got {:?}",
        outcome
    );
}

#[test]
fn epoch_mismatch_routes_to_unauthorized_slash_deploy() {
    use casper::rust::epoch::Epoch;
    let block = fixture_block();
    let auth_err = CasperError::SlashAuth(SlashAuthError::EpochMismatch {
        target: Epoch::new(0),
        current: Epoch::new(2),
    });

    let outcome = Validate::route_slash_validation_outcome(&block, Err(auth_err));

    assert!(
        matches!(
            outcome,
            Either::Left(BlockError::Invalid(InvalidBlock::UnauthorizedSlashDeploy))
        ),
        "EpochMismatch (a SlashAuthError variant) must produce UnauthorizedSlashDeploy; got {:?}",
        outcome
    );
}
