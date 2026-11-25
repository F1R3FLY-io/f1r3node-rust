use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::leaf_decode::Distribution;
use super::{Measurement, Stats};

#[derive(Serialize, Deserialize)]
struct Bundle {
    schema_version: u32,
    started_utc: String,
    finished_utc: String,
    finalized_block: Finalized,
    nodes: Vec<Node>,
    before: Vec<State>,
    after: Vec<State>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Finalized {
    block_number: u64,
    block_hash: String,
    post_state_hash: String,
    is_finalized: bool,
}
#[derive(Serialize, Deserialize)]
struct Node {
    node: String,
    schema_version: u32,
    complete: bool,
    measurements: Vec<Measurement>,
}
#[derive(Serialize, Deserialize)]
struct State {
    node: String,
    started: String,
    running: bool,
    health: String,
    oom_killed: bool,
}

fn mib(bytes: u64) -> String { format!("{:.3}", bytes as f64 / 1_048_576.0) }
fn average(d: &Distribution) -> String {
    if d.count == 0 {
        "n/a".into()
    } else {
        format!("{:.2}", d.sum as f64 / d.count as f64)
    }
}
fn spread(d: &Distribution) -> String {
    let s = d.summary();
    ["min", "p50", "p95", "max"]
        .into_iter()
        .map(|k| {
            s[k].as_u64()
                .map_or_else(|| "n/a".into(), |v| v.to_string())
        })
        .collect::<Vec<_>>()
        .join(" / ")
}
fn order(groups: &BTreeMap<String, Stats>) -> Vec<(&String, &Stats)> {
    let mut rows: Vec<_> = groups.iter().collect();
    rows.sort_by(|a, b| {
        b.1.record_bytes
            .sum
            .cmp(&a.1.record_bytes.sum)
            .then_with(|| a.0.cmp(b.0))
    });
    rows
}

fn render(bundle: &Bundle) -> Result<String, Box<dyn Error>> {
    if bundle.schema_version != 1 || !bundle.finalized_block.is_finalized || bundle.nodes.is_empty()
    {
        return Err("no verified finalized-root measurement".into());
    }
    let mut groups: BTreeMap<String, Stats> = BTreeMap::new();
    let mut reach: BTreeMap<(String, String), Stats> = BTreeMap::new();
    let mut node_names = BTreeSet::new();
    let mut common_selected = None;
    for node in &bundle.nodes {
        if !node.complete || node.schema_version != 1 {
            return Err("partial node measurement".into());
        }
        if !node_names.insert(&node.node) {
            return Err("duplicate node measurement".into());
        }
        let before = bundle
            .before
            .iter()
            .find(|s| s.node == node.node)
            .ok_or("node before-state missing")?;
        let after = bundle
            .after
            .iter()
            .find(|s| s.node == node.node)
            .ok_or("node after-state missing")?;
        if before.started != after.started
            || !before.running
            || !after.running
            || after.health != "healthy"
            || after.oom_killed
        {
            return Err("node state changed during capture".into());
        }
        let stores: BTreeSet<_> = node
            .measurements
            .iter()
            .map(|m| m.capture.prefix.as_str())
            .collect();
        if stores != BTreeSet::from(["rspace", "eval"]) || node.measurements.len() != 2 {
            return Err("missing or duplicate store measurement".into());
        }
        for m in &node.measurements {
            if m.capture.prefix == "rspace"
                && m.capture.selected_root != bundle.finalized_block.post_state_hash
            {
                return Err("nodes used different finalized roots".into());
            }
            let kinds: BTreeSet<_> = m.by_kind.keys().map(String::as_str).collect();
            let categories: BTreeSet<_> = m.by_reachability.keys().map(String::as_str).collect();
            if kinds != BTreeSet::from(["DataLeaf", "ContinuationsLeaf", "JoinsLeaf"])
                || categories
                    != BTreeSet::from([
                        "selected_root",
                        "other_recorded_roots_only",
                        "not_referenced_by_captured_roots",
                    ])
            {
                return Err("missing leaf variant or reachability category".into());
            }
            for split in m.by_reachability.values() {
                if split.keys().map(String::as_str).collect::<BTreeSet<_>>() != kinds {
                    return Err("incomplete reachability partition".into());
                }
            }
            for (kind, stats) in &m.by_kind {
                stats.validate()?;
                let mut combined = Stats::default();
                for split in m.by_reachability.values() {
                    split[kind].validate()?;
                    combined.merge(&split[kind]);
                }
                if combined != *stats {
                    return Err("reachability statistics do not reconcile by variant".into());
                }
            }
            let rows: u64 = m.by_kind.values().map(|s| s.record_bytes.count).sum();
            let bytes: u64 = m.by_kind.values().map(|s| s.record_bytes.sum).sum();
            if rows != m.capture.cold_rows as u64 || bytes != m.capture.encoded_key_value_bytes {
                return Err("leaf totals do not reconcile".into());
            }
            let split_rows: u64 = m
                .by_reachability
                .values()
                .flat_map(|g| g.values())
                .map(|s| s.record_bytes.count)
                .sum();
            let split_bytes: u64 = m
                .by_reachability
                .values()
                .flat_map(|g| g.values())
                .map(|s| s.record_bytes.sum)
                .sum();
            if split_rows != rows || split_bytes != bytes {
                return Err("reachability partitions do not reconcile".into());
            }
            let selected_rows: u64 = m.by_reachability["selected_root"]
                .values()
                .map(|s| s.record_bytes.count)
                .sum();
            let other_rows: u64 = m.by_reachability["other_recorded_roots_only"]
                .values()
                .map(|s| s.record_bytes.count)
                .sum();
            if selected_rows != m.capture.selected_root_leaf_hashes as u64
                || selected_rows + other_rows != m.capture.all_root_leaf_hashes as u64
                || m.capture.raw_key_rows + m.capture.bincode_key_rows != rows
            {
                return Err("root coverage or key-format counts do not reconcile".into());
            }
            if m.capture.prefix == "rspace" {
                let selected: BTreeMap<_, _> = m.by_reachability["selected_root"]
                    .iter()
                    .map(|(kind, stats)| {
                        let mut logical = stats.clone();
                        logical.record_bytes = Distribution::default();
                        (kind.clone(), logical)
                    })
                    .collect();
                if common_selected
                    .as_ref()
                    .is_some_and(|previous| previous != &selected)
                {
                    return Err("finalized-root decoded statistics differ across nodes".into());
                }
                common_selected = Some(selected);
            }
            for (kind, s) in &m.by_kind {
                groups
                    .entry(format!("{} / {kind}", m.capture.prefix))
                    .or_default()
                    .merge(s);
            }
            if m.capture.prefix == "rspace" {
                for (category, kinds) in &m.by_reachability {
                    for (kind, s) in kinds {
                        reach
                            .entry((category.clone(), kind.clone()))
                            .or_default()
                            .merge(s);
                    }
                }
            }
        }
    }
    let mut text=format!("# Cold Leaf Measurements\n\nFull scans captured from {} to {} across {} nodes. All chain-state scans use finalized block **{}**. No deploy was submitted and no node was restarted.\n\n- Block hash: `{}`\n- Post-state root: `{}`\n- [Raw measurements and histograms](leaves-observed.json)\n- [Leaf structure diagram](leaf-internals.svg)\n- [Earlier sampled overview](../size-overview.md)\n\n## Scope and Method\n\nThese are complete scans within the recorded per-environment read transactions, not sampled estimates. The counts include physical copies across nodes and retained state versions. They are not counts of unique logical transactions or channels.\n\nEach stored leaf is decoded as `PersistedData`, then as its source-derived datum, continuation, or join records. The decoder types are generated from the node source declarations and RhoTypes schema. Every encoded record must be consumed completely and reproduce its serialized size. Every cold leaf and traversed radix node must match its content hash.\n\nThe cold-store and history transactions are separate, not one atomic cross-environment snapshot. Every referenced leaf must exist in the captured cold-store snapshot. A missing node, missing leaf, decode error, or resource limit fails the measurement instead of producing a partial success.\n\nEncoded record bytes include the physical key, value, and LMDB adapter prefixes. They exclude LMDB page overhead. Distributions use exact frequency counts and nearest-rank percentiles. MiB means 1,048,576 bytes. No contract payloads, channel values, or private keys are included in the report.\n\n## Leaf Variants\n\nSorted by exact cardinality x mean encoded row size, including replicas.\n\n| Entity | Leaf rows | Mean row bytes | Total encoded MiB | Nested elements | Par occurrences |\n| --- | --- | --- | --- | --- | --- |\n",bundle.started_utc,bundle.finished_utc,bundle.nodes.len(),bundle.finalized_block.block_number,bundle.finalized_block.block_hash,bundle.finalized_block.post_state_hash);
    for (kind, s) in order(&groups) {
        text.push_str(&format!(
            "| {kind} | {} | {} | {} | {} | {} |\n",
            s.record_bytes.count,
            average(&s.record_bytes),
            mib(s.record_bytes.sum),
            s.element_bytes.count,
            s.par_inclusive_bytes.count
        ));
    }
    text.push_str("\n## Internal Cardinality and Size\n\nFor DataLeaf, an element is one datum and the inner count is its top-level Par values. For ContinuationsLeaf, an element is one waiting continuation and the inner count is its BindPatterns. For JoinsLeaf, an element is one channel group and the inner count is its channels.\n\nEach distribution column lists **min / p50 / p95 / max**. Empty populations are n/a, not a measured zero-size object.\n\n| Entity | Elements per leaf | Element bytes | Values, patterns, or channels per element | Row bytes |\n| --- | --- | --- | --- | --- |\n");
    for (kind, s) in order(&groups) {
        text.push_str(&format!(
            "| {kind} | {} | {} | {} | {} |\n",
            spread(&s.elements_per_leaf),
            spread(&s.element_bytes),
            spread(&s.values_patterns_or_channels_per_element),
            spread(&s.record_bytes)
        ));
    }
    text.push_str("\n## Nested Par Values\n\nA Par occurrence is a typed Par message in a decoded record, including empty Par messages. The walk includes all generated message fields and oneof alternatives. Opaque byte arrays, random states, and replay output bytes are not interpreted as additional Par messages.\n\nA Par size includes its entire encoded subtree. These inclusive sizes overlap for parent and child Par values, so their sum is not an additional storage total. Outermost Par bytes count non-overlapping Par subtrees within each stored record. These bytes are already included in the leaf totals. Neither measure is heap memory usage.\n\n| Entity | Par count | Par per leaf | Inclusive Par bytes | Mean inclusive bytes | Outermost Par count | Outermost Par MiB | Max typed traversal depth |\n| --- | --- | --- | --- | --- | --- | --- | --- |\n");
    for (kind, s) in order(&groups) {
        text.push_str(&format!(
            "| {kind} | {} | {} | {} | {} | {} | {} | {} |\n",
            s.par_inclusive_bytes.count,
            spread(&s.pars_per_leaf),
            spread(&s.par_inclusive_bytes),
            average(&s.par_inclusive_bytes),
            s.outer_par_count,
            mib(s.outer_par_bytes),
            s.max_type_depth
        ));
    }
    text.push_str("\n## Root Reachability\n\n- `selected_root`: the preferred physical cold row used by the common finalized block root.\n- `other_recorded_roots_only`: reachable from another recorded root or the current-root pointer, but not from the selected finalized root. This can include older history and newer unresolved state.\n- `not_referenced_by_captured_roots`: not used by the traversed roots. This can include in-flight checkpoint writes, retained unreferenced state, or a superseded key-format copy. It is not a proof that deletion is safe.\n\nThe reader prefers raw-hash keys, then bincode-hash keys, as the node does. Shared leaves are counted once per physical row in each node, even when multiple trie paths or roots reference them.\n\n| Category | Entity | Leaf rows | Encoded MiB | Nested elements | Par occurrences |\n| --- | --- | --- | --- | --- | --- |\n");
    for ((category, kind), s) in &reach {
        text.push_str(&format!(
            "| {category} | {kind} | {} | {} | {} | {} |\n",
            s.record_bytes.count,
            mib(s.record_bytes.sum),
            s.element_bytes.count,
            s.par_inclusive_bytes.count
        ));
    }
    text.push_str("\n## Common Finalized State\n\nThese decoded counts, element sizes, and Par distributions match across all scanned nodes. The table shows one copy of the common finalized root, not the sum of replicas. Physical row sizes can differ by eight bytes because raw-hash and bincode-hash keys have different lengths.\n\n| Entity | Reachable leaves | Nested elements | Element bytes | Par occurrences | Outermost Par bytes |\n| --- | --- | --- | --- | --- | --- |\n");
    for (kind, s) in common_selected
        .as_ref()
        .ok_or("common finalized root missing")?
    {
        text.push_str(&format!(
            "| {kind} | {} | {} | {} | {} | {} |\n",
            s.elements_per_leaf.count,
            s.element_bytes.count,
            s.element_bytes.sum,
            s.par_inclusive_bytes.count,
            s.outer_par_bytes
        ));
    }
    text.push_str("\n## Per-Node Chain State\n\n| Node | Entity | All rows | Encoded MiB | Finalized-root rows | Other-root rows | Unreferenced rows |\n| --- | --- | --- | --- | --- | --- | --- |\n");
    for node in &bundle.nodes {
        for m in &node.measurements {
            if m.capture.prefix != "rspace" {
                continue;
            }
            for (kind, s) in order(&m.by_kind) {
                text.push_str(&format!(
                    "| {} | {kind} | {} | {} | {} | {} | {} |\n",
                    node.node,
                    s.record_bytes.count,
                    mib(s.record_bytes.sum),
                    m.by_reachability["selected_root"][kind].record_bytes.count,
                    m.by_reachability["other_recorded_roots_only"][kind]
                        .record_bytes
                        .count,
                    m.by_reachability["not_referenced_by_captured_roots"][kind]
                        .record_bytes
                        .count
                ));
            }
        }
    }
    text.push_str("\n## Capture Evidence\n\n| Node / store | History txn | Cold txn | Recorded roots | Selected-root leaf hashes | All-root leaf hashes | Reader seconds | Decode seconds | Live cold-page MiB |\n| --- | --- | --- | --- | --- | --- | --- | --- | --- |\n");
    for node in &bundle.nodes {
        for m in &node.measurements {
            let c = &m.capture;
            text.push_str(&format!(
                "| {} / {} | {} | {} | {} | {} | {} | {:.3} | {:.3} | {} |\n",
                node.node,
                c.prefix,
                c.history_transaction,
                c.cold_transaction,
                c.recorded_roots,
                c.selected_root_leaf_hashes,
                c.all_root_leaf_hashes,
                c.reader_seconds,
                m.decode_seconds,
                mib(c.cold_page_bytes)
            ));
        }
    }
    text.push_str("\n## Safety and Limits\n\nThe probe opens LMDB data read-only and uses normal reader locking. Reader slots in lock.mdb can change. It releases the transactions before nested decoding. It does not use NO_LOCK and does not copy live LMDB files without a transaction.\n\nEach probe runs in a temporary container with a separate 512 MiB memory-limited cgroup, one CPU, no network, dropped capabilities, and a read-only root filesystem. It shares the node PID namespace for correct LMDB PID locking, but does not signal or restart node processes. The data volume permits reader-lock updates. The host kernel cannot enforce swap limits.\n\nBounds per environment: 10 seconds for capture, 256 MiB of encoded rows, one million rows or traversed nodes, and 60 seconds for decoding. Each decoded record is limited to 16 MiB. Nested traversal is limited to 256 typed levels and five million message visits per leaf. A limit failure produces no successful complete report.\n\nThe stored counts describe this capture interval. They do not establish long-term storage growth, retention correctness, or consensus correctness.\n\n## Reproduce\n\nRequirements: Docker, jq, Rust, Cargo, and protoc with proto3 support. Set PROTOC when a non-default compiler is required.\n\n```bash\nbash docs/erd/collect-leaves.sh docs/erd/storage\nCARGO_TARGET_DIR=target/erd-tools/build \\\n  cargo run --locked --manifest-path docs/erd/Cargo.toml --bin erd-leaf-stats -- \\\n  --report docs/erd/storage/leaves-observed.json docs/erd/storage/leaf-measurements.md\n```\n\nAdd `--check` to the report command to verify the Markdown against the saved measurements without accessing the shard.\n");
    Ok(text)
}

pub fn generate(input: &Path, output: &Path, check: bool) -> Result<(), Box<dyn Error>> {
    let bundle: Bundle = serde_json::from_slice(&fs::read(input)?)?;
    let text = render(&bundle)?;
    if check {
        if fs::read_to_string(output)? != text {
            return Err("stale leaf measurement report".into());
        }
    } else {
        fs::write(output, text)?;
    }
    println!("Validated complete cold-leaf measurement report");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundle() -> Bundle {
        let dir = tempfile::tempdir().unwrap();
        let root = crate::tests::fixture(dir.path());
        let state = || State {
            node: "fixture".into(),
            started: "2026-11-25T00:00:00Z".into(),
            running: true,
            health: "healthy".into(),
            oom_killed: false,
        };
        Bundle {
            schema_version: 1,
            started_utc: "2026-11-25T00:01:00Z".into(),
            finished_utc: "2026-11-25T00:02:00Z".into(),
            finalized_block: Finalized {
                block_number: 1,
                block_hash: "fixture".into(),
                post_state_hash: hex::encode(root),
                is_finalized: true,
            },
            nodes: vec![Node {
                node: "fixture".into(),
                schema_version: 1,
                complete: true,
                measurements: vec![
                    crate::measure(dir.path(), "rspace", Some(root)).unwrap(),
                    crate::measure(dir.path(), "eval", None).unwrap(),
                ],
            }],
            before: vec![state()],
            after: vec![state()],
        }
    }

    #[test]
    fn saved_histograms_round_trip_and_report_check_detects_changes() {
        let b = bundle();
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("input.json");
        let output = dir.path().join("report.md");
        fs::write(&input, serde_json::to_vec(&b).unwrap()).unwrap();
        generate(&input, &output, false).unwrap();
        generate(&input, &output, true).unwrap();
        let report = fs::read_to_string(&output).unwrap();
        assert!(report.contains("| rspace / DataLeaf | 3 |"));
        assert!(report.contains("| eval / DataLeaf | 0 | n/a | 0.000 | 0 | 0 |"));
        assert!(report.contains("| DataLeaf | 1 | 1 |"));
        fs::write(&output, "stale").unwrap();
        assert!(generate(&input, &output, true).is_err());
    }

    #[test]
    fn rejects_partial_missing_or_inconsistent_measurements() {
        let original = serde_json::to_value(bundle()).unwrap();
        for (pointer, value) in [
            ("/schema_version", serde_json::json!(99)),
            ("/nodes/0/complete", serde_json::json!(false)),
            ("/finalized_block/isFinalized", serde_json::json!(false)),
            ("/after/0/started", serde_json::json!("restarted")),
            (
                "/nodes/0/measurements/0/capture/selected_root",
                serde_json::json!("different"),
            ),
            (
                "/nodes/0/measurements/0/capture/cold_rows",
                serde_json::json!(6),
            ),
            (
                "/nodes/0/measurements/0/capture/all_root_leaf_hashes",
                serde_json::json!(0),
            ),
            (
                "/nodes/0/measurements/0/by_kind/DataLeaf/record_bytes/sum",
                serde_json::json!(0),
            ),
            (
                "/nodes/0/measurements/0/by_kind/DataLeaf/pars_per_leaf/frequencies",
                serde_json::json!({}),
            ),
        ] {
            let mut changed = original.clone();
            *changed.pointer_mut(pointer).unwrap() = value;
            let b = serde_json::from_value(changed).unwrap();
            assert!(render(&b).is_err(), "accepted {pointer}");
        }
        let mut b: Bundle = serde_json::from_value(original).unwrap();
        b.nodes[0].measurements.pop();
        assert!(render(&b).is_err());
        let mut b = bundle();
        b.nodes[0].measurements[0]
            .by_reachability
            .remove("selected_root");
        assert!(render(&b).is_err());
    }

    #[test]
    fn report_aggregates_physical_replicas_but_keeps_one_finalized_state() {
        let mut b = bundle();
        let mut replica = bundle().nodes.remove(0);
        replica.node = "replica".into();
        b.nodes.push(replica);
        let state = || State {
            node: "replica".into(),
            started: "2026-11-25T00:00:00Z".into(),
            running: true,
            health: "healthy".into(),
            oom_killed: false,
        };
        b.before.push(state());
        b.after.push(state());
        let report = render(&b).unwrap();
        assert!(report.contains("| rspace / DataLeaf | 6 |"));
        assert!(report.contains("| DataLeaf | 1 | 1 |"));
        b.nodes[1].node = "fixture".into();
        assert!(render(&b).is_err());
    }
}
