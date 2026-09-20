use std::fs::{self, File};
use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use casper_soak::{encoded, exclusive, hash, parse, regular, text, MAX_BYTES};
use eyre::{ensure, eyre, Result};
use serde_json::{json, Value};

use super::{Config, Provider, Reply};

pub struct Cli {
    config: Config,
    evidence: std::path::PathBuf,
    sequence: u64,
}

impl Cli {
    pub fn new(config: Config, evidence: &Path) -> Result<Self> {
        ensure!(
            evidence.is_dir() && !evidence.is_symlink(),
            "The transport evidence directory is invalid."
        );
        Ok(Self {
            config,
            evidence: evidence.to_owned(),
            sequence: 0,
        })
    }

    fn run(
        &mut self,
        kind: &str,
        args: &[String],
        request_digest: Option<String>,
    ) -> Result<Vec<u8>> {
        let temporary = tempfile::tempdir()?;
        let stdout = temporary.path().join("stdout");
        let stderr = temporary.path().join("stderr");
        let mut command = Command::new("timeout");
        command.args(["--signal=TERM", "--kill-after=2", "45"]);
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(File::create(&stdout)?)
            .stderr(File::create(&stderr)?);
        command
            .env_remove("GH_DEBUG")
            .env_remove("OCI_CLI_DEBUG")
            .env_remove("OCI_CLI_ENDPOINT");
        if kind == "github" && std::env::var_os("GH_TOKEN").is_none() {
            if let Some(token) = std::env::var_os("GITHUB_PERSONAL_ACCESS_TOKEN") {
                command.env("GH_TOKEN", token);
            }
        }
        unsafe {
            command.pre_exec(|| {
                let limit = libc::rlimit {
                    rlim_cur: MAX_BYTES,
                    rlim_max: MAX_BYTES,
                };
                if libc::setrlimit(libc::RLIMIT_FSIZE, &limit) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let status = command.status();
        let bytes = regular(&stdout, MAX_BYTES).unwrap_or_default();
        self.sequence += 1;
        exclusive(
            &self
                .evidence
                .join(format!("transport-{:05}.json", self.sequence)),
            &encoded(
                &json!({"service":kind,"exit_code":status.as_ref().ok().and_then(|s|s.code()),"request_sha256":request_digest,"response_sha256":hash(&bytes),"response_bytes":bytes.len(),"captured_epoch":self.now()}),
            )?,
            false,
        )?;
        ensure!(
            status?.success(),
            "The provider transport failed. Its outcome can be ambiguous."
        );
        ensure!(!bytes.is_empty(), "The provider response is empty.");
        Ok(bytes)
    }

    fn endpoint(&self, service: &str) -> Result<String> {
        let region = text(&self.config.value["storage"]["region"])?;
        Ok(match service {
            "object" => format!("https://objectstorage.{region}.oraclecloud.com"),
            "compute" => format!("https://iaas.{region}.oraclecloud.com"),
            "scheduler" => format!("https://resource-scheduler.{region}.oci.oraclecloud.com"),
            "functions" => format!("https://functions.{region}.oci.oraclecloud.com"),
            "identity" => format!("https://identity.{region}.oci.oraclecloud.com"),
            _ => return Err(eyre!("The OCI service is unsupported.")),
        })
    }
}

impl Provider for Cli {
    fn github(&mut self, path: &str) -> Result<Value> {
        ensure!(
            !path.starts_with('/') && !path.contains(['\r', '\n', ':']),
            "The GitHub path is invalid."
        );
        let args = vec![
            "gh".to_owned(),
            "api".to_owned(),
            "--hostname".to_owned(),
            "github.com".to_owned(),
            "--method".to_owned(),
            "GET".to_owned(),
            "-H".to_owned(),
            "Accept: application/vnd.github+json".to_owned(),
            path.to_owned(),
        ];
        let bytes = self.run("github", &args, None)?;
        let mut wrapped = b"{\"data\":".to_vec();
        wrapped.extend(bytes);
        wrapped.push(b'}');
        Ok(parse(&wrapped)?["data"].take())
    }

    fn github_post(&mut self, path: &str, body: &Value) -> Result<Value> {
        ensure!(
            path == format!(
                "repos/{}/actions/runners/generate-jitconfig",
                super::REPOSITORY
            ),
            "The GitHub write endpoint is unsupported."
        );
        let temporary = tempfile::tempdir()?;
        let input = temporary.path().join("request.json");
        let bytes = encoded(body)?;
        fs::write(&input, &bytes)?;
        let args = vec![
            "gh".into(),
            "api".into(),
            "--hostname".into(),
            "github.com".into(),
            "--method".into(),
            "POST".into(),
            "--input".into(),
            input.to_string_lossy().into_owned(),
            path.into(),
        ];
        parse(&self.run("github", &args, Some(hash(&bytes)))?)
    }

    fn oci(
        &mut self,
        service: &str,
        method: &str,
        path: &str,
        body: Option<&Value>,
        etag: Option<&str>,
    ) -> Result<Reply> {
        ensure!(
            matches!(method, "GET" | "PUT" | "POST" | "DELETE") && !path.contains(['\r', '\n']),
            "The provider operation is unsupported."
        );
        let uri = if matches!(service, "invoke" | "invoke-detached") {
            let region = text(&self.config.value["storage"]["region"])?;
            let host = path
                .strip_prefix("https://")
                .and_then(|s| s.split_once('/'))
                .map(|(host, _)| host)
                .ok_or_else(|| eyre!("The invocation endpoint is invalid."))?;
            ensure!(
                host.ends_with(&format!(".functions.{region}.oci.oraclecloud.com"))
                    && host
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b".-".contains(&c)),
                "The invocation endpoint is outside the configured OCI region."
            );
            path.to_owned()
        } else {
            ensure!(path.starts_with('/'), "The OCI path must be absolute.");
            format!("{}{path}", self.endpoint(service)?)
        };
        let temporary = tempfile::tempdir()?;
        let mut args = vec![
            "oci".to_owned(),
            "--no-retry".to_owned(),
            "--connection-timeout".to_owned(),
            "10".to_owned(),
            "--read-timeout".to_owned(),
            "30".to_owned(),
            "--region".to_owned(),
            text(&self.config.value["storage"]["region"])?.to_owned(),
            "raw-request".to_owned(),
            "--http-method".to_owned(),
            method.to_owned(),
            "--target-uri".to_owned(),
            uri,
        ];
        let mut headers = json!({"Content-Type":"application/json"});
        if let Some(tag) = etag {
            headers["if-match"] = json!(tag);
        }
        if matches!(service, "invoke" | "invoke-detached") {
            headers["fn-invoke-type"] = json!(if service == "invoke-detached" {
                "detached"
            } else {
                "sync"
            });
        }
        let header_path = temporary.path().join("headers.json");
        fs::write(&header_path, encoded(&headers)?)?;
        args.extend([
            "--request-headers".to_owned(),
            format!("file://{}", header_path.display()),
        ]);
        let mut request_digest = None;
        if let Some(body) = body {
            let path = temporary.path().join("request.json");
            let bytes = encoded(body)?;
            ensure!(
                bytes.len() as u64 <= MAX_BYTES,
                "The provider request exceeds its bound."
            );
            File::create(&path)?.write_all(&bytes)?;
            request_digest = Some(hash(&bytes));
            args.extend([
                "--request-body".to_owned(),
                format!("file://{}", path.display()),
            ]);
        }
        let bytes = self.run(service, &args, request_digest)?;
        let response = parse(&bytes)?;
        let status = text(&response["status"])?
            .split(' ')
            .next()
            .ok_or_else(|| eyre!("The response status is missing."))?
            .parse()?;
        let headers = response["headers"]
            .as_object()
            .ok_or_else(|| eyre!("The response headers are missing."))?;
        let header = |key: &str| -> Option<String> {
            headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .and_then(|(_, v)| v.as_str())
                .map(str::to_owned)
        };
        Ok(Reply {
            status,
            data: response["data"].clone(),
            etag: header("etag"),
            next_page: header("opc-next-page"),
        })
    }

    fn now(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }
    fn pause(&mut self, seconds: u64) { std::thread::sleep(Duration::from_secs(seconds)); }
}
