use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use chrono::NaiveDate;
use eyre::{ensure, eyre, Result};
use serde::Deserialize;
use toml::Value;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub manifests: Vec<String>,
    pub scanner: Scanner,
    pub exceptions: BTreeMap<String, Exception>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scanner {
    pub version: String,
    pub sha256: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exception {
    pub package: String,
    pub versions: Vec<String>,
    pub owner: String,
    #[serde(rename = "review-by")]
    pub review_by: toml::value::Datetime,
}

pub fn load(path: &Path) -> Result<Policy> { Ok(toml::from_str(&std::fs::read_to_string(path)?)?) }

fn required<'a>(value: &'a Value, path: &str) -> Result<&'a Value> {
    path.split('.').try_fold(value, |value, key| {
        value
            .get(key)
            .ok_or_else(|| eyre!("Missing policy field: {path}"))
    })
}

pub fn validate(policy: &Policy, deny: &Value, today: NaiveDate) -> Result<()> {
    let ignored = required(deny, "advisories.ignore")?
        .as_array()
        .ok_or_else(|| eyre!("Invalid advisory exceptions."))?;
    let mut ids = BTreeSet::new();
    for entry in ignored {
        let id = entry
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| eyre!("Every advisory exception requires an ID."))?;
        ensure!(
            entry
                .get("reason")
                .and_then(Value::as_str)
                .is_some_and(|s| !s.trim().is_empty()),
            "{id}: a reason is required."
        );
        ensure!(
            ids.insert(id.to_string()),
            "Duplicate advisory exception: {id}"
        );
    }
    ensure!(
        ids == policy.exceptions.keys().cloned().collect(),
        "Advisory exceptions must match supply-chain/policy.toml exactly."
    );
    for (id, entry) in &policy.exceptions {
        ensure!(
            !entry.owner.trim().is_empty() && !entry.package.trim().is_empty(),
            "{id}: an owner and a package are required."
        );
        ensure!(
            !entry.versions.is_empty()
                && entry
                    .versions
                    .iter()
                    .all(|v| semver::Version::parse(v).is_ok()),
            "{id}: exact package versions are required."
        );
        let date = entry
            .review_by
            .date
            .ok_or_else(|| eyre!("{id}: a review date is required."))?;
        ensure!(
            entry.review_by.time.is_none() && entry.review_by.offset.is_none(),
            "{id}: a date without a time is required."
        );
        let deadline =
            NaiveDate::from_ymd_opt(date.year.into(), date.month.into(), date.day.into())
                .ok_or_else(|| eyre!("{id}: invalid review date."))?;
        ensure!(deadline > today, "{id}: the exception requires review.");
    }
    for (path, value) in [
        ("graph.all-features", Value::Boolean(true)),
        ("graph.no-default-features", Value::Boolean(false)),
        ("advisories.unsound", Value::String("all".into())),
        ("advisories.unmaintained", Value::String("all".into())),
        ("advisories.yanked", Value::String("deny".into())),
        (
            "advisories.maximum-db-staleness",
            Value::String("P7D".into()),
        ),
        ("bans.wildcards", Value::String("deny".into())),
        ("bans.allow-wildcard-paths", Value::Boolean(true)),
        ("sources.unknown-git", Value::String("deny".into())),
        ("sources.unknown-registry", Value::String("deny".into())),
        ("sources.required-git-spec", Value::String("rev".into())),
        ("bans.build.executables", Value::String("deny".into())),
        ("bans.build.include-dependencies", Value::Boolean(true)),
    ] {
        ensure!(
            required(deny, path)? == &value,
            "deny.toml requires {path} = {value:?}."
        );
    }
    ensure!(
        required(deny, "bans.build.allow-build-scripts")?
            .as_array()
            .is_some_and(|a| !a.is_empty()),
        "Build scripts require an explicit allowance list."
    );
    for key in ["exclude", "targets"] {
        if let Some(value) = required(deny, "graph")?.get(key) {
            ensure!(
                value.as_array().is_some_and(Vec::is_empty),
                "The supply-chain graph must not exclude packages or targets."
            );
        }
    }
    for (section, key) in [
        ("graph", "exclude-dev"),
        ("graph", "exclude-unpublished"),
        ("advisories", "disable-yank-checking"),
    ] {
        if let Some(value) = required(deny, section)?.get(key) {
            ensure!(
                value.as_bool() == Some(false),
                "deny.toml must not enable {section}.{key}."
            );
        }
    }
    if let Some(entries) = required(deny, "licenses")?.get("clarify") {
        for entry in entries
            .as_array()
            .ok_or_else(|| eyre!("Invalid license clarifications."))?
        {
            let files = entry
                .get("license-files")
                .and_then(Value::as_array)
                .ok_or_else(|| eyre!("License clarifications require file hashes."))?;
            ensure!(
                !files.is_empty()
                    && files.iter().all(|f| f
                        .get("hash")
                        .and_then(Value::as_integer)
                        .is_some_and(|h| h > 0)),
                "License clarifications require file hashes."
            );
        }
    }
    Ok(())
}
