use std::collections::BTreeMap;
use std::fmt::Write;
use std::time::Instant;

use block_storage::rust::dag::block_dag_key_value_storage::BlockDagKeyValueStorage;
use metrics_util::debugging::{DebugValue, DebuggingRecorder};
use shared::rust::store::key_value_store::KvStoreError;

use super::backend;
use super::counting_fixture::hash;

pub fn run(
    storage: &BlockDagKeyValueStorage,
    window: i64,
    blocks: usize,
    sample_stage: &str,
) -> Result<(), KvStoreError> {
    let dag = storage.get_representation()?;
    let finalized = dag.block_number_unsafe(&dag.last_finalized_block())?;
    let cutoff = if backend::WINDOWED {
        finalized.saturating_sub(window).max(0)
    } else {
        0
    };
    let height = dag.get_max_height().saturating_sub(1);
    let diagnostics = backend::diagnostics(&dag);
    let resident = diagnostics.blocks;
    let edges = diagnostics.edges;
    assert!(diagnostics.minimum.is_none_or(|minimum| minimum >= cutoff));
    assert!(diagnostics.maximum.is_none_or(|maximum| maximum <= height));
    assert!(resident <= (height - cutoff + 1) as usize * 7);
    assert_eq!(diagnostics.block_numbers, resident);
    assert!(diagnostics.main_parents <= resident);
    assert!(diagnostics.self_justifications <= resident);
    assert!(diagnostics.finalized <= resident);
    assert!(diagnostics.heights <= (height - cutoff + 1) as usize);
    assert!(diagnostics.child_keys <= resident + edges);
    assert!(diagnostics.latest_messages <= 10);
    assert!(edges <= resident * 7);
    println!(
        "{{\"stage\":\"resident_diagnostics\",\"sample_stage\":\"{sample_stage}\",\"blocks\":{blocks},\"minimum\":{},\"maximum\":{},\"resident_blocks\":{resident},\"resident_edges\":{edges},\"child_keys\":{},\"heights\":{},\"block_numbers\":{},\"main_parents\":{},\"self_justifications\":{},\"finalized\":{},\"latest_messages\":{},\"measured_after_heap_sample\":true}}",
        diagnostics.minimum.expect("nonempty fixture"),
        diagnostics.maximum.expect("nonempty fixture"),
        diagnostics.child_keys, diagnostics.heights, diagnostics.block_numbers,
        diagnostics.main_parents, diagnostics.self_justifications, diagnostics.finalized,
        diagnostics.latest_messages,
    );
    let live = if let Some(live) = backend::live_diagnostics(storage) {
        assert_eq!(live.blocks, resident);
        assert_eq!(live.edges, edges);
        assert_eq!(live.cutoff, cutoff);
        assert_eq!(live.max_height, Some(height));
        assert_eq!(live.minimum, diagnostics.minimum);
        assert_eq!(live.maximum, diagnostics.maximum);
        assert_eq!(live.height_entries, resident);
        assert!(live.height_buckets <= (height - cutoff + 1) as usize);
        format!(
            "{{\"height_buckets\":{},\"height_entries\":{},\"minimum\":{},\"maximum\":{},\"cutoff\":{},\"max_height\":{height},\"resident_blocks\":{},\"resident_edges\":{}}}",
            live.height_buckets, live.height_entries, live.minimum.expect("nonempty fixture"),
            live.maximum.expect("nonempty fixture"), live.cutoff, live.blocks, live.edges,
        )
    } else {
        "null".to_string()
    };
    println!(
        "{{\"stage\":\"live_resident_diagnostics\",\"sample_stage\":\"{sample_stage}\",\"blocks\":{blocks},\"resident_height_index_available\":{},\"resident_height_index\":{live},\"measured_after_heap_sample\":true}}",
        backend::WINDOWED,
    );
    for (key_kind, index, resident) in [
        ("tip", blocks - 1, true),
        ("historical", 0, cutoff == 0),
        ("missing", blocks, false),
    ] {
        let key = hash(index);
        for operation in ["contains", "lookup", "children"] {
            for repetition in 0..3 {
                let recorder = DebuggingRecorder::new();
                let snapshotter = recorder.snapshotter();
                let (returned_rows, elapsed) = metrics::with_local_recorder(&recorder, || {
                    let start = Instant::now();
                    let measured = match operation {
                        "contains" => {
                            let found = backend::visible(&dag, &key)?;
                            let elapsed = start.elapsed().as_nanos();
                            assert_eq!(found, index < blocks);
                            (usize::from(found), elapsed)
                        }
                        "lookup" => {
                            let metadata = dag.lookup(&key)?;
                            let elapsed = start.elapsed().as_nanos();
                            assert_eq!(metadata.is_some(), index < blocks);
                            if let Some(metadata) = &metadata {
                                assert_eq!(metadata.block_hash, key);
                                let expected_height =
                                    if index == 0 { 0 } else { (index - 1) / 7 + 1 };
                                assert_eq!(metadata.block_number, expected_height as i64);
                            }
                            (usize::from(metadata.is_some()), elapsed)
                        }
                        "children" => {
                            let children = backend::children(&dag, &key)?;
                            let elapsed = start.elapsed().as_nanos();
                            assert_eq!(children.is_some(), index < blocks);
                            let expected = if index == 0 { (blocks - 1).min(7) } else { 0 };
                            assert_eq!(children.as_ref().map_or(0, |set| set.len()), expected);
                            if let Some(children) = &children {
                                for child in 1..=expected {
                                    assert!(children.contains(&hash(child)));
                                }
                            }
                            (expected, elapsed)
                        }
                        _ => unreachable!(),
                    };
                    Ok::<_, KvStoreError>(measured)
                })?;
                let mut operations = BTreeMap::<String, (u64, u64)>::new();
                for (key, _, _, value) in snapshotter.snapshot().into_vec() {
                    let name = key.key().name();
                    if name != "dag.history.query.count" && name != "dag.history.rows.visited" {
                        continue;
                    }
                    let Some(label) = key.key().labels().find(|label| label.key() == "operation")
                    else {
                        continue;
                    };
                    let DebugValue::Counter(value) = value else {
                        continue;
                    };
                    let totals = operations.entry(label.value().to_string()).or_default();
                    if name == "dag.history.query.count" {
                        totals.0 += value;
                    } else {
                        totals.1 += value;
                    }
                }
                let mut counts = String::from("null");
                if backend::WINDOWED {
                    let queries: u64 = operations.values().map(|(queries, _)| queries).sum();
                    let rows: u64 = operations.values().map(|(_, rows)| rows).sum();
                    if operation == "children" {
                        assert_eq!(operations.get("children"), Some(&(1, returned_rows as u64)));
                        assert!(queries <= 8);
                        assert!(rows <= 14);
                    } else {
                        assert_eq!(queries, u64::from(!resident));
                        assert_eq!(rows, u64::from(!resident && index < blocks));
                    }
                    counts = String::from("{");
                    for (position, (operation, (queries, rows))) in operations.iter().enumerate() {
                        if position > 0 {
                            counts.push(',');
                        }
                        write!(
                            &mut counts,
                            "\"{operation}\":{{\"queries\":{queries},\"rows\":{rows}}}"
                        )
                        .unwrap();
                    }
                    counts.push('}');
                }
                println!(
                    "{{\"stage\":\"query_probe\",\"sample_stage\":\"{sample_stage}\",\"blocks\":{blocks},\"key_kind\":\"{key_kind}\",\"resident\":{resident},\"cold_history\":{},\"operation\":\"{operation}\",\"repetition\":{repetition},\"returned_rows\":{returned_rows},\"instrumented_single_query_nanoseconds\":{elapsed},\"history_index_metrics_available\":{},\"history_index_operations\":{counts},\"metadata_reads\":null,\"metadata_cache_hits\":null}}",
                    key_kind == "historical" && !resident, backend::WINDOWED,
                );
            }
        }
    }
    Ok(())
}
