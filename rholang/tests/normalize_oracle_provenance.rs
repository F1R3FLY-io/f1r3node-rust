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

use std::collections::BTreeSet;
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
fn is_ident_char(c: char) -> bool { c.is_alphanumeric() || c == '_' }

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
            || !is_ident_char(
                haystack[..at]
                    .chars()
                    .next_back()
                    .expect("non-empty prefix"),
            );
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
    out.lines()
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
        marker_idx.push((i, Citation {
            path: path.to_string(),
            first_line: lo.parse().expect("citation line range is numeric"),
            last_line: hi.parse().expect("citation line range is numeric"),
            sha,
            marker_line: i + 1,
            block: String::new(),
        }));
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
        let c = cites.iter().find(|c| c.path == d.path).unwrap_or_else(|| {
            panic!(
                "DECLARED_DEVIATIONS names `{}`, which the oracle does not cite",
                d.path
            )
        });
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
// `rholang/tests/support/pretty_printer_oracle.rs` is the recursive twin
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
// the truth, and the claim was wrong the same way: **3 of the 10 functions are
// byte-identical under the declared rename; 7 carry deviations**, including
// deleted comments in three of the four public wrappers.
//
// ⚠ That count was 4/6 until `EPathMap.ps` became a private entry trie with a
// `ps()` accessor. `oracle_build_string_from_expr` reads a pathmap's entries, so
// the type change moved it out of the rename-only group and into GROUP 2b — by
// exactly two characters, declared there with its reason. The count is pinned
// rather than described precisely so a move like that cannot happen quietly.

/// The printer oracle, relative to the repository root.
const PP_ORACLE: &str = "rholang/tests/support/pretty_printer_oracle.rs";

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
/// **Twenty entries** across six functions. They fall into three groups, and
/// keeping them in a table rather than as prose is what stops a seventh from
/// appearing unannounced: [`no_undeclared_pretty_printer_deviations`] fails when
/// an entry stops being needed, so the list cannot rot into a licence for drift
/// either.
///
/// ⚠ The unit is an **entry**, not a textual hunk, and the two differ: the
/// `EPathMap.ps` entry is applied at two sites (`EPathmapBody` and
/// `EZipperBody`) by the one `str::replace`. The header said "sixteen hunks"
/// while the table held seventeen entries — a hand-maintained count that
/// nothing checked, drifting exactly as §1.2 of the consensus register says
/// such counts do. `every_pretty_printer_oracle_citation_matches_its_source`
/// prints the live figure on every run, and
/// [`no_undeclared_pretty_printer_deviations`] is what actually holds the table
/// honest; this number is descriptive, and is now unambiguous about what it
/// counts.
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
    // ── GROUP 2b: a TYPE CHANGE in a dependency, which the compiler forced ─
    //
    // ★ This is the group the mechanism exists for, and it is worth being
    // precise about why it is not drift. The oracle must stay the PRE-conversion
    // code, or the differential compares the driver against something adjusted
    // to agree with it. It must ALSO compile. When a type the copied body reads
    // changes shape, those two obligations meet, and the honest resolution is to
    // make the minimum edit that restores compilation and DECLARE it here —
    // never to let it pass silently, and never to "improve" the copy while in
    // there.
    //
    // EPathMap.ps stopped being a public list and became a homogeneous
    // Empty/PathMap<()>/PathMap<Par> entry trie. The old list-only expression
    // cannot represent map bindings. The independent oracle helper streams the
    // selected PathMap specialization in canonical order and returns only the
    // comma-separated element string; the cited remainder/bracketing logic
    // remains byte-for-byte the pre-conversion body.
    Deviation {
        path: "rholang/src/rust/interpreter/pretty_printer.rs",
        from: "let elements = self.oracle_build_vec(&pathmap.ps);",
        to: "let elements = self.oracle_build_epathmap_elements(pathmap);",
        reason: "REPRESENTATION-FORCED and mode-complete, not oracle drift. \
                 EPathMap now stores neutral empty, set PathMap<()> and map \
                 PathMap<Par> as distinct homogeneous modes; the historical \
                 Vec field cannot express the map mode. The oracle-only helper \
                 independently streams canonical trie order, renders set members \
                 or key/value pairs, and returns the same element string consumed \
                 by the untouched pre-conversion remainder/bracketing logic. \
                 Appears twice — EPathmapBody and EZipperBody.",
    },
    // ── GROUP 3: the SEMANTIC deviations, each marked at its own site ─────
    //
    // ★ The `EZipper` cursor-kind pair. `EZipper.cursor_kind` participates in
    // the type's `PartialEq`/`Hash` and reaches the event hash, so two zippers
    // agreeing on `current_path` but differing on the arm are `!=`, hash
    // differently, and address DIFFERENT entries (`5` vs `[5]`, keys `03 0a` and
    // `03 0a 00`). The copied body rendered only the segments, so it printed ONE
    // string for that pair. The driver now spends the discriminator through
    // `models::rust::pathmap_integration::render_cursor_position`, and the twin
    // takes the identical edit — otherwise the differential compares a fixed
    // driver against an unfixed twin, which is the failure mode this whole
    // section exists to prevent.
    //
    // The two printers stay INDEPENDENT implementations (that is the oracle's
    // whole evidential value); what is shared is a true LEAF, exactly as
    // `build_remainder_string` and `build_string_from_var` are shared. That they
    // agree is asserted by `pretty_printer::differential::every_expr_arm`, which
    // now drives both arms of the discriminator.
    Deviation {
        path: "rholang/src/rust/interpreter/pretty_printer.rs",
        from: "                        \"[]\".to_string()",
        to: "                        render_cursor_position(zipper.cursor_kind, &[])",
        reason: "The EMPTY-cursor row of the same defect: at the root, `Split`, \
                 `Bare` and `Prefix` all rendered `[]`. `render_cursor_position` \
                 returns exactly `[]` for `Split` — proto value 0, so no \
                 previously-representable zipper's bytes move — and marks the \
                 other two.",
    },
    Deviation {
        path: "rholang/src/rust/interpreter/pretty_printer.rs",
        from: "                        format!(\"[{}]\", path_segments.join(\", \"))",
        to: "                        render_cursor_position(zipper.cursor_kind, &path_segments)",
        reason: "The NON-EMPTY-cursor row. `format!(\"[{}]\", …)` is the split-arm \
                 rendering spelled unconditionally — the display-side twin of the \
                 unconditional split-arm KEY that #108 removed from the readers. \
                 The `Split` row is byte-identical to the copied expression, so \
                 the replay-compared bytes of every zipper that could exist \
                 before `cursor_kind` did are unchanged.",
    },
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
    // ── GROUP 3, continued: THE `where` GUARD ─────────────────────────────
    //
    // ★ The same shape as the `Match` target and the `cursor_kind` pair: a
    // field of the term the copied body did not read at all. `Receive.condition`
    // is the `where` clause; the printer emitted no `where` token anywhere, so
    // a GUARDED receive printed as an UNGUARDED one — well-formed Rholang,
    // plausible, and admitting strictly more than the term it claims to be.
    //
    // The twin takes the identical edit for `76de7d44`'s reason: the two
    // printers must stay INDEPENDENT implementations of *how* to print, but a
    // differential between a repaired driver and an unrepaired twin is not
    // evidence about anything. What is shared is the true LEAF
    // `pretty_printer::receive_guard` — the predicate "is there a guard?", not
    // the rendering — exactly as `render_cursor_position` is shared.
    //
    // Three hunks, because the render is spliced in at three points: the guard
    // is computed after `bound_shift += totally_free` (so it renders in the
    // body's de Bruijn environment, which is the environment
    // `p_input_normalizer` normalized it in) and before the body (the order the
    // bytes come out in), and both `format!` arms grow the clause.
    Deviation {
        path: "rholang/src/rust/interpreter/pretty_printer.rs",
        from: "            self.bound_shift += totally_free;\n            let body_str =",
        to: "            self.bound_shift += totally_free;\n            \
             // \u{26a0} NOT verbatim, and deliberately so \u{2014} declared in\n            \
             // `PP_DEVIATIONS`. The copied body rendered no `where` clause,\n            \
             // because the printer had no `where` token at all: a guarded\n            \
             // receive printed as an UNGUARDED one, which is a different term\n            \
             // with weaker admission. The twin takes the identical edit for the\n            \
             // reason `76de7d44` gave for `cursor_kind` \u{2014} otherwise the\n            \
             // differential compares a repaired driver against an unrepaired\n            \
             // twin, which is the failure mode the oracle exists to prevent.\n            \
             //\n            \
             // Position is load-bearing and is asserted, not asserted-about:\n            \
             // AFTER `self.bound_shift += totally_free` (the guard is normalized\n            \
             // in the body's environment) and BEFORE the body (the order the\n            \
             // bytes come out in). `DriveMutation::ReceiveConditionBeforeBoundShift`\n            \
             // separates the first.\n            \
             let where_clause = match receive_guard(r) {\n                \
             Some(condition) => {\n                    \
             format!(\" where {}\", self.oracle_build_string_from_message(condition))\n                \
             }\n                \
             None => String::new(),\n            \
             };\n            \
             let body_str =",
        reason: "THE `where`-guard repair itself, first hunk. `Receive.condition` \
                 was never read, so `for (@x <- c where x > 5) { … }` printed as \
                 `for (@x <- c) { … }`. Computed AFTER the `bound_shift` mutation \
                 because the guard is normalized in the body's environment \
                 (`p_input_normalizer`'s `InputPhase::Guard`: \"guard and body see \
                 the same de Bruijn levels\"), and BEFORE the body because that is \
                 the order the bytes come out in. \
                 `DriveMutation::ReceiveConditionBeforeBoundShift` separates the \
                 first of those from its negation.",
    },
    Deviation {
        path: "rholang/src/rust/interpreter/pretty_printer.rs",
        from: "                    \"for( {} ) {{\\n{}{}{}\\n{}}}\",\n                    binds_string,",
        to: "                    \"for( {}{} ) {{\\n{}{}{}\\n{}}}\",\n                    \
             binds_string,\n                    where_clause,",
        reason: "Second hunk: the non-empty-body arm's format string and its \
                 argument. The clause goes after `binds_string` and inside the \
                 parentheses because the grammar attaches `where` to the RECEIPT — \
                 `receipt: conc1(bind) optional('where' guard)` — not to a bind, so \
                 it follows EVERY bind. `{}{}` with an empty second argument is \
                 byte-identical to `{}` for every unguarded receive, which is what \
                 keeps the movement confined.",
    },
    Deviation {
        path: "rholang/src/rust/interpreter/pretty_printer.rs",
        from: "                Ok(format!(\"for( {} ) {{}}\", binds_string))",
        to: "                Ok(format!(\"for( {}{} ) {{}}\", binds_string, where_clause))",
        reason: "Third hunk: the empty-body arm. A guarded receive with an empty \
                 body is representable (`for (@x <- c where x > 5) { Nil }`) and \
                 would otherwise be the one shape that still dropped its guard.",
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
        3,
        "the printer oracle's documentation states that 3 of its {} functions are \
         byte-identical under the declared rename ALONE and {} carry declared deviations. \
         {rename_only_identical} survive the rename untouched now, so that documentation is \
         stale.",
        PP_NAMES.len(),
        PP_NAMES.len() - 3
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
// So the RELATIONSHIP is checked: if someone makes the twin a real copy, or
// removes the sharing, this test fails and the prose describing the
// differential's reach has to be rewritten with it.
//
// ===========================================================================
// ★ WHY THIS SECTION NO LONGER COUNTS LINES — the defect, and the repair
// ===========================================================================
//
// It used to. `TRAMPOLINE_TWIN_SHAPE` carried `(name, pre lines, twin lines)`
// and the test read the PRE column out of git and the TWIN column out of the
// WORKING TREE. That asymmetry is the whole bug. A frozen blob cannot move, so
// the pre column never went stale in eight months; the live file moves whenever
// anyone edits it, so the twin column went stale the first time somebody
// touched `reduce.rs` for an unrelated reason — a 1,057-line change (682+/375-)
// turned `11, 44, 167, 20, 28, 28` into `17, 47, 226, 24, 32, 32` and this test
// went red having found nothing wrong with the twin at all.
//
// The repair is NOT to update the numbers. A count is a PROXY: it is not the
// property "is a rewrite, not a copy", it merely correlated with it once, and a
// proxy any unrelated refactor invalidates was never testing the claim its name
// makes. Updating it would buy one more refactor's worth of silence and cost
// the next reader the same afternoon.
//
// So the section is split along the line the banner in `reduce.rs` already drew
// between "measured" and "is":
//
//   ▸ THE CITATION — the banner's table is a MEASUREMENT, stated in the past
//     tense ("measured, with a `_recursive` rename applied"). A measurement is
//     of a specific text, so BOTH columns are now re-derived from git: the pre
//     column at TRAMPOLINE_COMMIT and the twin column at
//     TWIN_MEASUREMENT_COMMIT, the commit that authored the table. That makes
//     the banner's own sentence — "re-derives the table above FROM GIT and
//     fails if any entry moves" — literally true, which it was not before, and
//     it can now only break if history is rewritten.
//
//   ▸ THE LIVE CLAIM — what is asserted about the twin AS IT STANDS is checked
//     structurally, three ways, none of which a reformat can move:
//
//       1. TOKEN-for-token non-identity, per function. Strictly stronger than
//          the byte-identity it replaces: a twin re-copied from the original
//          and then reformatted, or re-commented, is byte-different and token-
//          IDENTICAL, so the old check would have called it a rewrite.
//       2. THE SHARING, positively. The pre-trampoline `reduce.rs` contains the
//          token `combine_` ZERO times; the twin calls the helpers, and every
//          helper it calls is also called from OUTSIDE the twin family — which
//          is what "shares code with the machine it checks" means and what
//          bounds the differential's reach.
//       3. THE LIFTING, as an inequality on TOKENS, not lines: the arm table
//          that `eval_expr_to_expr` carried inline is gone, so the twin is a
//          multiple smaller than the original. Reformatting does not change a
//          token count; re-inlining 1,213 lines of arm logic changes it by
//          nearly 4×.
//
// Each of the three goes RED if the twin is re-copied and GREEN under a pure
// reformat, and `the_rewrite_check_can_go_red` demonstrates both directions
// rather than asserting them.
// ===========================================================================

/// The trampoline conversion. Its parent holds the evaluator this twin replaced.
const TRAMPOLINE_COMMIT: &str = "a929a2d6^";

/// The commit that authored `reduce.rs`'s RECURSIVE TWIN banner, and therefore
/// the text its TWIN column measured.
///
/// ⚠ A measurement needs a subject. Naming one is what stops this table from
/// being a claim about a file that keeps changing underneath it.
const TWIN_MEASUREMENT_COMMIT: &str = "29263381";

/// `(pre-trampoline name, its line count at TRAMPOLINE_COMMIT, twin line count
/// at TWIN_MEASUREMENT_COMMIT)` — the banner's table, as data.
///
/// Both columns are re-derived from git. Nothing here is read from the working
/// tree; see the section banner for why that asymmetry was the defect.
const TRAMPOLINE_TWIN_SHAPE: &[(&str, usize, usize)] = &[
    ("eval_expr", 20, 11),
    ("eval_expr_to_par", 59, 44),
    ("eval_expr_to_expr", 1213, 167),
    ("eval_single_expr", 21, 20),
    ("eval_to_i64", 48, 28),
    ("eval_to_bool", 48, 28),
];

/// How many times smaller, in TOKENS, `eval_expr_to_expr`'s twin must be than
/// the body it replaced.
///
/// The measured ratio is 8,268 → 2,215 tokens (3.73×) with the arm table lifted
/// into the `combine_*` helpers. The floor is 3, which leaves the twin room to
/// grow by a quarter for ordinary reasons and still trips long before 1,213
/// lines of arm logic could come back inline. It is a floor and not an equality
/// on purpose: the claim is "the table was lifted out", which is an ordering
/// fact, and an ordering fact is what a refactor cannot invalidate.
const ARM_TABLE_COLLAPSE_FLOOR: usize = 3;

/// The one function whose body carried the arm table, and so the only one the
/// collapse claim is about — the other five were small before the conversion
/// and are within a few tokens of their originals.
const ARM_TABLE_FN: &str = "eval_expr_to_expr";

const REDUCE: &str = "rholang/src/rust/interpreter/reduce.rs";
const REDUCE_EXPRESSION_ORACLE: &str = "rholang/src/rust/interpreter/reduce_expression_oracle.rs";

/// The live evaluator implementation plus the source-only recursive oracle it
/// expands in test builds. Keeping these separate lets the recursion census
/// distinguish production from reference recursion; concatenating them here
/// preserves this provenance gate's token/sharing analysis.
fn live_reduce_with_expression_oracle() -> String {
    let mut source = std::fs::read_to_string(repo_root().join(REDUCE))
        .unwrap_or_else(|e| panic!("cannot read {REDUCE}: {e}"));
    source.push('\n');
    let oracle = std::fs::read_to_string(repo_root().join(REDUCE_EXPRESSION_ORACLE))
        .unwrap_or_else(|e| panic!("cannot read {REDUCE_EXPRESSION_ORACLE}: {e}"));
    // The macro body is indented one extra level in its source file. The
    // provenance parser deliberately recognizes impl-level functions by their
    // four-space indentation, so analyze the macro expansion's indentation.
    for line in oracle.lines() {
        source.push_str(line.strip_prefix("    ").unwrap_or(line));
        source.push('\n');
    }
    source
}

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

/// The body of the `impl`-level function `name`, verbatim, or `None`.
fn impl_fn_body(text: &str, name: &str) -> Option<String> {
    let (a, b) = impl_fn_lines(text, name)?;
    Some(text.lines().collect::<Vec<_>>()[a - 1..b].join("\n"))
}

/// Rust tokens, over [`blank_rust`]ed text, so that comments, whitespace, line
/// breaks and string CONTENTS contribute nothing.
///
/// Multi-character operators are emitted as their component punctuation. That is
/// sufficient here and deliberately so: the sequence is used only for EQUALITY
/// and for COUNTING, and any consistent tokenisation answers both. What matters
/// is that the answer is invariant under `rustfmt` and under re-commenting,
/// which is exactly what byte comparison was not.
fn tokens(text: &str) -> Vec<String> {
    let blanked = blank_rust(text);
    let c: Vec<char> = blanked.chars().collect();
    let mut out = Vec::with_capacity(c.len() / 3);
    let mut i = 0usize;
    while i < c.len() {
        let ch = c[i];
        if ch.is_whitespace() {
            i += 1;
        } else if ch.is_alphanumeric() || ch == '_' {
            let start = i;
            while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_') {
                i += 1;
            }
            out.push(c[start..i].iter().collect());
        } else {
            out.push(ch.to_string());
            i += 1;
        }
    }
    out
}

/// The declared rename the twin's names took, plus the visibility normalisation,
/// applied symmetrically so neither shows up as a difference.
fn twin_rename(text: &str) -> String {
    let mut names: Vec<&str> = TRAMPOLINE_TWIN_SHAPE.iter().map(|(n, _, _)| *n).collect();
    names.sort_by_key(|n| std::cmp::Reverse(n.len()));
    let mut out = text.to_string();
    for n in names {
        out = replace_word(&out, n, &format!("{n}_recursive"));
    }
    out.replace("pub(crate) fn ", "pub fn ")
}

/// Every `combine_*` identifier OCCURRING in `blanked`, excluding the `fn`
/// declaration sites, restricted to the lines `keep` accepts.
fn combine_identifiers(blanked: &str, keep: impl Fn(usize) -> bool) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (idx, line) in blanked.lines().enumerate() {
        if !keep(idx) {
            continue;
        }
        let b = line.as_bytes();
        let mut from = 0usize;
        while let Some(rel) = line[from..].find("combine_") {
            let at = from + rel;
            let is_word_start = at == 0 || !is_ident_char(line[..at].chars().next_back().unwrap());
            let mut end = at;
            while end < b.len() && is_ident_char(line[end..].chars().next().unwrap_or(' ')) {
                end += 1;
            }
            // `fn combine_x` is the DEFINITION, not a use of the shared helper.
            let declared = line[..at].trim_end().ends_with("fn");
            if is_word_start && !declared {
                out.insert(line[at..end].to_string());
            }
            from = at + "combine_".len();
        }
    }
    out
}

/// Everything the live twin is claimed to be, measured in one pass so the main
/// test and its anti-vacuity twin evaluate the SAME predicate over different
/// texts.
struct TwinAssessment {
    /// Functions that could not be located at all.
    missing: Vec<String>,
    /// Twins that are a TOKEN-for-token copy of their pre-trampoline original.
    token_identical: Vec<String>,
    /// `combine_*` helpers the twin family calls.
    twin_calls: BTreeSet<String>,
    /// `combine_*` helpers called from outside the twin family — the machine.
    machine_calls: BTreeSet<String>,
    /// Occurrences of the token `combine_` in the PRE-trampoline file. The
    /// helpers did not exist then, so this anchors the sharing claim at 0.
    pre_combine_occurrences: usize,
    /// Tokens in `ARM_TABLE_FN` before the conversion, and in its twin now.
    arm_table_tokens: (usize, usize),
}

/// Assess `now` (a `reduce.rs` text) against `before` (the pre-trampoline one).
fn assess_twin(before: &str, now: &str) -> TwinAssessment {
    let blanked_now = blank_rust(now);
    let mut missing = Vec::new();
    let mut token_identical = Vec::new();
    let mut twin_lines: Vec<(usize, usize)> = Vec::with_capacity(TRAMPOLINE_TWIN_SHAPE.len());
    let mut arm_table_tokens = (0usize, 0usize);

    for &(name, _, _) in TRAMPOLINE_TWIN_SHAPE {
        let twin_name = format!("{name}_recursive");
        let (Some(pre_body), Some(span)) =
            (impl_fn_body(before, name), impl_fn_lines(now, &twin_name))
        else {
            if impl_fn_body(before, name).is_none() {
                missing.push(format!(
                    "  `{name}` is not an impl-level fn at {TRAMPOLINE_COMMIT}; the cited \
                     pre-trampoline evaluator has moved"
                ));
            } else {
                missing.push(format!("  `{twin_name}` no longer exists in {REDUCE}"));
            }
            continue;
        };
        twin_lines.push(span);
        let twin_body = now.lines().collect::<Vec<_>>()[span.0 - 1..span.1].join("\n");
        let pre_tokens = tokens(&twin_rename(&pre_body));
        let twin_tokens = tokens(&twin_rename(&twin_body));
        if pre_tokens == twin_tokens {
            token_identical.push(name.to_string());
        }
        if name == ARM_TABLE_FN {
            arm_table_tokens = (pre_tokens.len(), twin_tokens.len());
        }
    }

    // `impl_fn_lines` answers in 1-based inclusive line numbers; the scan below
    // enumerates 0-based, so the spans are converted ONCE here rather than at
    // every comparison.
    let twin_spans: Vec<std::ops::RangeInclusive<usize>> =
        twin_lines.iter().map(|&(a, b)| (a - 1)..=(b - 1)).collect();
    let in_twin = |idx: usize| twin_spans.iter().any(|s| s.contains(&idx));
    let twin_calls = combine_identifiers(&blanked_now, in_twin);
    let machine_calls = combine_identifiers(&blanked_now, |i| !in_twin(i));
    let pre_combine_occurrences = combine_identifiers(&blank_rust(before), |_| true).len();

    TwinAssessment {
        missing,
        token_identical,
        twin_calls,
        machine_calls,
        pre_combine_occurrences,
        arm_table_tokens,
    }
}

impl TwinAssessment {
    /// The three structural claims, as one verdict. `Ok(())` means the twin is a
    /// rewrite over shared combiners; `Err` lists every claim that failed, so a
    /// red result says which of the three moved.
    fn verdict(&self) -> Result<(), Vec<String>> {
        let mut bad = Vec::new();
        bad.extend(self.missing.iter().cloned());

        // (1) NOT A COPY.
        if !self.token_identical.is_empty() {
            bad.push(format!(
                "  TOKEN-IDENTICAL: {} of {} twins are a token-for-token copy of the \
                 pre-trampoline evaluator ({}). That is a CHANGE OF KIND, not a bug in itself: \
                 the banner in {REDUCE} says the twin is a rewrite sharing the `combine_*` \
                 helpers, and derives the differential's exact reach from that. If the twin has \
                 become a copy, the stated limitation is gone and the prose is wrong in the SAFE \
                 direction — which is still wrong. Rewrite the banner, then change this \
                 expectation.",
                self.token_identical.len(),
                TRAMPOLINE_TWIN_SHAPE.len(),
                self.token_identical.join(", ")
            ));
        }

        // (2) THE SHARING.
        if self.pre_combine_occurrences != 0 {
            bad.push(format!(
                "  ANCHOR LOST: the pre-trampoline `reduce.rs` at {TRAMPOLINE_COMMIT} mentions \
                 {} `combine_*` helper(s). It mentioned none when this check was written, which \
                 is what makes 'the twin calls helpers the original did not have' a statement \
                 about the conversion rather than a coincidence.",
                self.pre_combine_occurrences
            ));
        }
        if self.twin_calls.is_empty() {
            bad.push(
                "  NO SHARING: the twin family calls NO `combine_*` helper. The banner says the \
                 per-arm arithmetic, comparison and collection logic was lifted into those \
                 helpers and that the twin calls them — the same ones the trampoline calls. \
                 Either the lifting was undone (the twin is becoming a copy) or the helpers were \
                 renamed and this check now sees nothing."
                    .to_string(),
            );
        }
        let unshared: Vec<&String> = self.twin_calls.difference(&self.machine_calls).collect();
        if !unshared.is_empty() {
            bad.push(format!(
                "  NOT SHARED: the twin calls {:?}, which nothing OUTSIDE the twin family calls. \
                 The differential's reach is described in terms of the oracle SHARING code with \
                 the machine it checks — it proves the descend/combine wiring and cannot see \
                 inside a shared combiner. A helper only the oracle calls is not shared, and the \
                 description no longer holds.",
                unshared
            ));
        }

        // (3) THE LIFTING.
        let (pre, twin) = self.arm_table_tokens;
        if twin == 0 || pre < twin * ARM_TABLE_COLLAPSE_FLOOR {
            bad.push(format!(
                "  NO COLLAPSE: `{ARM_TABLE_FN}` was {pre} tokens before the conversion and its \
                 twin is {twin} now, a ratio below the {ARM_TABLE_COLLAPSE_FLOOR}× floor. The \
                 arm table is supposed to live in the `combine_*` helpers, not inline in the \
                 twin; a twin this size is carrying it again."
            ));
        }

        match bad.is_empty() {
            true => Ok(()),
            false => Err(bad),
        }
    }
}

/// One row of the RECURSIVE TWIN banner's markdown table, as it stands in
/// `reduce.rs` right now.
#[derive(Debug, PartialEq, Eq)]
struct BannerRow {
    name: String,
    pre_lines: usize,
    twin_lines: usize,
    /// The `byte-identical` column, verbatim.
    byte_identical: String,
}

/// Parse the banner's table out of a `reduce.rs` text.
///
/// ⚠ The banner is read from the LIVE file, not from a constant, and that is the
/// point: a citation nobody re-reads is prose. If someone edits a number in the
/// banner, or deletes the table, this parse disagrees with
/// [`TRAMPOLINE_TWIN_SHAPE`] and the check below is red — which is the property
/// the banner's own closing sentence claims for it.
fn banner_rows(reduce_src: &str) -> Vec<BannerRow> {
    let mut out = Vec::with_capacity(TRAMPOLINE_TWIN_SHAPE.len());
    for line in reduce_src.lines() {
        let t = line.trim_start();
        let Some(rest) = t.strip_prefix("//") else {
            continue;
        };
        let rest = rest.trim();
        if !rest.starts_with('|') {
            continue;
        }
        let cells: Vec<&str> = rest.trim_matches('|').split('|').map(str::trim).collect();
        if cells.len() != 4 || cells[0].starts_with("pre-trampoline") || cells[0].starts_with("--")
        {
            continue;
        }
        // ⚠ The banner writes `1,213`; the separator is presentation, not data.
        let num = |s: &str| s.replace(',', "").parse::<usize>().ok();
        let (Some(pre), Some(twin)) = (num(cells[1]), num(cells[2])) else {
            continue;
        };
        out.push(BannerRow {
            name: cells[0].to_string(),
            pre_lines: pre,
            twin_lines: twin,
            byte_identical: cells[3].to_string(),
        });
    }
    out
}

/// The banner's table, as [`TRAMPOLINE_TWIN_SHAPE`] says it should read.
fn expected_banner_rows() -> Vec<BannerRow> {
    TRAMPOLINE_TWIN_SHAPE
        .iter()
        .map(|&(name, pre, twin)| BannerRow {
            name: name.to_string(),
            pre_lines: pre,
            twin_lines: twin,
            byte_identical: "no".to_string(),
        })
        .collect()
}

/// ★ The banner's table is a CITATION: it is parsed out of the LIVE `reduce.rs`
/// and both of its columns are re-derived from git.
///
/// This is what the line-count check should always have been. It used to read
/// its second column out of the working tree — so an unrelated 1,057-line edit
/// to `reduce.rs` turned it red having found nothing wrong with the twin — while
/// never reading the banner it claimed to be checking. Now the *banner* is the
/// live text (edit a number there and this is red) and the *numbers* are
/// measurements of two named commits (refactor all you like and this is green).
/// The claim about the twin AS IT STANDS lives in
/// [`the_trampoline_twin_is_a_rewrite_not_a_copy`], where a line has no business
/// being counted at all.
#[test]
fn the_trampoline_banner_table_is_a_checkable_citation() {
    let before = git_show_path(TRAMPOLINE_COMMIT, REDUCE);
    let measured = git_show_path(TWIN_MEASUREMENT_COMMIT, REDUCE);
    let now = live_reduce_with_expression_oracle();

    // (i) THE BANNER SAYS WHAT THIS TEST THINKS IT SAYS.
    let rows = banner_rows(&now);
    assert_eq!(
        rows,
        expected_banner_rows(),
        "\n\u{2605} THE RECURSIVE TWIN BANNER IN {REDUCE} NO LONGER MATCHES ITS TABLE.\n\
         \n\
         The banner closes by claiming this test 're-derives the table above from git and fails \
         if any entry moves'. It can only claim that while the two agree. Either the banner was \
         edited (put the measurement back, or move TWIN_MEASUREMENT_COMMIT and both numbers \
         together) or the table was deleted and the claim with it.\n"
    );

    // (ii) …AND HISTORY AGREES WITH IT, IN BOTH COLUMNS.
    let mut problems = Vec::new();
    for row in &rows {
        let twin_name = format!("{}_recursive", row.name);
        let Some((p1, p2)) = impl_fn_lines(&before, &row.name) else {
            problems.push(format!(
                "  `{}` is not an impl-level fn at {TRAMPOLINE_COMMIT}",
                row.name
            ));
            continue;
        };
        let Some((c1, c2)) = impl_fn_lines(&measured, &twin_name) else {
            problems.push(format!(
                "  `{twin_name}` is not an impl-level fn at {TWIN_MEASUREMENT_COMMIT}"
            ));
            continue;
        };
        let (pre_len, twin_len) = (p2 - p1 + 1, c2 - c1 + 1);
        if (pre_len, twin_len) != (row.pre_lines, row.twin_lines) {
            problems.push(format!(
                "  `{}`: the banner records {} pre-trampoline lines and {} twin lines; git says \
                 {pre_len} and {twin_len}",
                row.name, row.pre_lines, row.twin_lines
            ));
        }
    }
    assert!(
        problems.is_empty(),
        "\n\u{2605} THE BANNER'S MEASUREMENT DOES NOT MATCH HISTORY.\n\
         \n\
         Both columns come from `git show`: the pre column at {TRAMPOLINE_COMMIT}, the twin \
         column at {TWIN_MEASUREMENT_COMMIT}. Neither is read from the working tree, so this \
         cannot go stale under a refactor — if it is red, either the banner's numbers were never \
         right or history has been rewritten.\n{}\n",
        problems.join("\n")
    );

    // (iii) ANTI-VACUITY — the parse must be able to disagree. A one-digit edit
    //       to the banner is rejected, and the unedited banner is accepted
    //       (asserted at (i)), so a parser that returned an empty table, or one
    //       that matched anything, fails here.
    let perturbed = now.replacen("|        167 | no", "|        168 | no", 1);
    assert_ne!(
        perturbed, now,
        "the banner perturbation must change the text"
    );
    assert_ne!(
        banner_rows(&perturbed),
        expected_banner_rows(),
        "\u{2605} THE BANNER PARSE CANNOT DISAGREE: a digit was changed in the table and the \
         parse still matched, so (i) is certifying nothing"
    );
    assert_eq!(
        banner_rows(&perturbed).len(),
        TRAMPOLINE_TWIN_SHAPE.len(),
        "the perturbation must change a VALUE, not the number of rows parsed, or the red above \
         would be about parsing rather than about content"
    );

    // ANTI-VACUITY: the citation really did run against a real, large body.
    let (p1, p2) = impl_fn_lines(&before, ARM_TABLE_FN)
        .unwrap_or_else(|| panic!("{ARM_TABLE_FN} exists at the cited commit"));
    assert!(
        p2 - p1 + 1 > 1000,
        "the cited `{ARM_TABLE_FN}` is only {} lines; this test is comparing against the wrong \
         thing",
        p2 - p1 + 1
    );
    println!(
        "trampoline banner: {} rows parsed from the live {REDUCE} and re-derived from git at \
         {TRAMPOLINE_COMMIT} and {TWIN_MEASUREMENT_COMMIT}",
        rows.len()
    );
}

/// ★ The evaluator twin, AS IT STANDS, is a REWRITE over shared combiners — not
/// a copy. Checked structurally: token non-identity, the sharing, the collapse.
///
/// None of the three reads a line number, so the 1,057-line refactor that broke
/// the old line-count proxy leaves all three untouched; each of them goes red if
/// the twin is re-copied from the original. `the_rewrite_check_can_go_red`
/// demonstrates both halves of that sentence.
#[test]
fn the_trampoline_twin_is_a_rewrite_not_a_copy() {
    let before = git_show_path(TRAMPOLINE_COMMIT, REDUCE);
    let now = live_reduce_with_expression_oracle();

    let a = assess_twin(&before, &now);
    if let Err(bad) = a.verdict() {
        panic!(
            "\n\u{2605} THE TRAMPOLINE TWIN IS NO LONGER A REWRITE OVER SHARED COMBINERS.\n\
             \n\
             {REDUCE}'s RECURSIVE TWIN banner states this relationship and the differential's \
             reach is described in terms of it, so the two move together.\n\
             \n\
             ⚠ Note what is NOT asserted here: nothing about line counts. If you arrived because \
             `reduce.rs` was refactored, that is not this test — read the failures below.\n\
             \n{}\n",
            bad.join("\n\n")
        );
    }

    // ANTI-VACUITY: the sharing set is substantial, not one incidental call.
    assert!(
        a.twin_calls.len() >= 15,
        "the twin calls only {} `combine_*` helper(s); the lifting produced 18, so either the \
         helpers are being re-inlined or this check has stopped seeing most of them",
        a.twin_calls.len()
    );
    println!(
        "trampoline twin: 0/{} token-identical; {} shared `combine_*` helpers (pre-trampoline \
         file had {}); `{ARM_TABLE_FN}` {} tokens \u{2192} {} ({:.2}\u{d7} collapse, floor \
         {ARM_TABLE_COLLAPSE_FLOOR}\u{d7})",
        TRAMPOLINE_TWIN_SHAPE.len(),
        a.twin_calls.len(),
        a.pre_combine_occurrences,
        a.arm_table_tokens.0,
        a.arm_table_tokens.1,
        a.arm_table_tokens.0 as f64 / a.arm_table_tokens.1.max(1) as f64
    );
}

/// ★ **Both directions**, which is what a line count never had.
///
/// The count this replaced could only go red — and it went red for a refactor
/// that changed nothing about the twin's kind. A check that discriminates has to
/// be shown doing both things:
///
/// * (a) the CONTROL: an irrelevant change — reformatting the twin's whitespace
///   and adding a comment — must leave the verdict GREEN. This is precisely the
///   case the line count failed.
/// * (b) the MUTATION: re-copying the pre-trampoline body over the twin must
///   turn the verdict RED, and red on ALL THREE claims, so no single check is
///   carrying the result alone.
#[test]
fn the_rewrite_check_can_go_red() {
    let before = git_show_path(TRAMPOLINE_COMMIT, REDUCE);
    let now = live_reduce_with_expression_oracle();

    // The baseline, so both cells below are measured against a known-green state.
    assert!(
        assess_twin(&before, &now).verdict().is_ok(),
        "the anti-vacuity cells need a green baseline; the live twin is already failing"
    );

    // ── (a) THE CONTROL — a pure reformat must NOT discriminate ──────────────
    let twin_span = impl_fn_lines(&now, &format!("{ARM_TABLE_FN}_recursive"))
        .unwrap_or_else(|| panic!("`{ARM_TABLE_FN}_recursive` exists in {REDUCE}"));
    let lines: Vec<&str> = now.lines().collect();
    let reformatted_twin: String = lines[twin_span.0 - 1..twin_span.1]
        .iter()
        .map(|l| format!("{l}   // an irrelevant comment\n\n"))
        .collect();
    let reformatted = format!(
        "{}\n{}\n{}",
        lines[..twin_span.0 - 1].join("\n"),
        reformatted_twin,
        lines[twin_span.1..].join("\n")
    );
    assert_ne!(
        reformatted, now,
        "the reformat must actually change the text"
    );
    let control = assess_twin(&before, &reformatted);
    assert!(
        control.verdict().is_ok(),
        "\u{2605} A PURE REFORMAT TURNED THE CHECK RED. That is the defect this replaced — a \
         proxy any unrelated edit invalidates. Failures were:\n{:?}",
        control.verdict().err()
    );

    // ── (b) THE MUTATION — the twin re-copied from the original ──────────────
    let pre_body = impl_fn_body(&before, ARM_TABLE_FN)
        .unwrap_or_else(|| panic!("`{ARM_TABLE_FN}` exists at {TRAMPOLINE_COMMIT}"));
    let recopied_body = twin_rename(&pre_body).replace("pub fn ", "pub(crate) fn ");
    let recopied = format!(
        "{}\n{}\n{}",
        lines[..twin_span.0 - 1].join("\n"),
        recopied_body,
        lines[twin_span.1..].join("\n")
    );
    assert_ne!(recopied, now, "the mutation must actually change the text");

    let m = assess_twin(&before, &recopied);
    let Err(bad) = m.verdict() else {
        panic!(
            "\u{2605} THE REWRITE CHECK CANNOT GO RED. The twin was replaced by a renamed copy \
             of the pre-trampoline `{ARM_TABLE_FN}` and the verdict was still green, so \
             `the_trampoline_twin_is_a_rewrite_not_a_copy` is certifying nothing."
        )
    };
    // …and red for the RIGHT reasons: all three claims, not one doing the work.
    assert!(
        bad.iter().any(|b| b.contains("TOKEN-IDENTICAL")),
        "the re-copied twin was not detected as a token-for-token copy: {bad:?}"
    );
    assert!(
        bad.iter().any(|b| b.contains("NO SHARING")),
        "the re-copied twin still appears to call the shared `combine_*` helpers, which the \
         pre-trampoline body does not: {bad:?}"
    );
    assert!(
        bad.iter().any(|b| b.contains("NO COLLAPSE")),
        "the re-copied twin still appears collapsed relative to the original: {bad:?}"
    );
    println!(
        "rewrite check: GREEN under reformat, RED on re-copy — {} structural claim(s) rejected \
         it:\n{}",
        bad.len(),
        bad.iter()
            .map(|b| format!(
                "    - {}",
                b.trim_start().split('.').next().unwrap_or(b).trim()
            ))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
