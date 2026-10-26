use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use casper_soak::{encoded, exclusive, manifest, record, regular, walk, MAX_BYTES};
use clap::Parser;
use eyre::{ensure, eyre, Result};
use serde_json::json;

#[derive(Parser)]
struct Args {
    #[arg(long)]
    evidence: PathBuf,
    #[arg(long)]
    output: PathBuf,
}

fn expected() -> BTreeMap<String, Vec<i64>> {
    let mut cases = BTreeMap::new();
    for (name, exits) in [
        ("complete", vec![0]),
        ("duplicate", vec![0]),
        ("missing", vec![1]),
        ("missing-zero", vec![1]),
        ("wrong-identity", vec![1]),
        ("missing-artifact", vec![1]),
        ("boolean-counter", vec![1]),
        ("mismatch", vec![1]),
        ("fault-applied", vec![0]),
        ("fault-not-applied", vec![1]),
        ("fault-restart-applied", vec![0]),
        ("fault-restart-unready", vec![1]),
        ("history", vec![0, 0, 0]),
        ("rollback", vec![0, 2]),
        ("failure-counter", vec![1, 2]),
        ("resource", vec![1, 1]),
        ("terminal", vec![0, 0]),
        ("terminal-before-exec", vec![1]),
        ("terminal-lock", vec![1]),
        ("active-stop", vec![0]),
        ("timeout", vec![1, 1]),
        ("capability", vec![3]),
        ("executable", vec![2]),
        ("configuration", vec![2]),
        ("policy", vec![2]),
        ("node-kind", vec![2]),
        ("pending-verification", vec![0]),
        ("missing-binding", vec![2]),
        ("open-merge", vec![2]),
        ("corrupt-capture", vec![0, 2]),
        ("empty-inventory", vec![0, 2]),
        ("cached-success", vec![1, 2]),
        ("incomplete-capture", vec![1, 2]),
    ] {
        cases.insert(name.to_owned(), exits);
    }
    for name in [
        "missing",
        "array",
        "duplicate",
        "nested-duplicate",
        "nan",
        "overflow",
        "utf8",
        "oversize",
        "boolean-schema",
        "traversal",
        "duplicate-scenarios",
        "symbolic-link",
        "fifo",
        "directory",
    ] {
        cases.insert(format!("invalid-{name}"), vec![2]);
    }
    let mut resume = vec![0, 0];
    resume.extend(vec![2; manifest::FIELDS.len() + 4]);
    cases.insert("manifest-resume".to_owned(), resume);
    cases
}

fn check(evidence: &Path, output: &Path) -> Result<()> {
    let cases = expected();
    let expected_files: BTreeMap<_, _> = cases
        .iter()
        .flat_map(|(case, exits)| {
            exits.iter().enumerate().map(move |(index, code)| {
                (format!("{case}/invocation-{:02}.json", index + 1), *code)
            })
        })
        .collect();
    let mut actual_files = BTreeSet::new();
    for path in walk(evidence)? {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| eyre!("The evidence path is invalid."))?;
        if !name.starts_with("invocation-") {
            continue;
        }
        let relative = path
            .strip_prefix(evidence)?
            .to_str()
            .ok_or_else(|| eyre!("The evidence path is invalid."))?;
        let expected = expected_files
            .get(relative)
            .ok_or_else(|| eyre!("An invocation is not registered."))?;
        let report = record(&path)?;
        ensure!(
            report["command"] == json!(["bash", "scripts/run-merge-recovery-soak.sh"]),
            "The invocation command differs."
        );
        ensure!(
            report["expected_exit"].as_i64() == Some(*expected)
                && report["actual_exit"].as_i64() == Some(*expected),
            "The invocation exit differs."
        );
        ensure!(
            actual_files.insert(relative.to_owned()),
            "An invocation is duplicated."
        );
    }
    ensure!(
        actual_files.len() == expected_files.len(),
        "A required invocation is missing."
    );
    for (suite, count) in [("manifest", 3), ("models", 4), ("driver", 9)] {
        let log = String::from_utf8(regular(&evidence.join(format!("{suite}.txt")), MAX_BYTES)?)?;
        let pattern = format!(
            r"(?m)^test result: ok\. {count} passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in [0-9.]+s$"
        );
        ensure!(
            regex::Regex::new(&pattern)?.find_iter(&log).count() == 1
                && !log.contains("test result: FAILED"),
            "The test suite result differs."
        );
    }
    exclusive(
        output,
        &encoded(
            &json!({"schema_version":1,"scope":"binding-inventory-only","driver_invocations":actual_files.len(),"registered_cases":cases.len(),"matched_exits":true,"claim_discharge":"pending","node_execution":false}),
        )?,
        false,
    )
}

fn main() {
    let args = Args::parse();
    if let Err(error) = check(&args.evidence, &args.output) {
        eprintln!("{error}");
        std::process::exit(2);
    }
}
