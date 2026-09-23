use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use eyre::{ensure, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::{check_hashes, digest, tool_sources, write_json, AREA};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub(crate) struct Case {
    pub module: String,
    pub configuration: String,
    pub expected_exit: i32,
    pub invariant: Option<String>,
}

#[derive(Clone, Deserialize)]
pub(crate) struct Plan {
    pub models: Vec<Case>,
}

#[derive(Deserialize, Serialize)]
pub(crate) struct ModelResult {
    #[serde(flatten)]
    pub case: Case,
    pub exit: i32,
    pub passed: bool,
    pub log: String,
    pub log_sha256: String,
}

#[derive(Deserialize, Serialize)]
pub(crate) struct Report {
    pub schema_version: u32,
    pub scope: String,
    pub passed: bool,
    pub models: Vec<ModelResult>,
    pub jar_sha256: String,
    pub sources: BTreeMap<String, String>,
    pub checker_sources: BTreeMap<String, String>,
    pub checker_executable_sha256: String,
    pub acceptance: String,
}

pub(crate) fn classify(code: i32, text: &str, case: &Case) -> bool {
    let errors: Vec<_> = text
        .lines()
        .filter(|line| line.starts_with("Error:"))
        .collect();
    if case.expected_exit == 0 {
        return code == 0
            && errors.is_empty()
            && text.contains("Model checking completed. No error has been found.");
    }
    let Some(invariant) = &case.invariant else {
        return false;
    };
    code == 12
        && errors
            == [
                format!("Error: Invariant {invariant} is violated."),
                "Error: The behavior up to this point is:".to_owned(),
            ]
        && text.contains("State 1:")
        && text.contains("State 2:")
}

pub(crate) fn validate_plan(area: &Path, plan: &Plan) -> Result<()> {
    let modules: BTreeSet<_> = plan.models.iter().map(|case| case.module.clone()).collect();
    ensure!(
        !modules.is_empty() && modules.len() == plan.models.len(),
        "The model inventory is empty or duplicated."
    );
    let mut configs = BTreeSet::new();
    for entry in fs::read_dir(area)? {
        let path = entry?.path();
        if path.extension().is_some_and(|ext| ext == "cfg") {
            if let Some(stem) = path.file_stem().and_then(|name| name.to_str()) {
                if stem.starts_with("MC_") {
                    configs.insert(stem.to_owned());
                }
            }
        }
    }
    ensure!(
        modules == configs,
        "The model inventory differs from the configurations."
    );
    let names = Regex::new(r"^MC_[A-Za-z_]+$")?;
    let invariants = Regex::new(r"^[A-Za-z]+$")?;
    let mut positives = BTreeSet::new();
    for case in &plan.models {
        ensure!(
            names.is_match(&case.module)
                && case.configuration == format!("{}.cfg", case.module)
                && area.join(format!("{}.tla", case.module)).is_file(),
            "Invalid or missing model: {}",
            case.module
        );
        if case.expected_exit == 0 && case.invariant.is_none() {
            positives.insert(case.module.as_str());
        } else {
            ensure!(
                case.expected_exit == 12
                    && case
                        .invariant
                        .as_deref()
                        .is_some_and(|name| invariants.is_match(name)),
                "Invalid expected outcome: {}",
                case.module
            );
        }
    }
    ensure!(
        positives == BTreeSet::from(["MC_ObserverSession", "MC_BoundedCapture"]),
        "Both positive models are required."
    );
    Ok(())
}

pub(crate) fn run_process(command: &mut Command, log: &Path, timeout: Duration) -> Result<i32> {
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(log)?;
    let mut child = command
        .stdout(Stdio::from(file.try_clone()?))
        .stderr(Stdio::from(file))
        .spawn()?;
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status.code().unwrap_or(-1));
        }
        if start.elapsed() >= timeout {
            child.kill()?;
            child.wait()?;
            return Ok(124);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

pub(crate) fn verify_report(root: &Path, path: &Path) -> Result<Report> {
    let report: Report = serde_json::from_slice(&fs::read(path)?)?;
    ensure!(
        report.schema_version == 1 && report.passed,
        "The model run failed."
    );
    check_hashes(root, &report.sources)?;
    ensure!(
        report.checker_sources == tool_sources(root)?,
        "The model checker inputs differ."
    );
    ensure!(
        report.checker_executable_sha256 == digest(&std::env::current_exe()?)?,
        "The model checker executable differs."
    );
    let area = root.join(AREA);
    let plan: Plan = serde_json::from_slice(&fs::read(area.join("verification-plan.json"))?)?;
    validate_plan(&area, &plan)?;
    ensure!(
        report.models.len() == plan.models.len(),
        "Model results are missing."
    );
    let parent = path.canonicalize()?.parent().unwrap().to_owned();
    for (case, result) in plan.models.iter().zip(&report.models) {
        ensure!(
            case == &result.case && result.passed,
            "A model result differs."
        );
        ensure!(
            result.log == format!("{}.log", case.module),
            "The model log name differs."
        );
        let log = parent.join(&result.log);
        ensure!(
            digest(&log)? == result.log_sha256,
            "The model log digest differs."
        );
        ensure!(
            classify(result.exit, &fs::read_to_string(log)?, case),
            "A model check failed."
        );
    }
    Ok(report)
}

pub(crate) fn run(root: &Path, jar: &Path, java: &Path, output: &Path) -> Result<()> {
    let area = root.join(AREA);
    let plan: Plan = serde_json::from_slice(&fs::read(area.join("verification-plan.json"))?)?;
    validate_plan(&area, &plan)?;
    let jar = jar.canonicalize()?;
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::create_dir(output)?;
    let output = output.canonicalize()?;
    let mut sources = BTreeMap::new();
    for entry in fs::read_dir(&area)? {
        let path = entry?.path();
        if path.is_file() {
            sources.insert(
                path.strip_prefix(root)?.to_string_lossy().into_owned(),
                digest(&path)?,
            );
        }
    }
    let checker_sources = tool_sources(root)?;
    let mut results = Vec::new();
    for case in plan.models {
        let log_name = format!("{}.log", case.module);
        let log = output.join(&log_name);
        let mut command = Command::new(java);
        command
            .current_dir(&area)
            .args(["-XX:+UseParallelGC", "-Xmx1g", "-cp"])
            .arg(&jar)
            .args(["tlc2.TLC", "-workers", "2", "-seed", "1", "-config"])
            .arg(&case.configuration)
            .arg("-metadir")
            .arg(output.join(format!("{}-states", case.module)))
            .arg(&case.module);
        let exit = run_process(&mut command, &log, Duration::from_secs(120))?;
        let passed = classify(exit, &fs::read_to_string(&log)?, &case);
        println!("{} {}", case.module, if passed { "PASS" } else { "FAIL" });
        results.push(ModelResult {
            case,
            exit,
            passed,
            log: log_name,
            log_sha256: digest(&log)?,
        });
    }
    check_hashes(root, &sources)?;
    ensure!(
        checker_sources == tool_sources(root)?,
        "The checker changed during verification."
    );
    let report = Report {
        schema_version: 1,
        scope: "bounded-model-checking".into(),
        passed: results.iter().all(|result| result.passed),
        models: results,
        jar_sha256: digest(&jar)?,
        sources,
        checker_sources,
        checker_executable_sha256: digest(&std::env::current_exe()?)?,
        acceptance: "pending".into(),
    };
    write_json(&output.join("report.json"), &report)?;
    ensure!(
        report.passed,
        "A model check failed. Retain the report and logs."
    );
    Ok(())
}
