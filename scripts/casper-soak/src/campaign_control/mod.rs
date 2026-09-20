use std::collections::BTreeSet;
use std::path::Path;

use casper_soak::{array, encoded, file_hash, hash, number, object, parse, record, relative, text};
use eyre::{ensure, eyre, Result};
use serde_json::{json, Value};

pub mod operations;
pub mod transport;

pub const REPOSITORY: &str = "F1R3FLY-io/f1r3node-rust";
pub const SLOTS: [&str; 3] = ["preflight", "baseline-dev-amd64", "baseline-dev-arm64"];
pub const SOURCE_PATHS: [&str; 8] = [
    "scripts/casper-soak/src/campaign_control/mod.rs",
    "scripts/casper-soak/src/campaign_control/transport.rs",
    "scripts/casper-soak/src/campaign_control/operations.rs",
    "scripts/casper-soak/src/bin/casper-campaign-control.rs",
    "scripts/casper-soak/src/bin/casper-campaign-supervisor.rs",
    "scripts/casper-soak/campaign-control.sh",
    "scripts/casper-soak/campaign-host.sh",
    "scripts/casper-soak/src/lib.rs",
];

#[derive(Clone, Debug)]
pub struct Reply {
    pub status: u16,
    pub data: Value,
    pub etag: Option<String>,
    pub next_page: Option<String>,
}

pub trait Provider {
    fn github(&mut self, path: &str) -> Result<Value>;
    fn oci(
        &mut self,
        service: &str,
        method: &str,
        path: &str,
        body: Option<&Value>,
        etag: Option<&str>,
    ) -> Result<Reply>;
    fn now(&self) -> u64;
    fn pause(&mut self, seconds: u64);
}

pub fn digest(value: &Value, length: usize) -> Result<&str> {
    let value = text(value)?;
    ensure!(
        value.len() == length
            && value
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)),
        "The digest is invalid."
    );
    Ok(value)
}

fn identifier(value: &Value) -> Result<&str> {
    let value = text(value)?;
    ensure!(
        !value.is_empty()
            && value.len() <= 255
            && value
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c)),
        "The identifier is invalid."
    );
    Ok(value)
}

pub fn keys(value: &Value, expected: &[&str]) -> Result<()> {
    let fields = object(value)?;
    ensure!(
        fields.len() == expected.len() && expected.iter().all(|key| fields.contains_key(*key)),
        "The record fields differ from the schema."
    );
    Ok(())
}

pub fn stamp(epoch: u64) -> Result<String> {
    ensure!(
        epoch > 0 && epoch <= 4102444800,
        "The clock is outside its supported range."
    );
    let time: libc::time_t = epoch.try_into()?;
    let mut tm = std::mem::MaybeUninit::<libc::tm>::uninit();
    ensure!(
        !unsafe { libc::gmtime_r(&time, tm.as_mut_ptr()) }.is_null(),
        "The clock cannot be formatted."
    );
    let tm = unsafe { tm.assume_init() };
    Ok(format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    ))
}

pub fn epoch(value: &Value) -> Result<u64> {
    let value = text(value)?;
    ensure!(
        value.len() >= 20 && value.is_ascii(),
        "The provider timestamp is invalid."
    );
    let bytes = value.as_bytes();
    ensure!(
        [4, 7, 10, 13, 16]
            .iter()
            .zip(b"--T::")
            .all(|(i, c)| bytes[*i] == *c),
        "The timestamp separators are invalid."
    );
    let suffix = &value[19..];
    ensure!(
        suffix == "Z"
            || suffix == "+00:00"
            || (suffix.starts_with('.')
                && (suffix.ends_with('Z') || suffix.ends_with("+00:00"))
                && suffix
                    .trim_start_matches('.')
                    .trim_end_matches('Z')
                    .trim_end_matches("+00:00")
                    .bytes()
                    .all(|c| c.is_ascii_digit())),
        "The timestamp is not UTC."
    );
    let read = |start, end| -> Result<i32> {
        Ok(value
            .get(start..end)
            .ok_or_else(|| eyre!("The timestamp is truncated."))?
            .parse()?)
    };
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    tm.tm_year = read(0, 4)? - 1900;
    tm.tm_mon = read(5, 7)? - 1;
    tm.tm_mday = read(8, 10)?;
    tm.tm_hour = read(11, 13)?;
    tm.tm_min = read(14, 16)?;
    tm.tm_sec = read(17, 19)?;
    let seconds: u64 = unsafe { libc::timegm(&mut tm) }.try_into()?;
    ensure!(
        stamp(seconds)?[..19] == value[..19],
        "The timestamp has invalid date fields."
    );
    Ok(seconds)
}

#[derive(Clone)]
pub struct Config {
    pub value: Value,
    pub digest: String,
}

impl Config {
    pub fn new(value: Value) -> Result<Self> {
        keys(&value, &[
            "schema_version",
            "repository",
            "control_revision",
            "campaign_id",
            "identity_digest",
            "approval_digest",
            "preflight_candidate_id",
            "environment",
            "storage",
            "compute",
            "supervisor",
            "source_digests",
            "activation",
        ])?;
        ensure!(
            number(&value["schema_version"])? == 1 && value["repository"] == REPOSITORY,
            "The controller repository or version is unsupported."
        );
        digest(&value["control_revision"], 40)?;
        digest(&value["identity_digest"], 64)?;
        digest(&value["approval_digest"], 64)?;
        ensure!(
            identifier(&value["campaign_id"])?.starts_with("task-017-12-"),
            "The campaign identifier is unsupported."
        );
        ensure!(
            matches!(
                text(&value["preflight_candidate_id"])?,
                "dev-amd64" | "dev-arm64"
            ),
            "The preflight candidate is unsupported."
        );
        keys(&value["environment"], &["id", "name", "branch"])?;
        ensure!(
            number(&value["environment"]["id"])? > 0
                && value["environment"]["name"] == "casper-campaign",
            "The dedicated approval environment is missing."
        );
        let branch = text(&value["environment"]["branch"])?;
        ensure!(
            !branch.is_empty()
                && branch.len() <= 128
                && !branch.contains(['*', '?', '[', ']', '\n', '\r']),
            "The deployment branch must be exact."
        );
        keys(&value["storage"], &[
            "region",
            "namespace",
            "bucket",
            "object",
            "policy_id",
            "policy_sha256",
        ])?;
        for field in ["region", "namespace", "bucket", "object", "policy_id"] {
            identifier(&value["storage"][field])?;
        }
        digest(&value["storage"]["policy_sha256"], 64)?;
        keys(&value["compute"], &["compartment_id", "candidates"])?;
        identifier(&value["compute"]["compartment_id"])?;
        keys(&value["compute"]["candidates"], &["dev-amd64", "dev-arm64"])?;
        for candidate in ["dev-amd64", "dev-arm64"] {
            let c = &value["compute"]["candidates"][candidate];
            keys(c, &[
                "boot_image_id",
                "shape",
                "ocpus",
                "subnet_id",
                "availability_domain",
                "image_digest",
                "image_config_digest",
                "node_binary_digest",
                "bootstrap_path",
                "bootstrap_sha256",
            ])?;
            for field in ["boot_image_id", "shape", "subnet_id"] {
                identifier(&c[field])?;
            }
            ensure!(
                number(&c["ocpus"])? > 0
                    && number(&c["ocpus"])? <= 32
                    && !text(&c["availability_domain"])?.is_empty(),
                "The machine shape is invalid."
            );
            for field in ["image_digest", "image_config_digest", "node_binary_digest"] {
                digest(
                    &json!(text(&c[field])?.strip_prefix("sha256:").unwrap_or("")),
                    64,
                )?;
            }
            digest(&c["bootstrap_sha256"], 64)?;
        }
        let supervisor = &value["supervisor"];
        keys(supervisor, &[
            "application_id",
            "function_id",
            "image_digest",
            "timing",
            "verification_sha256",
        ])?;
        identifier(&supervisor["application_id"])?;
        identifier(&supervisor["function_id"])?;
        digest(
            &json!(text(&supervisor["image_digest"])?
                .strip_prefix("sha256:")
                .unwrap_or("")),
            64,
        )?;
        digest(&supervisor["verification_sha256"], 64)?;
        keys(&supervisor["timing"], &[
            "schedule_seconds",
            "invocation_seconds",
            "api_seconds",
            "termination_seconds",
            "clock_seconds",
            "submission_seconds",
        ])?;
        let mut allowance = 0;
        for (_, seconds) in object(&supervisor["timing"])? {
            let seconds = number(seconds)?;
            ensure!(
                seconds > 0 && seconds <= 600,
                "A supervisor timing bound is missing or unsupported."
            );
            allowance += seconds;
        }
        ensure!(
            allowance <= 600,
            "The supervisor allowance exceeds the cleanup reserve."
        );
        object(&value["source_digests"])?;
        for path in SOURCE_PATHS {
            digest(&value["source_digests"][path], 64)?;
        }
        keys(&value["activation"], &["path", "sha256"])?;
        digest(&value["activation"]["sha256"], 64)?;
        let digest = hash(&encoded(&value)?);
        Ok(Self { value, digest })
    }

    pub fn verify_sources(&self, root: &Path) -> Result<()> {
        for (path, expected) in object(&self.value["source_digests"])? {
            ensure!(
                file_hash(&relative(root, path)?)? == digest(expected, 64)?,
                "A controller source differs from its pin."
            );
        }
        let reference = &self.value["activation"];
        let path = relative(root, text(&reference["path"])?)?;
        ensure!(
            file_hash(&path)? == digest(&reference["sha256"], 64)?,
            "The activation evidence differs from its pin."
        );
        let activation = record(&path)?;
        ensure!(
            activation["status"] == "accepted"
                && activation["control_revision"] == self.value["control_revision"]
                && activation["source_digests"] == self.value["source_digests"]
                && activation["supervisor_verification_sha256"]
                    == self.value["supervisor"]["verification_sha256"]
                && activation["evidence_kind"] == "deployed_service_verification",
            "Source-bound deployed-service acceptance is missing."
        );
        for claim in [
            "casper-soak-harness",
            "casper-soak-campaign",
            "casper-campaign-reservation",
            "casper-campaign-execution",
        ] {
            let bytes = casper_soak::regular(
                &root.join(format!("docs/claims/{claim}.md")),
                casper_soak::MAX_BYTES,
            )?;
            let text = std::str::from_utf8(&bytes)?;
            let header = text
                .split_once("```yaml\n")
                .and_then(|(_, tail)| tail.split_once("```"))
                .ok_or_else(|| eyre!("The required claim metadata is missing."))?
                .0;
            ensure!(
                header
                    .lines()
                    .filter(|line| *line == "status: discharged")
                    .count()
                    == 1
                    && !header.lines().any(|line| line == "status: pending"),
                "A required campaign claim remains pending."
            );
        }
        Ok(())
    }

    pub fn storage_path(&self) -> Result<String> {
        let s = &self.value["storage"];
        Ok(format!(
            "/n/{}/b/{}/o/{}",
            identifier(&s["namespace"])?,
            identifier(&s["bucket"])?,
            identifier(&s["object"])?
        ))
    }

    pub fn allowance(&self) -> Result<u64> {
        object(&self.value["supervisor"]["timing"])?
            .values()
            .try_fold(0, |sum, v| Ok(sum + number(v)?))
    }
}

pub fn approval_comment(
    config: &Config,
    request: &[u8],
    plan: &Value,
    run: &str,
) -> Result<String> {
    ensure!(
        !run.starts_with('0')
            && !run.is_empty()
            && run.len() <= 20
            && run.bytes().all(|c| c.is_ascii_digit()),
        "The run identifier is invalid."
    );
    Ok(format!(
        "casper-campaign-v1 request={} plan={} config={} control={} run={}/1",
        hash(request),
        hash(&encoded(plan)?),
        config.digest,
        text(&config.value["control_revision"])?,
        run
    ))
}

pub fn authenticate<P: Provider>(
    provider: &mut P,
    config: &Config,
    request: &[u8],
    plan: &Value,
    run: &str,
) -> Result<Value> {
    let expected = approval_comment(config, request, plan, run)?;
    let base = format!("repos/{REPOSITORY}");
    let execution = provider.github(&format!("{base}/actions/runs/{run}"))?;
    ensure!(
        execution["id"].as_u64().map(|id| id.to_string()).as_deref() == Some(run)
            && execution["run_attempt"] == 1
            && execution["event"] == "workflow_dispatch"
            && execution["path"] == ".github/workflows/merge-recovery-soak.yml"
            && execution["head_sha"] == config.value["control_revision"]
            && execution["head_branch"] == config.value["environment"]["branch"]
            && execution["repository"]["full_name"] == REPOSITORY
            && execution["status"] == "in_progress",
        "The workflow identity is not an active first-attempt campaign."
    );
    let env = provider.github(&format!("{base}/environments/casper-campaign"))?;
    ensure!(
        env["id"] == config.value["environment"]["id"]
            && env["name"] == "casper-campaign"
            && env["can_admins_bypass"] == false
            && env["deployment_branch_policy"]["custom_branch_policies"] == true
            && env["deployment_branch_policy"]["protected_branches"] == false,
        "The environment permits an unsupported approval path."
    );
    let rules: Vec<_> = array(&env["protection_rules"])?
        .iter()
        .filter(|r| r["type"] == "required_reviewers")
        .collect();
    ensure!(
        rules.len() == 1
            && rules[0]["prevent_self_review"] == true
            && !array(&rules[0]["reviewers"])?.is_empty(),
        "Independent required reviewers are missing."
    );
    let branches = provider.github(&format!(
        "{base}/environments/casper-campaign/deployment-branch-policies?per_page=100"
    ))?;
    ensure!(
        branches["total_count"] == 1
            && array(&branches["branch_policies"])?.len() == 1
            && branches["branch_policies"][0]["name"] == config.value["environment"]["branch"]
            && branches["branch_policies"][0]["type"] == "branch",
        "The environment permits an unexpected deployment reference."
    );
    let history = provider.github(&format!("{base}/actions/runs/{run}/approvals"))?;
    let reviews = array(&history)?;
    ensure!(
        reviews.len() <= 100,
        "The approval history exceeds its bound."
    );
    let matching: Vec<_> = reviews
        .iter()
        .filter(|review| {
            review["environments"].as_array().is_some_and(|envs| {
                envs.iter()
                    .any(|e| e["id"] == config.value["environment"]["id"])
            })
        })
        .collect();
    ensure!(
        matching.len() == 1
            && matching[0]["state"] == "approved"
            && matching[0]["comment"] == expected,
        "The exact request has no unambiguous approval."
    );
    let reviewer = &matching[0]["user"];
    let reviewer_id = number(&reviewer["id"])?;
    ensure!(
        reviewer_id != number(&execution["actor"]["id"])?
            && reviewer_id != number(&execution["triggering_actor"]["id"])?,
        "A workflow actor cannot approve this campaign."
    );
    let login = identifier(&reviewer["login"])?;
    let role = provider.github(&format!("{base}/collaborators/{login}/permission"))?;
    ensure!(
        role["user"]["id"] == reviewer_id
            && role["user"]["login"] == login
            && matches!(text(&role["role_name"])?, "maintain" | "admin"),
        "The reviewer is not a current repository maintainer."
    );
    let mut authorized = false;
    for configured in array(&rules[0]["reviewers"])? {
        match text(&configured["type"])? {
            "User" => authorized |= configured["reviewer"]["id"] == reviewer_id,
            "Team" => {
                let team = &configured["reviewer"];
                let slug = identifier(&team["slug"])?;
                let team_role =
                    provider.github(&format!("orgs/F1R3FLY-io/teams/{slug}/repos/{REPOSITORY}"))?;
                ensure!(
                    matches!(text(&team_role["role_name"])?, "maintain" | "admin"),
                    "The reviewer team has no verified maintainer role."
                );
                let membership = provider
                    .github(&format!("orgs/F1R3FLY-io/teams/{slug}/memberships/{login}"))?;
                authorized |= membership["state"] == "active";
            }
            _ => return Err(eyre!("The reviewer type is unsupported.")),
        }
    }
    ensure!(
        authorized,
        "The maintainer is not a required reviewer for this environment."
    );
    Ok(
        json!({"reviewer_id":reviewer_id,"reviewer_login":login,"role_name":role["role_name"],"binding":expected,"environment_id":env["id"]}),
    )
}

pub fn validate_plan(config: &Config, request: &[u8], plan: &Value) -> Result<String> {
    let request = parse(request)?;
    ensure!(
        request["campaign_id"] == config.value["campaign_id"]
            && plan["campaign_id"] == request["campaign_id"]
            && plan["stage"] == request["stage"]
            && plan["candidate_id"] == request["candidate_id"]
            && plan["identity_digest"] == config.value["identity_digest"]
            && plan["approval_digest"] == config.value["approval_digest"]
            && plan["harness_revision"] == config.value["control_revision"]
            && plan["memory_gb"] == 64
            && plan["cleanup_reserve_seconds"] == 600
            && plan["max_launches"] == 1,
        "The plan differs from the campaign binding."
    );
    let candidate = text(&plan["candidate_id"])?;
    ensure!(
        matches!(candidate, "dev-amd64" | "dev-arm64"),
        "The candidate is unsupported."
    );
    let selected = &config.value["compute"]["candidates"][candidate];
    for key in ["image_digest", "image_config_digest", "node_binary_digest"] {
        ensure!(
            plan[key] == selected[key],
            "The candidate differs from its immutable pin."
        );
    }
    let expected_platform = if candidate == "dev-amd64" {
        "linux/amd64"
    } else {
        "linux/arm64"
    };
    ensure!(
        plan["platform"] == expected_platform
            && plan["image_reference"]
                == format!(
                    "docker.io/f1r3flyindustries/f1r3fly-rust@{}",
                    text(&selected["image_digest"])?
                ),
        "The container platform or image reference differs."
    );
    match text(&plan["stage"])? {
        "preflight" => {
            ensure!(
                plan["candidate_id"] == config.value["preflight_candidate_id"]
                    && plan["duration_seconds"] == 0
                    && plan["runner_max_seconds"] == 14400
                    && request["preflight_run_id"].is_null(),
                "The preflight resource tuple is invalid."
            );
            Ok("preflight".to_owned())
        }
        "baseline" => {
            ensure!(
                plan["duration_seconds"] == 86400 && plan["runner_max_seconds"] == 93600,
                "The full baseline resource tuple is required."
            );
            Ok(format!("baseline-{candidate}"))
        }
        _ => Err(eyre!("Stability and replacement launches are not enabled.")),
    }
}

pub fn validate_state(config: &Config, state: &Value) -> Result<()> {
    keys(state, &[
        "schema_version",
        "config_digest",
        "campaign_id",
        "sequence",
        "slots",
    ])?;
    ensure!(
        state["schema_version"] == 1
            && state["config_digest"] == config.digest
            && state["campaign_id"] == config.value["campaign_id"]
            && number(&state["sequence"])? <= 100000,
        "The authoritative record has a different binding."
    );
    keys(&state["slots"], &SLOTS)?;
    let mut runs = BTreeSet::new();
    for name in SLOTS {
        let slot = &state["slots"][name];
        if slot.is_null() {
            continue;
        }
        ensure!(
            runs.insert(text(&slot["run_id"])?)
                && slot["slot"] == name
                && slot["config_digest"] == config.digest
                && slot["run_attempt"] == 1,
            "The reservation record is inconsistent."
        );
        ensure!(
            matches!(
                text(&slot["state"])?,
                "reserved" | "armed" | "submitting" | "launched" | "terminated"
            ),
            "The reservation state is unsupported."
        );
        digest(&slot["request_sha256"], 64)?;
        digest(&slot["reservation_id"], 64)?;
        array(&slot["product_failures"])?;
        array(&slot["infrastructure_failures"])?;
        let reserved = number(&slot["reserved_epoch"])?;
        let lifetime = number(&slot["plan"]["runner_max_seconds"])?;
        ensure!(
            lifetime == if name == "preflight" { 14400 } else { 93600 }
                && number(&slot["deadline_epoch"])? == reserved + lifetime
                && number(&slot["termination_epoch"])? + config.allowance()? == reserved + lifetime,
            "The stored deadline differs from the fixed budget."
        );
        ensure!(
            slot["state"] != "terminated"
                || (slot["termination_confirmed"] == true && !slot["instance_id"].is_null()),
            "Cleanup has no observed instance termination."
        );
    }
    ensure!(
        state["slots"]["preflight"].is_object()
            || SLOTS[1..].iter().all(|s| state["slots"][s].is_null()),
        "A baseline has no preflight reservation."
    );
    Ok(())
}

pub fn load<P: Provider>(provider: &mut P, config: &Config) -> Result<(Value, String)> {
    let reply = provider.oci("object", "GET", &config.storage_path()?, None, None)?;
    ensure!(
        reply.status == 200,
        "The authoritative reservation object is unavailable."
    );
    validate_state(config, &reply.data)?;
    let etag = reply
        .etag
        .ok_or_else(|| eyre!("The reservation object has no entity tag."))?;
    ensure!(
        !etag.is_empty() && etag.len() < 256 && !etag.contains(['\r', '\n']),
        "The entity tag is invalid."
    );
    Ok((reply.data, etag))
}

pub fn save<P: Provider>(
    provider: &mut P,
    config: &Config,
    state: &mut Value,
    etag: &str,
) -> Result<String> {
    state["sequence"] = json!(number(&state["sequence"])? + 1);
    validate_state(config, state)?;
    let reply = provider.oci(
        "object",
        "PUT",
        &config.storage_path()?,
        Some(state),
        Some(etag),
    )?;
    ensure!(
        reply.status == 200,
        "The conditional reservation write failed or is ambiguous. The slot must not be retried."
    );
    let next = reply.etag.ok_or_else(|| {
        eyre!("The write acknowledgment has no entity tag. The slot must not be retried.")
    })?;
    let (observed, observed_etag) = load(provider, config)?;
    ensure!(
        observed == *state && observed_etag == next,
        "The reservation write cannot be confirmed. The slot must not be retried."
    );
    Ok(next)
}

pub fn reserve<P: Provider>(
    provider: &mut P,
    config: &Config,
    request: &[u8],
    plan: &Value,
    run: &str,
    approval: &Value,
) -> Result<(Value, String, String)> {
    let name = validate_plan(config, request, plan)?;
    let (mut state, etag) = load(provider, config)?;
    ensure!(
        state["slots"][&name].is_null() && SLOTS.iter().all(|s| state["slots"][s]["run_id"] != run),
        "The slot or run identifier is already consumed."
    );
    if name != "preflight" {
        let previous = &state["slots"]["preflight"];
        let request = parse(request)?;
        ensure!(
            previous["state"] == "terminated"
                && previous["result"] == "passed"
                && previous["run_id"] == request["preflight_run_id"]
                && array(&previous["product_failures"])?.is_empty()
                && array(&previous["infrastructure_failures"])?.is_empty(),
            "A passing and terminated preflight is required."
        );
    }
    let now = provider.now();
    stamp(now)?;
    let deadline = now + number(&plan["runner_max_seconds"])?;
    let reservation_id = hash(approval_comment(config, request, plan, run)?.as_bytes());
    state["slots"][&name] = json!({"slot":name,"config_digest":config.digest,"run_id":run,"run_attempt":1,"request_sha256":hash(request),"reservation_id":reservation_id,"approval":approval,"plan":plan,"state":"reserved","reserved_epoch":now,"deadline_epoch":deadline,"termination_epoch":deadline-config.allowance()?,"schedule_id":null,"supervisor_ack":null,"instance_id":null,"termination_confirmed":false,"result":"pending","product_failures":[],"infrastructure_failures":[]});
    let next = save(provider, config, &mut state, &etag)?;
    Ok((state, next, name))
}

pub fn schedule_request(config: &Config, slot: &Value) -> Result<Value> {
    Ok(
        json!({"compartmentId":config.value["compute"]["compartment_id"],"displayName":format!("casper-deadline-{}", &text(&slot["reservation_id"])?[..32]),"action":"START_RESOURCE","recurrenceType":"ICAL","recurrenceDetails":"FREQ=DAILY;COUNT=1","timeStarts":stamp(number(&slot["termination_epoch"] )?)?,"resources":[{"id":config.value["supervisor"]["function_id"],"metadata":{"resourceType":"FunctionsFunction"},"parameters":[]}],"freeformTags":{"casper-campaign":config.value["campaign_id"],"casper-reservation":slot["reservation_id"]}}),
    )
}

pub fn verify_schedule(config: &Config, slot: &Value, schedule: &Value) -> Result<()> {
    let expected = schedule_request(config, slot)?;
    ensure!(
        schedule["lifecycleState"] == "ACTIVE"
            && schedule["action"] == expected["action"]
            && schedule["compartmentId"] == expected["compartmentId"]
            && schedule["recurrenceType"] == expected["recurrenceType"]
            && schedule["recurrenceDetails"] == expected["recurrenceDetails"]
            && epoch(&schedule["timeStarts"])? == number(&slot["termination_epoch"])?
            && schedule["resources"] == expected["resources"]
            && schedule["resourceFilters"]
                .as_array()
                .is_none_or(|v| v.is_empty())
            && schedule["timeEnds"].is_null()
            && schedule["freeformTags"] == expected["freeformTags"],
        "The deadline schedule is not active with the exact function and time."
    );
    Ok(())
}

pub fn verify_instance(config: &Config, slot: &Value, instance: &Value) -> Result<()> {
    let selected = &config.value["compute"]["candidates"][text(&slot["plan"]["candidate_id"])?];
    ensure!(
        instance["compartmentId"] == config.value["compute"]["compartment_id"]
            && instance["imageId"] == selected["boot_image_id"]
            && instance["shape"] == selected["shape"]
            && instance["shapeConfig"]["memoryInGBs"].as_f64() == Some(64.0)
            && instance["shapeConfig"]["ocpus"].as_f64()
                == Some(number(&selected["ocpus"])? as f64)
            && instance["freeformTags"]["casper-reservation"] == slot["reservation_id"]
            && instance["freeformTags"]["casper-campaign"] == config.value["campaign_id"]
            && instance["freeformTags"]["casper-config"] == config.digest
            && instance["freeformTags"]["casper-run"] == slot["run_id"],
        "The observed instance differs from the reserved identity."
    );
    ensure!(
        epoch(&instance["timeCreated"])? >= number(&slot["reserved_epoch"])?
            && epoch(&instance["timeCreated"])? < number(&slot["termination_epoch"])?,
        "The instance creation time is outside the reserved interval."
    );
    identifier(&instance["id"])?;
    Ok(())
}

pub fn host_admission(config: &Config, slot: &Value, observed: &Value, now: u64) -> Result<Value> {
    let plan = &slot["plan"];
    ensure!(
        slot["state"] == "launched"
            && observed["instance_id"] == slot["instance_id"]
            && observed["reservation_id"] == slot["reservation_id"]
            && observed["platform"] == plan["platform"]
            && observed["image_digest"] == plan["image_digest"]
            && observed["image_config_digest"] == plan["image_config_digest"]
            && observed["node_binary_digest"] == plan["node_binary_digest"]
            && observed["runner_label"] == format!("casper-{}", text(&slot["reservation_id"])?)
            && observed["runner_exclusive"] == true,
        "The host or executable identity differs from its reservation."
    );
    ensure!(
        number(&observed["memory_total_mib"])? >= 60416
            && observed["memory_max_bytes"] == 45056_u64 * 1024 * 1024
            && observed["memory_swap_max_bytes"] == 0
            && number(&observed["memory_available_mib"])? >= 8192
            && number(&observed["disk_free_mib"])? >= 8192
            && observed["disk_floor_mib"] == 4096
            && observed["disk_guardian_active"] == true
            && observed["memory_guardian_active"] == true
            && observed["metadata_access_blocked"] == true,
        "The required memory, disk, or credential protection is missing."
    );
    let duration = number(&plan["duration_seconds"])?;
    ensure!(
        now >= number(&slot["reserved_epoch"])?
            && now + duration + 600 <= number(&slot["termination_epoch"])?
            && config.allowance()? <= 600,
        "The complete workload and cleanup do not fit before independent termination."
    );
    Ok(
        json!({"admission":"admitted","workload_start_epoch":now,"workload_deadline_epoch":now+duration,"duration_seconds":duration,"reservation_id":slot["reservation_id"]}),
    )
}

pub fn final_verdict(slot: &Value) -> Result<&'static str> {
    if !array(&slot["product_failures"])?.is_empty() {
        return Ok("product_failure");
    }
    if !array(&slot["infrastructure_failures"])?.is_empty() || slot["termination_confirmed"] != true
    {
        return Ok("infrastructure_failure");
    }
    if slot["result"] == "passed" {
        Ok("passed")
    } else {
        Ok("incomplete")
    }
}
