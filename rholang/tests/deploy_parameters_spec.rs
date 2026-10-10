use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signed::Signed;
use models::rust::casper::protocol::casper_message::DeployData;
use models::rust::deploy_parameters::{DeployMapEntry, DeployParameter, RholangValue};
use models::rust::normalizer_env::normalizer_env_from_deploy;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::test_utils::resources::with_runtime;

#[tokio::test]
async fn deploy_parameters_execute_as_rholang_values() {
    with_runtime("deploy-parameters-", |mut runtime| async move {
        let cases = vec![
            (RholangValue::Bool(false), "false"),
            (RholangValue::Int(-42), "-42"),
            (RholangValue::String("text".into()), "\"text\""),
            (RholangValue::Bytes(vec![0, 255]), "\"00ff\".hexToBytes()"),
            (
                RholangValue::Tuple(vec![RholangValue::Int(1), RholangValue::Nil]),
                "(1, Nil)",
            ),
            (
                RholangValue::List(vec![RholangValue::Bool(true), RholangValue::Nil]),
                "[true, Nil]",
            ),
            (
                RholangValue::Set(vec![
                    RholangValue::Int(2),
                    RholangValue::Int(1),
                    RholangValue::Int(1),
                ]),
                "Set(1, 2)",
            ),
            (
                RholangValue::Map(vec![DeployMapEntry {
                    key: RholangValue::Tuple(vec![RholangValue::Int(1), RholangValue::Int(2)]),
                    value: RholangValue::List(vec![
                        RholangValue::Bytes(vec![255]),
                        RholangValue::Nil,
                    ]),
                }]),
                "{(1, 2): [\"ff\".hexToBytes(), Nil]}",
            ),
            (RholangValue::List(Vec::new()), "[]"),
            (RholangValue::Set(Vec::new()), "Set()"),
            (RholangValue::Map(Vec::new()), "{}"),
            (RholangValue::Nil, "Nil"),
            (RholangValue::Uri("rho:io:stdout".into()), "`rho:io:stdout`"),
        ];
        for (index, (value, literal)) in cases.into_iter().enumerate() {
            let term =
                format!("new input(`rho:deploy:param:input`) in {{ @\"actual{index}\"!(*input) }}");
            let deploy = Signed::create(
                DeployData {
                    term,
                    time_stamp: 1,
                    phlo_price: 1,
                    phlo_limit: 1_000_000,
                    valid_after_block_number: 0,
                    shard_id: "root".into(),
                    expiration_timestamp: None,
                    parameters: vec![DeployParameter {
                        name: "input".into(),
                        value,
                    }],
                },
                Box::new(Secp256k1),
                PrivateKey::from_bytes(&[1; 32]),
            )
            .unwrap();
            let deploy = DeployData::from_proto(DeployData::to_proto(deploy)).unwrap();
            let result = runtime
                .evaluate_with_env(
                    &deploy.data.term,
                    normalizer_env_from_deploy(&deploy).unwrap(),
                )
                .await
                .unwrap();
            assert!(result.errors.is_empty(), "{literal}: {:?}", result.errors);
            let result = runtime
                .evaluate_with_term(&format!("@\"expected{index}\"!({literal})"))
                .await
                .unwrap();
            assert!(result.errors.is_empty(), "{literal}: {:?}", result.errors);
            let actual = runtime
                .get_data(&RholangValue::String(format!("actual{index}")).to_par())
                .await;
            let expected = runtime
                .get_data(&RholangValue::String(format!("expected{index}")).to_par())
                .await;
            assert_eq!(actual.len(), 1, "{literal}");
            assert_eq!(expected.len(), 1, "{literal}");
            assert_eq!(actual[0].a.pars, expected[0].a.pars, "{literal}");
        }
    })
    .await;
}
