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
    #[serde(default)]
    pub yanked: BTreeMap<String, Yanked>,
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

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Yanked {
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

fn review_date(id: &str, review_by: &toml::value::Datetime) -> Result<NaiveDate> {
    let date = review_by
        .date
        .ok_or_else(|| eyre!("{id}: a review date is required."))?;
    ensure!(
        review_by.time.is_none() && review_by.offset.is_none(),
        "{id}: a date without a time is required."
    );
    NaiveDate::from_ymd_opt(date.year.into(), date.month.into(), date.day.into())
        .ok_or_else(|| eyre!("{id}: invalid review date."))
}

pub fn reviews_due(
    policy: &Policy,
    today: NaiveDate,
    days: i64,
) -> Result<Vec<(String, NaiveDate)>> {
    let mut due = Vec::new();
    let records = policy
        .exceptions
        .iter()
        .map(|(id, e)| (id, &e.review_by))
        .chain(policy.yanked.iter().map(|(spec, y)| (spec, &y.review_by)));
    for (id, review_by) in records {
        let deadline = review_date(id, review_by)?;
        if (deadline - today).num_days() <= days {
            due.push((id.clone(), deadline));
        }
    }
    due.sort_by_key(|(_, d)| *d);
    Ok(due)
}

fn exact_crate_spec(spec: &str) -> bool {
    spec.split_once('@').is_some_and(|(name, version)| {
        !name.is_empty()
            && !name.chars().any(char::is_whitespace)
            && semver::Version::parse(version).is_ok()
    })
}

pub fn validate(policy: &Policy, deny: &Value, today: NaiveDate) -> Result<()> {
    let ignored = required(deny, "advisories.ignore")?
        .as_array()
        .ok_or_else(|| eyre!("Invalid advisory exceptions."))?;
    let mut ids = BTreeSet::new();
    let mut yanked = BTreeSet::new();
    for entry in ignored {
        let (key, seen) = match (
            entry.get("id").and_then(Value::as_str),
            entry.get("crate").and_then(Value::as_str),
        ) {
            (Some(id), None) => (id, &mut ids),
            (None, Some(spec)) => {
                ensure!(
                    exact_crate_spec(spec),
                    "{spec}: a yanked-crate exception requires name@exact-version."
                );
                (spec, &mut yanked)
            }
            _ => eyre::bail!("Every advisory exception requires exactly one of an ID or a crate."),
        };
        ensure!(
            entry
                .get("reason")
                .and_then(Value::as_str)
                .is_some_and(|s| !s.trim().is_empty()),
            "{key}: a reason is required."
        );
        ensure!(
            seen.insert(key.to_string()),
            "Duplicate advisory exception: {key}"
        );
    }
    ensure!(
        ids == policy.exceptions.keys().cloned().collect(),
        "Advisory exceptions must match supply-chain/policy.toml exactly."
    );
    ensure!(
        yanked == policy.yanked.keys().cloned().collect(),
        "Yanked-crate exceptions must match supply-chain/policy.toml exactly."
    );
    for (spec, entry) in &policy.yanked {
        ensure!(
            exact_crate_spec(spec),
            "{spec}: a yanked-crate exception requires name@exact-version."
        );
        ensure!(
            !entry.owner.trim().is_empty(),
            "{spec}: an owner is required."
        );
        ensure!(
            review_date(spec, &entry.review_by)? > today,
            "{spec}: the exception requires review."
        );
    }
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
        ensure!(
            review_date(id, &entry.review_by)? > today,
            "{id}: the exception requires review."
        );
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
