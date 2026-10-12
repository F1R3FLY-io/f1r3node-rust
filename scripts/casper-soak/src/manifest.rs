use std::collections::BTreeSet;

use crate::*;

pub const FIELDS: &[&str] = &[
    "schema_version",
    "run_id",
    "phase",
    "candidate_id",
    "node_revision",
    "node_binary_digest",
    "image_digest",
    "harness_revision",
    "external_harness_revision",
    "source_digests",
    "configuration_digest",
    "profile_id",
    "profile_digest",
    "fixture_digest",
    "expectation_digest",
    "seed",
    "provider",
    "policy_variant",
    "evidence_kind",
    "capabilities",
    "tool_versions",
    "bounds",
    "assumptions",
    "resource_limits",
    "required_scenarios",
    "deadline",
    "merge_gate",
];
pub fn hex(value: &Value, width: usize) -> Result<()> {
    let value = text(value)?;
    ensure!(
        value.len() == width
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "The hexadecimal identity is invalid."
    );
    Ok(())
}
pub fn decimal(value: &Value) -> Result<u64> {
    let s = text(value)?;
    ensure!(
        !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && (s == "0" || !s.starts_with('0')),
        "The decimal identity is invalid."
    );
    Ok(s.parse()?)
}
pub fn validate(m: &Value) -> Result<()> {
    ensure!(
        FIELDS.iter().all(|key| m.get(*key).is_some()),
        "The manifest lacks required fields."
    );
    ensure!(
        number(&m["schema_version"])? == 1,
        "The manifest schema is unsupported."
    );
    for key in ["run_id", "candidate_id", "profile_id", "policy_variant"] {
        ensure!(
            !text(&m[key])?.trim().is_empty(),
            "A manifest identity is empty."
        );
    }
    for key in [
        "node_revision",
        "harness_revision",
        "external_harness_revision",
    ] {
        hex(&m[key], 40)?;
    }
    for key in [
        "node_binary_digest",
        "configuration_digest",
        "profile_digest",
        "fixture_digest",
        "expectation_digest",
    ] {
        hex(&m[key], 64)?;
    }
    ensure!(
        ["pre_pr216_merge", "post_pr216_merge"].contains(&text(&m["phase"])?),
        "The phase is invalid."
    );
    ensure!(
        ["docker", "subprocess"].contains(&text(&m["provider"])?),
        "The provider is invalid."
    );
    ensure!(
        ["synthetic_fixture", "node_observation"].contains(&text(&m["evidence_kind"])?),
        "The evidence kind is invalid."
    );
    if m["provider"] == "docker" {
        hex(&m["image_digest"], 64)?;
    } else {
        ensure!(
            m["image_digest"].is_null() && !text(&m["image_digest_reason"])?.trim().is_empty(),
            "The image absence needs a reason."
        );
    }
    decimal(&m["seed"])?;
    for key in [
        "source_digests",
        "capabilities",
        "tool_versions",
        "bounds",
        "resource_limits",
        "deadline",
    ] {
        ensure!(
            !object(&m[key])?.is_empty(),
            "A required manifest object is empty."
        );
    }
    for (name, digest) in object(&m["source_digests"])? {
        relative(Path::new("."), name)?;
        hex(digest, 64)?;
    }
    for key in ["assumptions", "required_scenarios"] {
        for value in array(&m[key])? {
            ensure!(
                !text(value)?.trim().is_empty(),
                "A manifest list entry is empty."
            );
        }
    }
    let scenarios = array(&m["required_scenarios"])?;
    let names: BTreeSet<_> = scenarios.iter().map(text).collect::<Result<_>>()?;
    ensure!(
        !names.is_empty() && names.len() == scenarios.len(),
        "Scenarios must be nonempty and unique."
    );
    if m["phase"] == "pre_pr216_merge" {
        ensure!(
            m["merge_gate"].is_null(),
            "The pre-merge gate must be null."
        );
    } else {
        object(&m["merge_gate"])?;
    }
    Ok(())
}
pub fn bind(input: &Path, output: &Path) -> Result<String> {
    let bytes = regular(input, MAX_BYTES)?;
    validate(&parse(&bytes)?)?;
    ensure!(
        !output.is_symlink(),
        "The run directory is a symbolic link."
    );
    let seal = output.join(".casper-manifest.json");
    let digest = hash(&bytes);
    if seal.try_exists()? || seal.is_symlink() {
        ensure!(
            regular(&seal, MAX_BYTES)? == bytes,
            "The retained manifest differs."
        );
        let state = output.join(".soak-state");
        let checkpoint = output.join(".soak-checkpoint-state.json");
        let has_state = state.try_exists()? || state.is_symlink();
        let has_checkpoint = checkpoint.try_exists()? || checkpoint.is_symlink();
        ensure!(
            has_state == has_checkpoint,
            "The checkpoint pair is incomplete."
        );
        if has_checkpoint {
            ensure!(
                record(&checkpoint)?["manifest_digest"] == digest,
                "The checkpoint identity differs."
            );
        } else {
            for entry in fs::read_dir(output)? {
                ensure!(
                    entry?.file_name() == ".casper-manifest.json",
                    "An incomplete run contains artifacts."
                );
            }
        }
    } else {
        ensure!(
            !output.exists() || fs::read_dir(output)?.next().is_none(),
            "An existing run cannot acquire an identity."
        );
        fs::create_dir_all(output)?;
        if let Err(error) = exclusive(&seal, &bytes, true) {
            ensure!(
                seal.is_file() && regular(&seal, MAX_BYTES)? == bytes,
                "Manifest publication failed: {}",
                error
            );
        }
    }
    Ok(digest)
}
