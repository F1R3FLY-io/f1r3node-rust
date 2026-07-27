//! ★ **The replay-compared `error_message` must not be a function of an
//! operator's environment.**
//!
//! # The chain this file pins, hop by hop
//!
//! ```text
//!   SystemDeployTrait::extract_result            (system_deploy.rs)
//!     └─ <Output as Extractor>::unapply -> None
//!         └─ SystemDeployPlatformFailure::UnexpectedResult(Vec<Par>)
//!             └─ <SystemDeployPlatformFailure as Display>::fmt
//!                 └─ show_seq_par                (system_deploy_user_error.rs)
//!                     └─ PrettyPrinter render    ← THE SPLIT IS HERE
//!                         └─ SystemDeployUserError::error_message
//!                             └─ ProcessedSystemDeploy::Failed { error_msg }   … into the block
//!                                 └─ ReplayRuntimeOps::replay_system_deploy_internal
//!                                     └─ `expected_error == actual_error`      (replay_runtime.rs)
//! ```
//!
//! Before the split, the render step consulted `PRETTY_PRINTER_OUTPUT_TRIM_AFTER`.
//! Two validators with different settings therefore computed different
//! `error_message` bytes for the **same** failing system deploy, and the last
//! hop reported `ReplayFailure::system_deploy_error_mismatch` for a deploy that
//! had executed identically on both. An operator-tunable environment variable
//! was a consensus input, and nothing in its name, its type, or its call site
//! said so.
//!
//! # What is exercised, and what is stubbed
//!
//! Nothing is stubbed between `extract_result` and `error_message`: a real
//! [`CheckBalance`] system deploy (`Output = RhoNumber`) is handed a `Par` that
//! is a `GString`, `RhoNumber::unapply` returns `None`, and every hop above runs
//! verbatim. Only the tuple space that would have *produced* that `Par` is
//! absent, and it contributes nothing to the bytes under test.
//!
//! # Why child processes
//!
//! `PRETTY_PRINTER_OUTPUT_TRIM_AFTER` is read from the process environment, so
//! setting it inside a threaded test binary would race every other test in it.
//! The sweep re-execs **itself** once per value — the discipline
//! `rholang/tests/stack_depth_gate.rs` and
//! `pretty_printer::the_capping_call_sites_are_reproduced` already use — and the
//! child reports a machine-readable line the parent compares.
//!
//! ⚠ The parent asserts the child's libtest summary says `1 passed`. Without
//! that, a mistyped filter makes the child run **zero** tests, exit 0, and the
//! whole sweep pass while proving nothing.

use casper::rust::util::rholang::costacc::check_balance::CheckBalance;
use casper::rust::util::rholang::system_deploy::SystemDeployTrait;
use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use crypto::rust::public_key::PublicKey;
use models::rhoapi::Par;
use models::rust::utils::new_gstring_par;
use rholang::rust::interpreter::pretty_printer::PrettyPrinter;
use rspace_plus_plus::rspace::history::Either;
use shared::rust::shared::printer::Printer;

/// The environment variable under test. Spelled once.
const VAR: &str = "PRETTY_PRINTER_OUTPUT_TRIM_AFTER";

/// Operator budgets to sweep.
///
/// Every one must be **strictly below** the shorter probe's render length: the
/// operator cap panics when the budget exceeds the string (a deliberate,
/// load-bearing behaviour documented on [`Printer::cap`]), and this file is
/// about bytes, not about that panic.
const BUDGETS: [usize; 3] = [4, 40, 200];

/// A failing system deploy's result, at two sizes.
///
/// `CheckBalance` declares `Output = RhoNumber`, so a `GString` payload makes
/// `RhoNumber::unapply` return `None` — which is exactly the condition
/// `extract_result` turns into `UnexpectedResult`.
///
/// The two sizes straddle [`Printer::CONSENSUS_TRIM_AFTER`] on purpose:
///
/// * `Short` renders well under the consensus budget, so the consensus render is
///   the **untrimmed** string — byte-identical to what a node with the variable
///   unset produced before this change.
/// * `Long` renders well over it, so the consensus render is the **trimmed**
///   string. Without this case the trimming arm would never run and "identical
///   across budgets" would only have been shown for the arm that does nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Probe {
    Short,
    Long,
}

impl Probe {
    fn label(self) -> &'static str {
        match self {
            Probe::Short => "short",
            Probe::Long => "long",
        }
    }

    /// Deploy-influenced content. Fixed bytes so the render is reproducible.
    fn result(self) -> Par {
        let payload = match self {
            // Longer than every swept budget, shorter than the consensus budget.
            Probe::Short => "abcdefghij".repeat(30),
            // Comfortably past the consensus budget.
            Probe::Long => "abcdefghij".repeat(400),
        };
        new_gstring_par(payload, Vec::new(), false)
    }
}

/// The consensus string, produced by the production path and nothing else.
fn error_message(probe: Probe) -> String {
    let deploy = CheckBalance {
        pk: PublicKey::from_bytes(&[0u8; 33]),
        rand: Blake2b512Random::create_from_bytes(&[]),
    };
    match deploy.extract_result(&probe.result()) {
        Either::Left(error) => error.error_message,
        Either::Right(value) => panic!(
            "the probe was supposed to be unextractable as a RhoNumber, but it yielded {value}"
        ),
    }
}

/// The operator-facing render of the same `Par`. Present only as this file's
/// anti-vacuity control: it is the thing that MUST still vary with the budget.
fn operator_render(probe: Probe) -> String {
    PrettyPrinter::new().build_channel_string(&probe.result())
}

/// The in-process half of the sweep. `#[ignore]`d because it reads the
/// process-global operator budget; [`the_consensus_error_message_is_byte_identical_across_budgets`]
/// re-execs it once per budget with the variable set.
#[test]
#[ignore = "driven by the_consensus_error_message_is_byte_identical_across_budgets"]
fn error_message_probe_child() {
    let budget: usize = std::env::var(VAR)
        .expect("the child is run with the budget set")
        .parse()
        .expect("the budget is an integer");

    for probe in [Probe::Short, Probe::Long] {
        let consensus = error_message(probe);
        let operator = operator_render(probe);

        // ANTI-VACUITY at the source: the operator control must have been
        // TRIMMED, not merely produced. A trimmed operator render is exactly
        // `budget` bytes plus the three-byte marker; anything else means the
        // budget was at or past the render's length, the control did not move
        // with the budget, and the parent's claim 2 would be comparing three
        // copies of one string.
        assert_eq!(
            operator.len(),
            budget + "...".len(),
            "the operator control for the {} probe at {VAR}={budget} was not trimmed \
             ({operator:?}), so it cannot act as a control",
            probe.label()
        );

        println!(
            "SD-ERR budget={budget} probe={} consensus_len={} consensus_hex={} operator_hex={}",
            probe.label(),
            consensus.len(),
            hex::encode(consensus.as_bytes()),
            hex::encode(operator.as_bytes())
        );
    }
}

/// ★ **THE EVIDENCE.** For the same failing system deploy, two (here: three)
/// different `PRETTY_PRINTER_OUTPUT_TRIM_AFTER` values yield **byte-identical**
/// `error_message`.
///
/// Three claims, each paired with the check that makes it able to fail:
///
/// 1. **`error_message` is byte-identical across budgets**, for a render under
///    the consensus budget and for one over it.
/// 2. **ANTI-VACUITY.** The *operator* render of the same `Par`, taken in the
///    same child at the same budget, must **differ** across budgets. Without it,
///    claim 1 would also hold of a probe that no budget could ever affect, and
///    reverting the split would not turn this file red.
/// 3. **The trimming arm actually ran.** The long probe's `error_message` must
///    be shorter than the short-probe-scaled render and must end in the trim
///    marker, so "identical" is not shown only for the arm that returns its
///    input unchanged.
#[test]
fn the_consensus_error_message_is_byte_identical_across_budgets() {
    if std::env::var(VAR).is_ok() {
        // Already inside somebody's sweep; do not recurse.
        return;
    }

    let exe = std::env::current_exe().expect("current_exe");
    let name = "error_message_probe_child";

    // budget → probe label → (consensus hex, operator hex)
    let mut observed: Vec<(usize, Probe, String, String)> = Vec::with_capacity(BUDGETS.len() * 2);

    for budget in BUDGETS {
        let output = std::process::Command::new(&exe)
            .args(["--exact", name, "--nocapture", "--ignored"])
            .env(VAR, budget.to_string())
            .output()
            .expect("failed to re-exec the error-message child");
        let text = String::from_utf8_lossy(&output.stdout).into_owned()
            + &String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "the error-message child failed at {VAR}={budget}:\n{text}"
        );
        assert!(
            text.contains("1 passed"),
            "the error-message child ran the wrong number of tests at {VAR}={budget} — the \
             filter `{name}` matched nothing, so this test proved NOTHING:\n{text}"
        );

        for probe in [Probe::Short, Probe::Long] {
            let line = text
                .lines()
                .find(|l| {
                    l.starts_with("SD-ERR") && l.contains(&format!("probe={}", probe.label()))
                })
                .unwrap_or_else(|| {
                    panic!(
                        "the child printed no SD-ERR line for the {} probe at {VAR}={budget}:\n\
                         {text}",
                        probe.label()
                    )
                });
            observed.push((
                budget,
                probe,
                field(line, "consensus_hex="),
                field(line, "operator_hex="),
            ));
        }
    }

    for probe in [Probe::Short, Probe::Long] {
        let rows: Vec<&(usize, Probe, String, String)> =
            observed.iter().filter(|(_, p, _, _)| *p == probe).collect();
        assert_eq!(
            rows.len(),
            BUDGETS.len(),
            "one row per budget for each probe"
        );

        // ── claim 1 ──
        let baseline = &rows[0].2;
        assert!(
            rows.iter()
                .all(|(_, _, consensus, _)| consensus == baseline),
            "★ THE CONSENSUS `error_message` CHANGED WITH {VAR} for the {} probe. Two validators \
             with different settings compute different bytes for the same failing system deploy, \
             and replay compares those bytes:\n{}",
            probe.label(),
            rows.iter()
                .map(|(budget, _, consensus, _)| format!("  {VAR}={budget}: {consensus}"))
                .collect::<Vec<_>>()
                .join("\n")
        );

        // ── claim 2 (anti-vacuity) ──
        let control = &rows[0].3;
        assert!(
            rows.iter().any(|(_, _, _, operator)| operator != control),
            "the OPERATOR render of the {} probe did not change across {BUDGETS:?} either, so \
             this probe cannot tell a split renderer from an unsplit one and claim 1 is \
             vacuous:\n{}",
            probe.label(),
            rows.iter()
                .map(|(budget, _, _, operator)| format!("  {VAR}={budget}: {operator}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    // ── claim 3: the consensus TRIMMING arm ran, and only for the long probe ──
    let bytes_of = |probe: Probe| -> Vec<u8> {
        let (_, _, hex_bytes, _) = observed
            .iter()
            .find(|(_, p, _, _)| *p == probe)
            .expect("both probes were observed");
        hex::decode(hex_bytes).expect("the child emits hex")
    };
    let short = String::from_utf8(bytes_of(Probe::Short)).expect("utf-8");
    let long = String::from_utf8(bytes_of(Probe::Long)).expect("utf-8");

    assert!(
        !short.ends_with("..."),
        "the short probe was trimmed, so it is not exercising the untrimmed arm: {short}"
    );
    assert!(
        long.ends_with("..."),
        "the long probe was NOT trimmed, so the consensus budget's trimming arm never ran and \
         claim 1 was only shown for the arm that returns its input unchanged: len {}",
        long.len()
    );
    assert!(
        long.len() <= Printer::CONSENSUS_TRIM_AFTER + "...".len() + PREFIX_ALLOWANCE,
        "the trimmed consensus render is {} bytes, past the {} - byte budget plus its prefix",
        long.len(),
        Printer::CONSENSUS_TRIM_AFTER
    );
}

/// `error_message` is `"Platform failure: "` plus the `Display` of the failure,
/// which itself prefixes `"Unable to proceed with "`. The budget applies to the
/// *render*, not to the assembled message, so the assembled message may exceed
/// the budget by those prefixes. 64 bytes covers both with room to spare.
const PREFIX_ALLOWANCE: usize = 64;

/// Read `name=<value>` out of a child summary line.
fn field(line: &str, name: &str) -> String {
    line.split_once(name)
        .unwrap_or_else(|| panic!("no `{name}` in `{line}`"))
        .1
        .split_whitespace()
        .next()
        .unwrap_or_else(|| panic!("no value after `{name}` in `{line}`"))
        .to_string()
}
