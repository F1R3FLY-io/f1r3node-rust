use std::collections::HashMap;

use models::rhoapi::Par;
use rholang::rust::interpreter::chromadb_service::{CollectionEntry, Metadata, MetadataValue};
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::interpreter::EvaluateResult;
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::rho_type::{RhoList, RhoMap, RhoNil, RhoNumber, RhoString};
use rholang::rust::interpreter::test_utils::resources::with_runtime;

/// These tests exercise `rho:chroma:*` system processes end-to-end, which
/// requires a live ChromaDB server. Probe the configured host with a fast
/// heartbeat HTTP call; if it's not reachable, treat the test as a no-op
/// success so unattended local / CI runs without ChromaDB do not fail.
///
/// Honors the `CHROMA_HOST` env var the way the production
/// `ChromaHttpClient::from_env()` does (defaulting to `http://localhost:8000`).
/// Returns `true` when ChromaDB responds to `/api/v2/heartbeat` within
/// 500ms, otherwise `false`.
async fn chromadb_available() -> bool {
    let base = std::env::var("CHROMA_HOST").unwrap_or_else(|_| "http://localhost:8000".to_string());
    let url = format!("{}/api/v2/heartbeat", base.trim_end_matches('/'));
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(500))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    matches!(client.get(&url).send().await, Ok(resp) if resp.status().is_success())
}

macro_rules! skip_if_no_chromadb {
    () => {
        if !chromadb_available().await {
            eprintln!(
                "Skipping ChromaDB integration test: no server at {} (set CHROMA_HOST or start chromadb to enable).",
                std::env::var("CHROMA_HOST").unwrap_or_else(|_| "http://localhost:8000".to_string())
            );
            return;
        }
    };
}

async fn success(runtime: &mut RhoRuntimeImpl, term: &str) -> Result<(), InterpreterError> {
    execute(runtime, term).await.map(|res| {
        assert!(
            res.errors.is_empty(),
            "{}",
            format!("Execution failed for: {}. Cause: {:?}", term, res.errors)
        )
    })
}

async fn execute(
    runtime: &mut RhoRuntimeImpl,
    term: &str,
) -> Result<EvaluateResult, InterpreterError> {
    runtime.evaluate_with_term(term).await
}

#[tokio::test]
async fn collection_should_yield_correct_meta_after_creation() {
    skip_if_no_chromadb!();
    let meta_contract = r#"
            new createCollection(`rho:chroma:collection:new`),
                getCollectionMeta(`rho:chroma:collection:meta`),
                stdout(`rho:io:stdout`), createRet, metaRet in {
                    createCollection!("test-collection", true, {"meta1" : 1, "two" : "42", "three" : 42, "meta2": "bar"}, *createRet) |
                    for(@res <- createRet) {
                        getCollectionMeta!("test-collection", *metaRet) |
                        for(@res <- metaRet) {
                            @0!(res)
                        }
                    }
            }
        "#;

    test_runtime(
        meta_contract,
        Some(
            Metadata::from([
                ("meta1".to_string(), MetadataValue::Number(1)),
                ("two".to_string(), MetadataValue::String("42".to_string())),
                ("three".to_string(), MetadataValue::Number(42)),
                (
                    "meta2".to_string(),
                    MetadataValue::String("bar".to_string()),
                ),
            ])
            .into(),
        ),
    )
    .await
}

#[tokio::test]
async fn collection_should_yield_correct_meta_after_creation_empty() {
    skip_if_no_chromadb!();
    let meta_contract = r#"
            new createCollection(`rho:chroma:collection:new`),
                getCollectionMeta(`rho:chroma:collection:meta`),
                createRet, metaRet in {
                    createCollection!("test-collection-nil-meta", true, Nil, *createRet) |
                    for(@res <- createRet) {
                        getCollectionMeta!("test-collection-nil-meta", *metaRet) |
                        for(@res <- metaRet) {
                            @0!(res)
                        }
                    }
            }
        "#;

    test_runtime(meta_contract, Some(RhoNil::create_par())).await
}

#[tokio::test]
async fn entry_should_be_queried() {
    skip_if_no_chromadb!();
    let meta_contract = r#"
        new createCollection(`rho:chroma:collection:new`),
            upsertEntries(`rho:chroma:collection:entries:new`),
            queryEntries(`rho:chroma:collection:entries:query`),
            createRet, upsertRet, queryRet in {
                createCollection!("test-collection-entries", true, Nil, *createRet) |
                for(@x <- createRet) {
                    upsertEntries!(
                        "test-collection-entries",
                        {
                            "doc1": ("Hello world!", Nil),
                            "doc2": (
                                "Hello world again!",
                                { "meta1": "42" }
                            )
                        },
                        *upsertRet
                    )
                } |
                for(@y <- upsertRet) {
                    queryEntries!("test-collection-entries", [ "Hello world" ], *queryRet)
                } |
                for(@res <- queryRet) {
                    @0!(res)
                }
        }
        "#;

    test_runtime(
        meta_contract,
        Some(RhoList::create_par(vec![RhoMap::create_par(
            HashMap::from([
                (
                    RhoString::create_par("doc1".into()),
                    CollectionEntry {
                        document: "Hello world!".to_string(),
                        metadata: None,
                    }
                    .into(),
                ),
                (
                    RhoString::create_par("doc2".into()),
                    CollectionEntry {
                        document: "Hello world again!".to_string(),
                        metadata: Some(Metadata::from([(
                            "meta1".to_string(),
                            MetadataValue::String("42".to_string()),
                        )])),
                    }
                    .into(),
                ),
            ]),
        )])),
    )
    .await
}

async fn test_runtime(contract: &str, expected: Option<Par>) {
    with_runtime("interpreter-spec-", |mut runtime| async move {
        success(&mut runtime, contract).await.unwrap();

        let tuple_space = runtime.get_hot_changes().await;

        let ch_zero = vec![RhoNumber::create_par(0)];
        println!("ch_zero: {:?}", ch_zero);

        let tuple_space_data = tuple_space.get(&ch_zero);
        println!("tuple_space_data: {:?}", tuple_space_data);

        let results = tuple_space_data.map(|row| row.data[0].a.pars[0].clone());

        assert_eq!(results, expected);
    })
    .await
}
