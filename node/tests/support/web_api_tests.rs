use models::rhoapi::expr::ExprInstance;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{
    Bundle, EList, EMap, ESet, ETuple, GDeployId, GDeployerId, GPrivate, KeyValuePair,
};

use super::*;

#[test]
fn test_deploy_response_full_view_includes_all_fields() {
    let response = DeployResponse {
        deploy_id: "abc123".to_string(),
        block_hash: "hash1".to_string(),
        block_number: 100,
        timestamp: 1700000000000,
        cost: 500,
        errored: false,
        is_finalized: true,
        deployer: Some("deployer1".to_string()),
        term: Some("new ret in { ret!(42) }".to_string()),
        system_deploy_error: Some(String::new()),
        sig_algorithm: Some("secp256k1".to_string()),
        valid_after_block_number: Some(0),
        transfers: Some(vec![]),
    };

    let json = serde_json::to_value(&response).unwrap();

    assert_eq!(json["deployId"], "abc123");
    assert_eq!(json["blockHash"], "hash1");
    assert_eq!(json["blockNumber"], 100);
    assert_eq!(json["cost"], 500);
    assert_eq!(json["isFinalized"], true);
    assert!(json.get("deployer").is_some());
    assert!(json.get("term").is_some());
    // D3 (DR-9): no phloPrice / phloLimit in the response.
    assert!(json.get("phloPrice").is_none());
    assert!(json.get("phloLimit").is_none());
    assert!(json.get("transfers").is_some());
}

#[test]
fn test_deploy_response_summary_view_omits_optional_fields() {
    let response = DeployResponse {
        deploy_id: "abc123".to_string(),
        block_hash: "hash1".to_string(),
        block_number: 100,
        timestamp: 1700000000000,
        cost: 500,
        errored: false,
        is_finalized: true,
        deployer: None,
        term: None,
        system_deploy_error: None,
        sig_algorithm: None,
        valid_after_block_number: None,
        transfers: None,
    };

    let json = serde_json::to_value(&response).unwrap();

    // Core fields present
    assert_eq!(json["deployId"], "abc123");
    assert_eq!(json["blockHash"], "hash1");
    assert_eq!(json["cost"], 500);
    assert_eq!(json["isFinalized"], true);

    // Optional fields omitted
    assert!(json.get("deployer").is_none());
    assert!(json.get("term").is_none());
    assert!(json.get("phloPrice").is_none());
    assert!(json.get("phloLimit").is_none());
    assert!(json.get("sigAlgorithm").is_none());
    assert!(json.get("validAfterBlockNumber").is_none());
    assert!(json.get("transfers").is_none());
}

#[test]
fn test_deploy_request_serialization() {
    let request = DeployRequest {
        data: DeployData {
            term: "contract".to_string(),
            time_stamp: 1234567890,
            valid_after_block_number: 0,
            shard_id: "".to_string(),
            expiration_timestamp: None,
        },
        deployer: "0123456789abcdef".to_string(),
        signature: "fedcba9876543210".to_string(),
        sig_algorithm: "secp256k1".to_string(),
        cosigners: Vec::new(),
    };

    let json = serde_json::to_string(&request).unwrap();
    let deserialized: DeployRequest = serde_json::from_str(&json).unwrap();

    assert_eq!(request.deployer, deserialized.deployer);
    assert_eq!(request.signature, deserialized.signature);
    assert_eq!(request.sig_algorithm, deserialized.sig_algorithm);
}

#[test]
fn test_rho_expr_serialization() {
    let expr = RhoExpr::ExprBool { data: true };
    let json = serde_json::to_string(&expr).unwrap();
    assert_eq!(json, r#"{"ExprBool":{"data":true}}"#);
}

#[test]
fn test_expr_from_par_proto_empty() {
    let par = Par::default();
    let result = expr_from_par_proto(par);
    assert!(result.is_none());
}

#[test]
fn test_expr_from_par_proto_single_bool() {
    let par = models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GBool(true)),
        }],
        ..Default::default()
    };
    let result = expr_from_par_proto(par);
    assert!(matches!(result, Some(RhoExpr::ExprBool { data: true })));
}

#[test]
fn test_expr_from_par_proto_multiple_exprs() {
    let par = models::par_from_default! {
        exprs: vec![
            Expr {
                expr_instance: Some(ExprInstance::GBool(true)),
            },
            Expr {
                expr_instance: Some(ExprInstance::GInt(42)),
            },
        ],
        ..Default::default()
    };
    let result = expr_from_par_proto(par);
    match result.as_ref() {
        Some(RhoExpr::ExprPar { data }) => {
            assert_eq!(data.len(), 2);
            assert!(matches!(data[0], RhoExpr::ExprBool { data: true }));
            assert!(matches!(data[1], RhoExpr::ExprInt { data: 42 }));
        }
        _ => panic!("Expected ExprPar with 2 elements"),
    }
}

#[test]
fn test_expr_from_expr_proto_primitive_types() {
    // Test GBool
    let expr = Expr {
        expr_instance: Some(ExprInstance::GBool(true)),
    };
    let result = expr_from_expr_proto(expr);
    assert!(matches!(result, Some(RhoExpr::ExprBool { data: true })));

    // Test GInt
    let expr = Expr {
        expr_instance: Some(ExprInstance::GInt(42)),
    };
    let result = expr_from_expr_proto(expr);
    assert!(matches!(result, Some(RhoExpr::ExprInt { data: 42 })));

    // Test GString
    let expr = Expr {
        expr_instance: Some(ExprInstance::GString("hello".to_string())),
    };
    let result = expr_from_expr_proto(expr);
    assert!(matches!(result.as_ref(), Some(RhoExpr::ExprString { data }) if data == "hello"));

    // Test GUri
    let expr = Expr {
        expr_instance: Some(ExprInstance::GUri("rho:io:stdout".to_string())),
    };
    let result = expr_from_expr_proto(expr);
    assert!(matches!(result.as_ref(), Some(RhoExpr::ExprUri { data }) if data == "rho:io:stdout"));

    // Test GByteArray
    let expr = Expr {
        expr_instance: Some(ExprInstance::GByteArray(vec![0x01, 0x02, 0x03])),
    };
    let result = expr_from_expr_proto(expr);
    assert!(matches!(result.as_ref(), Some(RhoExpr::ExprBytes { data }) if data == "010203"));
}

#[test]
fn test_expr_from_expr_proto_tuple() {
    let tuple = ETuple {
        ps: vec![
            models::par_from_default! {
                exprs: vec![Expr {
                    expr_instance: Some(ExprInstance::GInt(1)),
                }],
                ..Default::default()
            },
            models::par_from_default! {
                exprs: vec![Expr {
                    expr_instance: Some(ExprInstance::GString("hello".to_string())),
                }],
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    let expr = Expr {
        expr_instance: Some(ExprInstance::ETupleBody(tuple)),
    };
    let result = expr_from_expr_proto(expr);
    match result.as_ref() {
        Some(RhoExpr::ExprTuple { data }) => {
            assert_eq!(data.len(), 2);
            assert!(matches!(data[0], RhoExpr::ExprInt { data: 1 }));
            assert!(matches!(data[1], RhoExpr::ExprString { data: ref d } if d == "hello"));
        }
        _ => panic!("Expected ExprTuple"),
    }
}

#[test]
fn test_expr_from_expr_proto_list() {
    let list = EList {
        ps: vec![
            models::par_from_default! {
                exprs: vec![Expr {
                    expr_instance: Some(ExprInstance::GInt(1)),
                }],
                ..Default::default()
            },
            models::par_from_default! {
                exprs: vec![Expr {
                    expr_instance: Some(ExprInstance::GInt(2)),
                }],
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    let expr = Expr {
        expr_instance: Some(ExprInstance::EListBody(list)),
    };
    let result = expr_from_expr_proto(expr);
    match result.as_ref() {
        Some(RhoExpr::ExprList { data }) => {
            assert_eq!(data.len(), 2);
            assert!(matches!(data[0], RhoExpr::ExprInt { data: 1 }));
            assert!(matches!(data[1], RhoExpr::ExprInt { data: 2 }));
        }
        _ => panic!("Expected ExprList"),
    }
}

#[test]
fn test_expr_from_expr_proto_set() {
    let set = ESet {
        ps: vec![
            models::par_from_default! {
                exprs: vec![Expr {
                    expr_instance: Some(ExprInstance::GString("a".to_string())),
                }],
                ..Default::default()
            },
            models::par_from_default! {
                exprs: vec![Expr {
                    expr_instance: Some(ExprInstance::GString("b".to_string())),
                }],
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    let expr = Expr {
        expr_instance: Some(ExprInstance::ESetBody(set)),
    };
    let result = expr_from_expr_proto(expr);
    match result.as_ref() {
        Some(RhoExpr::ExprSet { data }) => {
            assert_eq!(data.len(), 2);
            assert!(matches!(data[0], RhoExpr::ExprString { data: ref d } if d == "a"));
            assert!(matches!(data[1], RhoExpr::ExprString { data: ref d } if d == "b"));
        }
        _ => panic!("Expected ExprSet"),
    }
}

#[test]
fn test_expr_from_expr_proto_map() {
    let map = EMap {
        kvs: vec![
            KeyValuePair {
                key: Some(models::par_from_default! {
                    exprs: vec![Expr {
                        expr_instance: Some(ExprInstance::GString("key1".to_string())),
                    }],
                    ..Default::default()
                }),
                value: Some(models::par_from_default! {
                    exprs: vec![Expr {
                        expr_instance: Some(ExprInstance::GInt(42)),
                    }],
                    ..Default::default()
                }),
            },
            KeyValuePair {
                key: Some(models::par_from_default! {
                    exprs: vec![Expr {
                        expr_instance: Some(ExprInstance::GString("key2".to_string())),
                    }],
                    ..Default::default()
                }),
                value: Some(models::par_from_default! {
                    exprs: vec![Expr {
                        expr_instance: Some(ExprInstance::GString("value2".to_string())),
                    }],
                    ..Default::default()
                }),
            },
        ],
        ..Default::default()
    };

    let expr = Expr {
        expr_instance: Some(ExprInstance::EMapBody(map)),
    };
    let result = expr_from_expr_proto(expr);
    match result.as_ref() {
        Some(RhoExpr::ExprMap { data }) => {
            assert_eq!(data.len(), 2);
            assert!(data.contains_key("key1"));
            assert!(data.contains_key("key2"));
            assert!(matches!(data["key1"], RhoExpr::ExprInt { data: 42 }));
            assert!(matches!(data["key2"], RhoExpr::ExprString { data: ref d } if d == "value2"));
        }
        _ => panic!("Expected ExprMap"),
    }
}

#[test]
fn test_unforg_from_proto_private() {
    let unforg = GUnforgeable {
        unf_instance: Some(UnfInstance::GPrivateBody(GPrivate {
            id: vec![0x01, 0x02, 0x03],
        })),
    };
    let result = unforg_from_proto(unforg);
    match result.as_ref() {
        Some(RhoExpr::ExprUnforg { data }) => {
            assert!(matches!(data, RhoUnforg::UnforgPrivate { data: ref d } if d == "010203"));
        }
        _ => panic!("Expected ExprUnforg with UnforgPrivate"),
    }
}

#[test]
fn test_unforg_from_proto_deploy() {
    let unforg = GUnforgeable {
        unf_instance: Some(UnfInstance::GDeployIdBody(GDeployId {
            sig: vec![0x04, 0x05, 0x06],
        })),
    };
    let result = unforg_from_proto(unforg);
    match result.as_ref() {
        Some(RhoExpr::ExprUnforg { data }) => {
            assert!(matches!(data, RhoUnforg::UnforgDeploy { data: ref d } if d == "040506"));
        }
        _ => panic!("Expected ExprUnforg with UnforgDeploy"),
    }
}

#[test]
fn test_unforg_from_proto_deployer() {
    let unforg = GUnforgeable {
        unf_instance: Some(UnfInstance::GDeployerIdBody(GDeployerId {
            public_key: vec![0x07, 0x08, 0x09],
        })),
    };
    let result = unforg_from_proto(unforg);
    match result.as_ref() {
        Some(RhoExpr::ExprUnforg { data }) => {
            assert!(matches!(data, RhoUnforg::UnforgDeployer { data: ref d } if d == "070809"));
        }
        _ => panic!("Expected ExprUnforg with UnforgDeployer"),
    }
}

#[test]
fn test_expr_from_bundle_proto() {
    let bundle = Bundle {
        body: Some(models::par_from_default! {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::GString("bundle_content".to_string())),
            }],
            ..Default::default()
        }),
        write_flag: true,
        read_flag: false,
    };
    let result = expr_from_bundle_proto(bundle);
    assert!(matches!(
        result,
        Some(RhoExpr::ExprBundle { ref data, write: true, read: false })
        if matches!(data.as_ref(), RhoExpr::ExprString { data } if data == "bundle_content")
    ));
}

#[test]
fn test_expr_from_bundle_proto_empty() {
    let bundle = Bundle {
        body: None,
        write_flag: false,
        read_flag: true,
    };
    let result = expr_from_bundle_proto(bundle);
    // Empty body bundle returns ExprBundle with ExprUnknown body
    assert!(matches!(
        result,
        Some(RhoExpr::ExprBundle {
            read: true,
            write: false,
            ..
        })
    ));
}

#[test]
fn test_extract_key_from_expr() {
    // Test string key
    let expr = RhoExpr::ExprString {
        data: "hello".to_string(),
    };
    assert_eq!(extract_key_from_expr(&expr), "hello");

    // Test int key
    let expr = RhoExpr::ExprInt { data: 42 };
    assert_eq!(extract_key_from_expr(&expr), "42");

    // Test bool key
    let expr = RhoExpr::ExprBool { data: true };
    assert_eq!(extract_key_from_expr(&expr), "true");

    // Test URI key
    let expr = RhoExpr::ExprUri {
        data: "rho:io:stdout".to_string(),
    };
    assert_eq!(extract_key_from_expr(&expr), "rho:io:stdout");

    // Test bytes key
    let expr = RhoExpr::ExprBytes {
        data: "010203".to_string(),
    };
    assert_eq!(extract_key_from_expr(&expr), "010203");

    // Test unforgeable keys
    let expr = RhoExpr::ExprUnforg {
        data: RhoUnforg::UnforgPrivate {
            data: "private".to_string(),
        },
    };
    assert_eq!(extract_key_from_expr(&expr), "private");

    // Test complex key type — serialized to JSON
    let expr = RhoExpr::ExprPar { data: vec![] };
    let key = extract_key_from_expr(&expr);
    assert!(
        !key.is_empty(),
        "complex keys should serialize to non-empty string"
    );
}
