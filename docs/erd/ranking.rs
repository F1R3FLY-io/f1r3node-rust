use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::path::Path;

use serde::Deserialize;

#[derive(Deserialize)]
struct Observation {
    started_utc: String,
    finished_utc: String,
    stores: Vec<Store>,
}

#[derive(Deserialize)]
struct Store {
    container: String,
    path: String,
    status: String,
    tables: Vec<Table>,
}

#[derive(Deserialize)]
struct Table {
    name: String,
    status: String,
    records: Option<u64>,
    page_bytes: Option<u64>,
    sample: Option<Sample>,
}

#[derive(Deserialize)]
struct Sample {
    records: u64,
    key_value_bytes: u64,
    complete: bool,
}

#[derive(Clone, Debug)]
struct Row {
    entity: String,
    instances: u64,
    records: u64,
    sampled: u64,
    bytes: Option<f64>,
    pages: u64,
    exact: bool,
}

fn row(environment: &str, table: &Table) -> Result<Row, Box<dyn Error>> {
    let records = table.records.ok_or("open table has no row count")?;
    let pages = table.page_bytes.ok_or("open table has no page size")?;
    let sampled = table.sample.as_ref().map_or(0, |s| s.records);
    if let Some(s) = &table.sample {
        if sampled > records
            || s.complete != (sampled == records)
            || (sampled == 0 && s.key_value_bytes != 0)
        {
            return Err(format!("inconsistent sample for {}", table.name).into());
        }
    }
    let bytes = if records == 0 {
        Some(0.0)
    } else {
        table
            .sample
            .as_ref()
            .filter(|s| s.records > 0)
            .map(|s| records as f64 * s.key_value_bytes as f64 / s.records as f64)
    };
    Ok(Row {
        entity: format!("{environment} / {}", table.name),
        instances: 1,
        records,
        sampled,
        bytes,
        pages,
        exact: records == sampled,
    })
}

fn merge(a: &mut Row, b: &Row) -> Result<(), Box<dyn Error>> {
    a.instances = a
        .instances
        .checked_add(b.instances)
        .ok_or("instance count overflow")?;
    a.records = a
        .records
        .checked_add(b.records)
        .ok_or("record count overflow")?;
    a.sampled = a
        .sampled
        .checked_add(b.sampled)
        .ok_or("sample count overflow")?;
    a.pages = a
        .pages
        .checked_add(b.pages)
        .ok_or("page byte count overflow")?;
    a.bytes = a.bytes.zip(b.bytes).map(|(a, b)| a + b);
    a.exact &= b.exact;
    Ok(())
}

fn sorted(rows: &[Row]) -> Vec<Row> {
    let mut result = rows.to_vec();
    result.sort_by(|a, b| {
        let order = match (a.bytes, b.bytes) {
            (Some(a), Some(b)) => b.total_cmp(&a),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        };
        order.then_with(|| a.entity.cmp(&b.entity))
    });
    result
}

fn size(bytes: f64) -> String {
    if bytes >= 1_048_576.0 {
        format!("{:.2} MiB", bytes / 1_048_576.0)
    } else if bytes >= 1024.0 {
        format!("{:.2} KiB", bytes / 1024.0)
    } else {
        format!("{bytes:.0} B")
    }
}

fn table(rows: &[Row]) -> String {
    let mut out = String::from("| Rank | Entity | DB instances | Cardinality | Average B/record | Cardinality x size | Live page bytes | Evidence |\n| --- | --- | --- | --- | --- | --- | --- | --- |\n");
    for (i, row) in sorted(rows).iter().enumerate() {
        let mean = row.bytes.filter(|_| row.records > 0).map_or_else(
            || "n/a".into(),
            |b| format!("{:.2}", b / row.records as f64),
        );
        let total = row.bytes.map_or_else(|| "unranked".into(), size);
        let rank = row
            .bytes
            .map_or_else(|| "-".into(), |_| (i + 1).to_string());
        let evidence = if row.bytes.is_none() {
            "Size not measured".to_string()
        } else if row.records == 0 {
            "Exact, empty".to_string()
        } else if row.exact {
            format!("Exact, all {} records", row.sampled)
        } else {
            let warning = if row.bytes.is_some_and(|b| b > row.pages as f64) {
                "; exceeds pages"
            } else {
                ""
            };
            format!("Sampled, {} records{warning}", row.sampled)
        };
        out.push_str(&format!(
            "| {rank} | `{}` | {} | {} | {mean} | {total} | {} | {evidence} |\n",
            row.entity,
            row.instances,
            row.records,
            size(row.pages as f64)
        ));
    }
    out
}

fn report(observation: &Observation) -> Result<String, Box<dyn Error>> {
    let mut groups: BTreeMap<String, Row> = BTreeMap::new();
    let mut nodes: BTreeMap<String, Vec<Row>> = BTreeMap::new();
    let mut absent = BTreeMap::<String, Vec<String>>::new();
    for store in &observation.stores {
        let environment = store
            .path
            .strip_prefix("/var/lib/rnode/")
            .or_else(|| store.path.strip_prefix("/var/lib/"))
            .unwrap_or(&store.path);
        if store.status != "read-only snapshot" {
            absent
                .entry(format!("{environment}: {}", store.status))
                .or_default()
                .push(store.container.clone());
            continue;
        }
        for t in &store.tables {
            if t.status != "open" {
                absent
                    .entry(format!("{environment} / {}: {}", t.name, t.status))
                    .or_default()
                    .push(store.container.clone());
                continue;
            }
            let r = row(environment, t)?;
            nodes
                .entry(store.container.clone())
                .or_default()
                .push(r.clone());
            if let Some(g) = groups.get_mut(&r.entity) {
                merge(g, &r)?;
            } else {
                groups.insert(r.entity.clone(), r);
            }
        }
    }
    let mut out = format!("# Entity Size Overview\n\nSorted in descending order by **cardinality x average encoded record size**.\n\nSnapshot: {} to {}. This uses the saved observations, not a new live measurement.\n\n[Raw observations](storage/observed.json) | [Measurement details](storage/observed.md) | [Storage formulas](storage/sizing.md) | [Memory assessments](memory/sizing.md)\n\n## Calculation\n\nFor each database instance:\n\n```text\naverage_record_bytes = sampled_key_value_bytes / sampled_records\nestimated_total_bytes = exact_record_count * average_record_bytes\n```\n\nSizes include encoded keys, encoded values, and adapter prefixes. They exclude LMDB page overhead.\nIf every record was sampled, the total is exact. Otherwise it is an estimate from the first keys in LMDB order, not a random sample.\nEmpty measured tables have an exact total of zero, but no measured average record size. Missing counts or sizes are not treated as zero.\n\nFor each whole-shard entity, add the per-instance products. The displayed average is that sum divided by the combined cardinality. It is not an unweighted average of node samples.\nAll sort comparisons use unrounded values. Display units use powers of 1024.\n\n**Sampling warning:** an estimated payload larger than its live page usage signals a nonrepresentative sample. Such rows are marked `exceeds pages`. The estimate is not capped or presented as exact. The ranking is therefore approximate where samples are incomplete.\n\nLive page usage is measured separately. It includes branch, leaf, and overflow pages, but excludes free pages and environment metadata. It is not heap usage, file length, or reserved map size.\n\n## Whole Shard, Including Replicas\n\nCardinality counts stored copies across nodes and queues, not unique chain entities. DB instances include empty tables. Different environments were read at different times.\n\n", observation.started_utc, observation.finished_utc);
    out.push_str("The later [complete cold-leaf scan](storage/leaf-measurements.md) gives exact totals for DataLeaf, ContinuationsLeaf, and JoinsLeaf. It also measures their internal records and root reachability. Its capture time differs from this sampled overview; the totals must not be combined.\n\n");
    out.push_str(&table(&groups.into_values().collect::<Vec<_>>()));
    out.push_str("\n## Per Node\n\nEach table below has its own descending ranking. These views avoid combining replica copies.\n");
    for (node, rows) in nodes {
        out.push_str(&format!("\n### {node}\n\n{}", table(&rows)));
    }
    out.push_str("\n## Not Ranked\n\nThe following environments or databases were absent from the sample. Absence is not a measured zero-row table.\n\n");
    if absent.is_empty() {
        out.push_str("None.\n");
    }
    for (reason, containers) in absent {
        out.push_str(&format!("- `{reason}`: {}.\n", containers.join(", ")));
    }
    out.push_str("\nThe 1,868 in-memory declarations cannot be ranked by total retained bytes from this snapshot. Per-type live counts and owned allocation sizes are not both available.\nDo not multiply resident DAG counts by encoded disk record sizes and call the result RAM usage.\nFilesystem-only entities and logical contract entities also lack separate measured products. Contract values are already represented within tuple-space storage. Adding them again would double-count data.\n\n## Regenerate\n\nThis command reads the saved snapshot and does not access Docker or the running nodes:\n\n```bash\nCARGO_TARGET_DIR=target/erd-tools/build \\\n  cargo run --locked --manifest-path docs/erd/Cargo.toml -- --rank-sizes\n```\n\nAdd `--check` to verify the saved overview without changing it. The ERD render check also validates this overview.\n");
    Ok(out)
}

pub fn generate(root: &Path, check: bool) -> Result<(), Box<dyn Error>> {
    let observation: Observation =
        serde_json::from_slice(&fs::read(root.join("docs/erd/storage/observed.json"))?)?;
    super::output(
        &root.join("docs/erd/size-overview.md"),
        &report(&observation)?,
        check,
    )?;
    println!("Validated descending entity size overview");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measured(name: &str, records: u64, sampled: u64, bytes: u64) -> Table {
        Table {
            name: name.into(),
            status: "open".into(),
            records: Some(records),
            page_bytes: Some(4096),
            sample: Some(Sample {
                records: sampled,
                key_value_bytes: bytes,
                complete: records == sampled,
            }),
        }
    }

    #[test]
    fn ranks_products_not_cardinality_or_individual_record_size() {
        let many = row("e", &measured("many", 1000, 100, 100)).unwrap();
        let few = row("e", &measured("few", 2, 2, 600)).unwrap();
        let medium = row("e", &measured("medium", 20, 10, 800)).unwrap();
        let result = sorted(&[few, many, medium]);
        assert_eq!(
            result.iter().map(|r| r.entity.as_str()).collect::<Vec<_>>(),
            ["e / medium", "e / many", "e / few"]
        );
        assert_eq!(result[0].bytes, Some(1600.0));
    }

    #[test]
    fn replica_total_is_sum_of_products_not_average_of_sample_means() {
        let mut large = row("e", &measured("a", 100, 10, 20)).unwrap();
        let small = row("e", &measured("a", 1, 1, 100)).unwrap();
        merge(&mut large, &small).unwrap();
        assert_eq!(large.bytes, Some(300.0));
        assert_eq!(large.records, 101);
        assert_eq!(large.instances, 2);
        assert!(!large.exact);
    }

    #[test]
    fn missing_sizes_are_not_zero_and_empty_tables_are_exact() {
        let empty = row("e", &measured("empty", 0, 0, 0)).unwrap();
        let unknown = row("e", &measured("unknown", 10, 0, 0)).unwrap();
        assert_eq!(empty.bytes, Some(0.0));
        assert!(empty.exact);
        assert!(unknown.bytes.is_none());
        let mut combined = empty.clone();
        merge(&mut combined, &unknown).unwrap();
        assert!(combined.bytes.is_none());
        let rows = sorted(&[unknown, empty]);
        assert_eq!(rows[0].entity, "e / empty");
        assert!(table(&rows).contains("Size not measured"));
    }

    #[test]
    fn invalid_samples_fail_and_bias_is_visible() {
        assert!(row("e", &measured("bad", 2, 3, 10)).is_err());
        let mut bad = measured("bad", 2, 1, 10);
        bad.sample.as_mut().unwrap().complete = true;
        assert!(row("e", &bad).is_err());
        let biased = row("e", &measured("biased", 1000, 1, 100)).unwrap();
        assert!(table(&[biased]).contains("exceeds pages"));
    }

    #[test]
    fn report_preserves_absent_environments_without_ranking_them() {
        let observation = Observation {
            started_utc: "start".into(),
            finished_utc: "finish".into(),
            stores: vec![
                Store {
                    container: "node".into(),
                    path: "/var/lib/rnode/absent".into(),
                    status: "environment absent".into(),
                    tables: vec![],
                },
                Store {
                    container: "node".into(),
                    path: "/var/lib/rnode/present".into(),
                    status: "read-only snapshot".into(),
                    tables: vec![measured("a", 1, 1, 10)],
                },
            ],
        };
        let rendered = report(&observation).unwrap();
        assert!(rendered.contains("`absent: environment absent`: node"));
        assert!(!rendered.contains("| `absent"));
        assert!(rendered.contains("Exact, all 1 records"));
    }
}
