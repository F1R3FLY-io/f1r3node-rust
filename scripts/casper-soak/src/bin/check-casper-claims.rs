use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use casper_soak::{
    array, encoded, exclusive, file_hash, parse, record, regular, relative, text, MAX_BYTES,
};
use clap::Parser;
use eyre::{ensure, eyre, Result};
use serde_json::{json, Value};

#[derive(Parser)]
struct Args {
    #[arg(long, default_value = ".")]
    root: PathBuf,
    #[arg(long)]
    output: PathBuf,
    #[arg(long)]
    strict: bool,
    #[arg(long)]
    claim: Option<String>,
}

fn fenced(path: &Path, language: &str) -> Result<String> {
    let content = String::from_utf8(regular(path, MAX_BYTES)?)?;
    let marker = format!("```{language}\n");
    let mut sections = content.split(&marker);
    sections.next();
    let body = sections
        .next()
        .ok_or_else(|| eyre!("The metadata block is missing."))?;
    if language == "json" {
        ensure!(
            sections.next().is_none(),
            "The metadata block is ambiguous."
        );
    }
    Ok(body
        .split_once("```")
        .ok_or_else(|| eyre!("The metadata block is incomplete."))?
        .0
        .to_owned())
}

fn field<'a>(header: &'a str, key: &str) -> Result<&'a str> {
    let prefix = format!("{key}: ");
    let values: Vec<_> = header
        .lines()
        .filter_map(|line| line.strip_prefix(&prefix))
        .collect();
    ensure!(
        values.len() == 1,
        "A required metadata field is missing or duplicated."
    );
    Ok(values[0])
}

fn discharged(root: &Path, spec: &str, header: &str, id: &str) -> Result<()> {
    ensure!(
        field(header, "construction")? == "not-applicable"
            && field(header, "refutation")? == "bounded-safety-pass"
            && field(header, "binding")? == "passed",
        "The declared claim has incomplete verification tiers."
    );
    let body = header
        .split_once("artifacts:\n")
        .ok_or_else(|| eyre!("The artifact inventory is missing."))?
        .1;
    ensure!(
        header.lines().filter(|line| *line == "artifacts:").count() == 1,
        "The artifact inventory is duplicated."
    );
    let mut artifacts = Vec::new();
    for line in body.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Some(path) = line.strip_prefix("  - ") {
            artifacts.push(path);
        } else if !line.starts_with(char::is_whitespace) && line.contains(':') {
            break;
        } else {
            return Err(eyre!("The artifact inventory syntax is unsupported."));
        }
    }
    ensure!(!artifacts.is_empty(), "The artifact inventory is empty.");
    let mut seen = BTreeSet::new();
    for artifact in artifacts {
        ensure!(seen.insert(artifact), "An artifact is duplicated.");
        let source = relative(root, artifact)?;
        let slug = if artifact == "scripts/casper-soak/Cargo.toml" {
            "scripts-casper-soak-cargo-toml".to_owned()
        } else {
            artifact
                .trim_start_matches('.')
                .replace(['/', '.', '_'], "-")
        };
        let ledger_path = relative(root, &format!("docs/casper/cbc-evidence/{slug}.md"))?;
        let ledger = parse(fenced(&ledger_path, "json")?.as_bytes())?;
        ensure!(
            ledger["status"] == "discharged"
                && ledger["waiver"].is_null()
                && array(&ledger["claim_ids"])?.contains(&json!(id)),
            "The artifact has no discharge for this claim."
        );
        ensure!(
            ledger["artifact"]["path"] == artifact
                && ledger["artifact"]["sha256"] == file_hash(&source)?
                && ledger["claim_digests"][spec] == file_hash(&relative(root, spec)?)?,
            "The discharged source or specification differs."
        );
        ensure!(
            ledger["phase_status"]["pre_pr216_merge"] == "discharged",
            "The phase is not discharged."
        );
        let evidence_path = relative(root, text(&ledger["evidence"]["ref"])?)?;
        ensure!(
            ledger["evidence"]["sha256"] == file_hash(&evidence_path)?,
            "The discharge evidence differs."
        );
        casper_soak::manifest::hex(&ledger["artifact"]["commit"], 40)?;
        ensure!(
            !text(&ledger["verified_at"])?.is_empty(),
            "The verification timestamp is missing."
        );
        let evidence = record(&evidence_path)?;
        ensure!(
            evidence["source_digests"][artifact] == file_hash(&source)?
                && evidence["claim_digests"][spec] == file_hash(&relative(root, spec)?)?
                && evidence["tiers"]["refutation"] == "bounded-safety-pass"
                && evidence["tiers"]["binding"] == "passed"
                && evidence["tiers"]["construction"] == "not-applicable",
            "The evidence does not bind the current source, specification, and tiers."
        );
        ensure!(
            evidence["claim_discharge"] == "discharged"
                && evidence["phase"] == "pre_pr216_merge"
                && array(&evidence["claim_ids"])?.contains(&json!(id)),
            "The evidence does not discharge this claim and phase."
        );
    }
    Ok(())
}

fn audit(args: &Args) -> Result<i32> {
    let root = args.root.canonicalize()?;
    let plan = record(&root.join(casper_soak::models::PLAN))?;
    let mut claims = vec![json!({"id":plan["claim"],"specification":plan["specification"]})];
    claims.extend(
        array(&plan["profile_verification"]["claims"])?
            .iter()
            .cloned(),
    );
    let required: BTreeSet<_> = (1..=8)
        .map(|n| format!("CLAIM-CASPER-SOAK-{n:03}"))
        .collect();
    if let Some(id) = &args.claim {
        ensure!(
            required.contains(id),
            "The requested claim is not registered."
        );
    }
    let mut seen = BTreeSet::new();
    let mut results = Vec::new();
    for claim in claims {
        let id = text(&claim["id"])?;
        ensure!(
            required.contains(id) && seen.insert(id.to_owned()),
            "The claim registry is invalid."
        );
        let spec = text(&claim["specification"])?;
        let header = fenced(&relative(&root, spec)?, "yaml")?;
        ensure!(
            field(&header, "claim_id")? == id,
            "The specification identity differs."
        );
        let status = field(&header, "status")?;
        ensure!(
            ["pending", "refuted", "discharged"].contains(&status),
            "The claim status is unsupported."
        );
        if status == "discharged" {
            discharged(&root, spec, &header, id)?;
        }
        if args.claim.as_deref().is_none_or(|selected| selected == id) {
            results.push(json!({"claim_id":id,"status":status,"soak":field(&header,"soak")?,"specification":spec,"specification_sha256":file_hash(&relative(&root,spec)?)?}));
        }
    }
    ensure!(seen == required, "A required claim is missing.");
    let all_discharged = results.iter().all(|claim| claim["status"] == "discharged");
    let result: Value = json!({"schema_version":1,"scope":"claim-ledger-audit","phase":"pre_pr216_merge","claim_discharge":if all_discharged {"discharged"} else {"pending"},"claims":results,"proof_execution":false});
    exclusive(&args.output, &encoded(&result)?, false)?;
    Ok(if args.strict && !all_discharged { 4 } else { 0 })
}

fn main() {
    let args = Args::parse();
    match audit(&args) {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    }
}
