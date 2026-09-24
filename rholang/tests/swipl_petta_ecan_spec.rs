use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use rholang::rust::interpreter::test_utils::utils::should_skip_ecan_test;
use tokio::process::Command;
use tokio::time::timeout;

/// End-to-end coverage that the sandboxed PeTTa we provide (`petta.sh`) can
/// execute the PLN (ECAN) test suite from the `dylon/PLN` `feature/mettatron`
/// branch. The PLN source is provided by the devshell via `PLN_DIR`; `petta.sh`
/// binds it into the sandbox and registers it as a MeTTaTron `library_path`, so
/// `!(import! &self (library PLN lib_pln))` resolves without network access.
///
/// The suite is driven entirely from Rust: it enumerates the PLN `.metta` files,
/// runs each through `petta.sh` inside the seccomp/bubblewrap sandbox, and
/// applies the same pass/fail rule as PLN's own `test.sh` (among the
/// `is ... should ...` lines, pass iff there is a `✅` and no `❌`).
/// Predicates blocked while running PLN tests in the sandbox. `git-import!` is
/// not listed here: `petta.sh` removes it structurally (it shadows
/// `lib_import.metta` to drop `git-import!` from the exported functions), so the
/// `!(git-import! ...)` calls in `ruletests/*` are inert and `(library PLN
/// lib_pln)` resolves solely through the `PLN_DIR` library_path — no duplicate
/// import.
const BLOCKED_PREDS: &str = "readln!";

/// Upper bound on a single PLN test's sandboxed execution. PLN inference is
/// heavier than the arithmetic in the other PeTTa specs, so this is generous.
const PER_FILE_TIMEOUT: Duration = Duration::from_secs(120);

/// PLN `.metta` files that do not run to a verdict, excluded with cause:
/// - `examples/Direct.metta` — its `?` query macro is written for MeTTaTron
///   evaluation semantics and drives `grandfather` down a compiled call path
///   that invokes `father` as the predicate `father/3` (PeTTa compiles an
///   N-arg function to an (N+1)-arg predicate). Stock PeTTa only specializes
///   `father/3` lazily on a direct reduce-call, which this path never makes, so
///   it aborts with `Unknown procedure: father/3` and emits no `✅`/`❌`. This is
///   a MeTTaTron-conformance gap in the file itself, unrelated to the sandbox.
const NOT_SANDBOX_RUNNABLE: &[&str] = &["examples/Direct.metta"];

fn pln_dir() -> String {
    std::env::var("PLN_DIR").expect("PLN_DIR must be set (checked by should_skip_ecan_test)")
}

fn petta_script() -> String {
    std::env::var("PETTA_SCRIPT_PATH").unwrap_or_else(|_| "./petta.sh".to_string())
}

/// Short `<subdir>/<file>` label for a PLN test path, used for reporting and for
/// matching against `NOT_SANDBOX_RUNNABLE`.
fn file_label(path: &Path) -> String {
    let parent = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    format!("{parent}/{name}")
}

/// Collects every `examples/*.metta` and `ruletests/*.metta` file under
/// `$PLN_DIR`, minus the explicitly excluded, non-runnable ones.
fn pln_test_files() -> Vec<PathBuf> {
    let root = pln_dir();
    let mut files = Vec::new();

    for sub in ["examples", "ruletests"] {
        let dir = PathBuf::from(&root).join(sub);
        let mut in_dir: Vec<PathBuf> = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("cannot read {}: {}", dir.display(), e))
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|p| p.extension().is_some_and(|ext| ext == "metta"))
            .filter(|p| !NOT_SANDBOX_RUNNABLE.contains(&file_label(p).as_str()))
            .collect();
        in_dir.sort();
        files.extend(in_dir);
    }

    files
}

/// The verdict of a PLN test, read from its own `is ... should ...` output.
enum Verdict {
    /// At least one `✅` and no `❌`.
    Pass(usize),
    /// One or more `❌`.
    Fail(usize),
    /// No assertion markers at all — the program never reached a `test`.
    None,
}

/// Applies PLN `test.sh`'s pass/fail rule to captured stdout: among the lines
/// that report an assertion (`is ... should ...`), pass iff at least one is `✅`
/// and none is `❌`.
///
/// The markers are authoritative; the process exit status is not. `petta.sh`'s
/// NORMAL mode ends by serializing the result envelope with `json_write_dict`,
/// which throws on a non-ground result (some PLN tests return terms with unbound
/// vars) and makes `petta.sh` exit non-zero *after* the assertion already
/// printed its `✅`. The canonical PeTTa runner emits results with `swrite`,
/// which tolerates that, so it exits cleanly — and the test's verdict is the same
/// `✅`/`❌` either way.
fn verdict(stdout: &str) -> Verdict {
    let assertion_lines = stdout
        .lines()
        .filter(|l| l.contains("is ") && l.contains("should "));

    let mut passes = 0usize;
    let mut fails = 0usize;
    for line in assertion_lines {
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

/// Runs a single PLN `.metta` file through `petta.sh` in the sandbox with
/// `git-import!` blocked, and returns its captured stdout.
async fn run_in_sandbox(file: &Path) -> Result<std::process::Output, String> {
    let mut cmd = Command::new(petta_script());
    cmd.arg(file)
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

#[tokio::test]
async fn test_sandboxed_petta_executes_pln_suite() {
    if should_skip_ecan_test() {
        return;
    }

    let files = pln_test_files();
    assert!(
        !files.is_empty(),
        "no PLN .metta test files found under PLN_DIR={}",
        pln_dir()
    );

    println!(
        "\n=== Sandboxed PeTTa executing the PLN (ECAN) suite: {} file(s) ===",
        files.len()
    );

    let mut failures: Vec<String> = Vec::new();

    for file in &files {
        let label = file_label(file);
        match run_in_sandbox(file).await {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                match verdict(&stdout) {
                    Verdict::Pass(n) => println!("  PASS   {label}  ({n} assertion(s))"),
                    Verdict::Fail(n) => {
                        println!("  FAIL   {label}  ({n} failing assertion(s) ❌)");
                        failures.push(format!("{label}: {n} failing assertion(s) (❌)"));
                    }
                    Verdict::None => {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        let tail: String =
                            stderr.lines().rev().take(3).collect::<Vec<_>>().join(" | ");
                        println!(
                            "  ERROR  {label}  (no ✅/❌ verdict; exit {:?}: {tail})",
                            output.status.code()
                        );
                        failures.push(format!(
                            "{label}: produced no assertion verdict (exit {:?}): {tail}",
                            output.status.code()
                        ));
                    }
                }
            }
            Err(why) => {
                println!("  ERROR  {label}  ({why})");
                failures.push(format!("{label}: {why}"));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "sandboxed PeTTa failed to execute {} of {} PLN test file(s):\n{}",
        failures.len(),
        files.len(),
        failures.join("\n")
    );
}
