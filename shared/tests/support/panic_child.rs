//! # Run a test's subject in a child process, and decide on the panic it reports
//!
//! Added by the merge of feature/cost-accounted-rho (plan step 1.9, panic-expectation
//! policy test). `shared/tests/panic_expectation_gate.rs` bans the test attribute that
//! expects a panic, and it bans an unwind interceptor in a test that asserts a panic. Its
//! third replacement pattern runs the subject in a child process and decides on the exit
//! status and the stderr of that child. This file holds that pattern once, so that each
//! rewritten test needs only a few lines. The shape follows
//! `rspace++/libs/rspace_rhotypes/tests/ffi_absent_required_child.rs`.
//!
//! ## How a test uses it
//!
//! ```text
//! let child = PanicChild::new(module_path!(), "the_test_name");
//! if !child.is_child() {
//!     child.expect_panic("the expected text");
//!     return;
//! }
//! <setup>
//! child.subject_reached();
//! <the statement that must panic>
//! ```
//!
//! The parent runs the same test binary again with the libtest filter `--exact` on the
//! name of this test. It sets [`CHILD_VARIABLE`] to `<libtest name>#<case>`. The child
//! takes the other branch. It prints a marker line when it reaches its subject, and then
//! it runs the subject.
//!
//! ## What the parent requires
//!
//! 1. The child ran exactly one test. A name that resolves to no test is refused.
//! 2. The child did not exit successfully.
//! 3. The child printed the marker. A panic in the setup does not count.
//! 4. A panic after the marker contains the expected text, as the libtest check of an
//!    expected panic message does. A location expectation instead requires that the
//!    location of that panic start with the given text.
//!
//! A child never starts another child.
//!
//! ## Where it is compiled
//!
//! Each crate that uses it includes this file once with `#[path]`. A library includes it
//! under `#[cfg(test)]`. The file uses `std` only, so no crate gains a dependency.

#![allow(
    dead_code,
    reason = "each crate that includes this file uses a different part of it"
)]

use std::fmt::Display;
use std::process::{Command, Output, Stdio};

/// The variable that puts a re-executed test binary into child mode. Its value is
/// `<libtest name>#<case>`.
pub const CHILD_VARIABLE: &str = "F1R3_PANIC_CHILD";

/// The case of [`PanicChild::expect_panic`].
pub const DEFAULT_CASE: &str = "default";

const MARKER: &str = "PANIC-CHILD subject_reached";
const REPORT: &str = "PANIC-CHILD report ";
const OUTPUT_LIMIT: usize = 64 * 1024;

/// What a panic after the marker must show.
#[derive(Clone, Copy, Debug)]
pub enum Expected<'a> {
    /// The panic message contains this text.
    Message(&'a str),
    /// The panic location, `file:line:column`, starts with this text.
    Location(&'a str),
}

impl Expected<'_> {
    fn text(&self) -> &str {
        match self {
            Expected::Message(text) | Expected::Location(text) => text,
        }
    }

    fn matches(&self, block: &PanicBlock) -> bool {
        match self {
            Expected::Message(text) => block.message.contains(text),
            Expected::Location(prefix) => block.location.starts_with(prefix),
        }
    }
}

/// One panic that the panic hook of the child printed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanicBlock {
    pub location: String,
    pub message: String,
}

/// The evidence for an accepted child.
#[derive(Debug)]
pub struct PanicReport {
    pub status: String,
    pub stdout: String,
    pub stderr: String,
    /// The first panic after the marker that meets the expectation.
    pub panic: PanicBlock,
    /// Every panic after the marker, in order.
    pub blocks: Vec<PanicBlock>,
    reports: Vec<(String, String)>,
}

impl PanicReport {
    pub fn message(&self) -> &str { &self.panic.message }

    pub fn location(&self) -> &str { &self.panic.location }

    /// The last value that the child wrote under `key` with [`PanicChild::report`].
    pub fn reported(&self, key: &str) -> Option<&str> {
        self.reports
            .iter()
            .rev()
            .find(|(reported, _)| reported == key)
            .map(|(_, value)| value.as_str())
    }
}

/// One test that runs its subject in a child process.
#[derive(Clone, Debug)]
pub struct PanicChild {
    name: String,
}

impl PanicChild {
    /// `module_path` is `module_path!()` at the test, and `function` is the name of the
    /// test function. libtest names a test by its module path without the crate name.
    pub fn new(module_path: &str, function: &str) -> PanicChild {
        let name = match module_path.split_once("::") {
            Some((_, rest)) => format!("{rest}::{function}"),
            None => function.to_string(),
        };
        PanicChild { name }
    }

    pub fn name(&self) -> &str { &self.name }

    /// The case that this process runs as a child of this test, or `None` in the parent.
    #[track_caller]
    pub fn case(&self) -> Option<String> {
        match child_case(&self.name, std::env::var(CHILD_VARIABLE).ok().as_deref()) {
            Ok(case) => case,
            Err(reason) => panic!("{reason}"),
        }
    }

    #[track_caller]
    pub fn is_child(&self) -> bool { self.case().is_some() }

    #[track_caller]
    pub fn is_case(&self, case: &str) -> bool { self.case().as_deref() == Some(case) }

    /// Prints the marker. Call it immediately before the first statement that can reach
    /// the expected panic.
    #[track_caller]
    pub fn subject_reached(&self) {
        let Some(case) = self.case() else {
            panic!("`subject_reached` runs only in a child of `{}`", self.name);
        };
        eprintln!("{MARKER} test={} case={case}", self.name);
    }

    /// Writes `key=value` to stderr for the parent, for example from a destructor that
    /// runs during the unwind. A newline in the value is escaped.
    #[track_caller]
    pub fn report(&self, key: &str, value: impl Display) {
        if self.case().is_none() {
            panic!("`report` runs only in a child of `{}`", self.name);
        }
        let value = value.to_string().replace('\\', "\\\\").replace('\n', "\\n");
        eprintln!("{REPORT}{key}={value}");
    }

    /// Runs the child for `case` and returns the evidence, or the reason for a refusal.
    pub fn try_expect_panic_in_case(
        &self,
        case: &str,
        expected: Expected<'_>,
    ) -> Result<PanicReport, String> {
        if let Ok(value) = std::env::var(CHILD_VARIABLE) {
            return Err(format!(
                "a child never starts another child, and this process is the child `{value}`"
            ));
        }
        if case.is_empty() || case.contains('#') {
            return Err(format!("the case `{case}` is empty or contains `#`"));
        }
        if expected.text().is_empty() {
            return Err("the expectation is empty, so every panic would meet it".to_string());
        }
        let executable = std::env::current_exe()
            .map_err(|error| format!("the test binary cannot name its own path: {error}"))?;
        let output = Command::new(&executable)
            .args([
                self.name.as_str(),
                "--exact",
                "--include-ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD_VARIABLE, format!("{}#{case}", self.name))
            .env("RUST_BACKTRACE", "0")
            .stdin(Stdio::null())
            .output()
            .map_err(|error| format!("the child of `{}` did not start: {error}", self.name))?;
        verdict(&self.name, case, &output, expected).map_err(|reason| {
            format!(
                "the child of `{}` (case `{case}`) is refused: {reason}.\n\
                 repeat it with: {CHILD_VARIABLE}='{}#{case}' {} '{}' --exact \
                 --include-ignored --nocapture --test-threads=1\n\
                 status: {}\n--- child stdout ---\n{}\n--- child stderr ---\n{}",
                self.name,
                self.name,
                executable.display(),
                self.name,
                output.status,
                tail(&String::from_utf8_lossy(&output.stdout)),
                tail(&String::from_utf8_lossy(&output.stderr)),
            )
        })
    }

    /// Requires that the child of the default case panic with `message`.
    #[track_caller]
    pub fn expect_panic(&self, message: &str) -> PanicReport {
        match self.try_expect_panic_in_case(DEFAULT_CASE, Expected::Message(message)) {
            Ok(report) => report,
            Err(reason) => panic!("{reason}"),
        }
    }

    /// Requires that the child of `case` panic with `message`.
    #[track_caller]
    pub fn expect_panic_in_case(&self, case: &str, message: &str) -> PanicReport {
        match self.try_expect_panic_in_case(case, Expected::Message(message)) {
            Ok(report) => report,
            Err(reason) => panic!("{reason}"),
        }
    }

    /// Requires that the child of `case` panic at a location that starts with `location`.
    #[track_caller]
    pub fn expect_panic_at_in_case(&self, case: &str, location: &str) -> PanicReport {
        match self.try_expect_panic_in_case(case, Expected::Location(location)) {
            Ok(report) => report,
            Err(reason) => panic!("{reason}"),
        }
    }
}

/// The case in `value` when it names the test `name`. A value that names another test, or
/// that has no case, is an error, because the libtest filter `--exact` runs only one test.
fn child_case(name: &str, value: Option<&str>) -> Result<Option<String>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    match value.rsplit_once('#') {
        Some((test, case)) if test == name && !case.is_empty() => Ok(Some(case.to_string())),
        Some((test, _)) if test != name => Err(format!(
            "this process is the child `{value}`, and the test `{name}` runs in it"
        )),
        _ => Err(format!(
            "{CHILD_VARIABLE}=`{value}` names no case of `{name}`"
        )),
    }
}

/// The decision of the parent on one finished child.
pub fn verdict(
    name: &str,
    case: &str,
    output: &Output,
    expected: Expected<'_>,
) -> Result<PanicReport, String> {
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if !stdout.lines().any(|line| line.trim() == "running 1 test") {
        return Err(
            "the child did not run exactly one test, so the name resolves to no test".to_string(),
        );
    }
    if output.status.success() {
        return Err("the subject returned without a panic".to_string());
    }
    let marker = format!("{MARKER} test={name} case={case}");
    let lines: Vec<&str> = stderr.lines().collect();
    let Some(start) = lines.iter().position(|line| *line == marker) else {
        return Err(
            "the child never reached its subject, and a panic in the setup does not count"
                .to_string(),
        );
    };
    let after = &lines[start + 1..];
    let blocks = panic_blocks_in(after);
    if blocks.is_empty() {
        return Err(
            "the child failed without a panic after its subject started: an abort, a signal \
             or an exit"
                .to_string(),
        );
    }
    let Some(panic) = blocks.iter().find(|block| expected.matches(block)).cloned() else {
        return Err(format!("no panic after the subject meets {expected:?}"));
    };
    let reports = after
        .iter()
        .filter_map(|line| line.strip_prefix(REPORT))
        .filter_map(|report| report.split_once('='))
        .map(|(key, value)| (key.to_string(), unescape(value)))
        .collect();
    Ok(PanicReport {
        status: output.status.to_string(),
        stdout,
        stderr,
        panic,
        blocks,
        reports,
    })
}

/// Every panic that a panic hook printed to `stderr`.
pub fn panic_blocks(stderr: &str) -> Vec<PanicBlock> {
    panic_blocks_in(&stderr.lines().collect::<Vec<_>>())
}

fn panic_blocks_in(lines: &[&str]) -> Vec<PanicBlock> {
    let mut blocks = Vec::new();
    let mut current: Option<(String, Vec<&str>)> = None;
    for line in lines {
        let header = panic_header(line);
        if header.is_some() || ends_a_message(line) {
            if let Some((location, message)) = current.take() {
                blocks.push(PanicBlock {
                    location,
                    message: message.join("\n"),
                });
            }
            current = header.map(|location| (location, Vec::new()));
            continue;
        }
        if let Some((_, message)) = current.as_mut() {
            message.push(line);
        }
    }
    if let Some((location, message)) = current {
        blocks.push(PanicBlock {
            location,
            message: message.join("\n"),
        });
    }
    blocks
}

/// The location in a header such as `thread 'main' panicked at src/a.rs:1:2:`, or in the
/// form with a thread id, `thread 'main' (42) panicked at src/a.rs:1:2:`.
fn panic_header(line: &str) -> Option<String> {
    if !line.starts_with("thread '") {
        return None;
    }
    let (_, location) = line.split_once(" panicked at ")?;
    Some(location.strip_suffix(':').unwrap_or(location).to_string())
}

fn ends_a_message(line: &str) -> bool {
    line.starts_with("note: run with `RUST_BACKTRACE")
        || line.starts_with("note: Some details are omitted")
        || line == "stack backtrace:"
        || line.starts_with("PANIC-CHILD ")
}

fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('n') => out.push('\n'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            },
            other => out.push(other),
        }
    }
    out
}

fn tail(text: &str) -> &str {
    if text.len() <= OUTPUT_LIMIT {
        return text;
    }
    let mut start = text.len() - OUTPUT_LIMIT;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}
