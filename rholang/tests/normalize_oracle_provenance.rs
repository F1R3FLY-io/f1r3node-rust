//! # The normalizer oracle's citations, checked rather than merely checkable
//!
//! `rholang/src/rust/interpreter/compiler/normalize_recursive.rs` is the
//! **recursive oracle twin** of the 26-function `normalize_ann_proc` SCC. It is
//! the thing the converted machine is differentiated against, so the whole
//! evidential value of `compiler::normalize_differential` — and of every Leg-2
//! result that rests on it — depends on the oracle being a *faithful copy* of
//! the pre-conversion code.
//!
//! Each block in that file carries a citation:
//!
//! ```text
//!    // FROM `normalizer/processes/p_match_normalizer.rs`, lines 13-135,
//!    // at commit 6ccf71f2 — byte-identical under the declared rename; …
//! ```
//!
//! Until this test existed those citations were claims a reader *could* verify
//! and nobody did. That is the same shape as a comment asserting a property with
//! nothing behind it, and here the stakes are higher than a comment: **if the
//! oracle silently drifts toward the machine, the differential compares the
//! machine against something that has been adjusted to agree with it, and every
//! green result becomes worthless without anyone noticing.**
//!
//! This test re-derives every cited block from git and compares it byte for
//! byte.
//!
//! ## What "byte-identical under the declared rename" means
//!
//! The oracle is not a raw copy — it could not be, because it must call
//! *itself* rather than re-entering production. [`rename_scc`] is the exact,
//! mechanical transformation that was applied, and it is spelled here as the
//! single source of truth:
//!
//! | step | why |
//! |---|---|
//! | every SCC name gains a `_recursive` suffix | so the copy is closed under its own recursion and can never re-enter the machine |
//! | `pub fn ` → `fn ` | the oracle exports nothing by accident |
//! | the two entry points regain `pub(crate)` | the differential drives `normalize_ann_proc_recursive` and `normalize_name_recursive` |
//! | intra-SCC `use …::<name>_recursive;` lines are dropped | the renamed twins are local to the merged module; the copied imports would dangle |
//!
//! Anything a maintainer changes in the oracle that is *not* one of those four
//! things makes this test red. That is the point.
//!
//! ## ⚠ It FAILS when git history is unavailable — it never skips
//!
//! A `return` on a missing git object would be the ninth check in this campaign
//! that cannot fail, two of which passed while executing nothing at all
//! (`docs/design/audits/theta-depth-traversals-2026-07-26.md` §12.5). A shallow
//! clone therefore produces a **failure with instructions**, not a silent pass.
//! CI must fetch full history (`fetch-depth: 0`).
//!
//! ## Anti-vacuity
//!
//! [`the_provenance_check_can_go_red`] perturbs one cited block by a single byte
//! in memory and requires the comparison to reject it — in both directions, so
//! a comparator that rejected *everything* would also fail.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The 26 functions of the SCC, exactly as the extraction renamed them.
///
/// ⚠ Sorted longest-first before use, so `normalize_p_send_sync` is rewritten
/// before `normalize_p_send` can claim its prefix. Getting that order wrong
/// produces `normalize_p_send_recursive_sync`, which would not compile — but
/// the ordering is stated rather than left to luck because a *new* SCC member
/// could reintroduce the hazard silently.
const SCC_NAMES: &[&str] = &[
    "normalize_ann_proc",
    "normalize_name",
    "normalize_collection",
    "normalize_p_bundle",
    "normalize_p_collect",
    "normalize_p_conjunction",
    "normalize_p_contr",
    "normalize_p_disjunction",
    "normalize_p_eval",
    "normalize_p_if",
    "normalize_p_input",
    "normalize_p_let",
    "normalize_p_match",
    "normalize_p_matches",
    "normalize_p_method",
    "normalize_p_negation",
    "normalize_p_new",
    "normalize_p_par",
    "normalize_p_send",
    "normalize_p_send_sync",
    "recognize_signed_join",
    "recognize_signed_term",
    "recognize_token_stack",
    "signature_to_ir",
    "signature_to_native_sig",
    "canon_quote",
];

/// Where the cited paths are rooted.
const SOURCE_ROOT: &str = "rholang/src/rust/interpreter/compiler/";

/// The oracle, relative to the repository root.
const ORACLE: &str = "rholang/src/rust/interpreter/compiler/normalize_recursive.rs";

/// ★ Edits to a cited block that go **beyond** [`rename_scc`], as data.
///
/// There is exactly one, and it exists because merging two source files that
/// resolved the bare name `Var` to *different* types made the annotation
/// ambiguous. It is a type annotation; no expression is touched.
///
/// Keeping this as a table rather than as a special case in the comparison has
/// a specific purpose: a maintainer cannot quietly widen a deviation, or add a
/// second one, without it appearing here — and [`no_undeclared_deviations`]
/// fails if an entry stops being needed, so the list cannot rot into a licence
/// for arbitrary drift either.
const DECLARED_DEVIATIONS: &[Deviation] = &[Deviation {
    path: "normalizer/collection_normalize_matcher.rs",
    from: "        remainder: Option<Var>,\n",
    to: "        remainder: Option<models::rhoapi::Var>,\n",
    reason: "`Var` is ambiguous once this block is merged with \
             `name_normalize_matcher.rs`: the former file resolved it to \
             `models::rhoapi::Var`, the latter to `rholang_parser::ast::Var`. \
             The annotation names the type the SOURCE file's own imports \
             resolved to, so the block's meaning is unchanged.",
}];

struct Deviation {
    path: &'static str,
    from: &'static str,
    to: &'static str,
    reason: &'static str,
}

/// One parsed citation.
#[derive(Debug)]
struct Citation {
    path: String,
    first_line: usize,
    last_line: usize,
    sha: String,
    /// Line number of the citation itself in the oracle, for failure messages.
    marker_line: usize,
    /// The block the citation introduces, as it stands in the oracle today.
    block: String,
}

// ===========================================================================
// the declared transformation
// ===========================================================================

/// `true` for the characters that may appear inside a Rust identifier, which is
/// what makes the replacement below a *word-boundary* replacement rather than a
/// substring one.
fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Replace every whole-word occurrence of `needle` with `replacement`.
///
/// Hand-rolled rather than pulled from `regex` so the transformation this test
/// pins has no dependency of its own: a checker that could be perturbed by a
/// crate upgrade is a weaker checker.
fn replace_word(haystack: &str, needle: &str, replacement: &str) -> String {
    let mut out = String::with_capacity(haystack.len());
    let bytes = haystack.as_bytes();
    let mut i = 0usize;
    while let Some(rel) = haystack[i..].find(needle) {
        let at = i + rel;
        let before_ok = at == 0
            || !is_ident_char(haystack[..at].chars().next_back().expect("non-empty prefix"));
        let after = at + needle.len();
        let after_ok = after >= bytes.len()
            || !is_ident_char(haystack[after..].chars().next().expect("non-empty suffix"));
        out.push_str(&haystack[i..at]);
        if before_ok && after_ok {
            out.push_str(replacement);
        } else {
            out.push_str(needle);
        }
        i = after;
    }
    out.push_str(&haystack[i..]);
    out
}

/// **The declared rename.** See the module docs for the four steps and why each
/// one exists.
fn rename_scc(text: &str) -> String {
    // Longest-first, so a shorter SCC name cannot claim a longer one's prefix.
    let mut names: Vec<&str> = SCC_NAMES.to_vec();
    names.sort_by_key(|n| std::cmp::Reverse(n.len()));

    let mut out = text.to_string();
    for n in names {
        out = replace_word(&out, n, &format!("{n}_recursive"));
    }
    out = out.replace("pub fn ", "fn ");
    out = out.replace(
        "fn normalize_ann_proc_recursive",
        "pub(crate) fn normalize_ann_proc_recursive",
    );
    out = out.replace(
        "fn normalize_name_recursive",
        "pub(crate) fn normalize_name_recursive",
    );
    out
        .lines()
        .filter(|l| {
            let t = l.trim();
            !(t.starts_with("use ") && t.ends_with("_recursive;") && t.contains("::"))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ===========================================================================
// reading the oracle and the history
// ===========================================================================

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the `rholang` crate always has a parent directory")
        .to_path_buf()
}

/// Parse every citation out of the oracle, together with the block it
/// introduces.
///
/// A block runs from the banner line that closes its citation to the banner
/// that opens the next one (or to end of file), with trailing blank lines
/// trimmed — exactly the shape the extraction emitted.
fn citations() -> Vec<Citation> {
    let oracle_path = repo_root().join(ORACLE);
    let text = std::fs::read_to_string(&oracle_path)
        .unwrap_or_else(|e| panic!("cannot read the oracle at {}: {e}", oracle_path.display()));
    let lines: Vec<&str> = text.lines().collect();

    // Index of every banner line, so a block's end is the banner ABOVE the next
    // citation rather than the citation itself.
    let banner = |l: &str| l.starts_with("// ===");

    let mut marker_idx: Vec<(usize, Citation)> = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let Some(rest) = l.strip_prefix("// FROM `") else {
            continue;
        };
        let Some((path, tail)) = rest.split_once("`, lines ") else {
            panic!("malformed citation at {ORACLE}:{}: {l}", i + 1);
        };
        let Some((range, tail)) = tail.split_once(", at commit ") else {
            panic!("malformed citation at {ORACLE}:{}: {l}", i + 1);
        };
        let Some((lo, hi)) = range.split_once('-') else {
            panic!("malformed line range at {ORACLE}:{}: {l}", i + 1);
        };
        let sha: String = tail.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
        assert!(
            sha.len() >= 7,
            "citation at {ORACLE}:{} names no commit: {l}",
            i + 1
        );
        marker_idx.push((
            i,
            Citation {
                path: path.to_string(),
                first_line: lo.parse().expect("citation line range is numeric"),
                last_line: hi.parse().expect("citation line range is numeric"),
                sha,
                marker_line: i + 1,
                block: String::new(),
            },
        ));
    }

    let starts: Vec<usize> = marker_idx.iter().map(|(i, _)| *i).collect();
    let mut out = Vec::with_capacity(marker_idx.len());
    for (n, (i, mut c)) in marker_idx.into_iter().enumerate() {
        // The block starts after the citation's own (possibly multi-line)
        // comment and the banner that closes it.
        let mut start = i;
        while start < lines.len() && !banner(lines[start]) {
            start += 1;
        }
        start += 1;
        // …and ends at the banner that opens the NEXT citation.
        let mut end = starts.get(n + 1).copied().unwrap_or(lines.len());
        while end > start && !banner(lines[end - 1]) && end != lines.len() {
            end -= 1;
        }
        if end > start && end != lines.len() {
            end -= 1; // drop the next block's opening banner
        }
        while end > start && lines[end - 1].trim().is_empty() {
            end -= 1;
        }
        c.block = lines[start..end].join("\n");
        out.push(c);
    }
    out
}

/// `git show <sha>:<path>` — **fails loudly** when history is unavailable.
fn git_show(sha: &str, path: &str) -> String { git_show_path(sha, &format!("{SOURCE_ROOT}{path}")) }

/// `git show <sha>:<full path>` — the repository-rooted form, for oracles whose
/// citations name a full path rather than one relative to a `SOURCE_ROOT`.
fn git_show_path(sha: &str, full_path: &str) -> String {
    let spec = format!("{sha}:{full_path}");
    let out = Command::new("git")
        .args(["-c", "core.fsmonitor=false", "show", &spec])
        .current_dir(repo_root())
        .output()
        .unwrap_or_else(|e| panic!("could not run `git show {spec}`: {e}"));

    assert!(
        out.status.success(),
        "\n★ PROVENANCE UNVERIFIABLE — `git show {spec}` failed.\n\
         \n\
         This test does NOT skip when history is missing, and that is deliberate: a check \
         that silently passes on a shallow clone is a check that cannot fail, which is the \
         defect class this campaign has now found eight times (audit §12.5).\n\
         \n\
         If this is CI, the checkout is shallow — set `fetch-depth: 0`.\n\
         If this is a local clone, run `git fetch --unshallow`.\n\
         If commit {sha} has genuinely left this repository, the oracle's citations are no \
         longer verifiable and the differential's evidential value must be re-established \
         before that is accepted.\n\
         \n\
         git said: {}\n",
        String::from_utf8_lossy(&out.stderr).trim()
    );
    String::from_utf8(out.stdout).expect("cited sources are UTF-8")
}

/// Re-derive one cited block from history: extract the lines, apply the declared
/// rename, then apply any declared deviation for that file.
fn rederive(c: &Citation) -> String {
    let source = git_show(&c.sha, &c.path);
    let lines: Vec<&str> = source.lines().collect();
    assert!(
        c.last_line <= lines.len(),
        "citation at {ORACLE}:{} claims lines {}-{} of `{}`, which has only {} lines at {}",
        c.marker_line,
        c.first_line,
        c.last_line,
        c.path,
        lines.len(),
        c.sha
    );
    assert!(
        c.first_line >= 1 && c.first_line <= c.last_line,
        "citation at {ORACLE}:{} has an inverted or zero line range {}-{}",
        c.marker_line,
        c.first_line,
        c.last_line
    );

    let cited = lines[c.first_line - 1..c.last_line].join("\n");
    let mut derived = rename_scc(&cited);
    for d in DECLARED_DEVIATIONS.iter().filter(|d| d.path == c.path) {
        derived = derived.replace(d.from, d.to);
    }
    derived
}

// ===========================================================================
// the tests
// ===========================================================================

/// ★ Every citation in the oracle is re-derived from git and compared byte for
/// byte.
#[test]
fn every_oracle_citation_matches_its_source() {
    let cites = citations();
    assert!(
        cites.len() >= 23,
        "the oracle carries {} citations; it had 23 when this check was written. Blocks are \
         not removed from an oracle without the SCC shrinking, so this is either a real \
         deletion or a parser that stopped matching the marker format.",
        cites.len()
    );

    let mut failures = Vec::new();
    for c in &cites {
        let derived = rederive(c);
        if derived.trim_end() != c.block.trim_end() {
            let d: Vec<String> = derived.lines().map(str::to_string).collect();
            let b: Vec<String> = c.block.lines().map(str::to_string).collect();
            let first = (0..d.len().max(b.len()))
                .find(|i| d.get(*i) != b.get(*i))
                .unwrap_or(0);
            failures.push(format!(
                "\n  `{}` lines {}-{} @ {} (cited at {ORACLE}:{})\n    \
                 first divergence at block line {}:\n      \
                 from git:   {:?}\n      in oracle:  {:?}",
                c.path,
                c.first_line,
                c.last_line,
                c.sha,
                c.marker_line,
                first + 1,
                d.get(first),
                b.get(first),
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "\n★ THE ORACLE HAS DRIFTED FROM ITS CITED SOURCES.\n\
         \n\
         `normalize_recursive.rs` is what the converted normalizer is differentiated \
         against. If it drifts TOWARD the machine, the differential compares the machine \
         with something adjusted to agree with it and every green result it produces is \
         worthless — silently.\n\
         \n\
         {} of {} citations do not match `git show | rename_scc`:{}\n\
         \n\
         Fix the ORACLE, not this test. If a change to the oracle is genuinely required, \
         it belongs in `DECLARED_DEVIATIONS` with a reason.\n",
        failures.len(),
        cites.len(),
        failures.join("")
    );

    println!("{} oracle citations verified against git", cites.len());
}

/// ★ **The check must be able to go red.** A guard nobody has watched fail is
/// not evidence.
///
/// One cited block is perturbed by a **single byte** — the kind of edit a
/// careless "fix" to the oracle would make — and the comparison must reject it.
/// The converse is asserted too: the unperturbed block must still match, so a
/// comparator that rejected everything would fail here rather than look strict.
#[test]
fn the_provenance_check_can_go_red() {
    let cites = citations();
    let c = cites
        .iter()
        .find(|c| c.path.ends_with("p_match_normalizer.rs"))
        .expect("the oracle cites `p_match_normalizer.rs`");
    let derived = rederive(c);

    // (a) the control — unperturbed, it matches.
    assert_eq!(
        derived.trim_end(),
        c.block.trim_end(),
        "the control arm of the anti-vacuity check does not hold: the cited block does not \
         match its source even before perturbation, so the red below would prove nothing"
    );

    // (b) a ONE-BYTE perturbation must be rejected.
    let idx = derived
        .find("bound_count")
        .expect("`p_match_normalizer.rs` mentions `bound_count`");
    let mut perturbed = derived.clone();
    perturbed.replace_range(idx..idx + 1, "B"); // `bound_count` -> `Bound_count`
    assert_ne!(
        perturbed.trim_end(),
        c.block.trim_end(),
        "★ THE PROVENANCE CHECK CANNOT GO RED. A one-byte change to a cited block compared \
         EQUAL to the oracle, so `every_oracle_citation_matches_its_source` is certifying \
         nothing and the oracle could drift toward the machine unnoticed."
    );
    assert_eq!(
        perturbed.len(),
        derived.len(),
        "the perturbation must be one byte for one byte, or it is testing length and not \
         content"
    );
}

/// ★ The deviation list must not rot into a licence for arbitrary drift.
///
/// Every entry must still be *needed*: if a declared deviation no longer applies
/// — because the source changed, or because someone reverted the oracle — the
/// entry is dead weight that quietly widens what the main test tolerates. This
/// fails when that happens, so the list can only shrink deliberately.
#[test]
fn no_undeclared_deviations() {
    let cites = citations();
    for d in DECLARED_DEVIATIONS {
        let c = cites
            .iter()
            .find(|c| c.path == d.path)
            .unwrap_or_else(|| panic!("DECLARED_DEVIATIONS names `{}`, which the oracle does not cite", d.path));
        let cited = {
            let source = git_show(&c.sha, &c.path);
            let lines: Vec<&str> = source.lines().collect();
            rename_scc(&lines[c.first_line - 1..c.last_line].join("\n"))
        };
        assert!(
            cited.contains(d.from),
            "DECLARED_DEVIATIONS entry for `{}` is STALE: the cited source no longer contains \
             {:?}, so the entry does nothing but widen what \
             `every_oracle_citation_matches_its_source` tolerates. Reason on file was: {}",
            d.path,
            d.from,
            d.reason
        );
        assert!(
            c.block.contains(d.to),
            "DECLARED_DEVIATIONS entry for `{}` claims the oracle spells it {:?}, but it does \
             not. Either the oracle was reverted — in which case delete this entry — or it \
             drifted further.",
            d.path,
            d.to
        );
    }
    println!(
        "{} declared deviation(s), all still required",
        DECLARED_DEVIATIONS.len()
    );
}

// ===========================================================================
// ★ THE SECOND ORACLE — the pretty printer's recursive twin
// ===========================================================================
//
// `rholang/src/rust/interpreter/pretty_printer_oracle.rs` is the recursive twin
// of the printer's explicit pushdown driver. Its bytes are consensus-relevant:
// the printer's output reaches a block through
// `build_channel_string` -> `cap` -> `error_message` and is compared by replay
// at `casper/src/rust/rholang/replay_runtime.rs`, so the differential that
// twin backs is not a nicety.
//
// It made the SAME kind of unbacked claim `779bf881` found in the normalizer
// oracle — "kept verbatim ... Nothing else is edited — not a format string, not
// an argument order, not a comment" — with NO commit, NO path and NO line
// range, so no reader could check it. Writing this section is what established
// the truth, and the claim was wrong the same way: **4 of the 10 functions are
// byte-identical under the declared rename; 6 carry deviations**, including
// deleted comments in three of the four public wrappers.

/// The printer oracle, relative to the repository root.
const PP_ORACLE: &str = "rholang/src/rust/interpreter/pretty_printer_oracle.rs";

/// The ten mutually-recursive entry points the extraction renamed. A name that
/// starts with `_` takes the prefix INSIDE the underscore (`_build_x` ->
/// `_oracle_build_x`), which is the convention the printer already used to mark
/// its non-capping inner forms.
const PP_NAMES: &[&str] = &[
    "build_string_from_expr",
    "build_string_from_message",
    "build_string_from_node",
    "build_channel_string",
    "_build_string_from_expr",
    "_build_channel_string",
    "_build_string_from_message",
    "build_vec",
    "build_pattern",
    "build_match_case",
];

/// ★ Every difference between a cited block and its source, beyond
/// [`pp_rename`], as DATA.
///
/// Sixteen hunks across six functions. They fall into three groups, and keeping
/// them in a table rather than as prose is what stops a seventh from appearing
/// unannounced: [`no_undeclared_pretty_printer_deviations`] fails when an entry
/// stops being needed, so the list cannot rot into a licence for drift either.
const PP_DEVIATIONS: &[Deviation] = &[
    // ── GROUP 1: comments the extraction DROPPED ──────────────────────────
    // The banner promised "not a comment". These are the counterexamples, and
    // they are why the promise had to be replaced with a measured claim. The
    // dropped text still exists — on the PRODUCTION twin of each wrapper — so
    // nothing was lost; what was wrong was saying it had not happened.
    Deviation {
        path: "rholang/src/rust/interpreter/pretty_printer.rs",
        from: "        // Instead of panicking on errors, return a fallback string\n        \
               // This matches Scala behavior where errors are handled gracefully\n",
        to: "",
        reason: "Dropped from `oracle_build_string_from_expr` and \
                 `oracle_build_channel_string`. Prose about why the production \
                 wrapper swallows errors; the oracle keeps the behaviour and not \
                 the commentary.",
    },
    Deviation {
        path: "rholang/src/rust/interpreter/pretty_printer.rs",
        from: "        // Instead of panicking on unknown types, return a fallback string\n        \
               // This matches Scala behavior where errors are handled gracefully\n",
        to: "",
        reason: "The same drop in `oracle_build_string_from_node`, whose wording \
                 differs by two words from the pair above.",
    },
    Deviation {
        path: "rholang/src/rust/interpreter/pretty_printer.rs",
        from: "            // \u{26a0} The fallback is NOT capped. That asymmetry is the pre-existing\n            \
               // behaviour and the driver's `Catch` frame reproduces it.\n",
        to: "",
        reason: "Dropped from `oracle_build_string_from_node`. The asymmetry it \
                 describes is real and is asserted by \
                 `the_capping_call_sites_are_reproduced`; only the note went.",
    },
    // ── GROUP 2: rustfmt, applied to the copy and not to the original ─────
    Deviation {
        path: "rholang/src/rust/interpreter/pretty_printer.rs",
        from: "            Err(err) => {\n                \
               // Return a fallback message instead of panicking\n                \
               format!(\"<unprintable expr: {}>\", err)\n            }",
        to: "            Err(err) => format!(\"<unprintable expr: {}>\", err),",
        reason: "Block arm collapsed to an expression arm (and its comment lost \
                 with the block) in `oracle_build_string_from_expr`. Same value, \
                 same type; a formatting difference, declared because \"verbatim\" \
                 must not quietly mean \"verbatim modulo formatting\".",
    },
    Deviation {
        path: "rholang/src/rust/interpreter/pretty_printer.rs",
        from: "            Err(err) => {\n                \
               // Return a fallback message instead of panicking\n                \
               format!(\"<unprintable channel: {}>\", err)\n            }",
        to: "            Err(err) => format!(\"<unprintable channel: {}>\", err),",
        reason: "The same collapse in `oracle_build_channel_string`.",
    },
    Deviation {
        path: "rholang/src/rust/interpreter/pretty_printer.rs",
        from: "    pub(crate) fn oracle_build_string_from_message<T: AsPpNode + ?Sized>(&mut self, m: &T) -> String {",
        to: "    pub(crate) fn oracle_build_string_from_message<T: AsPpNode + ?Sized>(\n        \
             &mut self,\n        m: &T,\n    ) -> String {",
        reason: "Signature re-wrapped across four lines: `pub fn` -> \
                 `pub(crate) fn` pushed it past the width limit. A consequence \
                 of the declared visibility narrowing, not an independent edit.",
    },
    // ── GROUP 3: the SEMANTIC deviations, each marked at its own site ─────
    Deviation {
        path: "rholang/src/rust/interpreter/pretty_printer.rs",
        from: "            let introduced_news_shift_idx: Vec<i32> =\n                \
               (0..n.bind_count).map(|i| i + self.bound_shift).collect();",
        to: "            // \u{26a0} NOT verbatim, and deliberately so: the ONE edit this body has\n            \
             // taken since it was copied. `(0..n.bind_count).map(|i| i +\n            \
             // self.bound_shift).collect()` was an unbounded `Vec<i32>`; the\n            \
             // contiguous run it built is now held as the interval itself,\n            \
             // identically to `descend_node`'s `PpNode::New`. The twin has to\n            \
             // move with the driver or the differential compares two different\n            \
             // computations.\n            \
             let introduced = self.new_bind_range(n.bind_count);",
        reason: "`New`'s introduced shift indices become an INTERVAL. The \
                 materialised form was an ~8 GiB request from an \
                 attacker-chosen `bind_count`; the interval closes that at the \
                 root AND is byte-preserving, where clamping it was not. The twin \
                 must move with the driver or the differential compares two \
                 different computations.",
    },
    Deviation {
        path: "rholang/src/rust/interpreter/pretty_printer.rs",
        from: "                self.build_variables(n.bind_count),",
        to: "                self.build_variables(introduced),",
        reason: "The same change, at the render call: `build_variables` now takes \
                 the interval so the names printed and the indices marked are one \
                 authority rather than two reads that happen to agree.",
    },
    Deviation {
        path: "rholang/src/rust/interpreter/pretty_printer.rs",
        from: "                    self.news_shift_indices = self\n                        \
               .news_shift_indices\n                        .clone()\n                        \
               .into_iter()\n                        .chain(introduced_news_shift_idx)\n                        \
               .collect();",
        to: "                    self.news_shift_indices.push(introduced);",
        reason: "The same change, at the record: one interval appended instead of \
                 a whole vector rebuilt per `New`.",
    },
    Deviation {
        path: "rholang/src/rust/interpreter/pretty_printer.rs",
        from: "        let quote_if_not_new = |s: String, news_shift_indices: Vec<i32>, bound_shift: i32| {",
        to: "        let quote_if_not_new = |s: String, news_shift_indices: &[NewBindRange], bound_shift: i32| {",
        reason: "Consequence of the interval change: `is_new_var` takes a slice, \
                 so the closure does too and the per-variable clone of the whole \
                 vector disappears.",
    },
    Deviation {
        path: "rholang/src/rust/interpreter/pretty_printer.rs",
        from: "                self.news_shift_indices.clone(),",
        to: "                &self.news_shift_indices,",
        reason: "Both call sites of that closure, for the same reason. Nothing \
                 mutates through it, so a borrow is sufficient.",
    },
    Deviation {
        path: "rholang/src/rust/interpreter/pretty_printer.rs",
        from: "                // \u{26a0} FOUND DEFECT, PRESERVED BYTE-FOR-BYTE. This was\n                \
               // `self.oracle_build_string_from_message(&m.target)` \u{2014} and `m.target`",
        to: "                // \u{26a0} NOT verbatim, and deliberately so: the ONE other edit this\n                \
             // body has taken. The copied line was\n                \
             // `self.build_string_from_message(&m.target)`, and `m.target`",
        reason: "`bd7cb45f` fixed the `Match` target defect, so the note above it \
                 changed from \"preserved\" to \"repaired\". Comment only.",
    },
    Deviation {
        path: "rholang/src/rust/interpreter/pretty_printer.rs",
        from: "                // The closed `PpNode` dispatch turns that into a type error;\n                \
               // spelling it `Unprintable` keeps the emitted bytes identical\n                \
               // while making the defect explicit and greppable.",
        to: "                // The closed `PpNode` dispatch turned that into a type error;\n                \
             // the repair keeps the SAME entry point the copied line named\n                \
             // (`*_build_string_from_message`, catching + capped + indent 0)\n                \
             // and projects the `Option` with the same `.expect` discipline\n                \
             // every other required `Option<Par>` field in this file uses.",
        reason: "The continuation of that note. Comment only.",
    },
    Deviation {
        path: "rholang/src/rust/interpreter/pretty_printer.rs",
        from: "                // \u{26a0} FIXING IT IS A SEPARATE, SEPARATELY REVIEWED CHANGE: these\n                \
               // bytes are block-resident and replay-compared\n                \
               // (`replay_runtime.rs:745-758`). See `PpNode`'s docs and\n                \
               // `a_match_target_renders_as_an_error_string_and_that_is_pinned`.\n                \
               self.oracle_build_string_from_node(PpNode::Unprintable(UNPRINTABLE_ANY)),",
        to: "                // Changed identically in `descend_node`'s `PpNode::Match`; see\n                \
             // `super::PpNode` for the byte delta and why it needed saying.\n                \
             self.oracle_build_string_from_message(\n                    \
             m.target\n                        .as_ref()\n                        \
             .expect(\"target field on Match was None, should be Some\"),\n                ),",
        reason: "THE `Match` target fix itself. Every `match` term used to render \
                 its target as an 81-byte error string; it now renders the target \
                 through the entry point the copied line named. The oracle takes \
                 the identical edit or the differential compares a fixed driver \
                 against an unfixed twin.",
    },
];

/// The declared rename for the printer oracle: the `oracle_` prefixes, plus the
/// visibility narrowing on the four wrappers.
fn pp_rename(text: &str) -> String {
    let mut names: Vec<&str> = PP_NAMES.to_vec();
    names.sort_by_key(|n| std::cmp::Reverse(n.len()));
    let mut out = text.to_string();
    for n in names {
        let renamed = if let Some(bare) = n.strip_prefix('_') {
            format!("_oracle_{bare}")
        } else {
            format!("oracle_{n}")
        };
        out = replace_word(&out, n, &renamed);
    }
    out.replace("    pub fn ", "    pub(crate) fn ")
}

/// Replace every comment and string/char literal with spaces, preserving
/// newlines, so brace counting cannot be fooled by a `{` inside a `format!`.
///
/// ⚠ A backslash-newline inside a string literal is a LINE CONTINUATION and its
/// newline must survive, or every line number after it shifts. That was a real
/// bug in the first draft of this function and it silently mis-located six of
/// the ten blocks.
fn blank_rust(src: &str) -> String {
    let c: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0usize;
    let push = |out: &mut String, ch: char| out.push(if ch == '\n' { '\n' } else { ' ' });
    while i < c.len() {
        match c[i] {
            '/' if c.get(i + 1) == Some(&'/') => {
                while i < c.len() && c[i] != '\n' {
                    out.push(' ');
                    i += 1;
                }
            }
            '/' if c.get(i + 1) == Some(&'*') => {
                let mut depth = 0usize;
                while i < c.len() {
                    if c[i] == '/' && c.get(i + 1) == Some(&'*') {
                        depth += 1;
                        out.push_str("  ");
                        i += 2;
                    } else if c[i] == '*' && c.get(i + 1) == Some(&'/') {
                        depth -= 1;
                        out.push_str("  ");
                        i += 2;
                        if depth == 0 {
                            break;
                        }
                    } else {
                        push(&mut out, c[i]);
                        i += 1;
                    }
                }
            }
            '"' => {
                out.push(' ');
                i += 1;
                while i < c.len() {
                    if c[i] == '\\' {
                        out.push(' ');
                        push(&mut out, *c.get(i + 1).unwrap_or(&' '));
                        i += 2;
                    } else if c[i] == '"' {
                        out.push(' ');
                        i += 1;
                        break;
                    } else {
                        push(&mut out, c[i]);
                        i += 1;
                    }
                }
            }
            '\'' if c.get(i + 2) == Some(&'\'') => {
                out.push_str("   ");
                i += 3;
            }
            ch => {
                out.push(ch);
                i += 1;
            }
        }
    }
    out
}

/// The lines of the function that starts at `start`, found by brace balance
/// over [`blank_rust`]ed text.
fn fn_block(raw: &[&str], blanked: &[&str], start: usize) -> String {
    let (mut depth, mut opened) = (0i64, false);
    for (offset, line) in blanked.iter().enumerate().skip(start) {
        for ch in line.chars() {
            match ch {
                '{' => {
                    depth += 1;
                    opened = true;
                }
                '}' => depth -= 1,
                _ => {}
            }
        }
        if opened && depth == 0 {
            return raw[start..=offset].join("\n");
        }
    }
    panic!("unterminated function starting at line {}", start + 1)
}

/// Parse the printer oracle's citations. Each is
/// `// VERBATIM from <path>, lines <a>-<b>, at commit <sha>`, and the block it
/// introduces is the NEXT `fn` — located by brace balance rather than by a
/// banner, so a doc comment between the citation and its function cannot move
/// the boundary.
fn pp_citations() -> Vec<Citation> {
    let path = repo_root().join(PP_ORACLE);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read the printer oracle at {}: {e}", path.display()));
    let raw: Vec<&str> = text.lines().collect();
    let blanked_text = blank_rust(&text);
    let blanked: Vec<&str> = blanked_text.lines().collect();
    assert_eq!(
        raw.len(),
        blanked.len(),
        "the literal-blanking pass changed the line count, so every block below \
         would be mis-located"
    );

    let mut out = Vec::new();
    for (i, line) in raw.iter().enumerate() {
        let Some(rest) = line.trim_start().strip_prefix("// VERBATIM from ") else {
            continue;
        };
        let Some((src, tail)) = rest.split_once(", lines ") else {
            panic!("malformed citation at {PP_ORACLE}:{}: {line}", i + 1);
        };
        let Some((range, tail)) = tail.split_once(", at commit ") else {
            panic!("malformed citation at {PP_ORACLE}:{}: {line}", i + 1);
        };
        let Some((lo, hi)) = range.split_once('-') else {
            panic!("malformed line range at {PP_ORACLE}:{}: {line}", i + 1);
        };
        let sha: String = tail.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
        assert!(
            sha.len() >= 7,
            "citation at {PP_ORACLE}:{} names no commit: {line}",
            i + 1
        );
        // The block is the next `fn`.
        let start = (i + 1..raw.len())
            .find(|&j| {
                let t = raw[j].trim_start();
                t.starts_with("fn ") || t.starts_with("pub fn ") || t.starts_with("pub(crate) fn ")
            })
            .unwrap_or_else(|| panic!("citation at {PP_ORACLE}:{} introduces no function", i + 1));
        out.push(Citation {
            path: src.to_string(),
            first_line: lo.parse().expect("citation line range is numeric"),
            last_line: hi.parse().expect("citation line range is numeric"),
            sha,
            marker_line: i + 1,
            block: fn_block(&raw, &blanked, start),
        });
    }
    out
}

/// Re-derive one cited printer block from history.
fn pp_rederive(c: &Citation) -> String {
    let source = git_show_path(&c.sha, &c.path);
    let lines: Vec<&str> = source.lines().collect();
    assert!(
        c.last_line <= lines.len() && c.first_line >= 1 && c.first_line <= c.last_line,
        "citation at {PP_ORACLE}:{} claims lines {}-{} of `{}`, which has {} lines at {}",
        c.marker_line,
        c.first_line,
        c.last_line,
        c.path,
        lines.len(),
        c.sha
    );
    let cited = lines[c.first_line - 1..c.last_line].join("\n");
    let mut derived = pp_rename(&cited);
    for d in PP_DEVIATIONS {
        derived = derived.replace(d.from, d.to);
    }
    derived
}

/// ★ Every citation in the PRINTER oracle is re-derived from git and compared
/// byte for byte.
#[test]
fn every_pretty_printer_oracle_citation_matches_its_source() {
    let cites = pp_citations();
    assert_eq!(
        cites.len(),
        PP_NAMES.len(),
        "the printer oracle carries {} citations for {} renamed entry points — every \
         function in that file must cite its source, or the file's claim is partial",
        cites.len(),
        PP_NAMES.len()
    );

    let mut identical = 0usize;
    let mut rename_only_identical = 0usize;
    let mut cited_lines = 0usize;
    let mut failures = Vec::new();
    for c in &cites {
        cited_lines += c.last_line - c.first_line + 1;
        // ⚠ TWO measures, and conflating them is how the original claim went
        // wrong. `rename_only` is what "kept verbatim" asserted: the block with
        // nothing but the mechanical rename applied. `derived` additionally
        // applies PP_DEVIATIONS, and it is what must match — every difference
        // has to be declared, but a difference is allowed to exist.
        let source = git_show_path(&c.sha, &c.path);
        let lines: Vec<&str> = source.lines().collect();
        if pp_rename(&lines[c.first_line - 1..c.last_line].join("\n")) == c.block {
            rename_only_identical += 1;
        }
        let derived = pp_rederive(c);
        if derived == c.block {
            identical += 1;
            continue;
        }
        let (d, o): (Vec<&str>, Vec<&str>) = (derived.lines().collect(), c.block.lines().collect());
        let first = (0..d.len().max(o.len()))
            .find(|&i| d.get(i) != o.get(i))
            .unwrap_or(0);
        failures.push(format!(
            "\n  {PP_ORACLE}:{} cites `{}` lines {}-{} at {}\n    first difference at block line {}:\n      \
             from history: {:?}\n      in the oracle: {:?}",
            c.marker_line,
            c.path,
            c.first_line,
            c.last_line,
            c.sha,
            first + 1,
            d.get(first),
            o.get(first)
        ));
    }

    assert!(
        failures.is_empty(),
        "\n\u{2605} THE PRINTER ORACLE HAS DRIFTED FROM ITS CITED SOURCE.\n\
         \n\
         The differential in `pretty_printer::differential` is only evidence while this \
         twin is the pre-conversion code. If it drifts toward the driver, the differential \
         compares the driver against something adjusted to agree with it and every green \
         result is worthless.\n\
         \n\
         Either restore the block, or — if the change is intended — declare it in \
         PP_DEVIATIONS with a reason.\n{}\n",
        failures.join("\n")
    );

    assert_eq!(
        identical,
        cites.len(),
        "internal: {} blocks matched but no failure was recorded",
        identical
    );

    // ★ ANTI-VACUITY, and the MEASURED claim the oracle's own documentation
    // states. This is the number the original "kept verbatim ... nothing else is
    // edited" banner got wrong, so it is pinned rather than described: if a
    // future edit makes more (or fewer) blocks survive the rename untouched, the
    // file's documentation is stale and this says so.
    //
    // It is also what stops the check above from being vacuous in the worst way:
    // a PP_DEVIATIONS table permissive enough to absorb everything would drive
    // this to 0 and fail here.
    assert_eq!(
        rename_only_identical,
        4,
        "the printer oracle's documentation states that 4 of its {} functions are \
         byte-identical under the declared rename ALONE and {} carry declared deviations. \
         {rename_only_identical} survive the rename untouched now, so that documentation is \
         stale.",
        PP_NAMES.len(),
        PP_NAMES.len() - 4
    );
    assert!(
        cited_lines >= 900,
        "only {cited_lines} pre-conversion lines are cited; the citations no longer cover \
         the body of the twin"
    );
    println!(
        "{} printer-oracle citations verified against git ({rename_only_identical} \
         byte-identical under the rename alone, {} carrying declared deviations, \
         {cited_lines} source lines)",
        cites.len(),
        cites.len() - rename_only_identical
    );
}

/// Every entry in [`PP_DEVIATIONS`] must still be REQUIRED: it must appear in
/// the cited history and be absent from the oracle. An entry that has stopped
/// mattering is an exemption outliving its reason.
#[test]
fn no_undeclared_pretty_printer_deviations() {
    let cites = pp_citations();
    let history: String = cites
        .iter()
        .map(|c| {
            let src = git_show_path(&c.sha, &c.path);
            let lines: Vec<&str> = src.lines().collect();
            pp_rename(&lines[c.first_line - 1..c.last_line].join("\n"))
        })
        .collect::<Vec<_>>()
        .join("\n");
    let oracle: String = cites
        .iter()
        .map(|c| c.block.clone())
        .collect::<Vec<_>>()
        .join("\n");

    for d in PP_DEVIATIONS {
        assert!(
            history.contains(d.from),
            "PP_DEVIATIONS entry is STALE: the cited history no longer contains\n  {:?}\n\
             reason on file: {}",
            d.from,
            d.reason
        );
        assert!(
            d.to.is_empty() || oracle.contains(d.to),
            "PP_DEVIATIONS claims the oracle spells it\n  {:?}\nbut it does not.\n\
             reason on file: {}",
            d.to,
            d.reason
        );
        assert!(
            !d.reason.trim().is_empty(),
            "a deviation without a reason is an undeclared deviation with extra steps"
        );
    }
    println!(
        "{} printer-oracle deviation(s), all still required",
        PP_DEVIATIONS.len()
    );
}

// ===========================================================================
// ★ THE THIRD ORACLE — the evaluator's recursive twin, which is NOT a copy
// ===========================================================================
//
// `reduce.rs`'s `*_recursive` family is the oracle for the trampoline
// differential. `reduce.rs:9249` used to describe it as "a faithful copy of the
// pre-trampoline evaluator over the shared `combine_*` helpers".
//
// The word "copy" was wrong, and measurably so. The twin is a REWRITE: the
// per-arm arithmetic, comparison and collection logic was lifted into the
// shared `combine_*` helpers and the twin calls them — the same helpers the
// trampoline calls. So this section does NOT check byte-identity, because there
// is none to check. It pins the RELATIONSHIP instead.
//
// ⚠ Why an inequality is the right guard here. The hazard the other two
// sections address is an oracle drifting toward its machine. This oracle
// already SHARES code with its machine by construction, which bounds what the
// differential can prove: it proves the descend/combine WIRING and cannot see
// inside a `combine_*` helper, because both sides would compute the same wrong
// answer. That bound is a property a reader must be told, and the way it gets
// silently lost is exactly what happened — a comment calling the twin a "copy".
// So the numbers are pinned: if someone makes the twin a real copy, or removes
// the sharing, this test fails and the prose describing the differential's
// reach has to be rewritten with it.

/// The trampoline conversion. Its parent holds the evaluator this twin replaced.
const TRAMPOLINE_COMMIT: &str = "a929a2d6^";

/// `(pre-trampoline name, its line count at TRAMPOLINE_COMMIT, twin line count)`.
///
/// Re-derived from git and from the live file; every number is checked.
const TRAMPOLINE_TWIN_SHAPE: &[(&str, usize, usize)] = &[
    ("eval_expr", 20, 11),
    ("eval_expr_to_par", 59, 44),
    ("eval_expr_to_expr", 1213, 167),
    ("eval_single_expr", 21, 20),
    ("eval_to_i64", 48, 28),
    ("eval_to_bool", 48, 28),
];

const REDUCE: &str = "rholang/src/rust/interpreter/reduce.rs";

/// The lines of the `impl`-level function named `name`, or `None`.
fn impl_fn_lines(text: &str, name: &str) -> Option<(usize, usize)> {
    let raw: Vec<&str> = text.lines().collect();
    let blanked_text = blank_rust(text);
    let blanked: Vec<&str> = blanked_text.lines().collect();
    if raw.len() != blanked.len() {
        panic!("literal blanking changed the line count while looking for `{name}`");
    }
    let heads = ["    fn ", "    pub fn ", "    pub(crate) fn "];
    let start = raw.iter().position(|l| {
        heads.iter().any(|h| {
            l.strip_prefix(h).is_some_and(|rest| {
                rest.starts_with(name)
                    && !rest[name.len()..].starts_with(|c: char| c.is_alphanumeric() || c == '_')
            })
        })
    })?;
    let (mut depth, mut opened) = (0i64, false);
    for (offset, line) in blanked.iter().enumerate().skip(start) {
        for ch in line.chars() {
            match ch {
                '{' => {
                    depth += 1;
                    opened = true;
                }
                '}' => depth -= 1,
                _ => {}
            }
        }
        if opened && depth == 0 {
            return Some((start + 1, offset + 1));
        }
    }
    None
}

/// ★ The evaluator twin is a REWRITE over shared combiners, not a copy — and the
/// numbers that say so are checked rather than asserted.
#[test]
fn the_trampoline_twin_is_a_rewrite_not_a_copy() {
    let before = git_show_path(TRAMPOLINE_COMMIT, REDUCE);
    let now = std::fs::read_to_string(repo_root().join(REDUCE))
        .unwrap_or_else(|e| panic!("cannot read {REDUCE}: {e}"));

    let mut identical = 0usize;
    let mut problems = Vec::new();
    for &(name, want_pre, want_twin) in TRAMPOLINE_TWIN_SHAPE {
        let twin_name = format!("{name}_recursive");
        let Some((p1, p2)) = impl_fn_lines(&before, name) else {
            problems.push(format!(
                "  `{name}` is not an impl-level fn at {TRAMPOLINE_COMMIT}; the cited \
                 pre-trampoline evaluator has moved"
            ));
            continue;
        };
        let Some((c1, c2)) = impl_fn_lines(&now, &twin_name) else {
            problems.push(format!("  `{twin_name}` no longer exists in {REDUCE}"));
            continue;
        };
        let (pre_len, twin_len) = (p2 - p1 + 1, c2 - c1 + 1);
        if (pre_len, twin_len) != (want_pre, want_twin) {
            problems.push(format!(
                "  `{name}`: the banner in {REDUCE} records {want_pre} pre-trampoline lines \
                 and {want_twin} twin lines; they are now {pre_len} and {twin_len}"
            ));
        }

        // Byte-identity, under the same `_recursive` rename the twin's names took.
        let pre_body: String = before.lines().collect::<Vec<_>>()[p1 - 1..p2].join("\n");
        let twin_body: String = now.lines().collect::<Vec<_>>()[c1 - 1..c2].join("\n");
        let mut renamed = pre_body;
        let mut names: Vec<&str> = TRAMPOLINE_TWIN_SHAPE.iter().map(|(n, _, _)| *n).collect();
        names.sort_by_key(|n| std::cmp::Reverse(n.len()));
        for n in names {
            renamed = replace_word(&renamed, n, &format!("{n}_recursive"));
        }
        if renamed.replace("    pub fn ", "    pub(crate) fn ") == twin_body {
            identical += 1;
        }
    }

    assert!(
        problems.is_empty(),
        "\n\u{2605} THE TRAMPOLINE TWIN'S SHAPE HAS MOVED.\n\
         \n\
         {REDUCE}'s RECURSIVE TWIN banner states this table, and the differential's reach \
         is described in terms of it. Update both together.\n{}\n",
        problems.join("\n")
    );

    assert_eq!(
        identical,
        0,
        "\n\u{2605} {identical} of the {} twin functions ARE now byte-identical copies of the \
         pre-trampoline evaluator.\n\
         \n\
         That is not a failure in itself — it is a CHANGE OF KIND. The banner in {REDUCE} \
         says the twin is a rewrite that shares the `combine_*` helpers, and derives from \
         that the differential's exact reach: it proves the descend/combine WIRING and \
         cannot see inside a shared combiner. If the twin has become a real copy, that \
         limitation is gone and the prose describing it is now wrong in the SAFE direction \
         — which is still wrong. Rewrite the banner, then change this expectation.\n",
        TRAMPOLINE_TWIN_SHAPE.len()
    );

    // ANTI-VACUITY: the comparison really did run against a real, large body.
    let (p1, p2) = impl_fn_lines(&before, "eval_expr_to_expr")
        .expect("eval_expr_to_expr exists at the cited commit");
    assert!(
        p2 - p1 + 1 > 1000,
        "the cited `eval_expr_to_expr` is only {} lines; this test is comparing against the \
         wrong thing",
        p2 - p1 + 1
    );
    println!(
        "trampoline twin: {}/{} byte-identical (expected 0 — it is a rewrite over shared \
         `combine_*` helpers, not a copy)",
        identical,
        TRAMPOLINE_TWIN_SHAPE.len()
    );
}
