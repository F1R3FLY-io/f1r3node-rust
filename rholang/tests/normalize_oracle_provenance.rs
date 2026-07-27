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
fn git_show(sha: &str, path: &str) -> String {
    let spec = format!("{sha}:{SOURCE_ROOT}{path}");
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
