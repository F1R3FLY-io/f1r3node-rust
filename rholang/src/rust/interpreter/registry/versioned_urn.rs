//! URN parsing for the versioned registry.
//!
//! Recognized shapes (per the Versioned Registry FIP §"Versioned code"):
//!
//! - `rho:lib:<service_ver>:<pub_key>:<project_id>:<project_ver>`
//! - `rho:serve:<service_ver>:<pub_key>:<project_id>:<project_ver>`
//! - `rho:registry:<registry_ver>`
//!
//! Version segments may contain `*` wildcards; this parser preserves
//! them verbatim. Semver semantics live in `semver.rs`.

/// A parsed versioned-registry URN.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedUrn {
    /// `"lib"`, `"serve"`, or `"registry"`.
    pub namespace: String,
    /// The service version (the version of the lib/serve/registry API).
    /// For `rho:registry:<ver>` this is the only version present.
    pub service_version: String,
    /// Deployer public key for `lib`/`serve`; absent for `registry`.
    pub pub_key: Option<String>,
    /// Project ID for `lib`/`serve`; absent for `registry`.
    pub project_id: Option<String>,
    /// Project version for `lib`/`serve`; absent for `registry`.
    pub project_version: Option<String>,
}

/// Parse a URN into its segments. Returns `None` on any structural
/// failure: wrong scheme, unknown namespace, wrong segment count, or an
/// empty segment.
///
/// Version segments may be exact (`1.2.3`), wildcarded (`*`, `1.*`,
/// `1.2.*`), or prereleases (`1.2.3-alpha`). The parser does not
/// validate them as semver — that is the resolver's job.
pub fn parse_urn(urn: &str) -> Option<ParsedUrn> {
    let body = urn.strip_prefix("rho:")?;
    let parts: Vec<&str> = body.split(':').collect();

    match parts.as_slice() {
        ["registry", ver] if !ver.is_empty() => Some(ParsedUrn {
            namespace: "registry".to_string(),
            service_version: (*ver).to_string(),
            pub_key: None,
            project_id: None,
            project_version: None,
        }),
        ["lib", svc_ver, pk, proj, proj_ver] | ["serve", svc_ver, pk, proj, proj_ver]
            if !svc_ver.is_empty()
                && !pk.is_empty()
                && !proj.is_empty()
                && !proj_ver.is_empty() =>
        {
            Some(ParsedUrn {
                namespace: parts[0].to_string(),
                service_version: (*svc_ver).to_string(),
                pub_key: Some((*pk).to_string()),
                project_id: Some((*proj).to_string()),
                project_version: Some((*proj_ver).to_string()),
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lib_exact() {
        let urn = "rho:lib:1.0.0:abc123:myproj:2.6.3";
        let parsed = parse_urn(urn).unwrap();
        assert_eq!(parsed.namespace, "lib");
        assert_eq!(parsed.service_version, "1.0.0");
        assert_eq!(parsed.pub_key.as_deref(), Some("abc123"));
        assert_eq!(parsed.project_id.as_deref(), Some("myproj"));
        assert_eq!(parsed.project_version.as_deref(), Some("2.6.3"));
    }

    #[test]
    fn parses_serve_exact() {
        let urn = "rho:serve:1.0.0:deadbeef:catalog:0.1.0";
        let parsed = parse_urn(urn).unwrap();
        assert_eq!(parsed.namespace, "serve");
        assert_eq!(parsed.service_version, "1.0.0");
        assert_eq!(parsed.pub_key.as_deref(), Some("deadbeef"));
        assert_eq!(parsed.project_id.as_deref(), Some("catalog"));
        assert_eq!(parsed.project_version.as_deref(), Some("0.1.0"));
    }

    #[test]
    fn parses_registry_exact() {
        let urn = "rho:registry:1.0.0";
        let parsed = parse_urn(urn).unwrap();
        assert_eq!(parsed.namespace, "registry");
        assert_eq!(parsed.service_version, "1.0.0");
        assert!(parsed.pub_key.is_none());
        assert!(parsed.project_id.is_none());
        assert!(parsed.project_version.is_none());
    }

    #[test]
    fn preserves_version_wildcards() {
        for (urn, svc, proj) in &[
            ("rho:lib:1.*:abc:proj:*", "1.*", Some("*")),
            ("rho:lib:1.0.*:abc:proj:2.6.*", "1.0.*", Some("2.6.*")),
            ("rho:serve:*:abc:proj:1.2.*", "*", Some("1.2.*")),
        ] {
            let parsed = parse_urn(urn).unwrap_or_else(|| panic!("failed: {urn}"));
            assert_eq!(parsed.service_version, *svc);
            assert_eq!(parsed.project_version.as_deref(), *proj);
        }

        let parsed = parse_urn("rho:registry:1.*").unwrap();
        assert_eq!(parsed.service_version, "1.*");
    }

    #[test]
    fn preserves_prerelease_tags() {
        let parsed = parse_urn("rho:lib:1.0.0:abc:proj:2.6.3-alpha").unwrap();
        assert_eq!(parsed.project_version.as_deref(), Some("2.6.3-alpha"));
    }

    #[test]
    fn rejects_unknown_scheme() {
        assert!(parse_urn("foo:lib:1.0.0:abc:proj:1.0.0").is_none());
        assert!(parse_urn("rho:id:zphj").is_none()); // legacy registry URI
        assert!(parse_urn("").is_none());
    }

    #[test]
    fn rejects_unknown_namespace() {
        assert!(parse_urn("rho:other:1.0.0:abc:proj:1.0.0").is_none());
        assert!(parse_urn("rho:io:stdout").is_none());
    }

    #[test]
    fn rejects_wrong_segment_count() {
        // lib needs 5 segments after rho:
        assert!(parse_urn("rho:lib:1.0.0:abc:proj").is_none());
        assert!(parse_urn("rho:lib:1.0.0:abc:proj:1.0.0:extra").is_none());
        // serve same
        assert!(parse_urn("rho:serve:1.0.0:abc:proj").is_none());
        // registry needs exactly 2 segments
        assert!(parse_urn("rho:registry").is_none());
        assert!(parse_urn("rho:registry:1.0.0:extra").is_none());
    }

    #[test]
    fn rejects_empty_segments() {
        assert!(parse_urn("rho:lib::abc:proj:1.0.0").is_none());
        assert!(parse_urn("rho:lib:1.0.0::proj:1.0.0").is_none());
        assert!(parse_urn("rho:lib:1.0.0:abc::1.0.0").is_none());
        assert!(parse_urn("rho:lib:1.0.0:abc:proj:").is_none());
        assert!(parse_urn("rho:registry:").is_none());
    }
}
