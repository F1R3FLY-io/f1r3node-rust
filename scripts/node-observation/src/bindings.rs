use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use eyre::{ensure, eyre, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{check_hashes, digest, models, tool_sources, write_json, AREA};

#[derive(Deserialize)]
pub(crate) struct Bindings {
    pub claims: Vec<Claim>,
}

#[derive(Deserialize)]
pub(crate) struct Claim {
    pub claim_id: String,
    pub claim: String,
    pub model: String,
    pub properties: Vec<Property>,
}

#[derive(Deserialize, Serialize)]
pub(crate) struct Property {
    pub property: usize,
    pub invariants: Vec<String>,
    pub coverage: String,
    pub tests: Vec<Test>,
}

#[derive(Deserialize, Serialize)]
pub(crate) struct Test {
    pub source: String,
    pub name: String,
}

#[derive(Serialize)]
pub(crate) struct Suite {
    pub executable: String,
    pub passed: BTreeSet<String>,
}

pub(crate) fn read_suites(log: &str) -> Result<BTreeMap<String, Suite>> {
    let specs = [
        (
            "shared/tests/soak_snapshot.rs",
            "tests/soak_snapshot.rs",
            "soak_snapshot",
            19,
        ),
        (
            "block-storage/tests/soak_snapshot.rs",
            "tests/soak_snapshot.rs",
            "soak_snapshot",
            26,
        ),
        (
            "block-storage/src/rust/dag/soak_snapshot.rs",
            "src/lib.rs",
            "block_storage",
            2,
        ),
        (
            "node/tests/soak_observer.rs",
            "tests/soak_observer.rs",
            "soak_observer",
            18,
        ),
    ];
    let header = Regex::new(r"(?m)^ +Running (?:tests/|unittests )")?;
    let starts: Vec<_> = header
        .find_iter(log)
        .map(|m| m.start())
        .chain([log.len()])
        .collect();
    ensure!(
        starts.len() == specs.len() + 1,
        "The suite inventory differs."
    );
    let passed_pattern = Regex::new(r"(?m)^test (\S+) \.\.\. ok$")?;
    let result_pattern = Regex::new(r"(?m)^test result:.*$")?;
    let mut suites = BTreeMap::new();
    for ((source, target, binary, count), range) in specs.into_iter().zip(starts.windows(2)) {
        let section = &log[range[0]..range[1]];
        let header = section.lines().next().unwrap_or_default();
        let pattern = Regex::new(&format!(r"\(target/debug/deps/({binary}-[a-f0-9]+)\)"))?;
        let matched = pattern
            .captures(header)
            .ok_or_else(|| eyre!("Unexpected suite: {source}"))?;
        ensure!(header.contains(target), "Unexpected target: {source}");
        let names: Vec<_> = passed_pattern
            .captures_iter(section)
            .map(|c| c[1].to_owned())
            .collect();
        let passed: BTreeSet<_> = names.iter().cloned().collect();
        let results: Vec<_> = result_pattern
            .find_iter(section)
            .map(|m| m.as_str())
            .collect();
        ensure!(
            names.len() == count
                && passed.len() == count
                && results.len() == 1
                && results[0].starts_with(&format!("test result: ok. {count} passed; 0 failed;")),
            "Incomplete or failed suite: {source}"
        );
        suites.insert(source.to_owned(), Suite {
            executable: format!("target/debug/deps/{}", &matched[1]),
            passed,
        });
    }
    Ok(suites)
}

pub(crate) fn check_tests(
    root: &Path,
    bindings: &Bindings,
    suites: &BTreeMap<String, Suite>,
) -> Result<Vec<serde_json::Value>> {
    let property_pattern = Regex::new(r"(?m)^(\d+)\. ")?;
    let mut checked = Vec::new();
    for claim in &bindings.claims {
        let text = fs::read_to_string(root.join(&claim.claim))?;
        let section = text
            .split_once("## Required properties\n")
            .ok_or_else(|| eyre!("The required properties are missing."))?
            .1
            .split("\n## ")
            .next()
            .unwrap_or_default();
        let properties: Vec<usize> = property_pattern
            .captures_iter(section)
            .map(|c| c[1].parse())
            .collect::<std::result::Result<_, _>>()?;
        ensure!(
            !properties.is_empty()
                && properties == (1..=properties.len()).collect::<Vec<_>>()
                && properties
                    == claim
                        .properties
                        .iter()
                        .map(|p| p.property)
                        .collect::<Vec<_>>(),
            "The property inventory differs."
        );
        let model = fs::read_to_string(root.join(AREA).join(format!("{}.tla", claim.model)))?;
        let config = fs::read_to_string(root.join(AREA).join(format!("MC_{}.cfg", claim.model)))?;
        for prop in &claim.properties {
            ensure!(
                !prop.tests.is_empty(),
                "A property has no executable tests."
            );
            for invariant in &prop.invariants {
                ensure!(
                    Regex::new(&format!(r"(?m)^{} ==", regex::escape(invariant)))?.is_match(&model)
                        && Regex::new(&format!(r"\b{}\b", regex::escape(invariant)))?
                            .is_match(&config),
                    "Missing or unchecked invariant: {invariant}"
                );
            }
            for test in &prop.tests {
                let name = test.name.rsplit("::").next().unwrap_or_default();
                let code = fs::read_to_string(root.join(&test.source))?;
                ensure!(
                    Regex::new(&format!(r"\bfn {}\s*\(", regex::escape(name)))?.is_match(&code),
                    "Missing test: {}",
                    test.name
                );
                ensure!(
                    suites
                        .get(&test.source)
                        .is_some_and(|s| s.passed.contains(&test.name)),
                    "Test did not pass: {}:{}",
                    test.source,
                    test.name
                );
            }
            let mut entry = serde_json::to_value(prop)?;
            entry["claim_id"] = json!(claim.claim_id);
            checked.push(entry);
        }
    }
    Ok(checked)
}

pub(crate) fn read_manifest(text: &str) -> Result<BTreeMap<String, String>> {
    let hash_pattern = Regex::new(r"^[a-f0-9]{64}$")?;
    let mut manifest = BTreeMap::new();
    for line in text.lines() {
        let (hash, name) = line
            .split_once("  ")
            .ok_or_else(|| eyre!("Invalid source manifest."))?;
        ensure!(
            hash_pattern.is_match(hash) && !name.is_empty(),
            "Invalid source manifest entry."
        );
        ensure!(
            manifest.insert(name.to_owned(), hash.to_owned()).is_none(),
            "Duplicate source manifest entry."
        );
    }
    ensure!(!manifest.is_empty(), "The source manifest is empty.");
    Ok(manifest)
}

pub(crate) fn run(
    root: &Path,
    isolated_manifest: &Path,
    rust_log: &Path,
    model_report: &Path,
    output: &Path,
) -> Result<()> {
    let inventory = root.join(AREA).join("bindings.json");
    let bindings: Bindings = serde_json::from_slice(&fs::read(&inventory)?)?;
    ensure!(
        bindings
            .claims
            .iter()
            .map(|c| c.claim_id.as_str())
            .collect::<Vec<_>>()
            == [
                "CLAIM-CASPER-NODE-OBSERVATION-001",
                "CLAIM-CASPER-NODE-OBSERVATION-002"
            ],
        "The claim inventory differs."
    );
    models::verify_report(root, model_report)?;
    let isolated = read_manifest(&fs::read_to_string(isolated_manifest)?)?;
    let inputs: BTreeMap<_, _> = isolated
        .into_iter()
        .filter(|(name, _)| {
            name.ends_with(".rs")
                || name.ends_with(".proto")
                || name.starts_with(".cargo/")
                || Path::new(name).file_name().is_some_and(|file| {
                    ["Cargo.toml", "Cargo.lock", "rust-toolchain.toml"]
                        .iter()
                        .any(|expected| file == *expected)
                })
        })
        .collect();
    check_hashes(root, &inputs)?;
    let checker_sources = tool_sources(root)?;
    let artifact_pattern = Regex::new(r"(?m)^  - (\S+\.rs)$")?;
    let mut claims = BTreeMap::new();
    for claim in &bindings.claims {
        let path = root.join(&claim.claim);
        let text = fs::read_to_string(&path)?;
        let artifacts: Vec<_> = artifact_pattern
            .captures_iter(&text)
            .map(|c| c[1].to_owned())
            .collect();
        ensure!(!artifacts.is_empty(), "The Rust claim inventory is empty.");
        for artifact in artifacts {
            ensure!(
                inputs.contains_key(&artifact) || checker_sources.contains_key(&artifact),
                "Untested source in claim inventory: {artifact}"
            );
        }
        claims.insert(claim.claim.clone(), digest(&path)?);
    }
    let suites = read_suites(&fs::read_to_string(rust_log)?)?;
    let checked = check_tests(root, &bindings, &suites)?;
    let evidence_inputs: BTreeMap<_, _> = [isolated_manifest, rust_log, model_report, &inventory]
        .into_iter()
        .map(|path| Ok((path.to_string_lossy().into_owned(), digest(path)?)))
        .collect::<Result<_>>()?;
    write_json(
        output,
        &json!({
            "schema_version": 1, "passed": true, "machine_checked_refinement": false,
            "properties": checked, "suites": suites, "isolated_sources_checked": inputs.len(),
            "inputs": evidence_inputs, "checker_sources": checker_sources,
            "checker_executable_sha256": digest(&std::env::current_exe()?)?,
            "claims": claims, "acceptance": "pending"
        }),
    )?;
    println!(
        "PASS: {} property bindings and {} isolated build inputs",
        checked.len(),
        inputs.len()
    );
    Ok(())
}
