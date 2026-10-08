//! The live leg of the ChromaDB search backend of the VectorDB spaces (P6 steps 7a to 7c, design
//! v3 4.9; register decision 61, tensor-chroma design D9 (a)).
//!
//! The suite compiles only with the `chromadb` feature, and it runs on the host against a real
//! ChromaDB server: the pinned image of `docker/shard.yml`. The script
//! `scripts/verify-reified/chroma-live.sh` starts a disposable server, sets `RRS_LIVE_CHROMA=1`,
//! `CHROMA_ENDPOINT`, a fresh `CHROMA_DATABASE` and `FASTEMBED_CACHE_DIR` under `target/`, and runs
//! the suite. Every test fails when `RRS_LIVE_CHROMA=1` or `CHROMA_ENDPOINT` is not set, or when
//! the server does not answer, so the leg never skips silently (decision 59).
//!
//! | Test | What it checks |
//! |---|---|
//! | `chroma_live_mirror_calls` | Every call of `ChromaMirror` against the server: the database of the node, the collection and its distance, the registry with its read-modify-write, the watermark, upserts, filtered ids, the query, deletes, and the listing. |
//! | `chroma_live_publish_and_reconcile` | A native deploy publishes its records to the server through the writer, and a reconciler pass repairs an orphan and a missing record. |
//! | `chroma_live_sbert_embedder` | The SBERT model gives one finite vector of 384 values for each text. |

#![cfg(feature = "chromadb")]

#[path = "support/mod.rs"]
mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use models::rhoapi::Par;
use rholang::rust::interpreter::reified::mirror_reconciler::{reconcile, Finalized};
use rholang::rust::interpreter::reified::vectordb_services::{
    self, channel_digest, collection_name, ChromaMirror, Embedder, MirrorBackend, MirrorRegistry,
    SbertEmbedder, VectorDbServices,
};
use rholang::rust::interpreter::spaces::mirror::{MirrorMetric, MirrorRecord};
use rholang::rust::interpreter::spaces::record;
use support::native::{NativeChain, AMPLE_LIMIT};
use support::session::{data_channel, key, native};

/// The mirror of the live server. It fails without `RRS_LIVE_CHROMA=1` and `CHROMA_ENDPOINT`.
fn live_mirror() -> ChromaMirror {
    assert_eq!(
        std::env::var("RRS_LIVE_CHROMA").as_deref(),
        Ok("1"),
        "the live leg needs RRS_LIVE_CHROMA=1: run scripts/verify-reified/chroma-live.sh"
    );
    let endpoint = std::env::var("CHROMA_ENDPOINT")
        .expect("the live leg needs CHROMA_ENDPOINT: run scripts/verify-reified/chroma-live.sh");
    ChromaMirror::from_env()
        .unwrap_or_else(|| panic!("CHROMA_ENDPOINT {endpoint} is not a valid endpoint"))
}

/// A collection name that no earlier run used.
fn fresh_collection(label: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("a clock after 1970")
        .as_nanos();
    format!("rrs-live-{label}-{nanos:x}")
}

fn record(id: u8, embedding: [f32; 2], channel: &str, document: Option<&str>) -> MirrorRecord {
    MirrorRecord {
        id: [id; 32],
        embedding: embedding.to_vec(),
        channel: channel.to_string(),
        persistent: id.is_multiple_of(2),
        document: document.map(str::to_string),
    }
}

/// Every call of the ChromaDB backend against the live server.
#[tokio::test]
async fn chroma_live_mirror_calls() {
    let mirror = live_mirror();
    let collection = fresh_collection("calls");
    assert_eq!(
        mirror.watermark(&collection).await,
        Ok(0),
        "no collection yet"
    );
    assert_eq!(
        mirror.ids(&collection, "c1").await,
        Ok(BTreeSet::new()),
        "no collection yet"
    );
    assert_eq!(
        mirror.delete(&collection, &[[1; 32]]).await,
        Ok(()),
        "a delete in a missing collection succeeds"
    );
    mirror
        .ensure_collection(&collection, MirrorMetric::L2)
        .await
        .expect("the collection is made");

    let space = [5; 32];
    mirror
        .register(
            &collection,
            &space,
            &BTreeMap::from([("c1".to_string(), vec![1, 2])]),
        )
        .await
        .expect("the first channel is registered");
    mirror
        .set_watermark(&collection, 7)
        .await
        .expect("the watermark is set");
    mirror
        .register(
            &collection,
            &space,
            &BTreeMap::from([("c1".to_string(), vec![1, 2]), ("c2".to_string(), vec![3])]),
        )
        .await
        .expect("the second channel is registered");
    let expected = MirrorRegistry {
        space: Some(space),
        channels: BTreeMap::from([("c1".to_string(), vec![1, 2]), ("c2".to_string(), vec![3])]),
        watermark: 7,
    };
    assert_eq!(
        mirror.registry(&collection).await,
        Ok(expected.clone()),
        "every read-modify-write keeps the other keys"
    );
    assert_eq!(mirror.watermark(&collection).await, Ok(7));
    mirror
        .ensure_collection(&collection, MirrorMetric::Cosine)
        .await
        .expect("an existing collection is kept");
    assert_eq!(
        mirror.registry(&collection).await,
        Ok(expected),
        "making an existing collection keeps its metadata"
    );

    let near = record(2, [1.0, 0.0], "c1", Some("doc"));
    let far = record(3, [0.0, 1.0], "c1", None);
    let other = record(4, [1.0, 0.0], "c2", None);
    mirror
        .upsert(&collection, &[near.clone(), far.clone(), other.clone()])
        .await
        .expect("the records are written");
    assert_eq!(
        mirror.ids(&collection, "c1").await,
        Ok(BTreeSet::from([near.id, far.id]))
    );
    let neighbours = mirror
        .query(&collection, "c1", &[0.9, 0.1], 1)
        .await
        .expect("the query answers");
    assert_eq!(neighbours.len(), 1);
    assert_eq!(
        neighbours[0].id,
        near.id.to_vec(),
        "the nearest record of c1"
    );
    mirror
        .delete(&collection, &[near.id, [9; 32]])
        .await
        .expect("a delete with an absent id succeeds");
    assert_eq!(
        mirror.ids(&collection, "c1").await,
        Ok(BTreeSet::from([far.id]))
    );
    let collections = mirror.collections().await.expect("the listing answers");
    assert!(collections.contains(&collection), "{collections:?}");
}

/// A native deploy publishes its records to the live server through the writer, and a pass of
/// the reconciler repairs an orphan and a missing record from the finalized state.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn chroma_live_publish_and_reconcile() {
    let mirror: Arc<dyn MirrorBackend> = Arc::new(live_mirror());
    let services = VectorDbServices::with_writer(
        Arc::new(vectordb_services::UnavailableEmbedder),
        Arc::clone(&mirror),
    );
    let _lock = support::backends::install(services).await;
    let mut chain = NativeChain::new().await;
    // The label makes the space id, and so the collection, new in every run.
    let label = fresh_collection("deploy");
    let setup = r#"new vdb(`rho:space:hashmap:vectordb`) in {
  for (m <- vdb!?("default"; capacity=4, dimensions=2, mirror="chroma")) {
    for (mn <- m!?("gensym")) {
      @"rrs-live-names"!(*m, *mn) | mn!([1.0, 0.0]) | mn!([0.0, 1.0])
    }
  }
}"#;
    let played = chain.play(setup, &label, AMPLE_LIMIT).await;
    assert!(
        played.result.errors.is_empty(),
        "the deploy succeeds: {:?}",
        played.result.errors
    );
    support::backends::writer().flush().await;
    let names = chain.values("rrs-live-names").await;
    let [m, mn] = <[Par; 2]>::try_from(names.into_iter().next().expect("the names"))
        .expect("a handle and a name");
    let space = native(&m);
    let collection = collection_name(&space.sid);
    let digest = channel_digest(&key(&mn));
    let stored: BTreeSet<[u8; 32]> = chain
        .data(&data_channel(space, &mn))
        .await
        .into_iter()
        .filter(|datum| {
            record::unwrap_vdb_datum(&datum.a)
                .expect("a VectorDB record")
                .embedding
                .is_some()
        })
        .map(|datum| {
            <[u8; 32]>::try_from(datum.source.hash.bytes().as_slice()).expect("a 32-byte id")
        })
        .collect();
    assert_eq!(stored.len(), 2);
    assert_eq!(
        mirror.ids(&collection, &digest).await,
        Ok(stored.clone()),
        "the writer published both records"
    );
    let registry = mirror.registry(&collection).await.expect("the registry");
    assert_eq!(registry.space, Some(space.sid));
    assert_eq!(
        registry.channels.get(&digest),
        Some(&key(&mn).as_bytes().to_vec())
    );

    let first = *stored.iter().next().expect("a record");
    mirror
        .delete(&collection, &[first])
        .await
        .expect("the record is taken out");
    mirror
        .upsert(&collection, &[record(0xEE, [0.5, 0.5], &digest, None)])
        .await
        .expect("the orphan is written");
    let state = chain.finalized_state();
    let report = reconcile(&vectordb_services::current(), state.as_ref(), &Finalized {
        block_number: 4,
        root: played.root.clone(),
    })
    .await;
    assert_eq!(report.repaired, BTreeSet::from([collection.clone()]));
    assert_eq!((report.deleted, report.added), (1, 1));
    assert_eq!(mirror.ids(&collection, &digest).await, Ok(stored));
    assert_eq!(mirror.watermark(&collection).await, Ok(4));
}

/// The SBERT model of the node gives one finite vector of 384 values for each text.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn chroma_live_sbert_embedder() {
    let _ = live_mirror();
    let embedder = SbertEmbedder::new();
    let vectors = embedder
        .embed(&["hello".to_string(), "world".to_string()])
        .await
        .expect("the model embeds");
    assert_eq!(vectors.len(), 2);
    for vector in &vectors {
        assert_eq!(vector.len(), 384);
        assert!(vector.iter().all(|value| value.is_finite()));
    }
    assert_ne!(vectors[0], vectors[1]);
}
