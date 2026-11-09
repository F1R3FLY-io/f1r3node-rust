use std::env;
use std::fs::{remove_file, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crypto::rust::hash::sha_256::Sha256Hasher;
use models::rhoapi::Par;
use serde_json::Value;
use tempfile::NamedTempFile;
use tokio::io::BufReader;
use tokio::process::Command;

use super::errors::InterpreterError;
use crate::rust::interpreter::rho_type::{
    RhoBoolean, RhoList, RhoMap, RhoNil, RhoNumber, RhoString,
};

/// MeTTa functions that `consensus_lockdown/1` keeps: the fork's `register_fun` lists
/// without interop, assertion, host, clock, sleep and entropy builtins. Sorted.
pub const PETTA_CONSENSUS_ALLOWED_FUNS: &str =
    "!= #* #+ #- #// #< #= #> #\\= #div #max #min #mod % * + - / < <= = == =? =alpha > \
>= abs-math acos-math add-atom add-translator-rule! alpha-unique-atom and append \
asin-math atan-math atom_chars atom_concat bind! car-atom cdr-atom ceil-math \
change-state! concat cons cons-atom copy_term cos cos-math decons decons-atom empty \
eval exclude-item exp filter-atom first first-from-pair floor-math foldl-atom \
get-atoms get-metatype get-mettatype get-state get-type id implies import! \
index-atom intersection-atom is-alpha-member is-expr is-ground is-inf is-member \
is-nan is-space is-var isinf-math isnan-math last length let let* library \
list_to_set log log-math map-atom match max max-atom member min min-atom msort \
nan-value neg-inf not or parse pos-inf pow-math println! py-int py-round py-str \
py-xor py_inf py_int py_nan py_neginf py_round py_round_ndigits py_str py_xor \
rand-choice rand-getrandbits rand-randbelow rand-randint rand-random rand-seed \
rand_getrandbits rand_randbelow rand_randint rand_random rand_seed randomFloat \
reduce remove-atom remove-translator-rule! repr repra reverse round-math \
second-from-pair sin sin-math size-atom sort sort-atom sqrt sqrt-math sread \
subtraction-atom superpose tan-math term_hash trunc-math union-atom unique-atom xor";

/// Inference budget of the handler path: `call_with_inference_limit/3`, checked again after the run.
pub const PETTA_MAX_INFERENCES: u64 = 10_000_000;

/// Phlo charged for each inference that a `rho:petta:execute` run uses.
pub const PETTA_PHLO_PER_INFERENCE: i64 = 1;

/// Phlo charged before the sandbox starts (tunable).
pub const PETTA_BASE_COST: i64 = 100_000;

/// Deterministic memory limits for the handler path, passed to `petta.sh` NODE mode.
pub const PETTA_STACK_LIMIT_BYTES: u64 = 1 << 30;
pub const PETTA_TABLE_SPACE_BYTES: u64 = 128 << 20;
pub const PETTA_MAX_INTEGER_SIZE_BYTES: u64 = 64 << 10;

/// Address-space limit (liveness only). An abort gives `memory_limit_exceeded`.
pub const PETTA_ADDRESS_SPACE_BYTES: u64 = 4 << 30;

/// Characters of all frames and the result. The stdout reader stops at 4x in bytes.
pub const PETTA_MAX_OUTPUT_CHARS: u64 = 1 << 20;

/// Wall-clock backstop (seconds), liveness only. A hit is a node fault.
const PETTA_WALLCLOCK_BACKSTOP_SECS: u64 = 600;

/// SWI-Prolog version of the pinned `swipl` image in `node/Dockerfile`.
pub const SWIPL_VERSION: &str = "10.0.2";

/// [`petta_tree_hash`] of `src` and `lib` of mlabs-haskell/PeTTa b9ca1884.
pub const PETTA_SRC_TREE_HASH: &str =
    "6338b387b02394e51bed0d84f532eb20f9ec4a373bfe862bbf88f0efac3b2ebf";

/// [`petta_tree_hash`] of the whole dylon/PLN fbb2ae0f tree.
pub const PLN_SRC_TREE_HASH: &str =
    "d47114554b1bb75ab090914e5360c8f26f9471bdb77035ce1f4e705a24a2d3a5";

crate::register_consensus_constant!(order = 18, name = PETTA_MAX_INFERENCES, u64_be);
crate::register_consensus_constant!(order = 19, name = PETTA_STACK_LIMIT_BYTES, u64_be);
crate::register_consensus_constant!(order = 20, name = PETTA_TABLE_SPACE_BYTES, u64_be);
crate::register_consensus_constant!(order = 21, name = PETTA_MAX_INTEGER_SIZE_BYTES, u64_be);
crate::register_consensus_constant!(order = 22, name = PETTA_ADDRESS_SPACE_BYTES, u64_be);
crate::register_consensus_constant!(order = 23, name = PETTA_MAX_OUTPUT_CHARS, u64_be);
crate::register_consensus_constant!(order = 24, name = PETTA_PHLO_PER_INFERENCE, u64_be);
crate::register_consensus_constant!(order = 25, name = PETTA_BASE_COST, u64_be);
crate::register_consensus_constant!(order = 26, name = PETTA_CONSENSUS_ALLOWED_FUNS, str_bytes);
crate::register_consensus_constant!(order = 27, name = SWIPL_VERSION, str_bytes);
crate::register_consensus_constant!(order = 28, name = PETTA_SRC_TREE_HASH, str_bytes);
crate::register_consensus_constant!(order = 29, name = PLN_SRC_TREE_HASH, str_bytes);

/// Hex SHA-256 of `path\0content` for each file under `subdirs` ("" is `root`), in sorted
/// relative-path order, skipping `.git`.
pub fn petta_tree_hash(root: &Path, subdirs: &[&str]) -> std::io::Result<String> {
    fn walk(root: &Path, dir: &Path, files: &mut Vec<String>) -> std::io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            if path.file_name().is_some_and(|n| n == ".git") {
                continue;
            }
            if path.is_dir() {
                walk(root, &path, files)?;
            } else {
                let rel = path.strip_prefix(root).expect("walked path is under root");
                files.push(rel.to_string_lossy().into_owned());
            }
        }
        Ok(())
    }

    let mut files = Vec::new();
    for sub in subdirs {
        walk(root, &root.join(sub), &mut files)?;
    }
    files.sort();
    let mut buf = Vec::new();
    for file in &files {
        buf.extend_from_slice(file.as_bytes());
        buf.push(0);
        buf.extend(std::fs::read(root.join(file))?);
    }
    Ok(hex::encode(Sha256Hasher::hash(buf)))
}

/// Compares `swipl --version`, `PETTA_DIR` and `PLN_DIR` (when set) with the pins.
pub fn check_backend() -> Result<(), String> {
    let out = std::process::Command::new("swipl")
        .arg("--version")
        .output()
        .map_err(|e| format!("cannot run swipl --version: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    let version = text.split_whitespace().nth(2).unwrap_or_default();
    if version != SWIPL_VERSION {
        return Err(format!(
            "swipl version {version:?}, expected {SWIPL_VERSION}"
        ));
    }

    let check_tree = |var: &str, subdirs: &[&str], pin: &str| -> Result<(), String> {
        match env::var(var) {
            Ok(dir) if !dir.is_empty() => {
                let hash = petta_tree_hash(Path::new(&dir), subdirs)
                    .map_err(|e| format!("cannot hash {var}={dir}: {e}"))?;
                if hash != pin {
                    return Err(format!("{var}={dir} has tree hash {hash}, expected {pin}"));
                }
                Ok(())
            }
            _ => Ok(()),
        }
    };
    check_tree("PETTA_DIR", &["src", "lib"], PETTA_SRC_TREE_HASH)?;
    check_tree("PLN_DIR", &[""], PLN_SRC_TREE_HASH)
}

/// A frame emitted by the MeTTa interpreter in NODE mode.
///
/// Each frame corresponds to a `println!`/`trace!` invocation inside the MeTTa program
/// and carries the channel URN and argument values to be forwarded to Rholang channels.
#[derive(Debug, Clone)]
pub struct Frame {
    pub channel: String,
    pub arguments: Vec<Par>,
}

struct PettaEnv {
    script_path: PathBuf,
    petta_dir: String,
    cache_dir: String,
    sandbox_lib_path: String,
    blocked_preds: Option<String>,
}

impl PettaEnv {
    fn from_env() -> Result<Self, InterpreterError> {
        let script_path =
            PathBuf::from(env::var("PETTA_SCRIPT_PATH").unwrap_or("./petta.sh".into()));

        if !script_path.exists() {
            return Err(InterpreterError::SwiplError(
                "Can't find petta.sh script.".into(),
            ));
        }

        let petta_dir = env::var("PETTA_DIR").map_err(|_| {
            InterpreterError::SwiplError("PETTA_DIR environment variable not set".into())
        })?;

        let cache_dir = env::var("CACHE_DIR").map_err(|_| {
            InterpreterError::SwiplError("CACHE_DIR environment variable not set".into())
        })?;

        let sandbox_lib_path = env::var("SANDBOX_LIB_PATH").map_err(|_| {
            InterpreterError::SwiplError("SANDBOX_LIB_PATH environment variable not set".into())
        })?;

        let blocked_preds = env::var("PETTA_BLOCKED_PREDS").ok();

        Ok(PettaEnv {
            script_path,
            petta_dir,
            cache_dir,
            sandbox_lib_path,
            blocked_preds,
        })
    }
}

fn create_metta_file(metta_code: &str) -> Result<(NamedTempFile, String), InterpreterError> {
    let mut metta_file = NamedTempFile::new()
        .map_err(|_| InterpreterError::SwiplError("Can't open temp file".into()))?;
    metta_file
        .write_all(metta_code.as_bytes())
        .map_err(|_| InterpreterError::SwiplError("Can't write MeTTa code to temp file".into()))?;
    metta_file
        .flush()
        .map_err(|_| InterpreterError::SwiplError("Can't flush MeTTa temp file".into()))?;

    let metta_file_path = metta_file
        .path()
        .to_str()
        .ok_or(InterpreterError::SwiplError(
            "Can't convert metta_file path to string".into(),
        ))?
        .to_string();

    if metta_file_path.contains('\'') {
        return Err(InterpreterError::SwiplError(
            "Temp file path contains unsafe character (single quote)".into(),
        ));
    }

    {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&metta_file_path)?;
        file.write_all(metta_code.as_bytes())?;
    }

    Ok((metta_file, metta_file_path))
}

/// Executes MeTTa code through the PeTTa (SWI-Prolog) interpreter and returns the result as a Rholang Par.
///
/// # Overview
///
/// This function provides low-level access to the PeTTa interpreter, which
/// provides MeTTa execution with SWI-Prolog. The execution is sandboxed using bubblewrap
/// and libsandbox.so (from Cloudflare) to apply syscall whitelisting. The execution has
/// a 10-second timeout to prevent runaway computations.
///
/// # Arguments
///
/// * `metta_code` - A string containing valid MeTTa code to execute
///
/// # Returns
///
/// Returns `Ok(Par)` containing the execution result as a Rholang Par structure, or an
/// `InterpreterError::SwiplError` if execution fails.
///
/// # JSON Output Schema
///
/// PeTTa returns results as JSON in the following envelope:
/// ```json
/// {"results": [...]}
/// ```
///
/// The `results` field contains a JSON array of MeTTa execution results. This entire JSON
/// structure is converted to a Rholang Par using the following mapping:
///
/// - `null` → `RhoNil`
/// - `true`/`false` → `RhoBoolean`
/// - Numbers → `RhoNumber` (must fit in i64, otherwise error)
/// - Strings → `RhoString`
/// - Arrays → `RhoList` (recursive conversion of elements)
/// - Objects → `RhoMap` (keys converted to RhoString, values recursively converted)
///
/// # Error Conditions
///
/// - `InterpreterError::SwiplError("Can't find petta.sh script.")` - petta.sh script not found at `$PETTA_SCRIPT_PATH`
/// - `InterpreterError::SwiplError("PETTA_DIR environment variable not set")` - PETTA_DIR not set
/// - `InterpreterError::SwiplError("CACHE_DIR environment variable not set")` - CACHE_DIR not set
/// - `InterpreterError::SwiplError("SANDBOX_LIB_PATH environment variable not set")` - SANDBOX_LIB_PATH not set
/// - `InterpreterError::SwiplError("MeTTa execution timed out...")` - Execution exceeded 10 seconds
/// - `InterpreterError::SwiplError("PeTTa execution failed...")` - petta.sh returned error
/// - `InterpreterError::SwiplError("Can't parse JSON output...")` - Invalid JSON from PeTTa
/// - `InterpreterError::SwiplError("Could not parse number as i64")` - Number exceeds i64 range
///
/// # Environment Variables
///
/// - `PETTA_SCRIPT_PATH` - Path to petta.sh script (default: `./petta.sh`)
/// - `PETTA_DIR` - Path to PeTTa installation directory (required)
/// - `CACHE_DIR` - Path to cache directory for patched libraries (required)
/// - `SANDBOX_LIB_PATH` - Path to libsandbox.so library (required)
/// - `PETTA_BLOCKED_PREDS` - Space-separated list of MeTTa predicates to block from the
///   running program (default: `readln!`). Each predicate in this list is removed from
///   the `&self` space and unregistered from the dispatcher before the user's program
///   executes.
///
/// # Timeout
///
/// Execution is limited to 10 seconds. Long-running computations will be terminated and return
/// a timeout error. This prevents malicious or buggy MeTTa code from blocking the node.
///
/// # See Also
///
/// - [`system_processes::petta_execute`] - System process wrapper for Rholang contracts
/// - [`value_to_par`] - JSON to Par conversion logic
/// - [`petta_execute_framed`] - Streaming variant that returns NDJSON frames
/// Executes MeTTa code with an optional override for the blocked-predicate list.
/// When `blocked_preds` is `Some(list)`, it is forwarded as `PETTA_BLOCKED_PREDS`
/// to petta.sh regardless of the process environment.
pub async fn petta_execute_with_blocks(
    metta_code: &str,
    blocked_preds: Option<&str>,
) -> Result<Par, InterpreterError> {
    let (_metta_file, metta_file_path) = create_metta_file(metta_code)?;
    let mut env = PettaEnv::from_env()?;
    if let Some(bp) = blocked_preds {
        env.blocked_preds = Some(bp.to_string());
    }

    let result = async {
        let proc_handle = tokio::spawn(tokio::time::timeout(Duration::from_secs(10), {
            let mut cmd = Command::new(&env.script_path);
            cmd.arg(&metta_file_path)
                // The node may run under an LD_PRELOAD (e.g. jemalloc from the
                // Docker entrypoint); inherited by petta.sh it hangs the
                // bubblewrap-sandboxed interpreter. The sandbox sets its own
                // LD_PRELOAD (libsandbox) internally, so drop it for the child.
                .env_remove("LD_PRELOAD")
                .env("PETTA_DIR", &env.petta_dir)
                .env("CACHE_DIR", &env.cache_dir)
                .env("SANDBOX_LIB_PATH", &env.sandbox_lib_path);
            if let Some(ref bp) = env.blocked_preds {
                cmd.env("PETTA_BLOCKED_PREDS", bp);
            }
            cmd.kill_on_drop(true).output()
        }));

        let output = proc_handle
            .await
            .map_err(|join_error| {
                InterpreterError::SwiplError(
                    format!("Error while joining with the PeTTa task: {}", join_error).into(),
                )
            })?
            .map_err(|elapsed| {
                InterpreterError::SwiplError(
                    format!("MeTTa execution timed out after {}", elapsed).into(),
                )
            })?
            .map_err(|e| {
                InterpreterError::SwiplError(format!("MeTTa execution failed: {}", e).into())
            })?;

        if !output.status.success() {
            let stderr_str = String::from_utf8_lossy(&output.stderr);
            return Err(InterpreterError::SwiplError(
                format!("PeTTa execution failed. stderr: {}", stderr_str).into(),
            ));
        }

        let str_output = String::from_utf8(output.stdout)
            .map_err(|_| InterpreterError::SwiplError("Can't interpret PeTTa output".into()))?;

        let value_output = serde_json::from_str::<Value>(str_output.as_str()).map_err(|e| {
            InterpreterError::SwiplError(
                format!(
                    "Can't parse JSON output from PeTTa execution: {}. Output was: {}",
                    e, str_output
                )
                .into(),
            )
        })?;

        let par_output = value_to_par(value_output)?;
        Ok(par_output)
    }
    .await;

    remove_file(&metta_file_path).ok();

    result
}

/// Executes MeTTa code through the PeTTa (SWI-Prolog) interpreter
/// (convenience wrapper that reads `PETTA_BLOCKED_PREDS` from the env).
pub async fn petta_execute(metta_code: &str) -> Result<Par, InterpreterError> {
    petta_execute_with_blocks(metta_code, None).await
}

/// Executes MeTTa code in NODE mode, returning NDJSON frames plus the final result.
///
/// Runs `petta.sh` with `PETTA_MODE=NODE` so that every `println!`/`trace!` inside the
/// MeTTa program emits a JSON line `{"channel":"...","arguments":[...]}` to stdout, and
/// the final result is emitted as `{"type":"result","value":[...]}`.
///
/// The function reads the child's stdout line-by-line (NDJSON) and separates:
/// - **frames** — lines with a `"channel"` field; their `"arguments"` are converted to `Vec<Par>`
/// - **result** — the final line with `"type":"result"`; its `"value"` is converted to a `Par`
///
/// # Returns
///
/// A pair. The first element is `Ok((frames, result))`, where `frames` contains all emission frames (may be empty) and
/// `result` is the MeTTa execution's return value as a `Par`.
/// The second element is the inference count of the run, from the result frame or the
/// error frame. It is `None` when the run gave no count, for example on a halt or an abort.
///
/// # Error Conditions
///
/// The setup errors of [`petta_execute`], without its 10-second timeout, plus:
/// - `InterpreterError::SwiplError("PeTTa execution error: <reason>")` — a canonical reason
///   such as `inference_limit_exceeded` or `consensus_forbidden(<name>)`
/// - `InterpreterError::SwiplError("MeTTa execution hit the 600s wall-clock backstop")`
/// - `InterpreterError::SwiplError("No result line found ...")` — stdout ended without a result line
/// - Unknown channel URNs in frames are silently skipped (the caller may handle them)
///
/// # Environment Variables
///
/// The paths of [`petta_execute`]. The mode, the allowlist and the limits come from the constants.
///
/// # See Also
///
/// - [`petta_execute`] — single-shot variant (NORMAL mode, backward compatible)
/// - [`Frame`] — per-println! emission frame
/// - [`system_processes::petta_execute`] — Rholang contract wrapper that forwards frames to rspace
pub async fn petta_execute_framed(
    metta_code: &str,
) -> (Result<(Vec<Frame>, Par), InterpreterError>, Option<u64>) {
    let mut inferences = None;
    let result = run_framed(metta_code, &mut inferences).await;
    (result, inferences)
}

async fn run_framed(
    metta_code: &str,
    inferences: &mut Option<u64>,
) -> Result<(Vec<Frame>, Par), InterpreterError> {
    let (_metta_file, metta_file_path) = create_metta_file(metta_code)?;
    let env = PettaEnv::from_env()?;

    let result = async {
        let mut cmd = Command::new(&env.script_path);
        cmd.arg(&metta_file_path)
            // Strip any inherited LD_PRELOAD (e.g. the node's jemalloc) so it
            // does not hang the sandboxed interpreter; see petta_execute_with_blocks.
            .env_remove("LD_PRELOAD")
            .env("PETTA_MODE", "NODE")
            .env("PETTA_DIR", &env.petta_dir)
            .env("CACHE_DIR", &env.cache_dir)
            .env("SANDBOX_LIB_PATH", &env.sandbox_lib_path)
            .env("PETTA_ALLOWED_FUNS", PETTA_CONSENSUS_ALLOWED_FUNS)
            .env("PETTA_MAX_INFERENCES", PETTA_MAX_INFERENCES.to_string())
            .env("PETTA_STACK_LIMIT_BYTES", PETTA_STACK_LIMIT_BYTES.to_string())
            .env("PETTA_TABLE_SPACE_BYTES", PETTA_TABLE_SPACE_BYTES.to_string())
            .env("PETTA_MAX_INTEGER_SIZE_BYTES", PETTA_MAX_INTEGER_SIZE_BYTES.to_string())
            .env("PETTA_ADDRESS_SPACE_BYTES", PETTA_ADDRESS_SPACE_BYTES.to_string())
            .env("PETTA_MAX_OUTPUT_CHARS", PETTA_MAX_OUTPUT_CHARS.to_string());
        let mut child = cmd
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| {
                InterpreterError::SwiplError(format!("Failed to spawn PeTTa process: {}", e).into())
            })?;

        let child_stdout = child
            .stdout
            .take()
            .ok_or_else(|| InterpreterError::SwiplError("Can't open PeTTa stdout pipe".into()))?;

        let stderr_handle = {
            let stderr = child.stderr.take();
            tokio::spawn(async move {
                if let Some(mut stderr) = stderr {
                    use tokio::io::AsyncReadExt;
                    let mut buf = String::new();
                    stderr.read_to_string(&mut buf).await.ok();
                    buf
                } else {
                    String::new()
                }
            })
        };

        let timeout_secs: u64 = PETTA_WALLCLOCK_BACKSTOP_SECS;
        let framed_result = tokio::time::timeout(Duration::from_secs(timeout_secs), async {
            use tokio::io::{AsyncBufReadExt, AsyncReadExt};
            let reader_cap = 4 * PETTA_MAX_OUTPUT_CHARS + 1;
            let reader = BufReader::new(child_stdout.take(reader_cap + 1));
            let mut read_bytes: u64 = 0;
            let mut lines = tokio_stream::wrappers::LinesStream::new(reader.lines());

            let mut frames: Vec<Frame> = Vec::new();
            let mut result_par: Option<Par> = None;

            use tokio_stream::StreamExt;
            while let Some(line_result) = lines.next().await {
                let line: String = line_result.map_err(|e| {
                    InterpreterError::SwiplError(
                        format!("Failed to read line from PeTTa stdout: {}", e).into(),
                    )
                })?;

                read_bytes += line.len() as u64 + 1;
                if read_bytes > reader_cap {
                    tracing::error!(
                        target: "f1r3fly.petta",
                        reader_cap,
                        "node fault: PeTTa stdout passed the reader cap"
                    );
                    return Err(InterpreterError::SwiplError(
                        "PeTTa output passed the reader cap".into(),
                    ));
                }

                let trimmed = line.trim().to_string();
                if trimmed.is_empty() {
                    continue;
                }

                let value: Value = serde_json::from_str(&trimmed).map_err(|e| {
                    InterpreterError::SwiplError(
                        format!(
                            "Can't parse JSON frame from PeTTa: {}. Line was: {}",
                            e, trimmed
                        )
                        .into(),
                    )
                })?;

                match value {
                    Value::Object(ref map) if map.contains_key("channel") => {
                        let channel = map
                            .get("channel")
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| {
                                InterpreterError::SwiplError(
                                    "Frame missing 'channel' string field".into(),
                                )
                            })?
                            .to_string();

                        let arguments_arr = map.get("arguments").and_then(|v| v.as_array());
                        let arguments = match arguments_arr {
                            Some(arr) => arr
                                .iter()
                                .map(|v| value_to_par(v.clone()))
                                .collect::<Result<Vec<Par>, _>>()?,
                            None => Vec::new(),
                        };

                        frames.push(Frame { channel, arguments });
                    }
                    Value::Object(ref map)
                        if map.get("type").and_then(|v| v.as_str()) == Some("result") =>
                    {
                        *inferences = map.get("inferences").and_then(Value::as_u64);
                        let value_field = map.get("value").ok_or_else(|| {
                            InterpreterError::SwiplError(
                                "Result frame missing 'value' field".into(),
                            )
                        })?;
                        result_par = Some(value_to_par(value_field.clone())?);
                    }
                    Value::Object(ref map) if map.contains_key("results") => {
                        result_par = Some(value_to_par(value)?);
                    }
                    Value::Object(ref map)
                        if map.get("type").and_then(|v| v.as_str()) == Some("error") =>
                    {
                        *inferences = map.get("inferences").and_then(Value::as_u64);
                        let reason = map
                            .get("reason")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");
                        return Err(InterpreterError::SwiplError(
                            format!("PeTTa execution error: {}", reason).into(),
                        ));
                    }
                    _ => {
                        return Err(InterpreterError::SwiplError(
                            format!("Unknown JSON frame from PeTTa: {}", trimmed).into(),
                        ));
                    }
                }
            }
            Ok::<_, InterpreterError>((frames, result_par))
        })
        .await
        .map_err(|_| {
            tracing::error!(
                target: "f1r3fly.petta",
                backstop_secs = PETTA_WALLCLOCK_BACKSTOP_SECS,
                "node fault: PeTTa execution hit the wall-clock backstop"
            );
            InterpreterError::SwiplError(
                format!(
                    "MeTTa execution hit the {}s wall-clock backstop",
                    PETTA_WALLCLOCK_BACKSTOP_SECS
                )
                .into(),
            )
        })?
        .map_err(|e| e)?;

        let (frames, result_par) = framed_result;

        let result_par = result_par.ok_or_else(|| {
            InterpreterError::SwiplError(
                "No result line found in PeTTa output (expected line with type:'result')".into(),
            )
        })?;

        // Collect stderr
        let stderr_output = stderr_handle.await.unwrap_or_default();

        // Wait for child and check exit
        let exit_status = child.wait().await.map_err(|e| {
            InterpreterError::SwiplError(format!("Failed to wait for PeTTa child: {}", e).into())
        })?;

        if !exit_status.success() {
            return Err(InterpreterError::SwiplError(
                format!("PeTTa execution failed. stderr: {}", stderr_output).into(),
            ));
        }

        Ok((frames, result_par))
    }
    .await;

    remove_file(&metta_file_path).ok();

    result
}

/// Converts a JSON Value to a Rholang Par structure.
///
/// This function recursively transforms JSON data returned by PeTTa into Rholang's internal
/// representation (Par). It is used internally by [`petta_execute`] to convert PeTTa results.
///
/// # Type Mapping
///
/// | JSON Type | Rholang Type | Notes |
/// |-----------|--------------|-------|
/// | `null` | `RhoNil` | Represents absence of value |
/// | `boolean` | `RhoBoolean` | Direct mapping |
/// | `number` | `RhoNumber` | **Must be integer in i64 range**, floats rejected |
/// | `string` | `RhoString` | Direct mapping, supports Unicode |
/// | `array` | `RhoList` | Elements recursively converted |
/// | `object` | `RhoMap` | Keys stringified, values recursively converted |
///
/// # Important Constraints
///
/// - **Only integers are supported**: JSON numbers must be integers in the range `i64::MIN` to
///   `i64::MAX`. Floating-point numbers and integers outside this range are explicitly rejected
///   with descriptive errors to prevent silent truncation and maintain consensus safety.
/// - **Object keys become strings**: All JSON object keys are converted to `RhoString` in the
///   resulting `RhoMap`.
/// - **Recursive conversion**: Nested structures (arrays in arrays, objects in objects, etc.)
///   are fully supported and recursively converted.
///
/// # Arguments
///
/// * `v` - A `serde_json::Value` to convert
///
/// # Returns
///
/// Returns `Ok(Par)` with the converted structure, or `InterpreterError::SwiplError` if
/// conversion fails (e.g., number doesn't fit in i64).
///
/// # Examples
///
/// ```ignore
/// use serde_json::json;
///
/// // Simple values
/// let nil = value_to_par(json!(null))?;
/// let bool = value_to_par(json!(true))?;
/// let num = value_to_par(json!(42))?;
/// let str = value_to_par(json!("hello"))?;
///
/// // Collections
/// let list = value_to_par(json!([1, 2, 3]))?;
/// let map = value_to_par(json!({"key": "value"}))?;
///
/// // Nested structures
/// let nested = value_to_par(json!({
///     "list": [1, 2, 3],
///     "map": {"inner": "value"}
/// }))?;
/// ```
fn value_to_par(v: Value) -> Result<Par, InterpreterError> {
    match v {
        Value::Null => Ok(RhoNil::create_par()),
        Value::Bool(b) => Ok(RhoBoolean::create_par(b)),
        Value::Number(n) => {
            if !n.is_i64() {
                if n.is_f64() {
                    return Err(InterpreterError::SwiplError(
                        format!(
                            "Floating-point numbers are not supported. Got: {}. \
                             Only integers in the range {} to {} are allowed.",
                            n,
                            i64::MIN,
                            i64::MAX
                        )
                        .into(),
                    ));
                } else {
                    return Err(InterpreterError::SwiplError(
                        format!(
                            "Number exceeds i64 range. Got: {}. \
                             Only integers in the range {} to {} are allowed.",
                            n,
                            i64::MIN,
                            i64::MAX
                        )
                        .into(),
                    ));
                }
            }
            let n64 = n.as_i64().unwrap();
            Ok(RhoNumber::create_par(n64))
        }
        Value::String(s) => Ok(RhoString::create_par(s)),
        Value::Array(values) => {
            let ps = values
                .into_iter()
                .map(value_to_par)
                .collect::<Result<_, _>>()?;
            Ok(RhoList::create_par(ps))
        }
        Value::Object(map) => {
            let hashmap = map
                .into_iter()
                .map(|(k, v)| {
                    let p = value_to_par(v)?;
                    Ok((RhoString::create_par(k), p))
                })
                .collect::<Result<_, InterpreterError>>()?;
            Ok(RhoMap::create_par(hashmap))
        }
    }
}

///// TESTS /////

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    #[test]
    fn test_petta_tree_hash_format() {
        let dir = tempfile::tempdir().unwrap();
        for (path, content) in [
            ("src/a", "x"),
            ("lib/b", "y"),
            (".git/c", "z"),
            ("other/d", "w"),
        ] {
            let file = dir.path().join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, content).unwrap();
        }
        assert_eq!(
            petta_tree_hash(dir.path(), &["src", "lib"]).unwrap(),
            "5fd30b96986205199049074cc773bb322db7bed661aa8d53ce45ed0373b37f6f"
        );
    }

    #[test]
    fn test_petta_backend_matches_pins() {
        use crate::rust::interpreter::test_utils::utils::should_skip_petta_test;

        if should_skip_petta_test() {
            return;
        }
        assert_eq!(check_backend(), Ok(()));
    }

    #[test]
    fn test_consensus_allowlist_is_sorted_and_unique() {
        let names: Vec<&str> = PETTA_CONSENSUS_ALLOWED_FUNS.split(' ').collect();
        assert!(names.windows(2).all(|w| w[0] < w[1]), "{names:?}");
    }

    #[test]
    fn test_value_to_par_null() {
        let result = value_to_par(json!(null)).unwrap();
        assert_eq!(result, RhoNil::create_par());
    }
    #[test]
    fn test_value_to_par_boolean() {
        let result = value_to_par(json!(true)).unwrap();
        assert_eq!(result, RhoBoolean::create_par(true));
    }
    #[test]
    fn test_value_to_par_small_number() {
        let result = value_to_par(json!(42)).unwrap();
        assert_eq!(result, RhoNumber::create_par(42));
    }
    #[test]
    fn test_value_to_par_max_i64() {
        let result = value_to_par(json!(i64::MAX)).unwrap();
        assert_eq!(result, RhoNumber::create_par(i64::MAX));
    }
    #[test]
    fn test_value_to_par_string() {
        let result = value_to_par(json!("hello")).unwrap();
        assert_eq!(result, RhoString::create_par("hello".into()));
    }
    #[test]
    fn test_value_to_par_array() {
        let result = value_to_par(json!([1, "two", true])).unwrap();
        assert_eq!(
            result,
            RhoList::create_par(vec![
                RhoNumber::create_par(1),
                RhoString::create_par("two".into()),
                RhoBoolean::create_par(true)
            ])
        );
    }
    #[test]
    fn test_value_to_par_nested_array() {
        let result = value_to_par(json!([[1, 2], [3, 4]])).unwrap();
        assert_eq!(
            result,
            RhoList::create_par(vec![
                RhoList::create_par(vec![RhoNumber::create_par(1), RhoNumber::create_par(2)]),
                RhoList::create_par(vec![RhoNumber::create_par(3), RhoNumber::create_par(4)])
            ])
        );
    }
    #[test]
    fn test_value_to_par_object() {
        let result = value_to_par(json!({"key": "value", "num": 123})).unwrap();
        assert_eq!(
            result,
            RhoMap::create_par(
                vec![
                    (
                        RhoString::create_par("key".into()),
                        RhoString::create_par("value".into())
                    ),
                    (
                        RhoString::create_par("num".into()),
                        RhoNumber::create_par(123)
                    )
                ]
                .into_iter()
                .collect()
            )
        );
    }
    #[test]
    fn test_value_to_par_nested_object() {
        let result = value_to_par(json!({
            "outer": {
                "inner": "value"
            }
        }))
        .unwrap();
        assert_eq!(
            result,
            RhoMap::create_par(
                vec![(
                    RhoString::create_par("outer".into()),
                    RhoMap::create_par(
                        vec![(
                            RhoString::create_par("inner".into()),
                            RhoString::create_par("value".into())
                        )]
                        .into_iter()
                        .collect()
                    )
                )]
                .into_iter()
                .collect()
            )
        );
    }

    #[test]
    fn test_value_to_par_complex_nested_structure() {
        let result = value_to_par(json!({
            "list": [1, 2, 3],
            "nested": {
                "bool": true,
                "null": null
            }
        }))
        .unwrap();

        assert_eq!(
            result,
            RhoMap::create_par(
                vec![
                    (
                        RhoString::create_par("list".into()),
                        RhoList::create_par(vec![
                            RhoNumber::create_par(1),
                            RhoNumber::create_par(2),
                            RhoNumber::create_par(3)
                        ])
                    ),
                    (
                        RhoString::create_par("nested".into()),
                        RhoMap::create_par(
                            vec![
                                (
                                    RhoString::create_par("bool".into()),
                                    RhoBoolean::create_par(true)
                                ),
                                (RhoString::create_par("null".into()), RhoNil::create_par())
                            ]
                            .into_iter()
                            .collect()
                        )
                    )
                ]
                .into_iter()
                .collect()
            )
        );
    }

    #[test]
    fn test_value_to_par_empty_array() {
        let result = value_to_par(json!([])).unwrap();
        assert_eq!(result, RhoList::create_par(vec![]));
    }

    #[test]
    fn test_value_to_par_empty_object() {
        let result = value_to_par(json!({})).unwrap();
        assert_eq!(result, RhoMap::create_par(std::collections::HashMap::new()));
    }

    #[test]
    fn test_value_to_par_negative_number() {
        let result = value_to_par(json!(-42)).unwrap();
        assert_eq!(result, RhoNumber::create_par(-42));
    }

    #[test]
    fn test_value_to_par_zero() {
        let result = value_to_par(json!(0)).unwrap();
        assert_eq!(result, RhoNumber::create_par(0));
    }

    #[test]
    fn test_value_to_par_false_boolean() {
        let result = value_to_par(json!(false)).unwrap();
        assert_eq!(result, RhoBoolean::create_par(false));
    }

    #[test]
    fn test_value_to_par_empty_string() {
        let result = value_to_par(json!("")).unwrap();
        assert_eq!(result, RhoString::create_par("".into()));
    }

    #[test]
    fn test_value_to_par_unicode_string() {
        let result = value_to_par(json!("Hello, 世界! 🌍")).unwrap();
        assert_eq!(result, RhoString::create_par("Hello, 世界! 🌍".into()));
    }

    #[test]
    fn test_value_to_par_float_rejected() {
        let result = value_to_par(json!(3.14));
        assert!(result.is_err(), "Floating-point numbers should be rejected");
        let err = result.unwrap_err();
        let err_msg = format!("{:?}", err);
        assert!(
            err_msg.contains("Floating-point") && err_msg.contains("not supported"),
            "Error should mention floating-point not supported, got: {}",
            err_msg
        );
    }

    #[test]
    fn test_value_to_par_large_uint_rejected() {
        let large_uint = u64::MAX;
        let result = value_to_par(json!(large_uint));
        assert!(
            result.is_err(),
            "Numbers exceeding i64::MAX should be rejected"
        );
        let err = result.unwrap_err();
        let err_msg = format!("{:?}", err);
        assert!(
            err_msg.contains("exceeds i64 range") || err_msg.contains("not supported"),
            "Error should mention range exceeded, got: {}",
            err_msg
        );
    }

    #[test]
    fn test_value_to_par_min_i64() {
        let result = value_to_par(json!(i64::MIN)).unwrap();
        assert_eq!(result, RhoNumber::create_par(i64::MIN));
    }

    /// Test that petta_execute times out after 10 seconds for long-running computations.
    /// This uses a large fibonacci number that should exceed the timeout.
    #[tokio::test]
    async fn test_petta_execute_timeout() {
        use crate::rust::interpreter::test_utils::utils::should_skip_petta_test;

        if should_skip_petta_test() {
            return;
        }

        // Fibonacci of a very large number should timeout (10 second limit)
        // fib(10000000) will take much longer than 10 seconds
        let metta_code = r#"
            (= (fib-tr $n $a $b) (if (== $n 0) $a (fib-tr (- $n 1) $b (+ $a $b))))
            (= (fib $n) (fib-tr $n 0 1))
            !(fib 10000000)
        "#;

        let result = petta_execute(metta_code).await;

        // Should fail with a timeout error
        assert!(
            result.is_err(),
            "Large fibonacci computation should timeout"
        );

        let err = result.unwrap_err();
        let err_msg = format!("{:?}", err);

        // Error should mention timeout
        assert!(
            err_msg.contains("timed out") || err_msg.contains("timeout"),
            "Error should be a timeout error, got: {}",
            err_msg
        );
    }
}
