use std::collections::BTreeSet;
use std::process::{Command, Stdio};
use std::time::Instant;

use regex::Regex;
use serde_json::json;

use crate::*;

pub const CLEAN: &str = "Model checking completed. No error has been found.";
pub const PLAN: &str = "formal/tlaplus/casper_soak/verification-plan.jsonc";
pub fn classify(code: i32, output: &str, property: Option<&str>) -> bool {
    let lines: Vec<_> = output.lines().collect();
    let errors: Vec<_> = lines
        .iter()
        .filter(|line| line.starts_with("Error:"))
        .copied()
        .collect();
    let violations: Vec<_> = Regex::new(r"Invariant ([A-Za-z][A-Za-z0-9_]*) is violated\.")
        .expect("static regex")
        .captures_iter(output)
        .map(|capture| capture[1].to_string())
        .collect();
    let Some(property) = property else {
        return code == 0 && lines.contains(&CLEAN) && errors.is_empty() && violations.is_empty();
    };
    if code != 12 || violations != [property] || output.contains(CLEAN) {
        return false;
    }
    let expected = format!("Error: Invariant {property} is violated.");
    let header = "Error: The behavior up to this point is:";
    let headers: Vec<_> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| **line == header || **line == "The behavior up to this point is:")
        .map(|(i, _)| i)
        .collect();
    headers.len() == 1
        && lines.iter().filter(|line| **line == expected).count() == 1
        && errors
            .iter()
            .all(|line| *line == expected || *line == header)
        && lines[headers[0] + 1..]
            .iter()
            .find(|line| !line.trim().is_empty())
            .is_some_and(|line| line.starts_with("State 1:"))
}
pub fn controls(plan: &Value) -> Result<Vec<Value>> {
    let mut controls = vec![plan["positive_control"].clone()];
    controls.extend(array(&plan["negative_controls"])?.iter().cloned());
    let mut names = BTreeSet::new();
    let mut properties = BTreeSet::new();
    let mut knobs = BTreeSet::new();
    ensure!(
        controls[0]["expected_exit"] == 0,
        "The clean control has the wrong expected exit."
    );
    for (index, control) in controls.iter().enumerate() {
        let name = text(&control["configuration"])?;
        ensure!(
            name.ends_with(".cfg")
                && Path::new(name).components().count() == 1
                && names.insert(name),
            "The control registration is invalid."
        );
        relative(Path::new("."), name)?;
        if index > 0 {
            ensure!(
                control["expected_exit"] == 12
                    && properties.insert(text(&control["property"])?)
                    && knobs.insert(text(&control["knob"])?),
                "A negative registration is invalid."
            );
        }
    }
    let positive: BTreeSet<_> = array(&controls[0]["properties"])?
        .iter()
        .map(text)
        .collect::<Result<_>>()?;
    ensure!(
        positive == properties,
        "Positive and negative properties differ."
    );
    Ok(controls)
}
pub fn validate_configuration(control: &Value, body: &str, knobs: &[String]) -> Result<()> {
    let invariants: Vec<_> = Regex::new(r"(?m)^INVARIANT\s+(\w+)\s*$")?
        .captures_iter(body)
        .map(|c| c[1].to_string())
        .collect();
    let mut required = BTreeSet::from(["TypeOK".to_string()]);
    if let Some(values) = control["properties"].as_array() {
        for value in values {
            required.insert(text(value)?.to_string());
        }
    } else {
        required.insert(text(&control["property"])?.to_string());
    }
    ensure!(
        invariants.len() == required.len()
            && invariants.into_iter().collect::<BTreeSet<_>>() == required,
        "Configuration invariants differ."
    );
    for knob in knobs {
        let expression = Regex::new(&format!(
            r"(?m)^\s*{}\s*=\s*(TRUE|FALSE)\s*$",
            regex::escape(knob)
        ))?;
        let found: Vec<_> = expression
            .captures_iter(body)
            .map(|c| c[1].to_string())
            .collect();
        let expected = if control["knob"] == *knob {
            "TRUE"
        } else {
            "FALSE"
        };
        ensure!(found == [expected], "A configuration defect knob differs.");
    }
    Ok(())
}
pub fn run(root: &Path, output: &Path, java: &str, jar: &Path, cap: u64) -> Result<i32> {
    ensure!(
        cap > 0 && cap <= 120,
        "The verifier cap must be between 1 and 120 seconds."
    );
    fs::create_dir_all(
        output
            .parent()
            .ok_or_else(|| eyre!("The output has no parent."))?,
    )?;
    fs::create_dir(output)?;
    let plan_path = root.join(PLAN);
    let plan_digest = file_hash(&plan_path)?;
    let plan = record(&plan_path)?;
    let controls = controls(&plan)?;
    let knobs: Vec<_> = controls[1..]
        .iter()
        .map(|c| text(&c["knob"]).map(str::to_owned))
        .collect::<Result<_>>()?;
    let model = relative(root, text(&plan["model"])?)?;
    let constant_pattern = Regex::new(r"(?m)^\s*(\w+)\s*=\s*(.*?)\s*$")?;
    let state_count_pattern =
        Regex::new(r"([\d,]+) states generated, ([\d,]+) distinct states found")?;
    let mut results = Vec::new();
    for control in &controls {
        let name = text(&control["configuration"])?;
        let config = relative(
            plan_path
                .parent()
                .ok_or_else(|| eyre!("Missing plan directory."))?,
            name,
        )?;
        let log_name = format!("{}.log", name.trim_end_matches(".cfg"));
        let log_path = output.join(&log_name);
        let started = Instant::now();
        let mut result = json!({"configuration":name,"expected_property":control["property"],"expected_exit":control["expected_exit"],"exit":null,"outcome":"tool_error","log":log_name});
        let attempt = (|| -> Result<()> {
            let config_text = String::from_utf8(regular(&config, MAX_BYTES)?)?;
            validate_configuration(control, &config_text, &knobs)?;
            let constants: Vec<_> = constant_pattern
                .captures_iter(&config_text)
                .map(|c| vec![c[1].to_string(), c[2].to_string()])
                .collect();
            result["constant_assignments"] = json!(constants);
            let before = [file_hash(&model)?, file_hash(&config)?, file_hash(jar)?];
            result["model_sha256"] = before[0].clone().into();
            result["configuration_sha256"] = before[1].clone().into();
            result["tlc_sha256"] = before[2].clone().into();
            let metadata = tempfile::tempdir()?;
            let log = File::create(&log_path)?;
            let status = Command::new("timeout")
                .args([
                    "--signal=TERM",
                    "--kill-after=5",
                    &cap.to_string(),
                    java,
                    "-Xmx512m",
                    "-XX:+UseParallelGC",
                    "-cp",
                ])
                .arg(jar)
                .args(["tlc2.TLC", "-workers", "1", "-seed", "1", "-metadir"])
                .arg(metadata.path())
                .arg("-config")
                .arg(&config)
                .arg(&model)
                .stdout(Stdio::from(log.try_clone()?))
                .stderr(Stdio::from(log))
                .status()?;
            let code = status.code().unwrap_or(128);
            let log = String::from_utf8_lossy(&regular(&log_path, MAX_BYTES)?).into_owned();
            result["exit"] = code.into();
            result["outcome"] =
                if before != [file_hash(&model)?, file_hash(&config)?, file_hash(jar)?] {
                    "input_changed"
                } else if code == 124 {
                    "timeout"
                } else if classify(code, &log, control["property"].as_str()) {
                    "passed"
                } else {
                    "unexpected_result"
                }
                .into();
            if let Some(line) = log.lines().find(|line| line.starts_with("TLC2 Version ")) {
                result["tlc_version"] = line.into();
            }
            if let Some(counts) = state_count_pattern.captures(&log) {
                result["states_generated"] = counts[1].replace(',', "").parse::<u64>()?.into();
                result["distinct_states"] = counts[2].replace(',', "").parse::<u64>()?.into();
            }
            Ok(())
        })();
        if attempt.is_err() && !log_path.exists() {
            fs::write(&log_path, "The verifier setup failed.\n")?;
        }
        result["elapsed_seconds"] = json!(started.elapsed().as_secs_f64());
        result["log_sha256"] = file_hash(&log_path)?.into();
        results.push(result);
    }
    let passed =
        results.iter().all(|r| r["outcome"] == "passed") && file_hash(&plan_path)? == plan_digest;
    let report = json!({"scope":"bounded-harness-refutation-only","construction":"not-applicable","driver_binding":"pending","profile_verification":"pending","status":if passed {"passed"} else {"failed"},"seed":1,"workers":1,"heap_limit_mb":512,"timeout_seconds":cap,"kill_grace_seconds":5,"plan_sha256":plan_digest,"runner_sha256":file_hash(&root.join("scripts/casper-soak/src/models.rs"))?,"harness_binary_sha256":file_hash(&std::env::current_exe()?)?,"model":plan["model"],"results":results});
    exclusive(&output.join("report.json"), &encoded(&report)?, false)?;
    Ok(if passed { 0 } else { 1 })
}
