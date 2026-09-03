// Deep-coverage tests for system_processes.rs bodies reachable without
// network access: io channels, vault address ops, registry ops (legacy and
// v1), auth token ops, block/deploy data, invalid blocks, and the test
// framework contracts (stdlog, deployerId:make, secp sign, authToken:make,
// block data set, invalidBlocks set, assertAck).

use std::collections::HashMap;

use crypto::rust::hash::blake2b256::Blake2b256;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};
use rholang::rust::interpreter::registry::registry::Registry;
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::system_processes::{
    test_framework_contracts, BlockData, DeployAuthority, DeployData, FixedChannels,
};
use rholang::rust::interpreter::test_utils::resources::{create_runtimes, with_runtime};
use rholang::rust::interpreter::util::vault_address::VaultAddress;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

fn string_channel(name: &str) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GString(name.to_string())),
        }],
        ..Default::default()
    }
}

fn gint_par(value: i64) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GInt(value)),
        }],
        ..Default::default()
    }
}

fn byte_array_par(bytes: Vec<u8>) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GByteArray(bytes)),
        }],
        ..Default::default()
    }
}

fn gstring_par(value: &str) -> Par { string_channel(value) }

fn guri_par(value: String) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GUri(value)),
        }],
        ..Default::default()
    }
}

fn gbool_par(value: bool) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GBool(value)),
        }],
        ..Default::default()
    }
}

async fn eval_ok(runtime: &mut RhoRuntimeImpl, term: &str) {
    let res = runtime.evaluate_with_term(term).await.unwrap();
    assert!(
        res.errors.is_empty(),
        "Expected success for: {}\nErrors: {:?}",
        term,
        res.errors
    );
}

async fn eval_err(runtime: &mut RhoRuntimeImpl, term: &str) {
    let res = runtime.evaluate_with_term(term).await.unwrap();
    assert!(
        !res.errors.is_empty(),
        "Expected error for: {}\nGot success",
        term
    );
}

async fn data_at(runtime: &RhoRuntimeImpl, name: &str) -> Vec<Par> {
    runtime
        .get_data(&string_channel(name))
        .await
        .into_iter()
        .flat_map(|d| d.a.pars)
        .collect()
}

async fn framework_runtime() -> RhoRuntimeImpl {
    let mut kvm = InMemoryStoreManager::new();
    let stores = kvm.r_space_stores().await.unwrap();
    let (runtime, _replay, _history) =
        create_runtimes(stores, false, &mut test_framework_contracts()).await;
    runtime
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdout_stderr_channels_and_acks() {
    with_runtime("sysproc-", |mut runtime| async move {
        eval_ok(
            &mut runtime,
            "new out(`rho:io:stdout`) in { out!(\"stdout says hi\") }",
        )
        .await;
        eval_ok(
            &mut runtime,
            "new err(`rho:io:stderr`) in { err!(\"stderr says hi\") }",
        )
        .await;
        eval_ok(
            &mut runtime,
            "new out(`rho:io:stdoutAck`) in { out!(\"acked out\", \"out-ack\") }",
        )
        .await;
        assert_eq!(data_at(&runtime, "out-ack").await, vec![Par::default()]);
        eval_ok(
            &mut runtime,
            "new err(`rho:io:stderrAck`) in { err!(\"acked err\", \"err-ack\") }",
        )
        .await;
        assert_eq!(data_at(&runtime, "err-ack").await, vec![Par::default()]);
        eval_ok(&mut runtime, "new dn(`rho:io:devNull`) in { dn!(42) }").await;
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn vault_address_operations() {
    with_runtime("sysproc-", |mut runtime| async move {
        let secp = Secp256k1;
        let (_sec, public) = secp.new_key_pair();
        let pk_hex = hex::encode(&public.bytes);
        let address = VaultAddress::from_public_key(&PublicKey::from_bytes(&public.bytes))
            .expect("address derivable from a real secp256k1 public key")
            .to_base58();

        eval_ok(
            &mut runtime,
            &format!(
                "new v(`rho:vault:address`) in {{ v!(\"fromPublicKey\", \"{}\".hexToBytes(), \"from-pk\") }}",
                pk_hex
            ),
        )
        .await;
        assert_eq!(data_at(&runtime, "from-pk").await, vec![gstring_par(&address)]);

        eval_ok(
            &mut runtime,
            &format!(
                "new v(`rho:vault:address`) in {{ v!(\"validate\", \"{}\", \"validated\") }}",
                address
            ),
        )
        .await;
        assert_eq!(data_at(&runtime, "validated").await, vec![Par::default()]);

        eval_ok(
            &mut runtime,
            "new v(`rho:vault:address`) in { v!(\"validate\", \"garbage-address\", \"invalid\") }",
        )
        .await;
        let invalid = data_at(&runtime, "invalid").await;
        assert_eq!(invalid.len(), 1);
        assert!(invalid[0]
            .exprs
            .iter()
            .any(|e| matches!(e.expr_instance, Some(ExprInstance::GString(_)))));

        eval_ok(
            &mut runtime,
            "new v(`rho:vault:address`) in { v!(\"validate\", 42, \"wrong-type\") }",
        )
        .await;
        assert_eq!(data_at(&runtime, "wrong-type").await, vec![Par::default()]);

        eval_ok(
            &mut runtime,
            "new unf, v(`rho:vault:address`) in { v!(\"fromUnforgeable\", *unf, \"from-unf\") }",
        )
        .await;
        let from_unf = data_at(&runtime, "from-unf").await;
        assert_eq!(from_unf.len(), 1);
        assert!(from_unf[0]
            .exprs
            .iter()
            .any(|e| matches!(e.expr_instance, Some(ExprInstance::GString(_)))));

        eval_err(
            &mut runtime,
            "new v(`rho:vault:address`) in { v!(\"unknownCommand\", 1, \"x\") }",
        )
        .await;
        eval_err(
            &mut runtime,
            "new v(`rho:vault:address`) in { v!(42, 1, \"x\") }",
        )
        .await;
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn registry_ops_build_uri() {
    with_runtime("sysproc-", |mut runtime| async move {
        let bytes = vec![0xde, 0xad, 0xbe, 0xef];
        let expected = guri_par(Registry::build_uri(&Blake2b256::hash(bytes)));

        eval_ok(
            &mut runtime,
            "new ops(`rho:registry:ops`) in { ops!(\"buildUri\", \"deadbeef\".hexToBytes(), \"uri-out\") }",
        )
        .await;
        assert_eq!(data_at(&runtime, "uri-out").await, vec![expected.clone()]);

        eval_ok(
            &mut runtime,
            "new ops(`rho:registry:ops`) in { ops!(\"buildUri\", 42, \"uri-default\") }",
        )
        .await;
        assert_eq!(data_at(&runtime, "uri-default").await, vec![Par::default()]);

        eval_err(
            &mut runtime,
            "new ops(`rho:registry:ops`) in { ops!(\"unknownOp\", 1, \"x\") }",
        )
        .await;

        eval_ok(
            &mut runtime,
            "new ops(`rho:registry:ops:1.0.0`) in { ops!(\"buildUri\", \"deadbeef\".hexToBytes(), \"uri-v1\") }",
        )
        .await;
        assert_eq!(data_at(&runtime, "uri-v1").await, vec![expected]);
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn registry_ops_v1_version_helpers() {
    with_runtime("sysproc-", |mut runtime| async move {
        eval_ok(
            &mut runtime,
            "new ops(`rho:registry:ops:1.0.0`) in { ops!(\"parseVersionedUri\", \"rho:lib:1.0.0:abc123:myproj:2.6.3\", \"parsed\") }",
        )
        .await;
        let parsed = data_at(&runtime, "parsed").await;
        assert_eq!(parsed.len(), 1);
        match parsed[0].exprs.first().and_then(|e| e.expr_instance.as_ref()) {
            Some(ExprInstance::ETupleBody(tuple)) => {
                assert_eq!(tuple.ps.len(), 5);
                assert!(tuple.ps.contains(&gstring_par("myproj")));
                assert!(tuple.ps.contains(&gstring_par("1.0.0")));
            }
            other => panic!("expected 5-tuple from parseVersionedUri, got {:?}", other),
        }

        eval_ok(
            &mut runtime,
            "new ops(`rho:registry:ops:1.0.0`) in { ops!(\"parseVersionedUri\", \"not a urn\", \"parse-bad\") }",
        )
        .await;
        assert_eq!(data_at(&runtime, "parse-bad").await, vec![Par::default()]);

        eval_ok(
            &mut runtime,
            "new ops(`rho:registry:ops:1.0.0`) in { ops!(\"matchesVersion\", (\"1.*\", \"1.2.3\"), \"mv-yes\") }",
        )
        .await;
        assert_eq!(data_at(&runtime, "mv-yes").await, vec![gbool_par(true)]);

        eval_ok(
            &mut runtime,
            "new ops(`rho:registry:ops:1.0.0`) in { ops!(\"matchesVersion\", (\"2.*\", \"1.2.3\"), \"mv-no\") }",
        )
        .await;
        assert_eq!(data_at(&runtime, "mv-no").await, vec![gbool_par(false)]);

        eval_ok(
            &mut runtime,
            "new ops(`rho:registry:ops:1.0.0`) in { ops!(\"matchesVersion\", 42, \"mv-bad\") }",
        )
        .await;
        assert_eq!(data_at(&runtime, "mv-bad").await, vec![gbool_par(false)]);

        eval_ok(
            &mut runtime,
            "new ops(`rho:registry:ops:1.0.0`) in { ops!(\"selectBestVersion\", (\"1.*\", [\"1.0.0\", \"1.2.0\", \"2.0.0\"]), \"best\") }",
        )
        .await;
        assert_eq!(data_at(&runtime, "best").await, vec![gstring_par("1.2.0")]);

        eval_ok(
            &mut runtime,
            "new ops(`rho:registry:ops:1.0.0`) in { ops!(\"selectBestVersion\", (\"3.*\", [\"1.0.0\"]), \"best-none\") }",
        )
        .await;
        assert_eq!(data_at(&runtime, "best-none").await, vec![Par::default()]);

        eval_err(
            &mut runtime,
            "new ops(`rho:registry:ops:1.0.0`) in { ops!(\"unknownOp\", 1, \"x\") }",
        )
        .await;
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn registry_lookup_serves_legacy_and_delegates_versioned_urns() {
    with_runtime("sysproc-", |mut runtime| async move {
        eval_ok(
            &mut runtime,
            "new rl(`rho:internal:registry_lookup`) in { rl!(\"rho:io:stdout\", \"legacy-out\") }",
        )
        .await;
        assert_eq!(
            data_at(&runtime, "legacy-out").await.len(),
            1,
            "a legacy URN must be served directly from the urn map"
        );

        eval_ok(
            &mut runtime,
            "new rl(`rho:internal:registry_lookup`) in { rl!(\"rho:lib:1.0.0:abc123:myproj:1.0.0\", \"ver-out\") }",
        )
        .await;
        let delegated = runtime.get_data(&FixedChannels::reg_v1_internal()).await;
        assert!(
            !delegated.is_empty(),
            "versioned URN lookup should park a lookupVersion request on the v1 internal channel"
        );
        let pars: Vec<Par> = delegated.into_iter().flat_map(|d| d.a.pars).collect();
        assert!(pars.contains(&gstring_par("lookupVersion")));

        eval_err(
            &mut runtime,
            "new rl(`rho:internal:registry_lookup`) in { rl!(\"rho:completely:unknown\", \"x\") }",
        )
        .await;
        eval_err(
            &mut runtime,
            "new rl(`rho:internal:registry_lookup`) in { rl!(42, \"x\") }",
        )
        .await;
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sys_auth_token_check_rejects_non_token() {
    with_runtime("sysproc-", |mut runtime| async move {
        eval_ok(
            &mut runtime,
            "new ops(`sys:authToken:ops`) in { ops!(\"check\", 42, \"checked\") }",
        )
        .await;
        assert_eq!(data_at(&runtime, "checked").await, vec![gbool_par(false)]);
        eval_err(
            &mut runtime,
            "new ops(`sys:authToken:ops`) in { ops!(\"notCheck\", 42, \"x\") }",
        )
        .await;
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn block_data_deploy_data_and_invalid_blocks() {
    with_runtime("sysproc-", |mut runtime| async move {
        let sender_bytes = vec![9u8, 8, 7];
        runtime
            .set_block_data(BlockData {
                block_number: 42,
                time_stamp: 1234,
                sender: PublicKey::from_bytes(&sender_bytes),
                seq_num: 1,
            })
            .await;
        eval_ok(
            &mut runtime,
            "new bd(`rho:block:data`) in { bd!(\"bd-out\") }",
        )
        .await;
        assert_eq!(
            data_at(&runtime, "bd-out").await,
            vec![
                gint_par(42),
                gint_par(1234),
                byte_array_par(sender_bytes.clone())
            ]
        );

        let deployer_bytes = vec![1u8, 2, 3, 4];
        runtime
            .set_deploy_data(DeployData {
                timestamp: 777,
                authority: DeployAuthority::Legacy(PublicKey::from_bytes(&deployer_bytes)),
                deploy_id: vec![5, 6],
            })
            .await;
        eval_ok(
            &mut runtime,
            "new dd(`rho:deploy:data`) in { dd!(\"dd-out\") }",
        )
        .await;
        let deploy_data = data_at(&runtime, "dd-out").await;
        assert_eq!(deploy_data.len(), 3);
        assert_eq!(deploy_data[0], gint_par(777));

        eval_ok(
            &mut runtime,
            "for (@_, @deployer, @_ <- @\"dd-out\") { new ops(`rho:system:deployerId:ops`) in { ops!(\"pubKeyBytes\", deployer, \"pk-out\") } }",
        )
        .await;
        assert_eq!(
            data_at(&runtime, "pk-out").await,
            vec![byte_array_par(deployer_bytes)]
        );

        runtime.set_invalid_blocks(HashMap::new()).await;
        eval_ok(
            &mut runtime,
            "new ib(`rho:casper:invalidBlocks`) in { ib!(\"ib-out\") }",
        )
        .await;
        assert_eq!(data_at(&runtime, "ib-out").await.len(), 1);
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn crypto_channel_type_errors() {
    with_runtime("sysproc-", |mut runtime| async move {
        eval_err(
            &mut runtime,
            "new h(`rho:crypto:sha256Hash`) in { h!(\"not bytes\", \"ack\") }",
        )
        .await;
        eval_err(
            &mut runtime,
            "new v(`rho:crypto:ed25519Verify`) in { v!(1, 2, 3, \"ack\") }",
        )
        .await;
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdlog_levels_and_errors() {
    let mut runtime = framework_runtime().await;
    eval_ok(
        &mut runtime,
        "new log(`rho:io:stdlog`) in { log!(\"trace\", \"t\") | log!(\"debug\", \"d\") | log!(\"info\", \"i\") | log!(\"warn\", \"w\") | log!(\"error\", \"e\") }",
    )
    .await;
    eval_err(
        &mut runtime,
        "new log(`rho:io:stdlog`) in { log!(\"loudly\", \"nope\") }",
    )
    .await;
    eval_err(
        &mut runtime,
        "new log(`rho:io:stdlog`) in { log!(42, \"nope\") }",
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn deployer_id_make_round_trips_through_ops() {
    let mut runtime = framework_runtime().await;
    eval_ok(
        &mut runtime,
        "new mk(`rho:test:deployerId:make`) in { mk!(\"deployerId\", \"aabb\".hexToBytes(), \"did-out\") }",
    )
    .await;
    let made = data_at(&runtime, "did-out").await;
    assert_eq!(made.len(), 1);

    eval_ok(
        &mut runtime,
        "for (@deployer <- @\"did-out\") { new ops(`rho:system:deployerId:ops`) in { ops!(\"pubKeyBytes\", deployer, \"bytes-out\") } }",
    )
    .await;
    assert_eq!(data_at(&runtime, "bytes-out").await, vec![byte_array_par(
        vec![0xaa, 0xbb]
    )]);

    eval_err(
        &mut runtime,
        "new mk(`rho:test:deployerId:make`) in { mk!(\"wrongTag\", \"aabb\".hexToBytes(), \"x\") }",
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn secp256k1_sign_produces_der_signature() {
    let mut runtime = framework_runtime().await;
    let secp = Secp256k1;
    let (sec, _public) = secp.new_key_pair();
    let hash = Blake2b256::hash(b"payload".to_vec());
    eval_ok(
        &mut runtime,
        &format!(
            "new sign(`rho:test:crypto:secp256k1Sign`) in {{ sign!(\"{}\".hexToBytes(), \"{}\".hexToBytes(), \"sig-out\") }}",
            hex::encode(&hash),
            hex::encode(&sec.bytes)
        ),
    )
    .await;
    let signature = data_at(&runtime, "sig-out").await;
    assert_eq!(signature.len(), 1);
    match signature[0]
        .exprs
        .first()
        .and_then(|e| e.expr_instance.as_ref())
    {
        Some(ExprInstance::GByteArray(der)) => assert!(!der.is_empty()),
        other => panic!("expected DER byte array, got {:?}", other),
    }

    eval_err(
        &mut runtime,
        "new sign(`rho:test:crypto:secp256k1Sign`) in { sign!(\"aa\".hexToBytes(), \"bb\".hexToBytes(), \"x\") }",
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn auth_token_make_then_check_is_true() {
    let mut runtime = framework_runtime().await;
    eval_ok(
        &mut runtime,
        "new mk(`sys:test:authToken:make`) in { mk!(\"token-out\") }",
    )
    .await;
    eval_ok(
        &mut runtime,
        "for (@token <- @\"token-out\") { new ops(`sys:authToken:ops`) in { ops!(\"check\", token, \"check-out\") } }",
    )
    .await;
    assert_eq!(data_at(&runtime, "check-out").await, vec![gbool_par(true)]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn block_data_set_then_get_round_trips() {
    let mut runtime = framework_runtime().await;
    eval_ok(
        &mut runtime,
        "new set(`rho:test:block:data:set`) in { set!(\"blockNumber\", 99, \"ack1\") }",
    )
    .await;
    assert_eq!(data_at(&runtime, "ack1").await, vec![Par::default()]);
    eval_ok(
        &mut runtime,
        "new set(`rho:test:block:data:set`) in { set!(\"sender\", \"aabbcc\".hexToBytes(), \"ack2\") }",
    )
    .await;
    eval_ok(
        &mut runtime,
        "new get(`rho:block:data`) in { get!(\"bd-out\") }",
    )
    .await;
    let block_data = data_at(&runtime, "bd-out").await;
    assert_eq!(block_data.len(), 3);
    assert_eq!(block_data[0], gint_par(99));
    assert_eq!(block_data[2], byte_array_par(vec![0xaa, 0xbb, 0xcc]));

    eval_err(
        &mut runtime,
        "new set(`rho:test:block:data:set`) in { set!(\"bogusKey\", 1, \"x\") }",
    )
    .await;
    eval_err(
        &mut runtime,
        "new set(`rho:test:block:data:set`) in { set!(\"blockNumber\", \"not a number\", \"x\") }",
    )
    .await;
    eval_err(
        &mut runtime,
        "new set(`rho:test:block:data:set`) in { set!(42, 1, \"x\") }",
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn casper_invalid_blocks_set_then_get() {
    let mut runtime = framework_runtime().await;
    eval_ok(
        &mut runtime,
        "new set(`rho:test:casper:invalidBlocks:set`) in { set!({\"blockHash\": \"validator\"}, \"ack\") }",
    )
    .await;
    assert_eq!(data_at(&runtime, "ack").await, vec![Par::default()]);
    eval_ok(
        &mut runtime,
        "new get(`rho:casper:invalidBlocks`) in { get!(\"ib-out\") }",
    )
    .await;
    let invalid_blocks = data_at(&runtime, "ib-out").await;
    assert_eq!(invalid_blocks.len(), 1);
    assert!(invalid_blocks[0]
        .exprs
        .iter()
        .any(|e| matches!(e.expr_instance, Some(ExprInstance::EMapBody(_)))));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn assert_ack_comparisons_and_suite_completion() {
    let mut runtime = framework_runtime().await;
    eval_ok(
        &mut runtime,
        "new assert(`rho:test:assertAck`) in { assert!(\"eq test\", 1, (1, \"==\", 1), \"one equals one\", \"a1\") }",
    )
    .await;
    assert_eq!(data_at(&runtime, "a1").await, vec![gbool_par(true)]);

    eval_ok(
        &mut runtime,
        "new assert(`rho:test:assertAck`) in { assert!(\"neq test\", 1, (1, \"!=\", 2), \"one is not two\", \"a2\") }",
    )
    .await;
    assert_eq!(data_at(&runtime, "a2").await, vec![gbool_par(true)]);

    eval_ok(
        &mut runtime,
        "new assert(`rho:test:assertAck`) in { assert!(\"bool test\", 1, true, \"trivially true\", \"a3\") }",
    )
    .await;
    assert_eq!(data_at(&runtime, "a3").await, vec![gbool_par(true)]);

    eval_err(
        &mut runtime,
        "new assert(`rho:test:assertAck`) in { assert!(\"bad assertion\", 1, [1], \"not an assertion\", \"a4\") }",
    )
    .await;

    eval_err(
        &mut runtime,
        "new assert(`rho:test:assertAck`) in { assert!(\"bad op\", 1, (1, \"<>\", 2), \"unknown operator\", \"a5\") }",
    )
    .await;

    eval_ok(
        &mut runtime,
        "new done(`rho:test:testSuiteCompleted`) in { done!(true) }",
    )
    .await;
}
