//! End-to-end coverage that the sandboxed PeTTa we provide (`petta.sh`) can
//! execute the full metta-moses and metta-attention (ECAN) `.metta` test suites
//! with no Python interpreter in the sandbox.
//!
//! Unlike the PLN suite (which uses `(library ...)` imports resolved through a
//! single `library_path`), these suites use repo-relative imports such as
//! `../../utilities/general-helpers` and PeTTa-library imports such as
//! `lib/lib_spaces`. `petta.sh`'s workspace mode (`PETTA_WORKSPACE_DIR`) binds
//! the whole repo read-only under `/tmp/session/ws`, keeps swipl's cwd at the
//! repo root, and sets `working_dir` to each test's own directory inside the
//! repo (so `../../x` resolves within it).
//!
//! The random/time/math primitives these suites relied on Python for are now
//! PeTTa builtins backed by `lib/pyrand.pl` (bit-exact with CPython's `random`),
//! so every seeded golden value is unchanged while no `py-call` is invoked.
//!
//! Each file is driven exactly like PLN's / the suites' own runners: run it, then
//! among the `is ... should ...` lines, pass iff there is a `✅` and no `❌`.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use rholang::rust::interpreter::test_utils::utils::should_skip_petta_workspace_test;
use tokio::process::Command;
use tokio::time::timeout;

/// Repo roots come from the devshell, mirroring `PLN_DIR`.
const MOSES_DIR_ENV: &str = "METTA_MOSES_DIR";
const ATTENTION_DIR_ENV: &str = "METTA_ATTENTION_DIR";

/// Predicates blocked while running suites in the sandbox (matches the PLN spec).
const BLOCKED_PREDS: &str = "readln!";

/// Upper bound on a single file's sandboxed execution. metta-moses includes a
/// full MOSES optimization benchmark (`demo-problems-benchmark-test`, a genetic
/// search with native-Prolog + tabling perf hacks) that legitimately runs for
/// ~80s, so this is generous.
const PER_FILE_TIMEOUT: Duration = Duration::from_secs(180);

/// metta-moses files excluded with cause. Each produces a real `❌` under both
/// the sandbox and the upstream Python-based runner, so they are pre-existing
/// assertion failures in the test logic, unrelated to the sandbox or the
/// de-pythonification, and are not part of the passing baseline this suite
/// guards. (`incremental-test`, which the Python runner also reports as failing,
/// actually passes here — its "failure" there is a runner assertion-count
/// artifact, not a `❌` — so it is intentionally not excluded.)
const MOSES_EXCLUDED: &[&str] = &[
    "utilities/tests/general-helper-functions-test.metta",
    "utilities/tests/lru-cache-test.metta",
    "feature-selection/tests/smd-test.metta",
    // Assertion-free files (every assertEqual/test is commented out): they emit
    // no ✅/❌ verdict, so this verdict-based harness cannot classify them. The
    // Python runner passes them trivially (its `passed == total` rule with
    // total == 0).
    "moses/tests/demo-problems-test.metta",
    "utilities/tests/py-call-test.metta",
    "deme/tests/create-deme-test.metta",
    "representation/tests/create-representation-test.metta",
];

/// metta-attention files excluded with cause. ForgettingAgent-test uses
/// colon-path module imports (`metta-attention:attention:...`) that require a
/// module registration the suite does not set up; the upstream runner excludes
/// it by name for the same reason ("Excluded N ForgettingAgent tests"). Every
/// other attention test passes in the sandbox.
/// `stochastic-importance-diffusion-test` is assertion-free (every assertEqual is
/// commented out), so like the assertion-free metta-moses files it emits no
/// verdict; the Python runner passes it trivially (`passed == total == 0`).
const ATTENTION_EXCLUDED: &[&str] = &[
    "attention/ForgettingAgent/tests/ForgettingAgent-test.metta",
    "attention-bank/bank/stochastic-importance-diffusion/tests/stochastic-importance-diffusion-test.metta",
];

fn petta_script() -> String {
    std::env::var("PETTA_SCRIPT_PATH").unwrap_or_else(|_| "./petta.sh".to_string())
}

/// Path of `file` relative to the repo root, used for reporting and exclusions.
fn rel_label(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .to_string_lossy()
        .into_owned()
}

/// Every `*test.metta` under `root` (recursively), minus the excluded ones.
/// Mirrors the suites' own `rglob("*test.metta")`; `*testold.metta` files do not
/// end in `test.metta` and are naturally skipped.
fn test_files(root: &Path, excluded: &[&str]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if path.is_dir() {
                if name != ".git" {
                    stack.push(path);
                }
            } else if name.ends_with("test.metta")
                && !excluded.contains(&rel_label(root, &path).as_str())
            {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// The verdict of a test, read from its own `is ... should ...` output.
enum Verdict {
    Pass(usize),
    Fail(usize),
    None,
}

/// Same rule as the suites' runners: among the assertion lines, pass iff at least
/// one `✅` and no `❌`.
fn verdict(stdout: &str) -> Verdict {
    let mut passes = 0usize;
    let mut fails = 0usize;
    for line in stdout
        .lines()
        .filter(|l| l.contains("is ") && l.contains("should "))
    {
        if line.contains('❌') {
            fails += 1;
        }
        if line.contains('✅') {
            passes += 1;
        }
    }

    if fails > 0 {
        Verdict::Fail(fails)
    } else if passes > 0 {
        Verdict::Pass(passes)
    } else {
        Verdict::None
    }
}

/// Runs a single `.metta` file through `petta.sh` in workspace mode (the whole
/// repo bound as the sandbox workspace) and returns its captured output.
async fn run_in_sandbox(
    root: &Path,
    file: &Path,
    workspace_name: &str,
) -> Result<std::process::Output, String> {
    let mut cmd = Command::new(petta_script());
    cmd.arg(file)
        .env("PETTA_WORKSPACE_DIR", root)
        // The suites import through a directory named after the repo
        // (`../../../metta-moses/utilities/x`), so the sandbox mount must carry
        // that canonical name. `root` may be a Nix store path whose basename is
        // a hash, so pass the project name explicitly rather than let petta.sh
        // derive it from the basename.
        .env("PETTA_WORKSPACE_NAME", workspace_name)
        .env("PETTA_BLOCKED_PREDS", BLOCKED_PREDS)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    match timeout(PER_FILE_TIMEOUT, cmd.output()).await {
        Err(_) => Err(format!("timed out after {PER_FILE_TIMEOUT:?}")),
        Ok(Err(e)) => Err(format!("failed to spawn petta.sh: {e}")),
        Ok(Ok(output)) => Ok(output),
    }
}

/// Drives an entire workspace suite: enumerate its `*test.metta`, run each in the
/// sandbox, and assert every one produces a passing verdict.
async fn run_workspace_suite(dir_env: &str, label: &str, excluded: &[&str]) {
    if should_skip_petta_workspace_test(dir_env, label) {
        return;
    }

    let root = PathBuf::from(std::env::var(dir_env).expect("checked by should_skip"));
    let files = test_files(&root, excluded);
    assert!(
        !files.is_empty(),
        "no *test.metta files found under {dir_env}={}",
        root.display()
    );

    println!(
        "\n=== Sandboxed PeTTa executing the {label} suite: {} file(s) ===",
        files.len()
    );

    let mut failures: Vec<String> = Vec::new();

    for file in &files {
        let name = rel_label(&root, file);
        match run_in_sandbox(&root, file, label).await {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                match verdict(&stdout) {
                    Verdict::Pass(n) => println!("  PASS   {name}  ({n} assertion(s))"),
                    Verdict::Fail(n) => {
                        println!("  FAIL   {name}  ({n} failing assertion(s) ❌)");
                        failures.push(format!("{name}: {n} failing assertion(s) (❌)"));
                    }
                    Verdict::None => {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        let tail: String =
                            stderr.lines().rev().take(3).collect::<Vec<_>>().join(" | ");
                        println!(
                            "  ERROR  {name}  (no ✅/❌ verdict; exit {:?}: {tail})",
                            output.status.code()
                        );
                        failures.push(format!(
                            "{name}: produced no assertion verdict (exit {:?}): {tail}",
                            output.status.code()
                        ));
                    }
                }
            }
            Err(why) => {
                println!("  ERROR  {name}  ({why})");
                failures.push(format!("{name}: {why}"));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "sandboxed PeTTa failed to execute {} of {} {label} test file(s):\n{}",
        failures.len(),
        files.len(),
        failures.join("\n")
    );
}

#[tokio::test]
async fn test_sandboxed_petta_executes_moses_suite() {
    run_workspace_suite(MOSES_DIR_ENV, "metta-moses", MOSES_EXCLUDED).await;
}

#[tokio::test]
async fn test_sandboxed_petta_executes_attention_suite() {
    run_workspace_suite(ATTENTION_DIR_ENV, "metta-attention", ATTENTION_EXCLUDED).await;
}
