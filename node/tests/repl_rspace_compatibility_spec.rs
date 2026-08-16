//! Black-box compatibility gates for the node REPL's RSpace rendering contract.
//!
//! The PathMap Bag is an internal authority. REPL clients must continue to see
//! the same accumulated tuple-space view after every command, without knowing
//! which index or copy-on-write root backs it.

use std::collections::HashMap;
use std::sync::Arc;

use node::rust::api::repl_grpc_service::repl::repl_server::Repl;
use node::rust::api::repl_grpc_service::repl::{CmdRequest, EvalRequest};
use node::rust::api::repl_grpc_service::ReplGrpcServiceImpl;
use rholang::rust::interpreter::external_services::ExternalServices;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::rho_runtime::create_runtime_from_kv_store;
use rholang::rust::interpreter::system_processes::test_framework_contracts;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

async fn service() -> ReplGrpcServiceImpl {
    let mut stores = InMemoryStoreManager::new();
    let store = stores
        .r_space_stores()
        .await
        .expect("in-memory RSpace stores");
    let runtime = create_runtime_from_kv_store(
        store,
        Arc::new(HashMap::new()),
        true,
        &mut test_framework_contracts(),
        Arc::new(Box::new(Matcher)),
        ExternalServices::noop(),
    )
    .await;
    ReplGrpcServiceImpl::new(runtime)
}

#[tokio::test]
async fn every_repl_command_returns_the_accumulated_rspace_view() {
    let service = service().await;

    let first = service
        .run(tonic::Request::new(CmdRequest {
            line: r#"@"repl-first"!(1)"#.to_string(),
        }))
        .await
        .expect("first REPL command")
        .into_inner()
        .output;
    assert!(first.contains("Storage Contents:"), "{first}");
    assert!(first.contains("repl-first"), "{first}");
    assert!(first.contains('1'), "{first}");

    let second = service
        .run(tonic::Request::new(CmdRequest {
            line: r#"@"repl-second"!(2)"#.to_string(),
        }))
        .await
        .expect("second REPL command")
        .into_inner()
        .output;
    assert!(second.contains("Storage Contents:"), "{second}");
    assert!(second.contains("repl-first"), "{second}");
    assert!(second.contains("repl-second"), "{second}");
    assert!(second.contains('1'), "{second}");
    assert!(second.contains('2'), "{second}");
}

#[tokio::test]
async fn eval_preserves_the_unmatched_sends_only_view() {
    let service = service().await;
    let output = service
        .eval(tonic::Request::new(EvalRequest {
            program: r#"@"unmatched-send"!(7)"#.to_string(),
            print_unmatched_sends_only: true,
            language: "rho".to_string(),
        }))
        .await
        .expect("REPL eval")
        .into_inner()
        .output;

    assert!(output.contains("Storage Contents:"), "{output}");
    assert!(output.contains("unmatched-send"), "{output}");
    assert!(output.contains('7'), "{output}");
    assert!(output.contains("Resting diagnostics:"), "{output}");
}
