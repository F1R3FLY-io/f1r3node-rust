use casper::rust::genesis::contracts::fs_genesis::bulkio_versioned_uri;
use casper::rust::genesis::contracts::standard_deploys::FS_GENERATOR_PUB_KEY;
use rholang::rust::interpreter::io::bulk::{parse_hex_root, validate_namespace};

use crate::error::BulkError;
use crate::manifest::ImportManifest;
use crate::stage::StageReport;

pub fn default_uri() -> String { bulkio_versioned_uri(&FS_GENERATOR_PUB_KEY) }

fn rho_string(s: &str) -> Result<String, BulkError> {
    if s.chars().any(char::is_control) {
        return Err(BulkError::Manifest(
            "strings in deploys must not contain control characters".into(),
        ));
    }
    Ok(format!(
        "\"{}\"",
        s.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}

fn hex_arg(s: &str, what: &str) -> Result<String, BulkError> {
    parse_hex_root(s).ok_or_else(|| {
        BulkError::Manifest(format!("{what} must be 64 lowercase hex characters"))
    })?;
    rho_string(s)
}

fn wrap(uri: &str, label: &str, call: &str, needs_deployer: bool) -> Result<String, BulkError> {
    let deployer = if needs_deployer {
        ",\n  deployerId(`rho:system:deployerId`)"
    } else {
        ""
    };
    Ok(format!(
        r#"new
  v1Api(`rho:registry:v1:internal`),
  stdout(`rho:io:stdout`),
  bulkCh{deployer}
in {{
  v1Api!("lookupVersion", {uri}, Nil, *bulkCh) |
  for (@bulkIo <- bulkCh) {{
    for (@reply <- @bulkIo!?({call})) {{
      stdout!(({label}, reply))
    }}
  }}
}}
"#,
        uri = rho_string(uri)?,
        label = rho_string(label)?,
    ))
}

pub fn stage(
    uri: &str,
    manifest: &ImportManifest,
    report: &StageReport,
    expires_in: i64,
) -> Result<String, BulkError> {
    if expires_in < 1 {
        return Err(BulkError::Manifest("expiresIn must be positive".into()));
    }
    let sources = manifest
        .source
        .locations
        .iter()
        .map(|l| rho_string(l))
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");
    let spec = format!(
        "{{\"stageId\": {}, \"namespace\": {}, \"sourceRoot\": {}, \"manifestHash\": {}, \"transform\": {}, \"sources\": [{}], \"expiresIn\": {}}}",
        hex_arg(&report.stage_id, "stageId")?,
        rho_string(&manifest.namespace)?,
        hex_arg(&report.source_root, "sourceRoot")?,
        hex_arg(&report.manifest_hash, "manifestHash")?,
        rho_string(&manifest.transform)?,
        sources,
        expires_in
    );
    wrap(
        uri,
        "bulk-io stage",
        &format!("\"stage\", *deployerId, {spec}"),
        true,
    )
}

pub fn attest(uri: &str, stage_id: &str, result_root: &str) -> Result<String, BulkError> {
    wrap(
        uri,
        "bulk-io attest",
        &format!(
            "\"attest\", *deployerId, {}, {}",
            hex_arg(stage_id, "stageId")?,
            hex_arg(result_root, "resultRoot")?
        ),
        true,
    )
}

pub fn commit(uri: &str, stage_id: &str) -> Result<String, BulkError> {
    wrap(
        uri,
        "bulk-io commit",
        &format!("\"commit\", *deployerId, {}", hex_arg(stage_id, "stageId")?),
        true,
    )
}

pub fn abort(uri: &str, stage_id: &str) -> Result<String, BulkError> {
    wrap(
        uri,
        "bulk-io abort",
        &format!("\"abort\", *deployerId, {}", hex_arg(stage_id, "stageId")?),
        true,
    )
}

pub fn expire(uri: &str, stage_id: &str) -> Result<String, BulkError> {
    wrap(
        uri,
        "bulk-io expire",
        &format!("\"expire\", {}", hex_arg(stage_id, "stageId")?),
        false,
    )
}

pub fn status(uri: &str, stage_id: &str) -> Result<String, BulkError> {
    wrap(
        uri,
        "bulk-io status",
        &format!("\"status\", {}", hex_arg(stage_id, "stageId")?),
        false,
    )
}

pub fn root(uri: &str, namespace: &str) -> Result<String, BulkError> {
    validate_namespace(namespace).map_err(|e| BulkError::Manifest(e.to_string()))?;
    wrap(
        uri,
        "bulk-io root",
        &format!("\"root\", {}", rho_string(namespace)?),
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_uses_generator_key() {
        let u = default_uri();
        assert!(u.starts_with("rho:serve:1.0.0:") && u.ends_with(":bulkio:1.0.0"));
    }

    #[test]
    fn deploy_terms_validate_arguments() {
        let sid = "ab".repeat(32);
        let t = attest("rho:serve:1.0.0:00:bulkio:1.0.0", &sid, &"cd".repeat(32)).unwrap();
        assert!(t.contains("\"attest\", *deployerId"));
        assert!(t.contains("deployerId(`rho:system:deployerId`)"));
        assert!(attest("u", "nothex", &sid).is_err());
        let s = status("u", &sid).unwrap();
        assert!(!s.contains("deployerId"));
        assert!(root("u", "../x").is_err());
    }

    #[test]
    fn strings_are_escaped() {
        assert_eq!(rho_string("a\"b\\c").unwrap(), "\"a\\\"b\\\\c\"");
        assert!(rho_string("a\nb").is_err());
    }
}
