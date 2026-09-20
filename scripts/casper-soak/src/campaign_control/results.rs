use super::*;

pub fn validate_worker(config: &Config, slot: &Value, worker: &Value, now: u64) -> Result<()> {
    keys(worker, &[
        "schema_version",
        "repository",
        "run_id",
        "run_attempt",
        "reservation_id",
        "config_digest",
        "plan_sha256",
        "instance_id",
        "runner_label",
        "admission",
        "host_protection",
        "evidence_kind",
        "node_launch_count",
        "started_epoch",
        "finished_epoch",
        "workload_elapsed_seconds",
        "integration_preflight",
        "measurement_completeness",
        "profile_verdicts",
        "product_failures",
        "result",
    ])?;
    ensure!(
        worker["schema_version"] == 1
            && worker["repository"] == REPOSITORY
            && worker["run_id"] == slot["run_id"]
            && worker["run_attempt"] == 1
            && worker["reservation_id"] == slot["reservation_id"]
            && worker["config_digest"] == config.digest
            && worker["plan_sha256"] == hash(&encoded(&slot["plan"])?)
            && worker["instance_id"] == slot["instance_id"]
            && worker["runner_label"] == format!("casper-{}", text(&slot["reservation_id"])?),
        "The worker result has a different execution binding."
    );
    ensure!(
        encoded(worker)?.len() <= 16384,
        "The worker result exceeds its bound."
    );
    let failures = array(&worker["product_failures"])?;
    ensure!(
        failures.len() <= 64
            && failures
                .iter()
                .all(|v| v.as_str().is_some_and(|s| !s.is_empty() && s.len() <= 128)),
        "The product failure record is invalid."
    );
    ensure!(
        matches!(text(&worker["result"])?, "passed" | "failed" | "incomplete"),
        "The worker result is invalid."
    );
    let start = number(&worker["started_epoch"])?;
    let finish = number(&worker["finished_epoch"])?;
    let elapsed = number(&worker["workload_elapsed_seconds"])?;
    ensure!(
        start >= number(&slot["reserved_epoch"])?
            && finish >= start
            && finish <= now
            && elapsed <= finish - start
            && finish + 600 <= number(&slot["termination_epoch"])?,
        "The workload timestamps exceed the reservation."
    );
    if worker["result"] == "passed" {
        ensure!(
            worker["admission"] == "admitted"
                && worker["host_protection"] == "passed"
                && worker["evidence_kind"] == "node_observation"
                && number(&worker["node_launch_count"])? > 0
                && worker["integration_preflight"] == "passed"
                && failures.is_empty()
                && worker["measurement_completeness"] == "complete"
                && worker["profile_verdicts"]["authority_finality"] == "passed"
                && worker["profile_verdicts"]["publication"] == "passed"
                && elapsed >= number(&slot["plan"]["duration_seconds"])?,
            "The worker result does not prove the complete workload."
        );
    }
    Ok(())
}

pub fn authenticate_worker<P: Provider>(
    provider: &mut P,
    config: &Config,
    slot: &Value,
    artifact: &Value,
) -> Result<()> {
    let run = identifier(&slot["run_id"])?;
    let base = format!("repos/{REPOSITORY}/actions");
    let execution = provider.github(&format!("{base}/runs/{run}"))?;
    ensure!(
        execution["run_attempt"] == 1
            && execution["head_sha"] == config.value["control_revision"]
            && execution["repository"]["full_name"] == REPOSITORY
            && execution["event"] == "workflow_dispatch"
            && execution["path"] == ".github/workflows/merge-recovery-soak.yml",
        "The worker run differs from the approved source."
    );
    let observed = provider.github(&format!("{base}/artifacts/{}", number(&artifact["id"])?))?;
    ensure!(
        observed == *artifact
            && artifact["expired"] == false
            && artifact["name"] == format!("casper-campaign-worker-{run}-1")
            && artifact["workflow_run"]["id"]
                .as_u64()
                .map(|v| v.to_string())
                .as_deref()
                == Some(run)
            && artifact["workflow_run"]["head_sha"] == config.value["control_revision"]
            && number(&artifact["size_in_bytes"])? <= 1048576,
        "The worker artifact has a different execution binding."
    );
    digest(
        &json!(text(&artifact["digest"])?
            .strip_prefix("sha256:")
            .unwrap_or("")),
        64,
    )?;
    let jobs = provider.github(&format!("{base}/runs/{run}/attempts/1/jobs?per_page=100"))?;
    ensure!(
        number(&jobs["total_count"])? == array(&jobs["jobs"])?.len() as u64
            && number(&jobs["total_count"])? <= 100,
        "The job inventory is incomplete."
    );
    let matching: Vec<_> = array(&jobs["jobs"])?
        .iter()
        .filter(|j| j["name"] == "Campaign Workload")
        .collect();
    ensure!(
        matching.len() == 1
            && matching[0]["runner_id"] == slot["runner_id"]
            && matching[0]["runner_name"] == format!("casper-{}", text(&slot["reservation_id"])?)
            && matching[0]["status"] == "completed"
            && array(&matching[0]["labels"])?
                .contains(&json!(format!("casper-{}", text(&slot["reservation_id"])?))),
        "The result did not come from the reserved workload runner."
    );
    Ok(())
}

pub fn finish<P: Provider>(
    provider: &mut P,
    config: &Config,
    request: &[u8],
    plan: &Value,
    run: &str,
    worker: Option<(&Value, &Value)>,
) -> Result<Value> {
    let name = validate_plan(config, request, plan)?;
    let (mut state, etag) = load(provider, config)?;
    let slot = &state["slots"][&name];
    ensure!(
        slot["run_id"] == run && slot["request_sha256"] == hash(request) && slot["plan"] == *plan,
        "The finalizer has a different reservation."
    );
    if slot["result"] == "pending" {
        let accepted = worker
            .map(|(result, artifact)| -> Result<()> {
                authenticate_worker(provider, config, slot, artifact)?;
                validate_worker(config, slot, result, provider.now())
            })
            .transpose();
        let slot = &mut state["slots"][&name];
        match (accepted, worker) {
            (Ok(Some(())), Some((result, artifact))) => {
                slot["worker_result"] = result.clone();
                slot["worker_artifact"] = artifact.clone();
                slot["result"] = result["result"].clone();
                slot["product_failures"]
                    .as_array_mut()
                    .ok_or_else(|| eyre!("The failure record is malformed."))?
                    .extend(array(&result["product_failures"])?.iter().cloned());
                if result["result"] != "passed" {
                    slot["infrastructure_failures"]
                        .as_array_mut()
                        .unwrap()
                        .push(json!("workload_non_passing"));
                }
            }
            _ => {
                slot["result"] = json!("incomplete");
                slot["infrastructure_failures"]
                    .as_array_mut()
                    .ok_or_else(|| eyre!("The failure record is malformed."))?
                    .push(json!("worker_evidence_missing_or_invalid"));
            }
        }
    }
    state["slots"][&name]["cleanup_requested"] = json!(true);
    save(provider, config, &mut state, &etag)?;
    if state["slots"][&name]["termination_confirmed"] != true {
        let function = operations::verify_service(provider, config)?;
        let path = format!(
            "{}/20181201/functions/{}/actions/invoke",
            text(&function["invokeEndpoint"])?.trim_end_matches('/'),
            identifier(&config.value["supervisor"]["function_id"])?
        );
        let _ = provider.oci("invoke-detached", "POST", &path, Some(&json!({})), None);
        let until = provider.now() + 600;
        loop {
            (state, _) = load(provider, config)?;
            if state["slots"][&name]["termination_confirmed"] == true
                || provider.now() + 47 >= until
            {
                break;
            }
            provider.pause(10);
        }
    }
    let slot = &state["slots"][&name];
    let result = final_verdict(slot)?;
    let worker = &slot["worker_result"];
    Ok(
        json!({"schema_version":1,"repository":REPOSITORY,"run_id":run,"run_attempt":1,
        "reservation_id":slot["reservation_id"],"plan":plan,"config_digest":config.digest,
        "result":result,"admission":worker["admission"],"evidence_kind":worker["evidence_kind"],
        "cloud_launch_count":if slot["instance_id"].is_null(){0}else{1},
        "node_launch_count":worker["node_launch_count"],"cleanup":{"complete":slot["termination_confirmed"]==true},
        "host_protection":{"status":worker["host_protection"]},"integration_preflight":worker["integration_preflight"],
        "soak_verdict":result,"workload_elapsed_seconds":worker["workload_elapsed_seconds"],
        "measurement_completeness":worker["measurement_completeness"],"profile_verdicts":worker["profile_verdicts"],
        "product_failures":slot["product_failures"],"infrastructure_failures":slot["infrastructure_failures"]}),
    )
}
