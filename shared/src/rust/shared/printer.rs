// See shared/src/main/scala/coop/rchain/shared/Printer.scala

use std::env;

/// Who a render is FOR — and therefore whose budget governs how much of it
/// survives.
///
/// # Why this is a type and not a verbosity flag
///
/// The two audiences are not two levels of detail. They differ in **who is
/// allowed to influence the bytes**:
///
/// | audience | budget source | who can change it | where the bytes go |
/// |---|---|---|---|
/// | [`Audience::Operator`] | `PRETTY_PRINTER_OUTPUT_TRIM_AFTER` | whoever starts the process | logs, stdout, the REPL, `rnode eval` |
/// | [`Audience::Consensus`] | [`Printer::CONSENSUS_TRIM_AFTER`] | a recompile, i.e. a protocol version | a block, and then a replay comparison |
///
/// A render that reaches a block must be a function of the block's contents and
/// of nothing else. `SystemDeployPlatformFailure::UnexpectedResult` renders a
/// `Par` into `SystemDeployUserError::error_message`; that string is written
/// into the block as `ProcessedSystemDeploy::Failed { error_msg }`; and
/// `ReplayRuntimeOps::replay_system_deploy_internal` compares it for byte
/// equality against the string the *replaying* validator computes. If the
/// proposer and the replayer disagree about the budget they disagree about the
/// bytes, and replay reports `ReplayFailure::system_deploy_error_mismatch` for
/// a deploy that executed identically on both. An environment variable in that
/// path is a consensus input wearing the costume of a debug knob.
///
/// Naming the audience at the call site is what makes the distinction
/// reviewable: a reader of `show_seq_par` can see that it asked for the
/// consensus budget, and a reader of `rho:io:stdout` can see that it did not.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Audience {
    /// A human reading one machine's output. The operator chose the budget, and
    /// only that machine's diagnostics are affected by the choice.
    Operator,
    /// Bytes that enter a block and are compared, byte for byte, by every
    /// validator that replays it.
    Consensus,
}

pub struct Printer;

impl Printer {
    /// The consensus render budget, in bytes, as a compile-time constant.
    ///
    /// # Choosing the number
    ///
    /// It trades two things against each other, and both directions matter:
    ///
    /// * **Larger is more discriminating.** `error_msg` is compared for
    ///   equality by replay. Two *different* failing results whose renders share
    ///   a prefix this long collapse to the same string, and replay then accepts
    ///   a mismatch it should have caught. The budget is the length at which
    ///   replay stops being able to tell two failures apart.
    /// * **Smaller bounds the block.** The string is written into the block, so
    ///   an unbounded render is an unbounded contribution to block size from a
    ///   single failing system deploy.
    ///
    /// 1 KiB sits far above the traffic and far below anything that would bloat
    /// a block. The values this path actually produces are renders of
    /// system-deploy *return values* — an `i64`, a boolean, a short tuple — and
    /// the longest fixed string anywhere on the path is the 81-byte
    /// `<unprintable: …>` fallback. 1 KiB is over twelve times that, so on real
    /// traffic the consensus render is byte-identical to an untrimmed one.
    ///
    /// ⚠ **This constant is part of the protocol.** Changing it changes the
    /// bytes every validator computes for a sufficiently long failing render,
    /// hence whether replay agrees between a node holding the old value and a
    /// node holding the new one. f1r3node has no activation-height machinery —
    /// `Validate::version` is exact equality against the *approved block's*
    /// version — so a change here ships as a coordinated upgrade to a new
    /// genesis-anchored protocol version, never as a rolling deployment.
    pub const CONSENSUS_TRIM_AFTER: usize = 1024;

    /// The **operator** budget, read from `PRETTY_PRINTER_OUTPUT_TRIM_AFTER`.
    ///
    /// `None` — the default, and the value on any node that has not set the
    /// variable — means "do not trim". A negative or unparseable value is also
    /// `None`, so a typo silently disables trimming rather than failing.
    ///
    /// ⚠ Must not be consulted for anything a block will carry. Use
    /// [`Printer::cap`] with [`Audience::Consensus`] there; that arm cannot
    /// reach this function.
    pub fn output_capped() -> Option<i32> {
        match env::var("PRETTY_PRINTER_OUTPUT_TRIM_AFTER") {
            Ok(value) => match value.parse::<i32>() {
                Ok(n) if n >= 0 => Some(n),

                _ => None,
            },

            Err(_) => None,
        }
    }

    /// Trim `rendered` to `audience`'s budget, appending `...` when a budget
    /// bites.
    ///
    /// # The two audiences do not share a truncation discipline, deliberately
    ///
    /// [`Audience::Operator`] reproduces the historic behaviour **including its
    /// panic when the budget exceeds the string's length**. That panic is not an
    /// accident that survived: it is the observable that
    /// `pretty_printer::the_capping_call_sites_are_reproduced` uses to decide
    /// *where* the printer caps. That test renders probes shorter than every
    /// budget it sweeps and requires the explicit driver and the recursive
    /// oracle to agree on whether each one panicked; replacing the panic with a
    /// `min` would make every probe succeed and the differential would no longer
    /// locate a single call site. Its own anti-vacuity check
    /// (`total_ok > 0 && total_panics > 0`) fails loudly if that happens, and it
    /// is the reason this function keeps a panic it would otherwise have no
    /// reason to keep.
    ///
    /// [`Audience::Consensus`] is **total**. It runs on a validator over a value
    /// a deploy can influence, and a panic there is a node crash a deploy author
    /// can trigger. A string shorter than the budget comes back whole with no
    /// marker — exactly what an untrimmed operator render produces — so a
    /// consensus render is byte-identical to the historic default
    /// (`PRETTY_PRINTER_OUTPUT_TRIM_AFTER` unset) for every render up to
    /// [`Printer::CONSENSUS_TRIM_AFTER`] bytes.
    ///
    /// # The char-boundary defect, and why flooring is the whole fix
    ///
    /// The historic body was `format!("{}...", &str[..n])`. `str` indexing
    /// panics for **two** distinct reasons — `n` past the end, and `n` in the
    /// middle of a multi-byte UTF-8 scalar — and only the first was intended.
    /// Both spellings of the same four characters reproduce it, at different
    /// offsets:
    ///
    /// | subject | bytes | budgets that panic |
    /// |---|---|---|
    /// | `"abcé"` precomposed (`é` = `U+00E9`) | 5 | 4 (boundary), ≥ 6 (range) |
    /// | `"abce\u{301}"` decomposed (`e` + combining acute) | 6 | 5 (boundary), ≥ 7 (range) |
    ///
    /// The content is deploy-controlled — a `GString` reaching `rho:io:stdout`
    /// is rendered by this path — and the offset is operator-configured, so the
    /// pair picks the crash.
    ///
    /// Flooring `n` to the nearest char boundary at or below it removes exactly
    /// that panic and nothing else: the walk runs only when `n` is already known
    /// to be in range, so the out-of-range panic is reached by the same inputs,
    /// with the same message, as before. At most three steps are needed, since a
    /// UTF-8 scalar occupies at most four bytes.
    pub fn cap(audience: Audience, rendered: &str) -> String {
        match audience {
            Audience::Operator => match Self::output_capped() {
                // ⚠ `n` may exceed `rendered.len()`, and the slice then panics.
                // LOAD-BEARING — see this function's documentation.
                Some(n) => {
                    let end = floor_boundary_in_range(rendered, n as usize);
                    format!("{}...", &rendered[..end])
                }

                None => rendered.to_string(),
            },

            Audience::Consensus => match rendered.len() > Self::CONSENSUS_TRIM_AFTER {
                true => {
                    let end = floor_char_boundary(rendered, Self::CONSENSUS_TRIM_AFTER);
                    format!("{}...", &rendered[..end])
                }

                false => rendered.to_string(),
            },
        }
    }
}

/// The greatest `b <= index` that is a UTF-8 scalar boundary of `s`.
///
/// `index` must satisfy `index <= s.len()`. `0` is always a boundary, so the
/// walk terminates; it takes at most three steps because a UTF-8 scalar occupies
/// at most four bytes.
///
/// (`str::floor_char_boundary` is the standard library's name for this and is
/// still unstable — <https://github.com/rust-lang/rust/issues/93743> — so it is
/// spelled out here.)
fn floor_char_boundary(s: &str, index: usize) -> usize {
    let mut boundary = index;
    while !s.is_char_boundary(boundary) {
        boundary -= 1;
    }
    boundary
}

/// [`floor_char_boundary`] when `index` is in range, and `index` untouched when
/// it is not — so the caller's slice still panics for an out-of-range budget,
/// with the same message and on the same inputs as before this function existed.
fn floor_boundary_in_range(s: &str, index: usize) -> usize {
    match index <= s.len() {
        true => floor_char_boundary(s, index),
        false => index,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The environment variable both audiences are defined against. Spelled once.
    const VAR: &str = "PRETTY_PRINTER_OUTPUT_TRIM_AFTER";

    /// The reproduction from the defect report, both spellings, every index.
    ///
    /// Expected prefixes are written out rather than derived, so a change to the
    /// flooring rule shows up as a wrong string instead of a silently-agreeing
    /// re-derivation. Each table is followed by an anti-vacuity check that the
    /// historic expression genuinely panics at the index the table calls the
    /// defect — without it, the row proves nothing.
    #[test]
    fn the_operator_cap_no_longer_splits_a_utf8_scalar() {
        // ── precomposed: `é` is U+00E9, two bytes, boundary missing at 4 ──
        const PRECOMPOSED: &str = "abcé";
        assert_eq!(PRECOMPOSED.len(), 5, "precomposed `é` is two bytes");
        for (index, prefix) in [
            (0usize, ""),
            (1, "a"),
            (2, "ab"),
            (3, "abc"),
            (4, "abc"), // ★ THE DEFECT: 4 splits `é`.
            (5, "abcé"),
        ] {
            assert_eq!(
                &PRECOMPOSED[..floor_boundary_in_range(PRECOMPOSED, index)],
                prefix,
                "flooring {index} into {PRECOMPOSED:?}"
            );
        }
        // ⚠ Formerly `catch_unwind(|| PRECOMPOSED[..4].to_string()).is_err()`. The
        // property is `4` not being a scalar boundary — which is EXACTLY what makes
        // `&PRECOMPOSED[..4]` panic, and is decidable without provoking one.
        // Strictly MORE discriminating: `.is_err()` was also satisfied by a panic
        // from anywhere else inside the closure (an allocation failure in
        // `to_string`, say), whereas `is_char_boundary` answers the question the row
        // is about and nothing else.
        assert!(
            !PRECOMPOSED.is_char_boundary(4),
            "byte 4 of `\"abcé\"` IS a scalar boundary, so `&…[..4]` would not have \
             panicked and this table is not exercising the defect"
        );

        // ── decomposed: `e` + U+0301, three bytes for the pair, gap at 5 ──
        // This is the spelling in the defect report: n=4 Ok, n=5 PANIC, n=6 Ok.
        const DECOMPOSED: &str = "abce\u{301}";
        assert_eq!(DECOMPOSED.len(), 6, "decomposed `é` is `e` plus two bytes");
        for (index, prefix) in [
            (0usize, ""),
            (1, "a"),
            (2, "ab"),
            (3, "abc"),
            (4, "abce"),
            (5, "abce"), // ★ THE DEFECT: 5 splits the combining acute.
            (6, "abce\u{301}"),
        ] {
            assert_eq!(
                &DECOMPOSED[..floor_boundary_in_range(DECOMPOSED, index)],
                prefix,
                "flooring {index} into {DECOMPOSED:?}"
            );
        }
        assert!(
            !DECOMPOSED.is_char_boundary(5),
            "byte 5 of `\"abce\\u{{301}}\"` IS a scalar boundary, so `&…[..5]` would not \
             have panicked and this table is not exercising the defect"
        );
        // …and the neighbours ARE boundaries, so the check above is discriminating
        // rather than a predicate that answers `false` everywhere.
        assert!(DECOMPOSED.is_char_boundary(4) && DECOMPOSED.is_char_boundary(6));

        // Out of range is NOT flooring's business and must stay a panic.
        assert_eq!(
            floor_boundary_in_range(PRECOMPOSED, 99),
            99,
            "an out-of-range budget is handed back untouched so the caller's slice still panics"
        );
    }

    /// The in-process half of the sweep. `#[ignore]`d because it reads the
    /// process-global operator budget and would otherwise race every other test
    /// in this binary; [`the_split_holds_across_operator_budgets`] re-execs it
    /// once per budget with the variable set.
    ///
    /// ⚠ This child runs only the probes that must **survive**. The short probe —
    /// the one whose panic is load-bearing — moved to [`short_probe_child`], because
    /// observing it here required `catch_unwind`, i.e. expecting a panic. Every
    /// `catch_unwind` below was replaced by a direct call: if one of these DOES panic
    /// the child dies, and the parent's `status.success()` + `1 passed` assertions
    /// report it with the child's own stderr attached — a louder and more specific
    /// signal than the boolean it used to fold the panic into.
    #[test]
    #[ignore = "driven by the_split_holds_across_operator_budgets in a child process"]
    fn audience_probe_child() {
        let budget: usize = std::env::var(VAR)
            .expect("the child is run with the budget set")
            .parse()
            .expect("the budget is an integer");

        // Longer than the budget AND multi-byte AT THE CUT → must NOT panic.
        //
        // ★ `€` is THREE bytes, deliberately. With the two-byte `é` an even
        // budget lands exactly on a scalar boundary, flooring would do nothing,
        // and `long_panicked=false` would be true for a reason that has nothing
        // to do with the fix. Three bytes make every budget that is not a
        // multiple of three land inside a scalar, and the assertion below
        // refuses to report on a probe where it does not.
        let multibyte = "€".repeat(budget);
        assert!(
            multibyte.len() > budget,
            "the long probe must exceed the budget"
        );
        assert!(
            !multibyte.is_char_boundary(budget),
            "ANTI-VACUITY: byte {budget} of the multi-byte probe IS a char boundary, so this \
             probe cannot decide whether the char-boundary panic was removed"
        );
        // Reaching the next line at all IS `long_panicked=false`.
        let _long = Printer::cap(Audience::Operator, &multibyte);
        let long_panicked = false;

        // THE SUBJECT OF THE SPLIT: one fixed probe, rendered for each audience.
        // The parent compares these across budgets.
        let subject = "0123456789".repeat(8); // 80 bytes: over every budget, under 1 KiB
        // The subject is longer than every swept budget, so the operator cap is
        // total on it; the consensus cap is total on everything.
        let operator = Printer::cap(Audience::Operator, &subject);
        let consensus = Printer::cap(Audience::Consensus, &subject);

        println!(
            "CAP-AUDIENCE budget={budget} long_panicked={long_panicked} \
             operator_hex={} consensus_hex={}",
            hex::encode(operator.as_bytes()),
            hex::encode(consensus.as_bytes())
        );
    }

    /// The half of the sweep that must **die**: the operator cap on a string
    /// SHORTER than the budget.
    ///
    /// That panic is load-bearing — `pretty_printer::the_capping_call_sites_are_reproduced`
    /// locates the printer's capping call sites by panic disposition, and a `min`-shaped
    /// repair would make every probe succeed and the differential locate nothing. See
    /// [`Printer::cap`]'s documentation.
    ///
    /// ⚠ It used to be observed by `catch_unwind` inside [`audience_probe_child`],
    /// folded into a `short_panicked=` boolean. That is a test expecting a panic, and
    /// `catch_unwind` is not a reliable way to intercept one — this crate is reachable
    /// as a path dependency of the mettail workspace, whose `dev`/`test` profile uses
    /// the cranelift backend, which emits no catch pads. The panic is now observed the
    /// only way an abort can be: from OUTSIDE the process.
    ///
    /// This child is expected to FAIL. Its parent, [`the_split_holds_across_operator_budgets`],
    /// asserts it did — and that it got far enough to be failing for the right reason.
    #[test]
    #[ignore = "driven by the_split_holds_across_operator_budgets in a child process"]
    fn short_probe_child() {
        let budget: usize = std::env::var(VAR)
            .expect("the child is run with the budget set")
            .parse()
            .expect("the budget is an integer");

        // Shorter than every swept budget → the operator cap must still panic.
        let short = "ab";
        assert!(
            short.len() < budget,
            "the short probe must be under the budget"
        );
        // ★ ANTI-VACUITY, printed BEFORE the subject: the parent requires this line,
        // so a child that died on startup or on the assertion above cannot be
        // mistaken for one that died at the cap.
        println!("SHORT-PROBE budget={budget} reached_subject=true");

        let rendered = Printer::cap(Audience::Operator, short);

        // Only reachable if the length panic is gone.
        println!("SHORT-PROBE survived=true rendered={rendered:?}");
    }

    /// ★ THE UNIT-SCALE STATEMENT OF THE SPLIT, swept over three operator
    /// budgets in child processes (the budget is process-global, so a threaded
    /// test binary cannot set it without racing).
    ///
    /// Three claims, each with the anti-vacuity check that makes it able to fail:
    ///
    /// 1. **The consensus render is byte-identical across budgets.** Vacuous
    ///    unless the same probe's *operator* render differs across the same
    ///    budgets — checked, so a probe that could not show a difference is
    ///    rejected rather than passing.
    /// 2. **The load-bearing length panic survives.** Vacuous unless some probe
    ///    is shorter than the budget — the `short` probe is, at every swept
    ///    budget, asserted in the child.
    /// 3. **The char-boundary panic is gone**, and the consensus arm is total.
    ///
    /// ⚠ The parent asserts the child's libtest summary says `1 passed`. Without
    /// it, a mistyped filter makes the child run **zero** tests, exit 0, and this
    /// test pass while proving nothing — the failure mode this repository has
    /// already hit twice.
    #[test]
    fn the_split_holds_across_operator_budgets() {
        // Budgets straddling the `é` parity and all under the 80-byte subject.
        const BUDGETS: [usize; 3] = [4, 7, 41];

        if std::env::var(VAR).is_ok() {
            // We are already inside somebody's sweep; do not recurse.
            return;
        }

        let module = module_path!()
            .split_once("::")
            .map(|(_, rest)| rest)
            .expect("module_path! is crate-qualified");
        let name = format!("{module}::audience_probe_child");
        let short_name = format!("{module}::short_probe_child");
        let exe = std::env::current_exe().expect("current_exe");

        let mut consensus_renders: Vec<(usize, String)> = Vec::with_capacity(BUDGETS.len());
        let mut operator_renders: Vec<(usize, String)> = Vec::with_capacity(BUDGETS.len());

        for budget in BUDGETS {
            let output = std::process::Command::new(&exe)
                .args(["--exact", &name, "--nocapture", "--ignored"])
                .env(VAR, budget.to_string())
                .output()
                .expect("failed to re-exec the audience child");
            let text = String::from_utf8_lossy(&output.stdout).into_owned()
                + &String::from_utf8_lossy(&output.stderr);
            assert!(
                output.status.success(),
                "the audience child failed at {VAR}={budget}:\n{text}"
            );
            assert!(
                text.contains("1 passed"),
                "the audience child ran the wrong number of tests at {VAR}={budget} — the filter \
                 `{name}` matched nothing, so this test proved NOTHING:\n{text}"
            );
            let line = text
                .lines()
                .find(|l| l.starts_with("CAP-AUDIENCE"))
                .unwrap_or_else(|| panic!("the child printed no CAP-AUDIENCE line:\n{text}"));

            // ── claim 2, measured OUT OF PROCESS ──
            // The short probe's panic cannot be folded into a boolean without
            // expecting a panic, so it runs in its own child, which is required to
            // FAIL — and to have got as far as the subject before doing so.
            let short = std::process::Command::new(&exe)
                .args(["--exact", &short_name, "--nocapture", "--ignored"])
                .env(VAR, budget.to_string())
                .output()
                .expect("failed to re-exec the short probe child");
            let short_text = String::from_utf8_lossy(&short.stdout).into_owned()
                + &String::from_utf8_lossy(&short.stderr);
            assert!(
                short_text.contains(&format!("SHORT-PROBE budget={budget} reached_subject=true")),
                "the short probe child never reached the cap at {VAR}={budget} — the filter \
                 `{short_name}` matched nothing, or it died earlier, so claim 2 measured \
                 NOTHING:\n{short_text}"
            );
            assert!(
                !short_text.contains("SHORT-PROBE survived=true"),
                "★ THE LOAD-BEARING PANIC IS GONE at {VAR}={budget}. The operator cap no longer \
                 panics when the budget exceeds the string, so \
                 `the_capping_call_sites_are_reproduced` can no longer locate the printer's \
                 capping call sites by panic disposition, and a `min`-shaped repair would pass \
                 unnoticed:\n{short_text}"
            );
            assert!(
                !short.status.success(),
                "the short probe child exited cleanly at {VAR}={budget}; the operator cap's \
                 length panic is gone:\n{short_text}"
            );
            assert!(
                short_text.contains("byte index") || short_text.contains("out of range"),
                "the short probe child failed at {VAR}={budget}, but not at a string-slice \
                 range panic — so claim 2 is attributing an unrelated failure to the operator \
                 cap:\n{short_text}"
            );
            // ── claim 3 ──
            assert!(
                line.contains("long_panicked=false"),
                "the operator cap still panics on a multi-byte cut at {VAR}={budget} — the \
                 char-boundary defect is not fixed:\n{line}"
            );

            consensus_renders.push((budget, field(line, "consensus_hex=")));
            operator_renders.push((budget, field(line, "operator_hex=")));
        }

        // ── claim 1 ──
        let (_, first_consensus) = &consensus_renders[0];
        assert!(
            consensus_renders
                .iter()
                .all(|(_, hex)| hex == first_consensus),
            "the consensus render is NOT a function of the string alone — it changed with \
             {VAR}:\n{consensus_renders:#?}"
        );

        // ANTI-VACUITY for claim 1: the probe must be capable of showing a
        // difference, or "identical" is a statement about a constant.
        let (_, first_operator) = &operator_renders[0];
        assert!(
            operator_renders
                .iter()
                .any(|(_, hex)| hex != first_operator),
            "the OPERATOR render did not change across {BUDGETS:?} either, so this probe cannot \
             tell a split renderer from an unsplit one and claim 1 is vacuous:\n\
             {operator_renders:#?}"
        );
    }

    /// Read `name=<value>` out of the child's summary line.
    fn field(line: &str, name: &str) -> String {
        line.split_once(name)
            .unwrap_or_else(|| panic!("no `{name}` in `{line}`"))
            .1
            .split_whitespace()
            .next()
            .unwrap_or_else(|| panic!("no value after `{name}` in `{line}`"))
            .to_string()
    }

    /// The consensus budget's own edge, checked directly rather than through a
    /// render: at the budget nothing is trimmed and no marker appears, one byte
    /// past it the marker appears and the prefix is exactly the budget.
    #[test]
    fn the_consensus_budget_trims_only_past_its_edge() {
        let at = "x".repeat(Printer::CONSENSUS_TRIM_AFTER);
        assert_eq!(
            Printer::cap(Audience::Consensus, &at),
            at,
            "a render exactly at the budget is returned whole, with no marker"
        );

        let past = "x".repeat(Printer::CONSENSUS_TRIM_AFTER + 1);
        let trimmed = Printer::cap(Audience::Consensus, &past);
        assert_eq!(trimmed.len(), Printer::CONSENSUS_TRIM_AFTER + "...".len());
        assert!(trimmed.ends_with("..."));

        // Total on a multi-byte cut. `€` is THREE bytes so the budget lands
        // inside a scalar whenever it is not a multiple of three — asserted,
        // because with a two-byte scalar and an even budget the cut would fall
        // on a boundary and this case would decide nothing.
        let multibyte = "€".repeat(Printer::CONSENSUS_TRIM_AFTER);
        assert!(
            !multibyte.is_char_boundary(Printer::CONSENSUS_TRIM_AFTER),
            "ANTI-VACUITY: the budget lands on a char boundary of the multi-byte probe, so this \
             case cannot decide whether the consensus arm floors"
        );
        let cut = Printer::cap(Audience::Consensus, &multibyte);
        assert!(
            cut.ends_with("..."),
            "the multi-byte probe must be over the budget and therefore trimmed"
        );
        assert!(
            cut.trim_end_matches('.').chars().all(|c| c == '€'),
            "flooring must cut between scalars, never inside one: {cut:?}"
        );
    }
}
