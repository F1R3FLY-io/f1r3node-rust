#[cfg(target_os = "linux")]
mod linux {
    use std::fs::{self, DirBuilder, File, OpenOptions};
    use std::io::ErrorKind;
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
    use std::path::{Component, Path};

    use casper_soak::{encoded, exclusive, number, object, parse, regular, text};
    use eyre::{ensure, eyre, Result};
    use serde_json::{json, Value};

    const SCOPE: &str = "local-baseline-reservation-store";
    const LIMIT: u64 = 4096;

    fn load(path: &Path) -> Result<Value> { parse(&regular(path, LIMIT)?) }

    fn keys(value: &Value, expected: &[&str]) -> Result<()> {
        let fields = object(value)?;
        ensure!(
            fields.len() == expected.len() && expected.iter().all(|key| fields.contains_key(*key)),
            "The record fields are missing or unsupported."
        );
        Ok(())
    }

    fn digest(value: &Value) -> Result<bool> {
        let value = text(value)?;
        Ok(value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
    }

    fn config_valid(config: &Value) -> Result<()> {
        keys(config, &[
            "schema_version",
            "campaign_id",
            "identity_digest",
            "approval_digest",
            "preflight_candidate_id",
        ])?;
        let suffix = text(&config["campaign_id"])?
            .strip_prefix("task-017-12-")
            .unwrap_or("");
        ensure!(
            number(&config["schema_version"])? == 1
                && (1..=48).contains(&suffix.len())
                && suffix
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
                && digest(&config["identity_digest"])?
                && digest(&config["approval_digest"])?
                && matches!(
                    text(&config["preflight_candidate_id"])?,
                    "dev-amd64" | "dev-arm64"
                ),
            "The campaign binding is invalid."
        );
        Ok(())
    }

    fn request_valid(request: &Value, config: &Value) -> Result<()> {
        keys(request, &[
            "schema_version",
            "campaign_id",
            "identity_digest",
            "approval_digest",
            "stage",
            "candidate_id",
            "run_id",
            "run_attempt",
        ])?;
        let run = text(&request["run_id"])?;
        ensure!(
            number(&request["schema_version"])? == 1
                && number(&request["run_attempt"])? == 1
                && request["campaign_id"] == config["campaign_id"]
                && request["identity_digest"] == config["identity_digest"]
                && request["approval_digest"] == config["approval_digest"]
                && (1..=20).contains(&run.len())
                && run.bytes().all(|byte| byte.is_ascii_digit())
                && !run.starts_with('0')
                && matches!(text(&request["stage"])?, "preflight" | "baseline")
                && matches!(text(&request["candidate_id"])?, "dev-amd64" | "dev-arm64")
                && (request["stage"] != "preflight"
                    || request["candidate_id"] == config["preflight_candidate_id"]),
            "The reservation request differs from the campaign binding or allowed run."
        );
        Ok(())
    }

    fn canonical_root(root: &Path, existing: bool) -> Result<()> {
        ensure!(
            root.is_absolute()
                && root
                    .components()
                    .all(|part| matches!(part, Component::RootDir | Component::Normal(_))),
            "The store root must be an absolute canonical path."
        );
        let checked = if existing {
            root
        } else {
            root.parent()
                .ok_or_else(|| eyre!("The root has no parent."))?
        };
        ensure!(
            checked.canonicalize()?.as_os_str() == checked.as_os_str(),
            "The store path contains a symbolic link or is not canonical."
        );
        Ok(())
    }

    fn directory(root: &Path) -> Result<File> {
        canonical_root(root, true)?;
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY)
            .open(root)?;
        let metadata = file.metadata()?;
        ensure!(
            metadata.is_dir()
                && metadata.uid() == unsafe { libc::geteuid() }
                && metadata.permissions().mode() & 0o7777 == 0o700,
            "The store root is not owner-only or has a different owner."
        );
        Ok(file)
    }

    fn init(root: &Path, input: &Path) -> Result<()> {
        let config = load(input)?;
        config_valid(&config)?;
        canonical_root(root, false)?;
        DirBuilder::new().mode(0o700).create(root)?;
        let directory = directory(root)?;
        directory.try_lock()?;
        exclusive(&root.join("campaign.json"), &encoded(&config)?, false)?;
        File::open(
            root.parent()
                .ok_or_else(|| eyre!("The root has no parent."))?,
        )?
        .sync_all()?;
        println!(
            "{}",
            json!({"scope": SCOPE, "execution_enabled": false, "approval_authenticated": false, "initialized": true})
        );
        Ok(())
    }

    fn reserve(root: &Path, input: &Path) -> Result<()> {
        let request = load(input)?;
        let directory = directory(root)?;
        directory.try_lock()?;
        let config = load(&root.join("campaign.json"))?;
        config_valid(&config)?;
        request_valid(&request, &config)?;
        let slots = [
            (
                "preflight.json",
                "preflight",
                text(&config["preflight_candidate_id"])?,
            ),
            ("baseline-dev-amd64.json", "baseline", "dev-amd64"),
            ("baseline-dev-arm64.json", "baseline", "dev-arm64"),
        ];
        let mut used_runs = Vec::new();
        let mut preflight = false;
        let mut baseline = false;
        let mut target = None;
        for (name, stage, candidate) in slots {
            let path = root.join(name);
            let occupied = match fs::symlink_metadata(&path) {
                Ok(_) => true,
                Err(error) if error.kind() == ErrorKind::NotFound => false,
                Err(error) => return Err(error.into()),
            };
            if occupied {
                let record = load(&path)?;
                keys(&record, &[
                    "scope",
                    "execution_enabled",
                    "approval_authenticated",
                    "request",
                ])?;
                request_valid(&record["request"], &config)?;
                let run = text(&record["request"]["run_id"])?;
                ensure!(
                    record["scope"] == SCOPE
                        && record["execution_enabled"] == false
                        && record["approval_authenticated"] == false
                        && record["request"]["stage"] == stage
                        && record["request"]["candidate_id"] == candidate
                        && !used_runs.iter().any(|used| used == run),
                    "An existing reservation record is inconsistent."
                );
                used_runs.push(run.to_owned());
                if stage == "preflight" {
                    preflight = true;
                } else {
                    baseline = true;
                }
            }
            if request["stage"] == stage && request["candidate_id"] == candidate {
                ensure!(!occupied, "The launch slot is already consumed.");
                target = Some(path);
            }
        }
        ensure!(
            !baseline || preflight,
            "A baseline reservation has no preflight reservation."
        );
        let run = text(&request["run_id"])?;
        ensure!(
            !used_runs.iter().any(|used| used == run),
            "The run identifier is already reserved."
        );
        ensure!(
            request["stage"] == "preflight" || preflight,
            "The preflight reservation is missing."
        );
        let value = json!({"scope": SCOPE, "execution_enabled": false, "approval_authenticated": false, "request": request});
        exclusive(
            &target.ok_or_else(|| eyre!("The launch slot is unsupported."))?,
            &encoded(&value)?,
            false,
        )?;
        println!("{value}");
        Ok(())
    }

    pub fn run() -> Result<()> {
        let args: Vec<_> = std::env::args_os().collect();
        ensure!(
            args.len() == 4,
            "Select init or reserve with a root and input file."
        );
        let root = Path::new(&args[2]);
        let input = Path::new(&args[3]);
        match args[1].to_str() {
            Some("init") => init(root, input),
            Some("reserve") => reserve(root, input),
            _ => Err(eyre!("Only init and reserve commands are supported.")),
        }
    }
}

fn main() {
    #[cfg(target_os = "linux")]
    let result = linux::run();
    #[cfg(not(target_os = "linux"))]
    let result: eyre::Result<()> = Err(eyre::eyre!("The reservation store requires Linux."));
    if let Err(error) = result {
        eprintln!("Campaign reservation rejected: {error}");
        std::process::exit(2);
    }
}
