//! # ★★ THE `Par` READ-CEILING SITE REGISTRY — the siblings, enumerated
//!
//! One root has now surfaced **four** times, independently:
//!
//! | # | where | shape |
//! |---|---|---|
//! | #120 | the audit's §7.3 | a term can be built, reduced and serialised but not read back |
//! | #129 | `output_value` | play accepts a depth-34 payload, replay refuses it |
//! | #130 | the trie's value slot | `decode_trie_path` is not total on `encode_trie_path`'s image |
//! | ★ **#4** | **`EPathMap`'s own prost wire** | **a map encodes at entry depth 34 and refuses to decode** — [`the_epathmap_wire_is_the_fourth_site`] |
//!
//! Each was found by walking into it. The standing lesson from this campaign is
//! that *"adding a guard ≠ enumerating siblings"* — seven sibling-blind patches
//! were shipped before it was written down. **This file is the enumeration**,
//! and it is a scan rather than a list so that a FIFTH site cannot be added
//! without someone deciding what it is.
//!
//! ## The class, stated precisely
//!
//! > A `Par` is **written by an unbounded encoder** and later **read by a
//! > bounded decoder**, with the two ends on different paths, different
//! > processes, or different times.
//!
//! Both halves matter. An encoder and decoder that are *both* unbounded are not
//! in the class, and this is not hypothetical: the **bincode** wire
//! (`models::rust::rholang::wire_encode` out, `par_codec` back) is iterative and
//! depth-unlimited in *both* directions, measured flat from depth 4 to 4,096.
//! ★ That is why the cold store never appears below: the codec campaign
//! (#46/#119/#121) already closed it, on both sides, from one generated table.
//! What remains open is **prost**, whose `RECURSION_LIMIT = 100` is private and
//! unconfigurable, and the **trie** codec, whose encoder is *documented total*
//! (R3F-2 — "trie keys must build for every legal runtime value") while its
//! decoder enforces `COLLECTION_DEPTH_LIMIT = 32`.
//!
//! ## What the scan does, and the trap it walks around
//!
//! It reads the production `src` trees, removes every `#[cfg(test)]` item's
//! span, and counts the bounded-reader spellings in [`READERS`].
//!
//! ⚠ **The exclusion rule is where this test could have quietly died.** The
//! first rule tried was *"truncate each file at its first `#[cfg(test)]`
//! module"*. `reduce.rs` has such a module at line **225 of 10,053**, so that
//! rule discarded 97 % of the largest interpreter file — including all five of
//! its `decode_trie_path` sites — and the scan came back clean and wrong.
//! [`the_test_module_exclusion_is_calibrated`] pins both directions: the rule
//! must exclude *something*, and it must still find the sites a
//! known-over-reaching rule loses.
//!
//! ## What a failure here means
//!
//! * **An undeclared site.** Somebody added a bounded read of a `Par`. Decide
//!   what feeds it and whether that writer is bounded, then add a row. That
//!   decision is the entire deliverable of this file.
//! * **A declared site that vanished.** Either it was removed (delete the row)
//!   or it moved to a spelling [`READERS`] does not know (add the spelling —
//!   and note that the scan is only as wide as that list, which is the one
//!   assumption this file cannot check for itself).
//!
//! ## Anti-vacuity ledger
//!
//! | # | mutation | result | caught by |
//! |---|---|---|---|
//! | M0 | *none* — the control | **green** | — |
//! | M1 | delete a row from [`REGISTRY`] | **red** | undeclared-site arm |
//! | M2 | inflate a row's count by one | **red** | count mismatch |
//! | M3 | add a row for a file with no sites | **red** | declared-but-absent arm |
//! | M4 | insert a real `Par::decode(` into `dispatch.rs` (a scanned production file) | **red** | count mismatch |
//! | M5 | revert the exclusion rule to "next column-0 `}`" | **red** | calibration, *and* two rows going absent |
//! | M6 | record the `EPathMap` boundary one level too high (33 → 34) | **red** | the fourth-site pin |
//!
//! ⚠★ **M3 first reported GREEN, and it was the harness lying, not the guard.**
//! The patch that was supposed to add the bogus row did not apply — its anchor
//! string had drifted from the source by one line-wrap — so a *no-op* was
//! recorded as "the mutation was not rejected". It was re-run with the patch
//! asserting its own application (`assert q != p`) and a byte-delta printed, and
//! it is red. This is the same failure the campaign has already met as *"the
//! control passed while the loop was broken"*: **a mutation that cannot be shown
//! to have been applied proves nothing about the guard it was aimed at.**

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, EPathMap, Expr, Par};
use models::rust::utils::new_gint_par;
use prost::Message;

// ---------------------------------------------------------------------------
// what is scanned
// ---------------------------------------------------------------------------

/// The production source trees. `tests/`, `benches/`, `fuzz/` and `target/` are
/// absent on purpose: a bounded read in a test is a *measurement of* the class,
/// not a member of it.
const SCANNED_ROOTS: &[&str] = &[
    "models/src",
    "rholang/src",
    "rspace++/src",
    "rspace++/libs",
    "casper/src",
    "node/src",
    "block-storage/src",
    "comm/src",
    "shared/src",
    "crypto/src",
    "rho-pure-eval/src",
    "graphz/src",
];

/// A bounded reader of `Par`-carrying bytes, by the spelling it is called with.
struct Reader {
    /// The literal the scan matches. A call, not an import — the trailing `(`
    /// keeps `use …::decode_trie_path;` out of the count.
    spelling: &'static str,
    /// What bounds it, and what it answers past the bound.
    bound: &'static str,
}

/// ⚠ **The one assumption this file cannot check for itself.** The scan is
/// exactly as wide as this list; a bounded read spelled some other way is
/// invisible to it. The list is short because the bounded readers are few:
/// prost's derived `decode` for the two `Par`-carrying messages that production
/// decodes, and the trie codec's entry point.
const READERS: &[Reader] = &[
    Reader {
        spelling: "Par::decode(",
        bound: "prost RECURSION_LIMIT = 100 message levels ⇒ term depth 33; \
                past it, DecodeError(RecursionLimitReached)",
    },
    Reader {
        spelling: "ListParWithRandom::decode(",
        bound: "prost RECURSION_LIMIT = 100, one level of envelope ⇒ term depth 32",
    },
    Reader {
        spelling: "decode_trie_path(",
        bound: "COLLECTION_DEPTH_LIMIT = 32 counted levels (⇒ 33 wrappers accepted); \
                past it, CodecError::DepthLimitExceeded — and its escape arm calls \
                Par::decode, so prost's limit applies inside it too",
    },
];

// ---------------------------------------------------------------------------
// the registry
// ---------------------------------------------------------------------------

/// What a site's write end looks like, which is what decides whether the site is
/// in the class at all.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Class {
    /// ★ In the class: an unbounded writer feeds it across a boundary, so bytes
    /// exist that this reader refuses and its writer produced.
    Asymmetric,
    /// The reader exists but no unbounded writer reaches it across a boundary —
    /// a definition, a debug assertion, a display path, or a reader whose
    /// refusal is absorbed rather than propagated.
    Bounded,
}

struct Site {
    /// Path relative to the workspace root.
    path: &'static str,
    /// Which member of [`READERS`].
    spelling: &'static str,
    /// How many calls the scan must find in this file's production half.
    count: usize,
    class: Class,
    /// ★ The write end, and what happens on refusal. This is the field that
    /// makes the row a decision rather than a line number.
    disposition: &'static str,
}

/// ★★ Every bounded read of a `Par` in production, with its write end named.
const REGISTRY: &[Site] = &[
    // ── #129: THE consensus-class member ────────────────────────────────────
    Site {
        path: "rholang/src/rust/interpreter/dispatch.rs",
        spelling: "Par::decode(",
        count: 1,
        class: Class::Asymmetric,
        disposition: "★★ #129. `decode_non_deterministic_output` — the trace's `output_value`. \
             WRITER: `dispatch_type`'s `encode_to_vec`, unbounded. BOUNDARY: the block \
             (`ProduceEventProto.outputValue` is `repeated bytes`, so the body decodes \
             without descending). REFUSAL: `Err` → `EvaluateResult::errors` → \
             `ReplayStatusMismatch` → `InvalidBlock::InvalidTransaction`, which IS \
             slashable. REACHABILITY: bounded at term depth 1 by measurement — see \
             `rholang/tests/output_value_write_side_reachability.rs`. \
             ⚠ This ONE site is the whole class inside the interpreter: it used to be \
             four copies (twice in `reduce.rs`, twice in `contract_call.rs`).",
    },
    // ── ★ THE FOURTH SITE: EPathMap's own prost wire ────────────────────────
    Site {
        path: "models/src/rust/rhoapi_ext.rs",
        spelling: "decode_trie_path(",
        count: 1,
        class: Class::Asymmetric,
        disposition: "★★★ THE FOURTH SITE, and the only one on a consensus WIRE FORMAT. \
             `impl prost::Message for EPathMap`'s `merge_field`, tag 8 \
             (`serialized_paths`) — the trie key stream a ground map encodes to. \
             WRITER: `encode_trie_path`, DOCUMENTED TOTAL and unlimited (R3F-2). \
             MEASURED by `the_epathmap_wire_is_the_fourth_site`: a map with a ground \
             entry of term depth 33 round-trips; at 34 `encode` emits 75 bytes and \
             `decode` answers `DepthLimitExceeded`. Same boundary as the bare `Par`, \
             a DIFFERENT reader and a DIFFERENT error. \
             ⚠ Which production path performs an EPathMap prost encode→decode across \
             a boundary is NOT established here; that is this site's open reachability \
             question, and it is the question #129's severity turned on.",
    },
    // ── #130: the trie codec itself ─────────────────────────────────────────
    Site {
        path: "models/src/rust/canonical_path.rs",
        spelling: "Par::decode(",
        count: 1,
        class: Class::Asymmetric,
        disposition: "★★ #130. `decode_trie_path`'s ESCAPE arm (`0x0F`), which reads a \
             ¬eval_stable payload back with prost. WRITER: `encode_trie_path`, total \
             and unlimited, which wrote those same bytes with `encode_to_vec`. This is \
             the asymmetry reaching INSIDE a data structure's own projection, and it \
             is what refuted #130's premise that the trie's value slot is a wasted \
             copy of its key.",
    },
    Site {
        path: "models/src/rust/canonical_path.rs",
        spelling: "decode_trie_path(",
        count: 1,
        class: Class::Bounded,
        disposition: "The reader's own definition (`pub fn decode_trie_path`), not a call of it. \
             Registered so the scan's arithmetic is complete rather than filtered.",
    },
    // ── the interpreter's zipper path composition ───────────────────────────
    Site {
        path: "rholang/src/rust/interpreter/reduce.rs",
        spelling: "decode_trie_path(",
        count: 5,
        class: Class::Bounded,
        disposition: "EZipper absolute-path composition and `getLeaf`. Every one of the five \
             ABSORBS refusal — `if let Ok(..)`, `.unwrap_or(composed_list_par)`, \
             `Err(_) => Ok(Par::default())` — so an over-deep key yields a DIFFERENT \
             ANSWER rather than an error. ⚠ That is a softer failure than #129's and \
             a harder one to notice; it is deterministic across nodes (same code, same \
             bound), so it is a correctness cliff, not a fork.",
    },
    // ── debug-only and display-only ─────────────────────────────────────────
    Site {
        path: "models/src/rust/pathmap_integration.rs",
        spelling: "decode_trie_path(",
        count: 2,
        class: Class::Bounded,
        disposition: "`cursor_entry_key`'s `debug_assert!` and the `entry_key_is_in_codec_image` \
             predicate it calls. Compiled out of release, so a consensus node never \
             runs them; in debug they are a CHECK of the image invariant, i.e. an \
             instrument pointed at this class rather than a member of it.",
    },
    Site {
        path: "rholang/src/rust/interpreter/pretty_printer.rs",
        spelling: "decode_trie_path(",
        count: 1,
        class: Class::Bounded,
        disposition: "Display. A refusal degrades rendering; nothing consensus-visible \
                      reads the result.",
    },
    Site {
        path: "rholang/src/rust/interpreter/pretty_printer_oracle.rs",
        spelling: "decode_trie_path(",
        count: 1,
        class: Class::Bounded,
        disposition: "The pretty-printer's differential oracle. Same disposition as the \
                      printer it checks.",
    },
    // ── FFI: the same read, but it PANICS ───────────────────────────────────
    Site {
        path: "rholang/src/lib.rs",
        spelling: "Par::decode(",
        count: 2,
        class: Class::Asymmetric,
        disposition: "`extern \"C\"` `get_data` / `get_joins`, both `.unwrap()`. ⚠ Same ceiling, \
             but the refusal is a PANIC across an FFI boundary rather than an `Err` — \
             strictly worse than #129's failure mode. The caller supplies the channel \
             bytes, and nothing bounds what it encodes. Both are annotated \
             \"FFI not used\" in situ; the row exists so that stops being a comment.",
    },
    Site {
        path: "rspace++/libs/rspace_rhotypes/src/lib.rs",
        spelling: "Par::decode(",
        count: 5,
        class: Class::Asymmetric,
        disposition: "The rspace FFI surface (`produce`/`consume`/`install`/`get_data` channel \
             and pattern arguments), every one `.unwrap()`. Same disposition as \
             `rholang/src/lib.rs`: a PANIC, not an `Err`, on bytes a caller encoded \
             with no matching bound.",
    },
    Site {
        path: "rspace++/libs/rspace_rhotypes/src/lib.rs",
        spelling: "ListParWithRandom::decode(",
        count: 2,
        class: Class::Asymmetric,
        disposition: "The same FFI surface's DATA argument. One envelope level tighter than the \
             bare `Par` (term depth 32, not 33), and also `.unwrap()`.",
    },
];

// ---------------------------------------------------------------------------
// the scan
// ---------------------------------------------------------------------------

fn workspace_root() -> PathBuf {
    // `CARGO_MANIFEST_DIR` is `<root>/rholang`; its parent is the workspace root.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the rholang crate directory has a parent")
        .to_path_buf()
}

/// `true` iff `line` opens a Rust item (the thing a `#[cfg(test)]` can sit on).
fn opens_an_item(line: &str) -> bool {
    let mut rest = line.trim_start();
    for prefix in ["pub ", "pub(crate) ", "pub(super) ", "pub(self) "] {
        if let Some(stripped) = rest.strip_prefix(prefix) {
            rest = stripped.trim_start();
            break;
        }
    }
    for prefix in ["async ", "unsafe ", "extern "] {
        if let Some(stripped) = rest.strip_prefix(prefix) {
            rest = stripped.trim_start();
        }
    }
    [
        "mod ", "fn ", "impl ", "impl<", "struct ", "enum ", "trait ", "const ", "static ", "use ",
    ]
    .iter()
    .any(|kw| rest.starts_with(kw))
}

/// Line indices (0-based) belonging to a `#[cfg(test)]` item.
///
/// An item's extent ends at the first following line consisting of `}` at
/// **exactly the item's own indentation**. That is sound for a `#[cfg(test)]`
/// applied at any nesting level — a top-level `mod tests {` closes at column 0,
/// an `#[cfg(test)] fn` inside an `impl` closes at the `impl`'s member
/// indentation — because rustfmt (enforced here by `rustfmt.toml`) puts a
/// block's closing brace at its opener's indentation. A one-line item (ending
/// in `;`, e.g. a `use`) is its own extent.
///
/// ⚠★ **Both simplifications of this rule were tried and both are wrong, in the
/// same silent direction.**
///
/// 1. *"Truncate the file at its first `#[cfg(test)] mod`."* `reduce.rs` has one
///    at line 225 of 10,053 ⇒ 97 % of the file disappears.
/// 2. *"End the extent at the next column-0 `}`."* Correct for top-level items,
///    but an INDENTED `#[cfg(test)] fn` inside an `impl` then runs to the end of
///    the whole `impl` ⇒ the rest of the impl disappears.
///
/// Both produce a clean scan and an empty finding.
/// [`the_test_module_exclusion_is_calibrated`] is what turned each of them from
/// a green suite into a failure.
fn cfg_test_item_lines(lines: &[&str]) -> Vec<bool> {
    let mut excluded = vec![false; lines.len()];
    let mut i = 0usize;
    while i < lines.len() {
        if lines[i].trim() == "#[cfg(test)]" {
            // Skip any further attributes / blank lines before the item.
            let mut j = i + 1;
            while j < lines.len()
                && (lines[j].trim().starts_with("#[") || lines[j].trim().is_empty())
            {
                j += 1;
            }
            if j < lines.len() && opens_an_item(lines[j]) {
                let end = if lines[j].trim_end().ends_with(';') {
                    j
                } else {
                    let indent: String =
                        lines[j].chars().take_while(|c| c.is_whitespace()).collect();
                    let closer = format!("{indent}}}");
                    let mut k = j;
                    while k < lines.len() && lines[k] != closer {
                        k += 1;
                    }
                    k.min(lines.len().saturating_sub(1))
                };
                for slot in excluded.iter_mut().take(end + 1).skip(i) {
                    *slot = true;
                }
                i = end + 1;
                continue;
            }
        }
        i += 1;
    }
    excluded
}

/// `(relative path, reader spelling) → line numbers`, over the production half
/// of every scanned file.
fn scan(exclude_cfg_test: bool) -> BTreeMap<(String, &'static str), Vec<usize>> {
    let root = workspace_root();
    let mut found: BTreeMap<(String, &'static str), Vec<usize>> = BTreeMap::new();
    let mut stack: Vec<PathBuf> = SCANNED_ROOTS.iter().map(|r| root.join(r)).collect();
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let lines: Vec<&str> = text.lines().collect();
            let excluded = if exclude_cfg_test {
                cfg_test_item_lines(&lines)
            } else {
                vec![false; lines.len()]
            };
            let relative = path
                .strip_prefix(&root)
                .expect("scanned paths live under the workspace root")
                .to_string_lossy()
                .into_owned();
            for (index, line) in lines.iter().enumerate() {
                if excluded[index] || line.trim_start().starts_with("//") {
                    continue;
                }
                for reader in READERS {
                    if line.contains(reader.spelling) {
                        found
                            .entry((relative.clone(), reader.spelling))
                            .or_default()
                            .push(index + 1);
                    }
                }
            }
        }
    }
    found
}

// ---------------------------------------------------------------------------
// 1. the exclusion rule is calibrated in BOTH directions
// ---------------------------------------------------------------------------

/// ★★ The rule that decides what "production" means must be shown to work, and
/// the failure it is guarding against is silence.
///
/// * It must **exclude something** — otherwise the scan is counting test code
///   and every registry count is noise.
/// * It must **not over-exclude** — `reduce.rs` has a `#[cfg(test)] mod` at line
///   225 of 10,053, so the natural rule *"truncate at the first test module"*
///   loses 97 % of the file and all five of its `decode_trie_path` sites. That
///   rule was tried, and it produced a clean, wrong scan.
#[test]
fn the_test_module_exclusion_is_calibrated() {
    let with = scan(true);
    let without = scan(false);

    let total_with: usize = with.values().map(|v| v.len()).sum();
    let total_without: usize = without.values().map(|v| v.len()).sum();
    assert!(
        total_without > total_with,
        "★ the `#[cfg(test)]` exclusion removed nothing ({total_without} hits either \
         way). Either every scanned file is free of test-side reads — which \
         `canonical_path.rs` alone refutes — or `cfg_test_item_lines` is not \
         matching, and the registry counts below are then counting test code."
    );

    // ★ The over-exclusion sentinel. These are production sites in a file whose
    // FIRST `#[cfg(test)] mod` sits near the top; a rule that truncates there
    // returns zero for this key.
    let reduce_sites = with
        .get(&(
            "rholang/src/rust/interpreter/reduce.rs".to_string(),
            "decode_trie_path(",
        ))
        .map(|v| v.len())
        .unwrap_or(0);
    assert_eq!(
        reduce_sites, 5,
        "★★ the exclusion rule found {reduce_sites} `decode_trie_path(` sites in \
         `reduce.rs`, not 5. That file's first `#[cfg(test)] mod` is at line 225 of \
         10,053: a rule that treats it as the end of production code discards the \
         rest of the file and this scan comes back clean and WRONG. If the sites \
         genuinely moved, update the registry — but check the rule first."
    );

    println!(
        "  exclusion calibrated: {total_without} hits raw, {total_with} after removing \
         `#[cfg(test)]` items; reduce.rs still contributes {reduce_sites}"
    );
}

// ---------------------------------------------------------------------------
// 2. THE ENUMERATION — the scan and the registry must agree exactly
// ---------------------------------------------------------------------------

/// ★★★ A fifth site cannot arrive unnoticed.
#[test]
fn every_bounded_par_read_in_production_is_registered() {
    assert!(
        !REGISTRY.is_empty() && !READERS.is_empty(),
        "an empty registry or reader list makes every assertion below vacuous"
    );

    let found = scan(true);
    assert!(
        !found.is_empty(),
        "★ the scan found NO bounded `Par` reads anywhere in {SCANNED_ROOTS:?}. The \
         registry names {} of them, so this is a scanner failure — a wrong root, a \
         path that no longer exists, or `CARGO_MANIFEST_DIR` not resolving to the \
         workspace root — not a clean tree.",
        REGISTRY.len()
    );

    let mut declared: BTreeMap<(String, &'static str), usize> = BTreeMap::new();
    for site in REGISTRY {
        let previous = declared.insert((site.path.to_string(), site.spelling), site.count);
        assert!(
            previous.is_none(),
            "two rows in REGISTRY declare `{}` in `{}`; one shadows the other",
            site.spelling,
            site.path
        );
    }

    let mut undeclared = Vec::new();
    let mut miscounted = Vec::new();
    for ((path, spelling), lines) in &found {
        match declared.get(&(path.clone(), spelling)) {
            None => undeclared.push(format!(
                "{path}: {} × `{spelling}` at {lines:?}",
                lines.len()
            )),
            Some(&expected) if expected != lines.len() => miscounted.push(format!(
                "{path}: `{spelling}` declared {expected}, found {} at {lines:?}",
                lines.len()
            )),
            Some(_) => {}
        }
    }

    assert!(
        undeclared.is_empty(),
        "★★★ UNDECLARED bounded `Par` read(s) in production:\n  {}\n\n\
         This root has surfaced four times already (#120, #129, #130, and \
         `EPathMap`'s prost wire). Adding one of these reads without saying what \
         feeds it is how the fifth arrives. Add a row to REGISTRY that names the \
         WRITER, says whether it is bounded, and says what the refusal does — \
         `Err`, panic, or a silently different answer.",
        undeclared.join("\n  ")
    );

    assert!(
        miscounted.is_empty(),
        "★★ bounded-read COUNT changed:\n  {}\n\n\
         A file that already has registered sites gained or lost one. The row's \
         disposition covers the sites that were there when it was written; \
         re-read it against what is there now.",
        miscounted.join("\n  ")
    );

    let mut absent = Vec::new();
    for site in REGISTRY {
        if !found.contains_key(&(site.path.to_string(), site.spelling)) {
            absent.push(format!("{}: `{}`", site.path, site.spelling));
        }
    }
    assert!(
        absent.is_empty(),
        "★ REGISTRY row(s) whose sites the scan cannot find:\n  {}\n\n\
         Either the read was removed (delete the row) or it is now spelled in a way \
         READERS does not match — and in that case the scan is blind to it, which \
         is worse than a stale row.",
        absent.join("\n  ")
    );

    let asymmetric = REGISTRY
        .iter()
        .filter(|s| s.class == Class::Asymmetric)
        .count();
    let sites: usize = found.values().map(|v| v.len()).sum();
    println!(
        "  {sites} bounded `Par` reads across {} file/reader pairs; {} of {} rows are \
         ASYMMETRIC (unbounded writer, bounded reader, real boundary)",
        found.len(),
        asymmetric,
        REGISTRY.len()
    );
    // ★ The dispositions are the deliverable, not the counts. Print them, so a
    // reader of the test output learns the enumeration rather than a tally —
    // and so a row whose disposition was never written shows up as blank.
    for site in REGISTRY.iter().filter(|s| s.class == Class::Asymmetric) {
        let bound = READERS
            .iter()
            .find(|r| r.spelling == site.spelling)
            .map(|r| r.bound)
            .expect("every registered spelling is a member of READERS");
        assert!(
            site.disposition.len() > 40,
            "★ `{}` (`{}`) has no real disposition. A row exists to say WHAT FEEDS \
             the read and WHAT THE REFUSAL DOES; without that it is a line number \
             with a checkbox.",
            site.path,
            site.spelling
        );
        println!("    ★ {}  `{}` ×{}", site.path, site.spelling, site.count);
        println!("        bound:       {bound}");
        println!("        disposition: {}", site.disposition);
    }
}

// ---------------------------------------------------------------------------
// 3. ★ THE FOURTH SITE, measured
// ---------------------------------------------------------------------------

fn elist(ps: Vec<Par>) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::EListBody(EList {
                ps,
                locally_free: vec![],
                connective_used: false,
                remainder: None,
            })),
        }],
        ..Default::default()
    }
}

/// `[[[…[0]…]]]` with `depth` bracket levels — the shape every ceiling in this
/// tree is stated in.
fn nested_list(depth: usize) -> Par {
    let mut par = new_gint_par(0, vec![], false);
    for _ in 0..depth {
        par = elist(vec![par]);
    }
    par
}

/// The last ground-entry term depth `EPathMap`'s prost decode accepts.
const EPATHMAP_LAST_DECODING_DEPTH: usize = 33;

/// ★★★ **The fourth independent site of the write/read asymmetry, and the only
/// one on a consensus WIRE FORMAT.**
///
/// `EPathMap` is a first-class Rholang value (`ExprInstance::EPathmapBody`). A
/// *ground* map encodes to proto field 8, `serialized_paths` — the stream of
/// `encode_trie_path` keys, an encoder that is **documented total and
/// unlimited** because "trie keys must build for every legal runtime value"
/// (R3F-2). `merge_field`'s tag-8 arm reads them back with `decode_trie_path`,
/// which enforces `COLLECTION_DEPTH_LIMIT`.
///
/// ⇒ The map encodes at every depth and stops decoding at one. That is #120's
/// sentence — *"built, reduced and serialised, but not read back"* — arriving at
/// a fourth place, by a **different reader** and with a **different error**
/// (`DepthLimitExceeded`, not `RecursionLimitReached`) than the three known
/// ones.
///
/// ⚠ **Honest status.** What is measured here is the CODEC asymmetry. Which
/// production path performs an `EPathMap` prost encode→decode *across a
/// boundary* is **not** established — and that is precisely the reachability
/// question that decided #129's severity, so it must not be assumed here by
/// analogy. The registry row records it as this site's open question.
#[test]
fn the_epathmap_wire_is_the_fourth_site() {
    // The accepting side, and its neighbours — so the boundary is exhibited
    // rather than asserted.
    for depth in [0usize, 1, 16, 32, EPATHMAP_LAST_DECODING_DEPTH] {
        let map = EPathMap::new(vec![nested_list(depth)], vec![], false, None);
        let bytes = map.encode_to_vec();
        let decoded = EPathMap::decode(&bytes[..]);
        assert!(
            decoded.is_ok(),
            "★ an EPathMap with a ground entry of term depth {depth} encoded to {} \
             bytes and did NOT decode: {:?}. The accepting side of this boundary is \
             supposed to reach {EPATHMAP_LAST_DECODING_DEPTH}; if the read ceiling \
             moved DOWN, values that round-tripped yesterday do not today.",
            bytes.len(),
            decoded.err()
        );
    }

    // ★ The refusing side. Encode must SUCCEED — that is the asymmetry; a test
    // that only showed "decode fails" could be satisfied by a failed build.
    let refused_depth = EPATHMAP_LAST_DECODING_DEPTH + 1;
    let map = EPathMap::new(vec![nested_list(refused_depth)], vec![], false, None);
    let bytes = map.encode_to_vec();
    assert!(
        !bytes.is_empty(),
        "★ the depth-{refused_depth} EPathMap encoded to nothing. The whole finding is \
         that the WRITE succeeds; without that this is not an asymmetry, it is a \
         builder failure."
    );

    let error = EPathMap::decode(&bytes[..]).expect_err(
        "★★ the depth-{refused_depth} EPathMap DECODED. The read ceiling moved UP: \
         this node now accepts EPathMap byte strings it refused before, which \
         changes the set a validator admits. That is a consensus-visible widening \
         and is F1r3node's coordinated decision, not an incidental one.",
    );

    // ★ The error KIND, not merely "something failed". A truncated buffer, a
    // wrong tag or a reserved byte would all satisfy a bare `is_err()` while
    // meaning something entirely different.
    let rendered = error.to_string();
    assert!(
        rendered.contains("DepthLimitExceeded"),
        "★ the depth-{refused_depth} EPathMap failed to decode, but not with the \
         depth limit: {rendered:?}. This test claims a DEPTH ceiling; any other \
         refusal is a different defect wearing its clothes."
    );
    assert!(
        rendered.contains("serialized_paths"),
        "★ the refusal did not come from the tag-8 `serialized_paths` arm: \
         {rendered:?}. The finding is specifically about the trie key stream a \
         GROUND map encodes to; a refusal from the tag-1 field walk would mean the \
         map was not taking the field-8 arm and this measurement is of something else."
    );

    println!(
        "  EPathMap prost wire: ground entry depth {EPATHMAP_LAST_DECODING_DEPTH} \
         round-trips; depth {refused_depth} encodes to {} bytes and decodes to \
         {rendered}",
        bytes.len()
    );
    println!(
        "  ⇒ encode is TOTAL (R3F-2), decode is bounded — the same shape as #120, \
         #129 and #130, at a fourth site"
    );
}
