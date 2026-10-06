use super::*;

fn success(reply: Reply, code: u16) -> Result<Value> {
    ensure!(reply.status == code, "The provider rejected the operation.");
    Ok(reply.data)
}

pub fn verify_service<P: Provider>(provider: &mut P, config: &Config) -> Result<Value> {
    let supervisor = &config.value["supervisor"];
    let function = success(
        provider.oci(
            "functions",
            "GET",
            &format!(
                "/20181201/functions/{}",
                identifier(&supervisor["function_id"])?
            ),
            None,
            None,
        )?,
        200,
    )?;
    ensure!(
        function["id"] == supervisor["function_id"]
            && function["applicationId"] == supervisor["application_id"]
            && function["imageDigest"] == supervisor["image_digest"]
            && function["lifecycleState"] == "ACTIVE"
            && number(&function["detachedModeTimeoutInSeconds"])?
                >= SLOTS.len() as u64 * number(&supervisor["timing"]["termination_seconds"])? + 120
            && function["config"]["CASPER_CAMPAIGN_CONFIG_SHA256"] == config.digest,
        "The deployed supervisor identity or timeout is not verified."
    );
    let policy = success(
        provider.oci(
            "identity",
            "GET",
            &format!(
                "/20160918/policies/{}",
                identifier(&config.value["storage"]["policy_id"])?
            ),
            None,
            None,
        )?,
        200,
    )?;
    ensure!(
        policy["lifecycleState"] == "ACTIVE"
            && hash(&encoded(&policy["statements"])?)
                == text(&config.value["storage"]["policy_sha256"])?,
        "The deployed storage policy differs from its reviewed identity."
    );
    Ok(function)
}

pub fn arm<P: Provider>(
    provider: &mut P,
    config: &Config,
    name: &str,
    reservation: &str,
) -> Result<Value> {
    ensure!(SLOTS.contains(&name), "The supervisor slot is invalid.");
    let (mut state, etag) = load(provider, config)?;
    let slot = &state["slots"][name];
    ensure!(
        slot["state"] == "reserved"
            && slot["reservation_id"] == reservation
            && provider.now() + 600 < number(&slot["termination_epoch"])?,
        "The supervisor cannot arm this reservation."
    );
    let schedule = success(
        provider.oci(
            "scheduler",
            "GET",
            &format!("/20240430/schedules/{}", identifier(&slot["schedule_id"])?),
            None,
            None,
        )?,
        200,
    )?;
    verify_schedule(config, slot, &schedule)?;
    let ack = json!({"reservation_id":reservation,"config_digest":config.digest,
        "function_id":config.value["supervisor"]["function_id"],
        "image_digest":config.value["supervisor"]["image_digest"],"observed_epoch":provider.now()});
    state["slots"][name]["supervisor_ack"] = ack.clone();
    state["slots"][name]["state"] = json!("armed");
    save(provider, config, &mut state, &etag)?;
    Ok(ack)
}

pub fn base64(bytes: &[u8]) -> String {
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    for chunk in bytes.chunks(3) {
        let a = chunk[0] as usize;
        let b = *chunk.get(1).unwrap_or(&0) as usize;
        let c = *chunk.get(2).unwrap_or(&0) as usize;
        result.push(alphabet[a >> 2] as char);
        result.push(alphabet[((a & 3) << 4) | (b >> 4)] as char);
        result.push(if chunk.len() > 1 {
            alphabet[((b & 15) << 2) | (c >> 6)] as char
        } else {
            '='
        });
        result.push(if chunk.len() > 2 {
            alphabet[c & 63] as char
        } else {
            '='
        });
    }
    result
}

pub fn bootstrap<P: Provider>(
    provider: &mut P,
    config: &Config,
    slot: &Value,
    template: &str,
) -> Result<(String, Value)> {
    for token in ["__CASPER_RESERVATION__", "__CASPER_JIT__"] {
        ensure!(
            template.matches(token).count() == 1,
            "The bootstrap template binding is invalid."
        );
    }
    let label = format!("casper-{}", text(&slot["reservation_id"])?);
    let registration = provider.github_post(
        &format!("repos/{REPOSITORY}/actions/runners/generate-jitconfig"),
        &json!({"name":label,"runner_group_id":config.value["compute"]["runner_group_id"],
            "labels":["self-hosted",label],"work_folder":"_work"}),
    )?;
    ensure!(
        registration["runner"]["name"] == label && number(&registration["runner"]["id"])? > 0,
        "The just-in-time runner identity differs."
    );
    let custom: BTreeSet<_> = array(&registration["runner"]["labels"])?
        .iter()
        .map(|v| text(&v["name"]))
        .collect::<Result<_>>()?;
    ensure!(
        custom == BTreeSet::from(["self-hosted", label.as_str()]),
        "The runner labels are not exclusive."
    );
    let jit = text(&registration["encoded_jit_config"])?;
    ensure!(
        !jit.is_empty()
            && jit.len() <= 20000
            && jit
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"+/=".contains(&b)),
        "The runner configuration is invalid."
    );
    let script = template
        .replace("__CASPER_RESERVATION__", text(&slot["reservation_id"])?)
        .replace("__CASPER_JIT__", jit);
    Ok((
        base64(script.as_bytes()),
        registration["runner"]["id"].clone(),
    ))
}

pub fn launch_request(config: &Config, slot: &Value, bootstrap: &str) -> Result<Value> {
    let candidate = &config.value["compute"]["candidates"][text(&slot["plan"]["candidate_id"])?];
    ensure!(
        !bootstrap.is_empty()
            && bootstrap.len() <= 32768
            && bootstrap
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"+/=\n".contains(&b)),
        "The launch bootstrap is not bounded base64."
    );
    Ok(
        json!({"compartmentId":config.value["compute"]["compartment_id"],
        "availabilityDomain":candidate["availability_domain"],
        "displayName":format!("casper-{}", &text(&slot["reservation_id"])?[..32]),
        "shape":candidate["shape"],"shapeConfig":{"memoryInGBs":64,"ocpus":candidate["ocpus"]},
        "sourceDetails":{"sourceType":"image","imageId":candidate["boot_image_id"]},
        "createVnicDetails":{"subnetId":candidate["subnet_id"],"assignPublicIp":false},
        "instanceOptions":{"areLegacyImdsEndpointsDisabled":true},
        "metadata":{"user_data":bootstrap},
        "freeformTags":{"casper-campaign":config.value["campaign_id"],
            "casper-reservation":slot["reservation_id"],"casper-config":config.digest,
            "casper-run":slot["run_id"],
            "casper-runner-label":format!("casper-{}",text(&slot["reservation_id"])?)}}),
    )
}

fn owned(config: &Config, slot: &Value, instance: &Value) -> bool {
    instance["compartmentId"] == config.value["compute"]["compartment_id"]
        && instance["freeformTags"]["casper-reservation"] == slot["reservation_id"]
        && instance["freeformTags"]["casper-campaign"] == config.value["campaign_id"]
        && instance["freeformTags"]["casper-config"] == config.digest
        && instance["freeformTags"]["casper-run"] == slot["run_id"]
}

fn url_component(value: &str) -> String {
    value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

pub fn find_instances<P: Provider>(
    provider: &mut P,
    config: &Config,
    slot: &Value,
) -> Result<Vec<Value>> {
    let mut found = Vec::new();
    let mut page: Option<String> = None;
    let mut pages = BTreeSet::new();
    for _ in 0..20 {
        let mut path = format!(
            "/20160918/instances?compartmentId={}&limit=100",
            identifier(&config.value["compute"]["compartment_id"])?
        );
        if let Some(ref token) = page {
            path.push_str(&format!("&page={}", url_component(token)));
        }
        let reply = provider.oci("compute", "GET", &path, None, None)?;
        ensure!(
            reply.status == 200,
            "The instance inventory is unavailable."
        );
        for instance in array(&reply.data)? {
            if owned(config, slot, instance) {
                identifier(&instance["id"])?;
                ensure!(
                    !found
                        .iter()
                        .any(|item: &Value| item["id"] == instance["id"]),
                    "The instance inventory contains duplicates."
                );
                found.push(instance.clone());
            }
        }
        if let Some(token) = reply.next_page {
            ensure!(
                token.len() <= 2048 && pages.insert(token.clone()),
                "The instance inventory pagination is invalid."
            );
            page = Some(token);
        } else {
            return Ok(found);
        }
    }
    Err(eyre!("The instance inventory exceeds its page bound."))
}

pub fn dispatch<P: Provider>(
    provider: &mut P,
    config: &Config,
    request: &[u8],
    plan: &Value,
    run: &str,
    bootstrap: &str,
) -> Result<Value> {
    validate_plan(config, request, plan)?;
    let approval = authenticate(provider, config, request, plan, run)?;
    let function = verify_service(provider, config)?;
    let (mut state, mut etag, name) = reserve(provider, config, request, plan, run, &approval)?;
    let schedule_body = schedule_request(config, &state["slots"][&name])?;
    let schedule = success(
        provider.oci(
            "scheduler",
            "POST",
            "/20240430/schedules",
            Some(&schedule_body),
            None,
        )?,
        200,
    )?;
    let schedule_id = identifier(&schedule["id"])?.to_owned();
    let limit = provider.now() + 120;
    loop {
        let observed = success(
            provider.oci(
                "scheduler",
                "GET",
                &format!("/20240430/schedules/{schedule_id}"),
                None,
                None,
            )?,
            200,
        )?;
        if observed["lifecycleState"] == "ACTIVE" {
            verify_schedule(config, &state["slots"][&name], &observed)?;
            break;
        }
        ensure!(
            provider.now() < limit && observed["lifecycleState"] == "CREATING",
            "The deadline schedule did not become active."
        );
        provider.pause(5);
    }
    state["slots"][&name]["schedule_id"] = json!(schedule_id);
    save(provider, config, &mut state, &etag)?;
    let reservation = text(&state["slots"][&name]["reservation_id"])?.to_owned();
    let invoke = format!(
        "{}/20181201/functions/{}/actions/invoke",
        text(&function["invokeEndpoint"])?.trim_end_matches('/'),
        identifier(&config.value["supervisor"]["function_id"])?
    );
    let ack = success(
        provider.oci(
            "invoke",
            "POST",
            &invoke,
            Some(&json!({"action":"arm","slot":name,"reservation_id":reservation})),
            None,
        )?,
        200,
    )?;
    (state, etag) = load(provider, config)?;
    let slot = &state["slots"][&name];
    ensure!(
        slot["state"] == "armed"
            && slot["reservation_id"] == reservation
            && slot["supervisor_ack"] == ack
            && ack["config_digest"] == config.digest
            && ack["image_digest"] == config.value["supervisor"]["image_digest"]
            && provider.now() >= number(&ack["observed_epoch"])?
            && provider.now() - number(&ack["observed_epoch"])? <= 120,
        "The independent supervisor acknowledgment is missing or stale."
    );
    verify_service(provider, config)?;
    let (bootstrap, runner_id) = self::bootstrap(provider, config, slot, bootstrap)?;
    let launch = launch_request(config, slot, &bootstrap)?;
    ensure!(
        provider.now() + number(&plan["duration_seconds"])? + 1200
            < number(&slot["termination_epoch"])?,
        "The remaining lifetime cannot hold preparation, workload, and cleanup."
    );
    state["slots"][&name]["runner_id"] = runner_id;
    state["slots"][&name]["state"] = json!("submitting");
    state["slots"][&name]["launch_request_sha256"] = json!(hash(&encoded(&launch)?));
    state["slots"][&name]["submit_before_epoch"] = json!(
        provider.now() + number(&config.value["supervisor"]["timing"]["submission_seconds"])?
    );
    etag = save(provider, config, &mut state, &etag)?;
    ensure!(
        provider.now() < number(&state["slots"][&name]["submit_before_epoch"])?,
        "The launch submission window expired. The slot remains consumed."
    );
    let response = provider.oci(
        "compute",
        "POST",
        "/20160918/instances",
        Some(&launch),
        None,
    );
    let mut instance = None;
    match response {
        Ok(reply) if reply.status == 200 => instance = Some(reply.data),
        _ => state["slots"][&name]["infrastructure_failures"]
            .as_array_mut()
            .ok_or_else(|| eyre!("The failure record is malformed."))?
            .push(json!("launch_response_unknown")),
    }
    if instance.is_none() {
        let matches = find_instances(provider, config, &state["slots"][&name])?;
        ensure!(
            matches.len() <= 1,
            "Multiple instances match one launch reservation."
        );
        instance = matches.into_iter().next();
    }
    if let Some(instance) = instance {
        verify_instance(config, &state["slots"][&name], &instance)?;
        state["slots"][&name]["instance_id"] = instance["id"].clone();
        state["slots"][&name]["state"] = json!("launched");
        save(provider, config, &mut state, &etag)?;
    } else {
        save(provider, config, &mut state, &etag)?;
        return Err(eyre!(
            "The launch outcome is unknown. The deadline supervisor retains responsibility."
        ));
    }
    Ok(
        json!({"scope":"campaign-launch-control","reservation_id":reservation,
        "instance_id":state["slots"][&name]["instance_id"],"runner_label":format!("casper-{reservation}"),
        "launch_submissions":1,"workload_admitted":false,"termination_confirmed":false,
        "slot":name,"config_digest":config.digest,
        "snapshot":state,"snapshot_sha256":hash(&encoded(&state)?)}),
    )
}

pub fn supervise<P: Provider>(provider: &mut P, config: &Config) -> Result<Value> {
    let mut reports = Vec::new();
    for name in SLOTS {
        let (state, _) = load(provider, config)?;
        let slot = &state["slots"][name];
        if slot.is_null()
            || slot["state"] == "terminated"
            || (provider.now() < number(&slot["termination_epoch"])?
                && slot["cleanup_requested"] != true)
        {
            continue;
        }
        match terminate_slot(provider, config, name, slot) {
            Ok(report) => reports.push(report),
            Err(_) => {
                let failure = (|| -> Result<()> {
                    let (mut current, etag) = load(provider, config)?;
                    current["slots"][name]["infrastructure_failures"]
                        .as_array_mut()
                        .ok_or_else(|| eyre!("The failure history is malformed."))?
                        .push(json!("termination_unconfirmed"));
                    save(provider, config, &mut current, &etag)?;
                    Ok(())
                })();
                reports.push(json!({"slot":name,"termination_confirmed":false,
                    "failure_recorded":failure.is_ok()}));
            }
        }
    }
    let confirmed = reports.iter().all(|r| r["termination_confirmed"] == true);
    Ok(
        json!({"scope":"oci-lifetime-supervisor","config_digest":config.digest,
        "termination_confirmed":confirmed && !reports.is_empty(),"reports":reports}),
    )
}

fn terminate_slot<P: Provider>(
    provider: &mut P,
    config: &Config,
    name: &str,
    slot: &Value,
) -> Result<Value> {
    let mut instances = find_instances(provider, config, slot)?;
    if let Some(id) = slot["instance_id"].as_str() {
        if !instances.iter().any(|i| i["id"] == id) {
            let instance = success(
                provider.oci(
                    "compute",
                    "GET",
                    &format!("/20160918/instances/{}", identifier(&json!(id))?),
                    None,
                    None,
                )?,
                200,
            )?;
            ensure!(
                instance["id"] == id
                    && instance["compartmentId"] == config.value["compute"]["compartment_id"],
                "The known instance identity changed."
            );
            instances.push(instance);
        }
    }
    ensure!(
        !instances.is_empty() && instances.len() <= 3,
        "No complete bounded instance inventory is available."
    );
    let stop =
        provider.now() + number(&config.value["supervisor"]["timing"]["termination_seconds"])?;
    let mut confirmed = Vec::new();
    let mut infrastructure = Vec::new();
    if instances.len() > 1 {
        infrastructure.push(json!("multiple_instances_for_one_reservation"));
    }
    for instance in instances {
        let id = identifier(&instance["id"])?.to_owned();
        if verify_instance(config, slot, &instance).is_err() {
            infrastructure.push(json!("instance_identity_mismatch"));
        }
        let path = format!("/20160918/instances/{id}");
        let mut terminated = instance["lifecycleState"] == "TERMINATED";
        let mut submissions = 0;
        while !terminated && provider.now() + 47 < stop {
            let observed = success(provider.oci("compute", "GET", &path, None, None)?, 200)?;
            ensure!(
                observed["id"] == id
                    && observed["compartmentId"] == config.value["compute"]["compartment_id"],
                "The termination observation has the wrong identity."
            );
            if observed["lifecycleState"] == "TERMINATED" {
                terminated = true;
                break;
            }
            if observed["lifecycleState"] != "TERMINATING" && submissions < 3 {
                let _ = provider.oci(
                    "compute",
                    "DELETE",
                    &format!(
                        "{path}?preserveBootVolume=false&preserveDataVolumesCreatedAtLaunch=false"
                    ),
                    None,
                    None,
                );
                submissions += 1;
            }
            provider.pause(5);
        }
        ensure!(
            terminated,
            "The instance did not reach TERMINATED within the observation bound."
        );
        confirmed.push(json!(id));
    }
    let (mut current, etag) = load(provider, config)?;
    ensure!(
        current["slots"][name]["reservation_id"] == slot["reservation_id"],
        "The reservation changed during supervision."
    );
    if current["slots"][name]["instance_id"].is_null() {
        current["slots"][name]["instance_id"] = confirmed[0].clone();
    }
    current["slots"][name]["terminated_instance_ids"] = json!(confirmed);
    current["slots"][name]["termination_confirmed"] = json!(true);
    current["slots"][name]["termination_observed_epoch"] = json!(provider.now());
    current["slots"][name]["state"] = json!("terminated");
    if provider.now() > number(&slot["deadline_epoch"])? {
        infrastructure.push(json!("instance_lifetime_exceeded"));
    }
    current["slots"][name]["infrastructure_failures"]
        .as_array_mut()
        .ok_or_else(|| eyre!("The failure history is malformed."))?
        .extend(infrastructure);
    save(provider, config, &mut current, &etag)?;
    Ok(
        json!({"slot":name,"termination_confirmed":true,"observed_epoch":provider.now(),
        "verdict":final_verdict(&current["slots"][name])?}),
    )
}
