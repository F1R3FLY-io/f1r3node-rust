use std::env;
use std::fs::{remove_file, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

use models::rhoapi::Par;
use serde_json::Value;
use tempfile::NamedTempFile;
use tokio::io::BufReader;
use tokio::process::Command;

use super::errors::InterpreterError;
use crate::rust::interpreter::rho_type::{
    RhoBoolean, RhoList, RhoMap, RhoNil, RhoNumber, RhoString,
};

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
/// `Ok((frames, result))` where `frames` contains all emission frames (may be empty) and
/// `result` is the MeTTa execution's return value as a `Par`.
///
/// # Error Conditions
///
/// Same as [`petta_execute`] plus:
/// - `InterpreterError::SwiplError("No result line found ...")` — stdout ended without a result line
/// - Unknown channel URNs in frames are silently skipped (the caller may handle them)
///
/// # Environment Variables
///
/// Same as [`petta_execute`], plus `PETTA_MODE=NODE` is set automatically.
///
/// # See Also
///
/// - [`petta_execute`] — single-shot variant (NORMAL mode, backward compatible)
/// - [`Frame`] — per-println! emission frame
/// - [`system_processes::petta_execute`] — Rholang contract wrapper that forwards frames to rspace
pub async fn petta_execute_framed(metta_code: &str) -> Result<(Vec<Frame>, Par), InterpreterError> {
    let (_metta_file, metta_file_path) = create_metta_file(metta_code)?;
    let env = PettaEnv::from_env()?;

    let result = async {
        let mut cmd = Command::new(&env.script_path);
        cmd.arg(&metta_file_path)
            .env("PETTA_MODE", "NODE")
            .env("PETTA_DIR", &env.petta_dir)
            .env("CACHE_DIR", &env.cache_dir)
            .env("SANDBOX_LIB_PATH", &env.sandbox_lib_path);
        if let Some(ref bp) = env.blocked_preds {
            cmd.env("PETTA_BLOCKED_PREDS", bp);
        }
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

        let timeout_secs: u64 = 10;
        let framed_result = tokio::time::timeout(Duration::from_secs(timeout_secs), async {
            use tokio::io::AsyncBufReadExt;
            let reader = BufReader::new(child_stdout);
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
            InterpreterError::SwiplError("MeTTa execution timed out after 10 seconds".into())
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
