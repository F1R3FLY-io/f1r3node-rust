use std::error::Error;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use crypto::rust::signatures::signed::Signed;
use models::rust::casper::protocol::casper_message::DeployData;
use models::rust::deploy_request::DeployRequest;
use rholang::rust::interpreter::util::vault_address::VaultAddress;
use serde_json::{json, Value};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

fn private_file(path: &Path, bytes: &[u8]) -> Result<()> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?
        .write_all(bytes)?;
    Ok(())
}

fn prepare(directory: &Path) -> Result<()> {
    fs::create_dir_all(directory)?;
    let mut public = serde_json::Map::new();
    let mut wallets = String::new();
    let mut bonds = String::new();
    for role in ["boot", "validator1", "validator2", "validator3", "deployer"] {
        let (secret, key) = Secp256k1.new_key_pair();
        private_file(
            &directory.join(format!("{role}.key")),
            hex::encode(&secret.bytes).as_bytes(),
        )?;
        let address = VaultAddress::from_public_key(&key)
            .ok_or("invalid generated key")?
            .to_base58();
        wallets.push_str(&format!("{address},1000000000000000\n"));
        if role.starts_with("validator") {
            bonds.push_str(&format!("{} 1000\n", hex::encode(&key.bytes)));
        }
        public.insert(role.into(), json!(hex::encode(&key.bytes)));
    }
    private_file(&directory.join("wallets.txt"), wallets.as_bytes())?;
    private_file(&directory.join("bonds.txt"), bonds.as_bytes())?;
    private_file(
        &directory.join("public.json"),
        &serde_json::to_vec_pretty(&public)?,
    )?;
    Ok(())
}

fn request(base: &str, path: &str, body: Option<Value>) -> Result<Value> {
    let (mut command, base) = if let Some(container) = base.strip_prefix("docker://") {
        if !container.starts_with("cold-compression-")
            || !container
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err("only disposable validation containers are supported".into());
        }
        let mut command = Command::new("docker");
        command.args(["exec", "-i", container, "curl"]);
        (command, "http://127.0.0.1:40403")
    } else if base
        .strip_prefix("http://127.0.0.1:")
        .and_then(|port| port.parse::<u16>().ok())
        .is_some_and(|port| port > 0)
    {
        (Command::new("curl"), base)
    } else {
        return Err("only disposable loopback endpoints are supported".into());
    };
    command.args([
        "--silent",
        "--show-error",
        "--fail-with-body",
        "--connect-timeout",
        "3",
        "--max-time",
        "20",
        "--max-filesize",
        "16777216",
    ]);
    if body.is_some() {
        command.args([
            "-H",
            "Content-Type: application/json",
            "--data-binary",
            "@-",
        ]);
    }
    command.arg(format!("{base}/api/{path}"));
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Some(value) = body {
        child
            .stdin
            .take()
            .ok_or("missing curl stdin")?
            .write_all(&serde_json::to_vec(&value)?)?;
    }
    drop(child.stdin.take());
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(format!(
            "request {path} failed: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}

fn namespace(directory: &Path) -> Result<String> {
    let keys: Value = serde_json::from_slice(&fs::read(directory.join("public.json"))?)?;
    Ok(format!(
        "cold-compression-{}",
        keys["deployer"].as_str().ok_or("missing deployer")?
    ))
}

fn deployment_timestamp() -> Result<i64> {
    static LAST: AtomicI64 = AtomicI64::new(0);
    let now = i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())?;
    let previous = LAST
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |previous| {
            previous.checked_add(1).map(|next| now.max(next))
        })
        .map_err(|_| "deploy timestamp exhausted")?;
    Ok(now.max(previous + 1))
}

fn deploy(directory: &Path, base: &str, term: String) -> Result<String> {
    let height = request(base, "status", None)?["lastFinalizedBlockNumber"]
        .as_i64()
        .ok_or("missing finalized height")?;
    let key = fs::read_to_string(directory.join("deployer.key"))?;
    let secret =
        PrivateKey::from_bytes(&hex::decode(key.trim()).map_err(|_| "invalid test signing key")?);
    let signed = Signed::create(
        DeployData {
            term,
            time_stamp: deployment_timestamp()?,
            phlo_price: 1.into(),
            phlo_limit: 5_000_000,
            valid_after_block_number: height,
            shard_id: "root".into(),
            expiration_timestamp: None,
            parameters: Vec::new(),
        },
        Box::new(Secp256k1),
        secret,
    )
    .map_err(|_| "failed to sign test deploy")?;
    let signature = hex::encode(&signed.sig);
    request(
        base,
        "deploy",
        Some(serde_json::to_value(DeployRequest::from_signed(&signed))?),
    )?;
    Ok(signature)
}

fn wait_finalized(base: &str, signatures: &[String]) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(600);
    let mut pending = signatures.to_vec();
    while !pending.is_empty() {
        let mut next = Vec::new();
        for signature in pending {
            let status = request(
                base,
                &format!("deploy-finalization-status/{signature}"),
                None,
            )?;
            match status["state"].as_str() {
                Some("Finalized") => {}
                Some("Failed" | "Expired") => {
                    return Err(format!("deploy terminal failure: {status}").into())
                }
                _ => next.push(signature),
            }
        }
        pending = next;
        if Instant::now() > deadline {
            return Err(format!("{} deploys did not finalize", pending.len()).into());
        }
        if !pending.is_empty() {
            std::thread::sleep(Duration::from_secs(3));
        }
    }
    Ok(())
}

fn workload(directory: &Path, endpoints: &[String]) -> Result<()> {
    let started = Instant::now();
    let ns = namespace(directory)?;
    let padding = "cold-continuation".repeat(256);
    let setup = format!(
        r#"@"{ns}-state"!(0, 0, 0) |
contract @"{ns}-update"(@kind, @id, reply) = {{
  for (@counter, @locks, @total <- @"{ns}-state") {{
    match kind {{
      "counter" => {{ @"{ns}-state"!(counter + 1, locks, total) | reply!([id, "{padding}"]) }}
      "lock" => {{ @"{ns}-state"!(counter, locks + 1, total + 1000) | @"{ns}-locks"!(id, 1000) | reply!([id, "{padding}"]) }}
    }}
  }}
}}"#
    );
    let initial = deploy(directory, &endpoints[0], setup)?;
    wait_finalized(&endpoints[0], std::slice::from_ref(&initial))?;
    let mut evidence = json!({"initialization":initial,"counter":[],"lock":[]});
    for phase in ["counter", "lock"] {
        let mut signatures = Vec::new();
        for first in (0..100).step_by(5) {
            let batch = std::thread::scope(|scope| {
                let workers: Vec<_> = (first..first + 5).map(|i| {
                    let term = format!(r#"new reply in {{ @"{ns}-update"!("{phase}", {i}, *reply) | for (_ <- reply) {{ Nil }} }}"#);
                    let base = &endpoints[i % endpoints.len()];
                    scope.spawn(move || deploy(directory, base, term))
                }).collect();
                workers
                    .into_iter()
                    .map(|worker| worker.join().map_err(|_| "deploy worker panicked")?)
                    .collect::<Result<Vec<_>>>()
            })?;
            signatures.extend(batch);
            evidence[phase] = json!(signatures);
            fs::write(
                directory.join("deploys.json"),
                serde_json::to_vec_pretty(&evidence)?,
            )?;
        }
        wait_finalized(&endpoints[0], &signatures)?;
        println!("{phase}: 100 finalized deploys");
    }
    evidence["elapsed_seconds"] = json!(started.elapsed().as_secs_f64());
    fs::write(
        directory.join("workload.json"),
        serde_json::to_vec_pretty(&evidence)?,
    )?;
    Ok(())
}

fn verify(directory: &Path, endpoints: &[String]) -> Result<Value> {
    let ns = namespace(directory)?;
    let anchor = request(&endpoints[0], "last-finalized-block", None)?;
    let hash = anchor["blockInfo"]["blockHash"]
        .as_str()
        .ok_or("missing anchor hash")?;
    let root = anchor["blockInfo"]["postStateHash"]
        .as_str()
        .filter(|root| !root.is_empty())
        .ok_or("missing anchor state root")?;
    let term = format!(
        r#"new return in {{ for (@counter, @locks, @total <- @"{ns}-state") {{ return!([counter, locks, total]) }} }}"#
    );
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut observations = Vec::new();
    let mut state_queries = 0;
    for base in endpoints {
        while request(base, &format!("is-finalized/{hash}"), None)? != json!(true) {
            if Instant::now() > deadline {
                return Err("common anchor did not finalize".into());
            }
            std::thread::sleep(Duration::from_secs(2));
        }
        let block = request(base, &format!("block/{hash}"), None)?;
        if block["blockInfo"]["blockHash"] != hash || block["blockInfo"]["postStateHash"] != root {
            return Err("common finalized block or state root differs".into());
        }
        let mut observation = json!({"endpoint":base,"block":block["blockInfo"]});
        if request(base, "status", None)?["isReadOnly"] == true {
            let result = request(
                base,
                "explore-deploy-by-block-hash",
                Some(json!({"term":term,"blockHash":hash,"usePreStateHash":false})),
            )?;
            let expected = json!([{"ExprList":{"data":[{"ExprInt":{"data":"100"}},{"ExprInt":{"data":"100"}},{"ExprInt":{"data":"100000"}}]}}]);
            if result["expr"] != expected || result["block"]["blockHash"] != hash {
                return Err(
                    format!("unexpected finalized state at {base}: {}", result["expr"]).into(),
                );
            }
            observation["expr"] = result["expr"].clone();
            state_queries += 1;
        }
        observations.push(observation);
    }
    if state_queries == 0 {
        return Err("a read-only observer is required to verify finalized state".into());
    }
    Ok(json!({"anchor":anchor,"observations":observations}))
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [command, directory] if command == "prepare" => prepare(Path::new(directory)),
        [command, directory, endpoints] if command == "run" || command == "verify" => {
            let endpoints: Vec<String> = endpoints.split(',').map(String::from).collect();
            if endpoints.is_empty() || endpoints.len() > 5 {
                return Err("invalid endpoint count".into());
            }
            if command == "run" {
                workload(Path::new(directory), &endpoints)
            } else {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&verify(Path::new(directory), &endpoints)?)?
                );
                Ok(())
            }
        }
        _ => Err(
            "usage: cold_store_validation prepare DIRECTORY | run/verify DIRECTORY ENDPOINTS"
                .into(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrent_deploy_timestamps_are_unique() {
        let mut timestamps = std::thread::scope(|scope| {
            let workers: Vec<_> = (0..8)
                .map(|_| {
                    scope.spawn(|| {
                        (0..128)
                            .map(|_| deployment_timestamp().unwrap())
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            workers
                .into_iter()
                .flat_map(|worker| worker.join().unwrap())
                .collect::<Vec<_>>()
        });
        timestamps.sort_unstable();
        assert_eq!(timestamps.len(), 1024);
        assert!(timestamps.windows(2).all(|pair| pair[0] < pair[1]));
    }
}
