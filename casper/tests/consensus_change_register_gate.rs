//! ★★ **The consensus-change register's DRIFT GATE.**
//!
//! @watches: docs/consensus/ Cargo.lock Cargo.toml block-storage/ casper/ comm/ crypto/ graphz/ models/ rho-pure-eval/ rholang/ rspace++/ shared/ .github/workflows/
//!
//! ★ **The `@watches:` line above is a ROUTING declaration, not documentation.** This gate
//! went red *correctly and immediately* three times and nobody was told, because no delivery
//! channel existed: no git hook had ever run in either repo, and neither working branch had
//! an upstream, so CI had observed none of the work. The post-commit gate router
//! (`scripts/gate-router-post-commit`) derives its gate → domain map by grepping for this
//! line, and a hook's stdout reaches the COMMITTER by construction — which is the whole
//! mechanism, because every concurrent agent commits under the same git identity, so
//! `%an`/`%cn` carries no routing information at all.
//!
//! ⚠ **The path set is DERIVED, not chosen** — and it was RE-DERIVED on 2026-07-30, because
//! the previous derivation was faithful to its own recipe and the recipe covered only one of
//! this gate's two path-sensitive clauses. It is now the union of THREE sources:
//!
//! * **(a) `docs/consensus/`** — the prose and index this gate parses.
//! * **(b) the top-level prefixes of `path =` rows still pinned at `at = "HEAD"`.** ★ This
//!   set SHRANK from eight prefixes to four: 20 entry-owned citation rows were repinned to
//!   fixed SHAs (register §7.7.5's rule 3, which had been stated and not applied), and **a
//!   SHA-pinned coordinate cannot go stale**, so watching its file is noise. Only the nine
//!   `(doc)`-owned rows still open a file at `HEAD` — by design, since a document-level
//!   citation is a claim about the tree *now*.
//! * **(c) ★★ THE OBLIGATION SET — which the previous derivation OMITTED ENTIRELY.** The
//!   frontier and coverage clauses fire on any commit touching the `path =` closure of
//!   `casper` (ten crates), minus `tests/`, `benches/` and `src/test/`, plus `Cargo.lock`.
//!   Those commits *owe a register row*, so their committer is exactly who should be told.
//!   `Cargo.toml` is included because [`workspace_members`] and [`path_dependencies`] derive
//!   the closure FROM it, so a `members` or `path =` edit silently moves the obligation set.
//!
//! ⚠ **Why the omission mattered, measured rather than argued.** Of the six obligation-set
//! commits on the living frontier during the 2026-07-30 session, **three would NOT have been
//! routed** by the previous set — `87ee699c`, `1eb65221` and `88ec2734`, every one of them
//! because they touch `models/codegen/schema_codegen.rs` while the declaration listed the *file*
//! `models/build.rs` and no prefix covering the *directory* `models/codegen/`. ★ That is the
//! "too narrow" failure the routing brief warned about, and it was invisible because the
//! recipe that produced the line never mentioned the clause it left uncovered.
//!
//! ⚠ **Crate ROOTS, not `<crate>/src/`** — the same reason [`obligation_pathspec`] is
//! crate-rooted: a `src`-rooted prefix loses `models/codegen/`, the GENERATOR that emits the
//! wire tables, which is precisely the path all three unrouted commits touched.
//!
//! Re-derive it, do not extend it by hand:
//!
//! ```text
//! # (a)
//! echo docs/consensus/
//! # (b) — only rows still opened at HEAD
//! awk '/^\[\[citation\]\]/{a="";p=""} /^at = /{a=$3} /^path = /{p=$3} \
//!      /^$/{if(a=="\"HEAD\"" && p!="") print p}' docs/consensus/register.toml \
//!   | tr -d '"' | cut -d/ -f1-2 | sort -u
//! # (c) — the closure of `casper` along `path =`, crate-rooted, plus Cargo.lock/Cargo.toml
//! #       (the authoritative computation is `obligation_pathspec()` in this file)
//! ```
//!
//! ★ **A path in (c) but not in (b) still belongs here**, and the distinction is worth
//! keeping in mind when trimming: (b) says *"a claim in the register points into this file"*,
//! (c) says *"a commit to this file owes the register a claim"*. The second is the larger
//! obligation and the one that had no routing at all.
//!
//! `docs/consensus/consensus-change-register.md` §7.1 states the requirement this file
//! discharges:
//!
//! > A commit that lands on a consensus-critical path without a corresponding register entry
//! > or a typed exemption must cause a **build failure that names the commit**, not a
//! > discrepancy somebody may notice later.
//!
//! §7.2 specified the gate and then said, of itself, *"Status: DESIGNED, NOT BUILT … Until
//! they exist, this register is exactly the discipline-dependent artefact §1.2 argues
//! against."* This is the build.
//!
//! # What it checks, and against what
//!
//! Three artefacts, and the gate is the only thing that reads all three:
//!
//! | artefact | role |
//! |---|---|
//! | `docs/consensus/consensus-change-register.md` | the human record. **The domain.** |
//! | `docs/consensus/register.toml` | the machine index. **The pins.** |
//! | `git` | the repository. **The ground truth.** |
//!
//! Every clause is a set operation over `git` output and those two files. None needs a
//! build, a network call, or a judgement — the property §7.3 trades everything else for.
//!
//! # The five witnessed drift classes
//!
//! These are not hypothetical. Each one happened, was caught by a human, and is on file.
//!
//! 1. **IN-FLIGHT STALENESS.** CBR-027 read *"IN FLIGHT — not present in the tree at the
//!    time of writing"* while `6ff46f8a` had already landed. ⇒ [`check_in_flight_staleness`].
//! 2. **TRANSCRIBED `file:line` NON-RE-DERIVABILITY.** CBR-027's `Files` row cited
//!    `wrapping_add` / `wrapping_sub` coordinates that the fix had deleted. ⇒
//!    [`check_citation_rederivability`].
//! 3. **PARTIAL-UPDATE DRIFT.** §5.1 still read *"Share of the 40"* while the paragraph
//!    beside it had been recounted to 44. ⇒ [`check_derived_figures`].
//! 4. **A STALE PROSE CLAIM ABOUT THE WORLD.** CBR-L09's residual 3 said the `NaN`
//!    comparison divergence awaited a ruling; `19510082` had resolved it **46 minutes**
//!    after the commit the row was written against. ⇒ [`check_open_question_freshness`],
//!    and read its documentation before trusting it: the class is **only partly**
//!    decidable here, and the undecidable part is named and counted rather than papered
//!    over.
//! 5. **A JUSTIFICATION THAT WAS WRONG WHEN WRITTEN.** CBR-006 §(c) argued that
//!    `has_locally_free` needed no change, and the argument was refuted by its own
//!    commit's message two paragraphs earlier. ⇒ **NOT CHECKED, and cannot be.** See
//!    [`the_gate_states_the_classes_it_cannot_cover`] and register §7.6 finding 5.
//!
//! # ★ Two limits, stated here as prominently as the coverage
//!
//! A gate whose advertised coverage exceeds its real coverage is worse than no gate.
//!
//! * **Cross-repository rows are unenforceable.** 13 of the register's entries are
//!   Surface L (`mettail-rust`). Their commits do not resolve here — that is asserted
//!   *positively* by [`check_foreign_rows`], so a Surface-L row that accidentally names an
//!   f1r3node SHA fails. What cannot be checked is whether those entries are **true**.
//!   12 of the 13 carry `source_gate = "NOT_NAMED"`, which is the measured size of the
//!   gap.
//! * **The task tracker is not read, and cannot be.** Nothing in this repository reads it,
//!   *which is precisely why the tracker is the copy that drifted furthest.* A clause that
//!   consulted it would need a network call and would make CI's verdict a function of a
//!   mutable database — see [`check_open_question_freshness`]'s rejection of candidate (a).
//!
//! # Why this file lives in `casper/tests/`
//!
//! §7.2 nominated `shared/tests/` because `shared` "does not depend on `models`, `rholang`
//! or `rspace++`, so the gate cannot be broken by the code it polices". `casper` **can** be
//! broken by the code it polices — `casper/src/` is in the path set. The trade, stated
//! rather than hidden:
//!
//! * `casper` owns the consensus decision the register is *about*.
//!   `casper/src/rust/validate.rs:273` is §5.5's conjunction argument, cited twice in the
//!   register; block admission and replay comparison are here too. Subject and home coincide.
//! * `casper/tests/` already holds the repository's cross-cutting audit gates that read
//!   artefacts rather than exercise code (`system_deploy_error_message_determinism.rs`,
//!   `deploy_ingress_depth_ceiling.rs`), and the typed-decision idiom this file follows
//!   (`casper/tests/genesis/contracts/rho_spec_floor_spec.rs`'s `FloorBreach`).
//! * `casper` is in CI's crate matrix, and CI checks out **full history** for the whole
//!   `test` job — not for `rholang` alone — which the `git` clauses require.
//! * ⚠ The cost: this file `use`s nothing from `casper`, so it has no *semantic* dependency
//!   on the code it polices; what remains is a *build* dependency. A `casper` that does not
//!   compile is already a hard CI failure, so the gate is unavailable only in a state that
//!   is red anyway. That is a weaker guarantee than `shared` would have given, and it is
//!   the reason it is written down here.
//!
//! # ⚠ No test in this file expects a panic
//!
//! Every clause is a pure function returning [`Result<(), DriftBreach>`], and every guard
//! asserts on the returned **value**. A `#[should_panic]` guard cannot say *which* refusal
//! it caught and would go green on an unrelated failure — and in this workspace a `panic!`
//! does not unwind across the `proc_macro` bridge under cranelift, so the harness can abort
//! printing nothing. Same rule as `rho_spec_floor_spec.rs`.
//!
//! # ★★ The non-vacuity floor
//!
//! A gate run that finds nothing **FAILS**. Without that, a gate that stops scanning passes
//! forever — which is the defect it exists to prevent, applied to itself. Four floors:
//! the obligation set, the index rows, the citation corpus, and the decidable open-question
//! set. Each is watched RED in [`the_floor_refuses_an_empty_obligation_set`] and its
//! siblings.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

// ════════════════════════════════════════════════════════════════════════════════════════
// 1 · The typed decision
// ════════════════════════════════════════════════════════════════════════════════════════

/// A refusal, as a **value**.
///
/// Every clause returns `Result<(), DriftBreach>`; nothing here panics on a policy
/// violation. The variants are deliberately specific — "the register drifted" is not a
/// diagnosis, and a guard that cannot name which clause fired has not been shown to work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriftBreach {
    /// The obligation set is empty or below its floor: the path set or the anchor is wrong.
    EmptyObligation { found: usize, floor: usize },
    /// A table in `register.toml` has no rows, so every assertion over it is vacuous.
    EmptyIndexTable { table: &'static str },
    /// `register.toml` violated the declared grammar. Never a silent skip.
    Malformed {
        line_number: usize,
        line: String,
        why: &'static str,
    },
    /// Commits in range touching a consensus-critical path that appear in no row.
    Unregistered {
        window: &'static str,
        commits: Vec<(String, String)>,
    },
    /// A SHA is both an entry commit and an exemption.
    DoubleListed { commits: Vec<String> },
    /// A row names a SHA that resolves here but is **not** an ancestor of `HEAD` — what a
    /// rebase or an abandoned branch produces.
    AbandonedRow { commits: Vec<String> },
    /// **Class 1.** An entry says `IN_FLIGHT`/`OPEN` while a SHA it names has landed.
    InFlightButLanded { offenders: Vec<(String, String)> },
    /// A Surface-L row names a SHA that resolves in *this* repository.
    ForeignShaResolvesLocally { offenders: Vec<(String, String)> },
    /// A Surface-L row carries neither a named source gate nor the `NOT_NAMED` token.
    ForeignRowWithoutSourceGate { ids: Vec<String> },
    /// **Class 2.** A cited coordinate no longer contains the token the prose names.
    StaleCitation { offenders: Vec<StaleCitation> },
    /// The citation corpus and the document's coordinate set disagree.
    CitationCorpusDrift {
        missing_rows: Vec<String>,
        extra_rows: Vec<String>,
    },
    /// **Class 3.** A stated figure disagrees with the projection of the summary table.
    FigureDisagrees {
        site: String,
        quantity: String,
        stated: i64,
        projected: i64,
    },
    /// An arithmetic identity over the summary table failed: a row failed to parse, or a
    /// cell is malformed. Checked *before* any comparison — the instrument before the
    /// measurement.
    TableIdentityFailed {
        quantity: String,
        sum: usize,
        rows: usize,
    },
    /// **Class 4, decidable part.** An open question's falsifier no longer holds.
    OpenQuestionNoLongerOpen {
        number: i64,
        path: String,
        line: i64,
        token: String,
    },
    /// **Class 4, decidable part.** A `CLOSED` question's falsifier still holds.
    ClosedQuestionStillOpen {
        number: i64,
        path: String,
        line: i64,
        token: String,
    },
    /// The set of open questions the gate declines to check is not the declared set.
    UndecidableSetDrift {
        expected: usize,
        found: usize,
        kinds: Vec<String>,
    },
    /// An exemption carries a reason outside §3.2's closed enum.
    UntypedExemption { commit: String, reason: String },
    /// An exemption carries no evidence. A row that excuses without discharging is a shrug.
    UndischargedExemption { commit: String },
    /// An entry is missing an axis cell, or a cell is outside §4.2's closed vocabulary.
    BadAxisCell {
        id: String,
        axis: String,
        value: String,
    },
    /// §6.4's budget moved without the diff that raising it requires.
    UnverifiedBudget { stated: i64, counted: usize },
    /// The prose headings and the index ids disagree.
    ProseIndexDivergence {
        prose_only: Vec<String>,
        index_only: Vec<String>,
    },
    /// ★ **§4.1's ROW SET and the index's entry set disagree.** Distinct from
    /// [`Self::ProseIndexDivergence`], which compares the *headings*: an entry can have a
    /// heading, a body and an index row and still be missing from the glyph table — which is
    /// exactly what happened to **CBR-040**, undetected until a human read the table.
    SummaryTableDivergence {
        summary_only: Vec<String>,
        index_only: Vec<String>,
    },
    /// An entry's index row disagrees with the §4.1 row that displays it.
    SummaryRowDisagrees {
        id: String,
        field: &'static str,
        prose: String,
        index: String,
    },
    /// A typed path exclusion excludes nothing, so the row is dead weight.
    VacuousPathExclusion { subdir: &'static str },
    /// A commit on the living frontier is unregistered and older than the fuse.
    FrontierFuseBlown {
        commits: Vec<(String, String, i64)>,
        grace_days: i64,
    },
    /// A stated-figure anchor does not occur exactly once, so the figure it names is
    /// ambiguous. ★ Checked because an anchor that starts matching twice would otherwise
    /// silently make the comparison a coin flip.
    AnchorNotUnique {
        site: String,
        anchor: String,
        count: usize,
    },
}

/// One stale citation, carrying everything a reader needs to fix it without re-deriving.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleCitation {
    pub owner: String,
    pub path: String,
    pub line: i64,
    pub at: String,
    pub token: String,
    /// What is actually at the cited line, so the message names the fix.
    pub found: String,
}

impl fmt::Display for DriftBreach {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyObligation { found, floor } => write!(
                f,
                "non-vacuity: the obligation set holds {found} commit(s), floor is {floor}. A \
                 gate that finds nothing to check cannot fail, so the derived path set or \
                 `register_base` is wrong — not the register."
            ),
            Self::EmptyIndexTable { table } => write!(
                f,
                "non-vacuity: `register.toml` has no `[[{table}]]` rows, so every assertion \
                 over that table compared two empty sets."
            ),
            Self::Malformed {
                line_number,
                line,
                why,
            } => write!(
                f,
                "`register.toml` line {line_number} is not in the declared grammar ({why}): \
                 {line:?}. The parser refuses what it does not recognise rather than \
                 skipping it, because a skipped row is an unasserted row."
            ),
            Self::Unregistered { window, commits } => {
                write!(
                    f,
                    "{} commit(s) in the {window} window touch a consensus-critical path and \
                     appear in no entry and no exemption:",
                    commits.len()
                )?;
                for (sha, subject) in commits {
                    write!(f, "\n  {sha}  {subject}")?;
                }
                write!(
                    f,
                    "\n\nEither add a register entry, or add an `[[exempt]]` row with a typed \
                     reason from §3.2 and the evidence discharging it."
                )
            }
            Self::DoubleListed { commits } => write!(
                f,
                "double-listed: {commits:?} appear as both an entry commit and an exemption. \
                 The partition must be exact."
            ),
            Self::AbandonedRow { commits } => write!(
                f,
                "stale row: {commits:?} resolve in this repository but are NOT ancestors of \
                 HEAD, which is what a rebase or an abandoned branch produces. The row \
                 describes a commit no longer in the history it claims."
            ),
            Self::InFlightButLanded { offenders } => {
                write!(f, "class 1 — IN-FLIGHT STALENESS:")?;
                for (id, sha) in offenders {
                    write!(
                        f,
                        "\n  {id} is not LANDED, yet {sha} is an ancestor of HEAD"
                    )?;
                }
                write!(
                    f,
                    "\n\nThis is CBR-027's first drift, exactly. Update the entry's Status, or \
                     remove a SHA it does not own."
                )
            }
            Self::ForeignShaResolvesLocally { offenders } => write!(
                f,
                "a Surface-L row names a SHA that resolves HERE: {offenders:?}. Surface-L \
                 commits belong to `mettail-rust`; one that resolves locally means the \
                 surface, or the SHA, is wrong."
            ),
            Self::ForeignRowWithoutSourceGate { ids } => write!(
                f,
                "Surface-L entries {ids:?} carry no `source_gate`. This gate cannot check a \
                 cross-repository row, so the row must at least NAME what does — or say \
                 `NOT_NAMED`, which is counted."
            ),
            Self::StaleCitation { offenders } => {
                write!(
                    f,
                    "class 2 — {} NON-RE-DERIVABLE citation(s):",
                    offenders.len()
                )?;
                for c in offenders {
                    write!(
                        f,
                        "\n  [{}] {}:{} at {} claims {:?}\n        but the ±{CITATION_WINDOW} \
                         line window there reads {:?}",
                        c.owner, c.path, c.line, c.at, c.token, c.found
                    )?;
                }
                write!(
                    f,
                    "\n\nA coordinate a reviewer would follow no longer leads anywhere. Repair \
                     the coordinate, or repin `at` if the claim is about a pre-change state."
                )
            }
            Self::CitationCorpusDrift {
                missing_rows,
                extra_rows,
            } => write!(
                f,
                "the citation corpus is not the document's coordinate set.\n  in the prose \
                 with no `[[citation]]` row: {missing_rows:?}\n  rows citing a coordinate the \
                 prose no longer contains: {extra_rows:?}\n\nThe corpus is DERIVED from the \
                 document so it cannot be narrowed by deleting a row."
            ),
            Self::FigureDisagrees {
                site,
                quantity,
                stated,
                projected,
            } => write!(
                f,
                "class 3 — PARTIAL-UPDATE DRIFT at {site}: it states {quantity} = {stated}, \
                 the §4.1 table projects {projected}. ★ A number that is a projection of a \
                 table must be COMPUTED by a machine that reads the table."
            ),
            Self::TableIdentityFailed {
                quantity,
                sum,
                rows,
            } => write!(
                f,
                "the instrument is broken before the measurement: the {quantity} split sums \
                 to {sum} over {rows} rows. A row failed to parse, an identifier is \
                 duplicated, or an axis cell is malformed — fix that before reading any \
                 figure comparison."
            ),
            Self::OpenQuestionNoLongerOpen {
                number,
                path,
                line,
                token,
            } => write!(
                f,
                "class 4 — open question {number} is recorded as still open, but its falsifier \
                 no longer holds: {token:?} is gone from {path}:{line}. The question may have \
                 been answered by a change that never came back to update the row — which is \
                 the CBR-L09 shape."
            ),
            Self::ClosedQuestionStillOpen {
                number,
                path,
                line,
                token,
            } => write!(
                f,
                "open question {number} is recorded CLOSED, but its falsifier still holds: \
                 {token:?} is still at {path}:{line}. A closure that did not happen is worse \
                 than an open question, because nobody is looking."
            ),
            Self::UndecidableSetDrift {
                expected,
                found,
                kinds,
            } => write!(
                f,
                "the set of open questions this gate DECLINES to check has changed: expected \
                 {expected}, found {found} ({kinds:?}). ★ This set is the gate's honest \
                 coverage gap and is asserted EXACTLY, so it cannot grow quietly."
            ),
            Self::UntypedExemption { commit, reason } => write!(
                f,
                "untyped exemption: {commit} carries reason {reason:?}, which is not in §3.2's \
                 closed enum. A free-text reason is a shrug; adding a variant is a code change \
                 and therefore reviewed."
            ),
            Self::UndischargedExemption { commit } => write!(
                f,
                "undischarged exemption: {commit} carries no evidence. A row that excuses a \
                 commit without naming the test or measurement that discharges it excuses \
                 nothing."
            ),
            Self::BadAxisCell { id, axis, value } => write!(
                f,
                "{id}'s axis `{axis}` is {value:?}, which is not in §4.2's closed vocabulary \
                 MOVES / NO / N/A / UNVERIFIED."
            ),
            Self::UnverifiedBudget { stated, counted } => write!(
                f,
                "§6.4's UNVERIFIED budget is {stated}; the table projects {counted}. ★ A \
                 budget that can only rise is a ratchet, not a measurement — so this is \
                 asserted EXACTLY, and BOTH directions are a visible diff."
            ),
            Self::ProseIndexDivergence {
                prose_only,
                index_only,
            } => write!(
                f,
                "prose/index divergence.\n  headings with no index row: {prose_only:?}\n  index \
                 rows with no heading: {index_only:?}"
            ),
            Self::SummaryTableDivergence {
                summary_only,
                index_only,
            } => write!(
                f,
                "§4.1 GLYPH-ROW divergence — the summary table and `register.toml` do not hold \
                 the same entries.\n  §4.1 rows with no index entry: {summary_only:?}\n  index \
                 entries with NO §4.1 GLYPH ROW: {index_only:?}\n\
                 ★ The second list is the one CBR-040 was on. An entry can have a heading, a \
                 full body and an index row and still be invisible in the one table §5's \
                 figures are projected from — so every share, every count and every percentage \
                 computed over §4.1 would be short by exactly these entries, silently. Add the \
                 missing row to §4.1 (or the missing `[[entry]]` to `register.toml`); do not \
                 relax this clause."
            ),
            Self::SummaryRowDisagrees {
                id,
                field,
                prose,
                index,
            } => write!(
                f,
                "{id}: §4.1's row says {field} = {prose:?}, `register.toml` says {index:?}. \
                 The index is a PROJECTION of the prose; a disagreement means one of them was \
                 edited alone."
            ),
            Self::VacuousPathExclusion { subdir } => write!(
                f,
                "the `{subdir}` path exclusion excludes nothing in range, so the row carries \
                 no obligation and is dead weight. Delete it, or say why it must stay."
            ),
            Self::FrontierFuseBlown {
                commits,
                grace_days,
            } => {
                write!(
                    f,
                    "{} unregistered commit(s) on the living frontier are older than the \
                     {grace_days}-day fuse:",
                    commits.len()
                )?;
                for (sha, subject, age) in commits {
                    write!(f, "\n  {sha}  ({age} d)  {subject}")?;
                }
                write!(
                    f,
                    "\n\nThe fuse is measured in COMMIT DATES, not wall clock, so this verdict \
                     is reproducible on this checkout. Register them, or exempt them."
                )
            }
            Self::AnchorNotUnique {
                site,
                anchor,
                count,
            } => write!(
                f,
                "the stated-figure anchor for {site} occurs {count} time(s), not once: \
                 {anchor:?}. A figure comparison against an ambiguous anchor is a coin flip, \
                 so it is a failure instead."
            ),
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════════════════
// 2 · git
// ════════════════════════════════════════════════════════════════════════════════════════

/// The repository root. `CARGO_MANIFEST_DIR` is `<root>/casper`.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the `casper` crate always has a parent directory")
        .to_path_buf()
}

/// `git <args>`, with the fsmonitor disabled so a stale daemon cannot change the answer.
///
/// ⚠ **Fails loudly when history is unavailable, and never skips.** A check that silently
/// passes on a shallow clone is a check that cannot fail — the same rule
/// `rholang/tests/normalize_oracle_provenance.rs` states, for the same reason. CI checks
/// out full history (`fetch-depth: 0`) for the whole `test` job.
fn git(args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .args(["-c", "core.fsmonitor=false"])
        .args(args)
        .current_dir(repo_root())
        .output()
        .unwrap_or_else(|e| panic!("could not run `git {}`: {e}", args.join(" ")));
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

fn git_expect(args: &[&str]) -> String {
    git(args).unwrap_or_else(|e| {
        panic!(
            "\n★ HISTORY UNAVAILABLE — `git {}` failed: {e}\n\n\
             This gate does NOT skip when history is missing. If this is CI the checkout is \
             shallow — set `fetch-depth: 0`. If this is a local clone, run \
             `git fetch --unshallow`.\n",
            args.join(" ")
        )
    })
}

/// Does `sha` name a commit object in this repository?
///
/// ⚠ Distinct from "is an ancestor". `merge-base --is-ancestor` exits non-zero for BOTH a
/// negative answer and an unknown revision, and §4.1's out-of-range paragraph conflated the
/// two for four `mettail-rust` SHAs. The gate keeps them apart.
fn resolves(sha: &str) -> bool { git(&["cat-file", "-e", &format!("{sha}^{{commit}}")]).is_ok() }

fn is_ancestor(sha: &str, of: &str) -> bool {
    resolves(sha) && git(&["merge-base", "--is-ancestor", sha, of]).is_ok()
}

fn show(at: &str, path: &str) -> Option<String> { git(&["show", &format!("{at}:{path}")]).ok() }

/// ⚠⚠ **A REVISION ARGUMENT MUST BE FENCED WITH `--`, OR A FILE CAN IMPERSONATE A COMMIT.**
///
/// `git log -1 --format=%ct <sha>` is ambiguous the moment a path named `<sha>` exists in the
/// working tree, and git refuses rather than guessing:
///
/// ```text
/// fatal: ambiguous argument 'ff244c69': both revision and filename
/// ```
///
/// That is not hypothetical: on 2026-07-30 a commit hook wrote its JSONL ledger to a file
/// literally named `$sha` in the repository root, and the gate answered with
///
/// ```text
/// ★ HISTORY UNAVAILABLE — `git log -1 --format=%ct ff244c69` failed: …
/// If this is CI the checkout is shallow — set `fetch-depth: 0`.
/// ```
///
/// ★ **The diagnosis was wrong, and confidently so.** History was complete; one stray
/// untracked file was shadowing a revision. A gate whose refusal names the wrong cause sends
/// its reader to `git fetch --unshallow`, which cannot help, and the real cause — an eight-byte
/// filename — is invisible in the message. Appending `--` makes the argument unambiguously a
/// revision, so no file in any tree can ever change this gate's verdict.
///
/// Every rev-taking helper below routes through here for that reason.
fn rev_only(args: &[&str]) -> String {
    let fenced = fenced_args(args);
    git_expect(&fenced.iter().map(String::as_str).collect::<Vec<_>>())
}

/// The argument vector [`rev_only`] hands to git — split out so the fence is a VALUE that can
/// be asserted, not a line that has to be read.
fn fenced_args(args: &[&str]) -> Vec<String> {
    let mut fenced: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    fenced.push("--".to_string());
    fenced
}

/// A git subcommand this gate invokes WITHOUT a `--` fence, and the reason it is safe to.
///
/// ★ A typed exception table rather than a naked allowance: each row states why the subcommand
/// cannot confuse a path for a revision, so [`no_unfenced_git_call_can_be_shadowed_by_a_path`]
/// can hold the set EXACTLY and a new unfenced call has to justify itself.
struct UnfencedSubcommand {
    subcommand: &'static str,
    why: &'static str,
}

const UNFENCED_SUBCOMMANDS: &[UnfencedSubcommand] = &[
    UnfencedSubcommand {
        subcommand: "cat-file",
        why: "takes OBJECT names only and accepts no pathspec; the call additionally writes \
              `<sha>^{commit}`, a peel suffix that is not a legal filename component in this \
              position, so the argument cannot parse as a path.",
    },
    UnfencedSubcommand {
        subcommand: "merge-base",
        why: "takes COMMIT arguments only and accepts no pathspec, so there is no path/rev \
              grammar for a file to sit in.",
    },
    UnfencedSubcommand {
        subcommand: "show",
        why: "the call writes the unambiguous `<rev>:<path>` object form, which git parses as \
              one object name and never as two arguments.",
    },
];

fn subject(sha: &str) -> String {
    rev_only(&["log", "-1", "--format=%s", sha])
        .trim()
        .to_string()
}

/// Committer date as a Unix timestamp.
fn commit_time(sha: &str) -> i64 {
    rev_only(&["log", "-1", "--format=%ct", sha])
        .trim()
        .parse()
        .unwrap_or_else(|e| panic!("unparseable commit time for {sha}: {e}"))
}

// ════════════════════════════════════════════════════════════════════════════════════════
// 3 · The strict `register.toml` parser
// ════════════════════════════════════════════════════════════════════════════════════════

/// A scalar in the index. The grammar admits exactly these three shapes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Val {
    Str(String),
    Int(i64),
    List(Vec<String>),
}

impl Val {
    fn as_str(&self) -> &str {
        match self {
            Self::Str(s) => s,
            _ => "",
        }
    }
    fn as_int(&self) -> i64 {
        match self {
            Self::Int(i) => *i,
            _ => i64::MIN,
        }
    }
    fn as_list(&self) -> &[String] {
        match self {
            Self::List(v) => v,
            _ => &[],
        }
    }
}

pub type Row = BTreeMap<String, Val>;

/// The parsed index: a header of scalars, plus one vector of rows per array-of-tables.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Index {
    pub header: Row,
    pub tables: BTreeMap<String, Vec<Row>>,
}

impl Index {
    fn rows(&self, table: &str) -> &[Row] {
        self.tables.get(table).map(Vec::as_slice).unwrap_or(&[])
    }
    fn int(&self, key: &str) -> i64 { self.header.get(key).map(Val::as_int).unwrap_or(i64::MIN) }
    fn str(&self, key: &str) -> &str { self.header.get(key).map(Val::as_str).unwrap_or("") }
}

/// Parse the declared subset of TOML, **refusing** anything outside it.
///
/// # Why hand-rolled rather than the `toml` crate
///
/// No workspace crate depends on a TOML parser, so using one means an edge in
/// `casper/Cargo.toml`, which puts the gate's ability to *run* behind a dependency the gate
/// does not need. The grammar the index uses is four productions wide, and a parser that
/// **fails on anything it does not recognise** cannot silently skip a row — which is the
/// property that matters, and is the opposite of what a permissive parser gives.
///
/// # The grammar, in full
///
/// ```text
/// file    ::= line*
/// line    ::= comment | blank | table | assign
/// comment ::= '#' .*
/// table   ::= '[[' ident ']]'
/// assign  ::= ident '=' value
/// value   ::= '"' .* '"' | digit+ | '[' string (',' string)* ']'
/// ```
///
/// An `assign` before any `table` populates the header. Inline tables, nesting, dotted keys
/// and multi-line values are refused, and the refusal names the line.
pub fn parse_index(text: &str) -> Result<Index, DriftBreach> {
    let mut index = Index::default();
    let mut open: Option<(String, Row)> = None;

    for (n, raw) in text.lines().enumerate() {
        let line_number = n + 1;
        let line = raw.trim_end();
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        // Indentation is not part of the grammar: a key that looks nested is refused rather
        // than flattened, because flattening would invent a row shape the author did not.
        if line != trimmed {
            return Err(DriftBreach::Malformed {
                line_number,
                line: line.to_string(),
                why: "indented — nesting is not in the grammar",
            });
        }
        if let Some(rest) = trimmed.strip_prefix("[[") {
            let Some(name) = rest.strip_suffix("]]") else {
                return Err(DriftBreach::Malformed {
                    line_number,
                    line: line.to_string(),
                    why: "`[[` without a closing `]]`",
                });
            };
            if !name.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
                return Err(DriftBreach::Malformed {
                    line_number,
                    line: line.to_string(),
                    why: "table name is not a lower_snake identifier",
                });
            }
            if let Some((table, row)) = open.take() {
                index.tables.entry(table).or_default().push(row);
            }
            open = Some((name.to_string(), Row::new()));
            continue;
        }
        if trimmed.starts_with('[') {
            return Err(DriftBreach::Malformed {
                line_number,
                line: line.to_string(),
                why: "a single-bracket `[table]` is not in the grammar; use `[[table]]`",
            });
        }
        let Some((key, value)) = trimmed.split_once(" = ") else {
            return Err(DriftBreach::Malformed {
                line_number,
                line: line.to_string(),
                why: "not `key = value` (the separator is exactly one space either side)",
            });
        };
        if key.is_empty()
            || !key
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            return Err(DriftBreach::Malformed {
                line_number,
                line: line.to_string(),
                why: "key is not a lower_snake identifier (dotted keys are refused)",
            });
        }
        let parsed = parse_value(value).ok_or_else(|| DriftBreach::Malformed {
            line_number,
            line: line.to_string(),
            why: "value is not a quoted string, a decimal integer, or a list of strings",
        })?;
        let target = match open.as_mut() {
            Some((_, row)) => row,
            None => &mut index.header,
        };
        if target.insert(key.to_string(), parsed).is_some() {
            return Err(DriftBreach::Malformed {
                line_number,
                line: line.to_string(),
                why: "duplicate key in one row — a silently overwritten pin is an unasserted pin",
            });
        }
    }
    if let Some((table, row)) = open.take() {
        index.tables.entry(table).or_default().push(row);
    }
    Ok(index)
}

fn parse_value(v: &str) -> Option<Val> {
    let v = v.trim();
    if let Some(inner) = v.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        let inner = inner.trim();
        if inner.is_empty() {
            return Some(Val::List(Vec::new()));
        }
        let mut out = Vec::new();
        for part in inner.split(',') {
            let p = part.trim();
            let s = p.strip_prefix('"')?.strip_suffix('"')?;
            if s.contains('"') {
                return None;
            }
            out.push(s.to_string());
        }
        return Some(Val::List(out));
    }
    if let Some(inner) = v.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        // An escaped quote is the only escape the emitter produces.
        return Some(Val::Str(inner.replace("\\\"", "\"")));
    }
    if !v.is_empty() && v.chars().all(|c| c.is_ascii_digit()) {
        return v.parse().ok().map(Val::Int);
    }
    None
}

fn read_index() -> Index {
    let path = repo_root().join(INDEX_PATH);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read the machine index at {}: {e}", path.display()));
    parse_index(&text).unwrap_or_else(|b| panic!("\n{b}\n"))
}

fn read_register() -> String {
    let path = repo_root().join(REGISTER_PATH);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read the register at {}: {e}", path.display()))
}

const REGISTER_PATH: &str = "docs/consensus/consensus-change-register.md";
const INDEX_PATH: &str = "docs/consensus/register.toml";

/// ★ **The window for clause 2, and the rule it encodes.**
///
/// A citation is *re-derivable* iff its `token` occurs within ±`CITATION_WINDOW` lines of
/// the cited line in `git show <at>:<path>`.
///
/// **Why a window and not an exact line.** An exact-line rule fails on any edit *above* the
/// citation in the same file, so every entry would rot on a schedule set by unrelated work —
/// the failure mode §7.6 says to avoid, and the one that produces exception lists. A window
/// of three tolerates a comment inserted above while still failing when the cited construct
/// is **gone**, which is the case that matters: CBR-027's cell cited `wrapping_add`, and
/// after the fix that call exists nowhere, at any line.
///
/// **Why the token and not the line number alone.** A line number alone is not a claim about
/// anything; it cannot be wrong, only unhelpful. The token is what makes the coordinate
/// falsifiable.
const CITATION_WINDOW: i64 = 3;

// ════════════════════════════════════════════════════════════════════════════════════════
// 4 · The DERIVED consensus-critical path set
// ════════════════════════════════════════════════════════════════════════════════════════

/// A crate from which the consensus-critical path set is derived, with its own obligation.
///
/// ⚠ **This is the only hand-declared input to the path set, and it is one row.** §3.1's
/// path set was hand-listed and *was already wrong once*: it omitted `rho-pure-eval/src/`,
/// the component that decides `where` verdicts, and two entries live there. The repair for a
/// hand-maintained mirror of a computable domain is to compute it — so the path set is the
/// **path-dependency closure** of the root below, and `rho-pure-eval` is recovered by
/// derivation rather than by remembering.
struct RootCrate {
    /// The crate directory, relative to the repository root.
    dir: &'static str,
    /// ★ Why this crate is a root. A root without an obligation is an assumption.
    obligation: &'static str,
}

/// ★ **One root, and the argument for it.**
///
/// `casper` decides block admission (`block_status.rs`), compares replays
/// (`rholang/replay_runtime.rs`), and gates the protocol version at
/// `casper/src/rust/validate.rs:273` — §5.5's conjunction argument. Anything `casper` can
/// reach can therefore move a verdict or a post-state hash, and anything it cannot reach
/// cannot: `node` is excluded by derivation, because `casper` does not depend on it.
const ROOT_CRATES: &[RootCrate] = &[RootCrate {
    dir: "casper",
    obligation: "owns block admission, replay comparison, and `Validate::version` — the exact-equality \
                 version gate of §5.5. Its dependency closure is the set of code that can move an axis.",
}];

/// ★ **A path that is inside a closure crate and is nonetheless NOT consensus-critical**,
/// each row with the reason cargo does not compile it into the library.
///
/// The set is asserted **EXACTLY** against its own falsifier — a row that excludes nothing
/// in range is dead weight and fails [`the_path_exclusions_each_exclude_something`]. That is
/// what stops this table becoming the hand-list it replaced.
struct PathExclusion {
    subdir: &'static str,
    why: &'static str,
}

const PATH_EXCLUSIONS: &[PathExclusion] = &[
    PathExclusion {
        subdir: "tests",
        why: "integration tests. Cargo compiles them into test binaries, never into the library a \
              node runs.",
    },
    PathExclusion {
        subdir: "benches",
        why:
            "benchmark harnesses. Same reason, and `decda6dd` is the witness that they do land in \
              range.",
    },
    PathExclusion {
        subdir: "src/test",
        why: "⚠ a test tree INSIDE `src`: `casper/src/test/resources/*.rho` are fixtures read by \
              tests. ★ `casper/src/main/resources/` is deliberately NOT excluded — CBR-030 is a \
              genesis contract that lives there, so the exclusion is `src/test`, never `src`.",
    },
];

/// The workspace members, read from the root manifest rather than listed.
fn workspace_members() -> Vec<String> {
    let manifest = std::fs::read_to_string(repo_root().join("Cargo.toml"))
        .expect("the workspace root manifest is always present");
    let body = manifest
        .split_once("members = [")
        .expect("`[workspace] members` is always present")
        .1;
    let body = body.split_once(']').expect("`members` is always closed").0;
    body.lines()
        .filter_map(|l| l.trim().strip_prefix('"'))
        .filter_map(|l| l.split('"').next())
        .map(str::to_string)
        .collect()
}

/// The `path = "../x"` dependency edges declared by one member's manifest.
fn path_dependencies(member: &str) -> Vec<String> {
    let manifest = match std::fs::read_to_string(repo_root().join(member).join("Cargo.toml")) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        let Some(rest) = line.split_once("path = \"") else {
            continue;
        };
        let Some(raw) = rest.1.split('"').next() else {
            continue;
        };
        // Normalise `member/../x` without touching the filesystem.
        let mut parts: Vec<&str> = member.split('/').collect();
        for segment in raw.split('/') {
            match segment {
                "." | "" => {}
                ".." => {
                    parts.pop();
                }
                s => parts.push(s),
            }
        }
        let joined = parts.join("/");
        if !joined.is_empty() && !out.contains(&joined) {
            out.push(joined);
        }
    }
    out
}

/// The closure crates: [`ROOT_CRATES`] plus everything reachable along `path =` edges.
fn closure_crates() -> BTreeSet<String> {
    let members: BTreeSet<String> = workspace_members().into_iter().collect();
    let mut seen = BTreeSet::new();
    let mut stack: Vec<String> = ROOT_CRATES.iter().map(|r| r.dir.to_string()).collect();
    while let Some(dir) = stack.pop() {
        if !seen.insert(dir.clone()) {
            continue;
        }
        for dep in path_dependencies(&dir) {
            if members.contains(&dep) {
                stack.push(dep);
            }
        }
    }
    seen
}

/// The `git log -- <pathspec>` arguments the obligation set is enumerated over.
///
/// Crate-rooted, minus [`PATH_EXCLUSIONS`], plus `Cargo.lock`.
///
/// ★ **Why crate-rooted and not `<crate>/src/`.** A `src`-rooted derivation loses
/// `models/build.rs` and `models/codegen/` — the GENERATOR that emits the schema tables, which
/// §3.4(4) names as a false-negative class of its own and which `903cefb3` already proved
/// can leave a stale table in `OUT_DIR` while the build reports success. Crate-rooted is
/// complete by construction and subtracts only what a typed row justifies.
///
/// ★ **Why `Cargo.lock`.** §3.4(1) and §7.5 extension 2: CBR-L06 proves a `prost` or
/// `thiserror` bump alone can move published bytes, so a dependency change is a consensus
/// change under this report's own definition. It contributes zero commits in range today,
/// which makes including it free — and means the first one that lands will be seen.
fn obligation_pathspec() -> Vec<String> {
    let crates = closure_crates();
    let mut spec: Vec<String> = crates.iter().map(|c| format!("{c}/")).collect();
    spec.push("Cargo.lock".to_string());
    for c in &crates {
        for x in PATH_EXCLUSIONS {
            spec.push(format!(":(exclude){c}/{}/", x.subdir));
        }
    }
    spec
}

/// Every commit in `range` that touches a consensus-critical path, newest first.
fn obligation_set(range: &str, pathspec: &[String]) -> Vec<String> {
    let mut args: Vec<&str> = vec!["log", "--format=%h", range, "--"];
    for s in pathspec {
        args.push(s);
    }
    git_expect(&args)
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

// ════════════════════════════════════════════════════════════════════════════════════════
// 5 · Projections of the prose
// ════════════════════════════════════════════════════════════════════════════════════════

/// One row of §4.1's summary table — the register's single authoritative table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummaryRow {
    pub id: String,
    pub surface: String,
    /// V · T · B · P · H · A · M, in §4.2's closed vocabulary, mapped from the glyphs.
    pub axes: [String; 7],
    pub direction: String,
    pub grade: String,
}

pub const AXIS_KEYS: [&str; 7] = [
    "axis_value",
    "axis_verdict",
    "axis_bytes_bincode",
    "axis_bytes_prost",
    "axis_post_state_hash",
    "axis_acceptance",
    "axis_metering",
];

const AXIS_VOCABULARY: [&str; 4] = ["MOVES", "NO", "N/A", "UNVERIFIED"];

/// §3.2's closed exclusion enum, plus the one variant this work added.
///
/// ★ `VERDICT_NEUTRAL_MEASURED` was added for `383a8b56`, which moves `rho-pure-eval`'s
/// evaluator — the component that **decides `where` verdicts** — onto a shared trampoline.
/// "Byte identity" is not the claim that matters for a verdict evaluator; verdict identity
/// is. Adding a variant is a code change and therefore reviewed, which is exactly what
/// §7.2 clause 4 says should happen.
const CLOSED_REASONS: [&str; 10] = [
    "TESTS_ONLY",
    "DOCS_ONLY",
    "HYGIENE",
    "BYTE_NEUTRAL_MEASURED",
    "CHARGE_NEUTRAL_MEASURED",
    "VERDICT_NEUTRAL_MEASURED",
    "DORMANT",
    "INFRA",
    "SUPERSEDED",
    "DEP_BUMP_BYTE_NEUTRAL",
];

/// Lines of the register that are inside a fenced block.
///
/// ⚠ Load-bearing: Appendix A's template is a `### CBR-0NN` heading with a `| Status |`
/// row inside a ```` fence. A scan that did not track fences would read the *template* as a
/// forty-fifth entry, and its `LANDED / IN FLIGHT / OPEN` cell as a status.
fn fenced(lines: &[&str]) -> Vec<bool> {
    let mut out = vec![false; lines.len()];
    let mut fence: Option<usize> = None;
    for (i, l) in lines.iter().enumerate() {
        let ticks = l.chars().take_while(|c| *c == '`').count();
        if ticks >= 3 {
            match fence {
                None => {
                    fence = Some(ticks);
                    out[i] = true;
                    continue;
                }
                Some(open) if ticks >= open => {
                    fence = None;
                    out[i] = true;
                    continue;
                }
                _ => {}
            }
        }
        out[i] = fence.is_some();
    }
    out
}

fn glyph_to_axis(g: &str) -> Option<&'static str> {
    match g.replace('*', "").trim() {
        "●" => Some("MOVES"),
        "○" => Some("NO"),
        "·" => Some("N/A"),
        "?" => Some("UNVERIFIED"),
        _ => None,
    }
}

fn grade_to_word(g: &str) -> Option<&'static str> {
    match g.replace('*', "").trim() {
        "W" => Some("WITNESSED"),
        "M" => Some("MECHANISM_ONLY"),
        "L" => Some("LATENT"),
        "D" => Some("DORMANT"),
        "NM" => Some("NEUTRALITY_MEASURED"),
        "?" => Some("UNVERIFIED"),
        _ => None,
    }
}

/// Parse §4.1's `| [CBR-…] |` rows. **The single authoritative table.**
pub fn summary_rows(register: &str) -> Vec<SummaryRow> {
    let lines: Vec<&str> = register.lines().collect();
    let inside = fenced(&lines);
    let mut out = Vec::with_capacity(48);
    for (i, l) in lines.iter().enumerate() {
        if inside[i] || !l.starts_with("| [CBR-") {
            continue;
        }
        let cells: Vec<&str> = l
            .trim()
            .trim_matches('|')
            .split('|')
            .map(str::trim)
            .collect::<Vec<_>>();
        let id = cells[0]
            .trim_start_matches('[')
            .split(']')
            .next()
            .expect("a summary row always opens with `[CBR-…]`")
            .to_string();
        let mut axes: [String; 7] = Default::default();
        for k in 0..7 {
            axes[k] = glyph_to_axis(cells[4 + k])
                .unwrap_or("MALFORMED")
                .to_string();
        }
        out.push(SummaryRow {
            id,
            surface: cells[1].to_string(),
            axes,
            direction: {
                let d = cells[11].replace('*', "").trim().to_string();
                if d == "—" || d == "-" {
                    "NOT_APPLICABLE".to_string()
                } else {
                    d
                }
            },
            grade: grade_to_word(cells[12]).unwrap_or("MALFORMED").to_string(),
        });
    }
    out
}

/// Every `### CBR-…` heading outside a fence, in document order.
pub fn entry_headings(register: &str) -> Vec<String> {
    let lines: Vec<&str> = register.lines().collect();
    let inside = fenced(&lines);
    lines
        .iter()
        .enumerate()
        .filter(|(i, l)| !inside[*i] && l.starts_with("### CBR-"))
        .map(|(_, l)| l.trim_start_matches("### ").trim().to_string())
        .filter(|id| !id.contains(' '))
        .collect()
}

/// A `path:line` coordinate as the prose writes it, with the entry that owns it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Coordinate {
    pub owner: String,
    pub cited: String,
    pub line: i64,
}

/// Every distinct `` `path.ext:NNN` `` coordinate in the register, attributed to its entry.
///
/// The owner is the enclosing `### CBR-…` section, or `(doc)` for a coordinate in a
/// section-level paragraph — the attribution resets at every `#` or `##` heading, so §5.1's
/// citations are not credited to the last entry above them.
pub fn document_coordinates(register: &str) -> BTreeSet<Coordinate> {
    let lines: Vec<&str> = register.lines().collect();
    let inside = fenced(&lines);
    let mut owner = String::from("(doc)");
    let mut out = BTreeSet::new();
    for (i, l) in lines.iter().enumerate() {
        if !inside[i] {
            if let Some(rest) = l.strip_prefix("### CBR-") {
                if !rest.contains(' ') {
                    owner = format!("CBR-{}", rest.trim());
                }
            } else if l.starts_with("# ") || l.starts_with("## ") {
                owner = String::from("(doc)");
            }
        }
        if inside[i] {
            continue;
        }
        for (cited, line) in coordinates_in(l) {
            out.insert(Coordinate {
                owner: owner.clone(),
                cited,
                line,
            });
        }
    }
    out
}

/// `` `some/path.rs:123` `` and `` `some/path.rs:123-456` `` inside one line.
///
/// ⚠ **Parity-free by construction, and that is a measured requirement rather than a
/// preference.** The obvious implementation — `split('`')` and keep the odd spans — assumes
/// every line has an even number of backticks. The register does not: a line carrying a
/// double-backtick span, or a lone backtick inside a table cell, shifts the parity and the
/// extractor then reads code spans as prose and prose as code spans. That produced two
/// disagreements on the first run — one coordinate invented and one lost — so the scan anchors
/// on the **extension** and walks outward to the delimiting backticks instead.
fn coordinates_in(line: &str) -> Vec<(String, i64)> {
    const EXTS: [&str; 6] = [".rs", ".rho", ".proto", ".js", ".toml", ".yml"];
    let bytes = line.as_bytes();
    let path_char = |b: u8| b.is_ascii_alphanumeric() || b"_./+-".contains(&b);
    let mut out = Vec::new();
    for ext in EXTS {
        let mut from = 0usize;
        while let Some(rel) = line[from..].find(ext) {
            let ext_start = from + rel;
            from = ext_start + ext.len();
            // The extension must be followed by `:` then at least one digit.
            let after_ext = ext_start + ext.len();
            if bytes.get(after_ext) != Some(&b':') {
                continue;
            }
            let mut digits_end = after_ext + 1;
            while bytes.get(digits_end).is_some_and(u8::is_ascii_digit) {
                digits_end += 1;
            }
            if digits_end == after_ext + 1 {
                continue;
            }
            // Walk left over the path, which must open at a backtick.
            let mut path_start = ext_start;
            while path_start > 0 && path_char(bytes[path_start - 1]) {
                path_start -= 1;
            }
            if path_start == 0 || bytes[path_start - 1] != b'`' {
                continue;
            }
            // The span closes at a backtick, optionally after a `-NNN` range tail.
            let mut end = digits_end;
            if bytes.get(end) == Some(&b'-') {
                let mut tail = end + 1;
                while bytes.get(tail).is_some_and(u8::is_ascii_digit) {
                    tail += 1;
                }
                if tail > end + 1 {
                    end = tail;
                }
            }
            if bytes.get(end) != Some(&b'`') {
                continue;
            }
            let path = &line[path_start..after_ext];
            if let Ok(n) = line[after_ext + 1..digits_end].parse() {
                out.push((path.to_string(), n));
            }
        }
    }
    out
}

// ════════════════════════════════════════════════════════════════════════════════════════
// 6 · The clauses
// ════════════════════════════════════════════════════════════════════════════════════════

/// The union of every Surface-N commit an entry names.
fn entry_commits(index: &Index, surface: Option<&str>) -> BTreeSet<String> {
    index
        .rows("entry")
        .iter()
        .filter(|r| surface.is_none_or(|s| r.get("surface").map(Val::as_str) == Some(s)))
        .flat_map(|r| {
            r.get("commits")
                .map(Val::as_list)
                .unwrap_or(&[])
                .iter()
                .cloned()
        })
        .collect()
}

fn exempt_commits(index: &Index, window: Option<&str>) -> BTreeSet<String> {
    index
        .rows("exempt")
        .iter()
        .filter(|r| window.is_none_or(|w| r.get("window").map(Val::as_str) == Some(w)))
        .filter_map(|r| r.get("commit").map(|c| c.as_str().to_string()))
        .collect()
}

/// ★★ **Clause 0 — the non-vacuity floor. Checked FIRST, always.**
///
/// A gate that finds nothing to check cannot fail, and a gate that cannot fail is worse than
/// no gate: it trains its readers to trust it. Four sets must be non-empty, and the
/// obligation set must additionally clear a **pinned floor** — because "non-empty" would
/// still pass if a pathspec typo cut 81 obligations to 1.
///
/// The floor is a *lower bound that has been measured*, not a target: 78 is §3.1's own
/// **MEASURED** count of commits in `7293d57c..dc383ed1` touching the path set, so a
/// derivation that produces fewer than that has lost something the register already found by
/// hand.
pub fn check_non_vacuity(
    obligations: usize,
    index: &Index,
    citations_checked: usize,
    decidable_questions: usize,
) -> Result<(), DriftBreach> {
    const OBLIGATION_FLOOR: usize = 78;
    if obligations < OBLIGATION_FLOOR {
        return Err(DriftBreach::EmptyObligation {
            found: obligations,
            floor: OBLIGATION_FLOOR,
        });
    }
    for table in ["entry", "exempt", "citation", "open_question"] {
        if index.rows(table).is_empty() {
            return Err(DriftBreach::EmptyIndexTable { table: leak(table) });
        }
    }
    if citations_checked == 0 {
        return Err(DriftBreach::EmptyIndexTable {
            table: "citation (all unchecked)",
        });
    }
    if decidable_questions == 0 {
        return Err(DriftBreach::EmptyIndexTable {
            table: "open_question (all undecidable)",
        });
    }
    Ok(())
}

/// The four table names are compile-time constants; this keeps the variant `&'static str`
/// without an allocation or a lifetime on [`DriftBreach`].
fn leak(table: &str) -> &'static str {
    match table {
        "entry" => "entry",
        "exempt" => "exempt",
        "citation" => "citation",
        "open_question" => "open_question",
        _ => "unknown",
    }
}

/// **Clause 1 — coverage over the EXACT-PARTITION window**, and **clause 2 — exactness**.
///
/// Over `register_base..partition_head` the partition is asserted with `=`: every obligation
/// is an entry commit or an exemption, and nothing is both. This is the window §4.1's
/// `57 + 21 = 78` claim quantifies over, which is why `partition_head` is pinned rather than
/// being `HEAD`.
pub fn check_partition_coverage(
    obligations: &[String],
    entries: &BTreeSet<String>,
    exempt: &BTreeSet<String>,
) -> Result<(), DriftBreach> {
    let both: Vec<String> = entries.intersection(exempt).cloned().collect();
    if !both.is_empty() {
        return Err(DriftBreach::DoubleListed { commits: both });
    }
    let missing: Vec<(String, String)> = obligations
        .iter()
        .filter(|c| !entries.contains(*c) && !exempt.contains(*c))
        .map(|c| (c.clone(), subject(c)))
        .collect();
    if !missing.is_empty() {
        return Err(DriftBreach::Unregistered {
            window: "PARTITION",
            commits: missing,
        });
    }
    Ok(())
}

/// ★ **Clause 3 — the LIVING FRONTIER, with a fuse measured in commit dates.**
///
/// §6.5 records that the anchored range has **eroded**: entries now name commits after
/// `partition_head`, and the register is deliberately living. So the frontier is not held to
/// `=`; it is held to `=` *eventually*, on a fuse.
///
/// **Why a fuse rather than a hard failure.** A hard failure over `..HEAD` reddens CI the
/// moment any agent lands a consensus-path commit, including one landed seconds before this
/// gate runs. That does not catch drift; it catches *concurrency*, and a gate that fires on
/// the wrong thing gets disabled. **Why not simply exclude the frontier**: then the gate's
/// advertised coverage would exceed its real coverage, which is the one thing §7 must not do.
///
/// **Why commit dates and not the wall clock.** Both dates are in the objects, so the verdict
/// is a pure function of the checkout: re-running on the same tree gives the same answer, and
/// the gate is not flaky. Every witnessed drift was caught inside one day, so the fuse is
/// longer than every drift observed.
pub fn check_frontier_fuse(
    frontier: &[String],
    entries: &BTreeSet<String>,
    exempt: &BTreeSet<String>,
    head_time: i64,
    grace_days: i64,
) -> Result<(), DriftBreach> {
    const DAY: i64 = 86_400;
    let mut blown: Vec<(String, String, i64)> = Vec::new();
    for c in frontier {
        if entries.contains(c) || exempt.contains(c) {
            continue;
        }
        let age = (head_time - commit_time(c)) / DAY;
        if age > grace_days {
            blown.push((c.clone(), subject(c), age));
        }
    }
    if !blown.is_empty() {
        return Err(DriftBreach::FrontierFuseBlown {
            commits: blown,
            grace_days,
        });
    }
    Ok(())
}

/// ★ **Clause 4 — row liveness. This REPLACES §7.2's specified "stale row" clause, which
/// was measured to be wrong.**
///
/// §7.2 clause 3 specified `E ⊎ X ⊆ O`, failing on any row naming a commit outside the
/// obligation set. **Built as specified, that clause goes RED on a correct row.** `719f2432`
/// is one of CBR-030's two commits and touches only
/// `casper/tests/genesis/contracts/genesis_overflow_guard_shape.rs` — it is the entry's
/// *evidence* commit, which is a legitimate thing for an entry to name and is not in
/// $`\mathcal{O}`$ by construction.
///
/// The property §7.2 actually wanted was *"catch a rebase"*, and the direct test for that is
/// ancestry: a row's Surface-N SHA must **resolve** and must be an **ancestor of `HEAD`**. An
/// abandoned or rewritten commit fails; an evidence-only commit passes.
/// ⚠ The two `git` predicates are **injected** rather than called directly, for one reason:
/// this repository currently contains no commit that resolves and is not an ancestor of
/// `HEAD`, so the RED cell for this clause cannot be built from a real SHA. Injecting the
/// predicates lets [`the_liveness_clause_refuses_a_commit_that_is_not_an_ancestor`] exhibit
/// the failure exactly, rather than leaving the clause unwatched because the tree happens to
/// be tidy today. **An unfalsified refusal is an unproven refusal.**
pub fn check_row_liveness(
    entries: &BTreeSet<String>,
    exempt: &BTreeSet<String>,
    resolves_here: &dyn Fn(&str) -> bool,
    is_ancestor_of_head: &dyn Fn(&str) -> bool,
) -> Result<(), DriftBreach> {
    let abandoned: Vec<String> = entries
        .iter()
        .chain(exempt.iter())
        .filter(|c| resolves_here(c) && !is_ancestor_of_head(c))
        .cloned()
        .collect();
    if !abandoned.is_empty() {
        return Err(DriftBreach::AbandonedRow { commits: abandoned });
    }
    Ok(())
}

/// ★★ **DRIFT CLASS 1 — in-flight staleness.**
///
/// CBR-027 read *"IN FLIGHT — not present in the tree at the time of writing"* while
/// `6ff46f8a` had already landed, and it was caught by a human reading `git log`. CBR-007
/// drifted the same way in the same window.
///
/// The decision procedure is one `git` call per SHA and needs no prose parsing: **no entry
/// whose status is not `LANDED` may name a SHA that is an ancestor of `HEAD`.**
///
/// ⚠ **The control that shows the clause is not vacuous**: CBR-L08 is `IN_FLIGHT` and names
/// no SHA at all, and CBR-028 is `OPEN` and names five *characterising* commits that are
/// ancestors of HEAD. The clause must not fire on either — which is why it tests
/// `commits`-as-owned only for `IN_FLIGHT`, and treats `OPEN` (an unrepaired hazard, whose
/// commits characterise rather than implement it) as exempt by *status semantics* rather than
/// by an exception row.
pub fn check_in_flight_staleness(index: &Index) -> Result<(), DriftBreach> {
    let mut offenders = Vec::new();
    for row in index.rows("entry") {
        let status = row.get("status").map(Val::as_str).unwrap_or("");
        if status != "IN_FLIGHT" {
            continue;
        }
        let id = row.get("id").map(Val::as_str).unwrap_or("?").to_string();
        for sha in row.get("commits").map(Val::as_list).unwrap_or(&[]) {
            if is_ancestor(sha, "HEAD") {
                offenders.push((id.clone(), sha.clone()));
            }
        }
    }
    if !offenders.is_empty() {
        return Err(DriftBreach::InFlightButLanded { offenders });
    }
    Ok(())
}

/// ★★ **DRIFT CLASS 2 — transcribed `file:line` re-derivability.**
///
/// See [`CITATION_WINDOW`] for the rule and the argument for the window's width.
///
/// Each `[[citation]]` row is checked **at its own `at`**, never at `HEAD`. §7.6 gives the
/// reason: a coordinate in an entry about `f5b2e820` should be checkable against `f5b2e820`
/// forever, and anchoring at `HEAD` would make every entry rot on a schedule set by unrelated
/// work — the failure mode that produces exception lists.
pub fn check_citation_rederivability(index: &Index) -> Result<(), DriftBreach> {
    let mut offenders = Vec::new();
    for row in index.rows("citation") {
        let Some(path) = row.get("path").map(Val::as_str) else {
            continue;
        };
        let at = row.get("at").map(Val::as_str).unwrap_or("HEAD");
        let line = row.get("line").map(Val::as_int).unwrap_or(i64::MIN);
        let token = row.get("token").map(Val::as_str).unwrap_or("");
        let owner = row.get("owner").map(Val::as_str).unwrap_or("?").to_string();
        let Some(text) = show(at, path) else {
            offenders.push(StaleCitation {
                owner,
                path: path.to_string(),
                line,
                at: at.to_string(),
                token: token.to_string(),
                found: format!("`git show {at}:{path}` does not resolve"),
            });
            continue;
        };
        let file: Vec<&str> = text.lines().collect();
        let lo = (line - CITATION_WINDOW).max(1);
        let hi = (line + CITATION_WINDOW).min(file.len() as i64);
        let hit = (lo..=hi).any(|k| file[(k - 1) as usize].contains(token));
        if !hit {
            let at_line = if line >= 1 && line <= file.len() as i64 {
                file[(line - 1) as usize].trim().to_string()
            } else {
                format!("<line {line} is past end of file ({} lines)>", file.len())
            };
            offenders.push(StaleCitation {
                owner,
                path: path.to_string(),
                line,
                at: at.to_string(),
                token: token.to_string(),
                found: at_line,
            });
        }
    }
    if !offenders.is_empty() {
        return Err(DriftBreach::StaleCitation { offenders });
    }
    Ok(())
}

/// ★ The citation corpus is **DERIVED from the document**, so it cannot be narrowed by
/// deleting a row.
///
/// Asserted as set equality between the `[[citation]]` rows and every coordinate the prose
/// yields. A coordinate the prose adds and the index lacks is an unpinned citation; a row the
/// prose no longer contains is a pin with nothing to hold.
pub fn check_citation_corpus(
    index: &Index,
    document: &BTreeSet<Coordinate>,
) -> Result<(), DriftBreach> {
    let rows: BTreeSet<Coordinate> = index
        .rows("citation")
        .iter()
        .map(|r| Coordinate {
            owner: r.get("owner").map(Val::as_str).unwrap_or("").to_string(),
            cited: r.get("cited").map(Val::as_str).unwrap_or("").to_string(),
            line: r.get("line").map(Val::as_int).unwrap_or(i64::MIN),
        })
        .collect();
    let show = |c: &Coordinate| format!("[{}] {}:{}", c.owner, c.cited, c.line);
    let missing: Vec<String> = document.difference(&rows).map(show).collect();
    let extra: Vec<String> = rows.difference(document).map(show).collect();
    if !missing.is_empty() || !extra.is_empty() {
        return Err(DriftBreach::CitationCorpusDrift {
            missing_rows: missing,
            extra_rows: extra,
        });
    }
    Ok(())
}

// ────────────────────────────────────────────────────────────────────────────────────────
// DRIFT CLASS 3 — every stated aggregate is PROJECTED from the entry table, not stored
// ────────────────────────────────────────────────────────────────────────────────────────

/// The projection of §4.1's table: the only legitimate source for every aggregate the
/// register states.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Projection {
    pub rows: usize,
    pub surface: BTreeMap<String, usize>,
    pub direction: BTreeMap<String, usize>,
    pub grade: BTreeMap<String, usize>,
    /// `MOVES` count per axis, in [`AXIS_KEYS`] order.
    pub axis_moves: [usize; 7],
    pub unverified_cells: usize,
}

/// Project the table, **checking the instrument before the measurement**.
///
/// ★ The arithmetic identities are computed and asserted here, *before* any comparison
/// against the prose. `Σ surface = n` does not need to know what the prose claims; it detects
/// a row that failed to parse, a duplicated identifier, or a malformed axis cell — faults
/// that would otherwise make every subsequent comparison compare against a wrong projection
/// and report the **prose** as the defect.
pub fn project(rows: &[SummaryRow]) -> Result<Projection, DriftBreach> {
    let n = rows.len();
    let mut p = Projection {
        rows: n,
        surface: BTreeMap::new(),
        direction: BTreeMap::new(),
        grade: BTreeMap::new(),
        axis_moves: [0; 7],
        unverified_cells: 0,
    };
    for r in rows {
        *p.surface.entry(r.surface.clone()).or_default() += 1;
        *p.direction.entry(r.direction.clone()).or_default() += 1;
        *p.grade.entry(r.grade.clone()).or_default() += 1;
        for (k, a) in r.axes.iter().enumerate() {
            if a == "MOVES" {
                p.axis_moves[k] += 1;
            }
            if a == "UNVERIFIED" {
                p.unverified_cells += 1;
            }
            if !AXIS_VOCABULARY.contains(&a.as_str()) {
                return Err(DriftBreach::BadAxisCell {
                    id: r.id.clone(),
                    axis: AXIS_KEYS[k].to_string(),
                    value: a.clone(),
                });
            }
        }
    }
    for (name, split) in [
        ("surface", &p.surface),
        ("direction", &p.direction),
        ("grade", &p.grade),
    ] {
        let sum: usize = split.values().sum();
        if sum != n {
            return Err(DriftBreach::TableIdentityFailed {
                quantity: name.to_string(),
                sum,
                rows: n,
            });
        }
    }
    // Each axis column must classify every row: the seven columns each sum to n.
    for (k, key) in AXIS_KEYS.iter().enumerate() {
        let classified = rows
            .iter()
            .filter(|r| AXIS_VOCABULARY.contains(&r.axes[k].as_str()))
            .count();
        if classified != n {
            return Err(DriftBreach::TableIdentityFailed {
                quantity: format!("axis column {key}"),
                sum: classified,
                rows: n,
            });
        }
    }
    Ok(p)
}

/// What a stated figure is a projection *of*.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quantity {
    Rows,
    Surface(&'static str),
    Direction(&'static str),
    Grade(&'static str),
    UnverifiedCells,
}

/// A figure the prose states, its site, and the literal text that immediately precedes it.
///
/// ⚠ **A closed list, deliberately.** §7.5 extension 4 says why: *"a regex hunting for every
/// integer in a 5,900-line report would produce false positives faster than anyone would keep
/// the gate enabled."* Each anchor is asserted **UNIQUE** in the document, so an anchor that
/// starts matching twice is a failure rather than a coin flip.
///
/// ★ **And a rule the register now follows because of this table: a projected figure is
/// written as a DIGIT.** The Abstract used to spell them — *"Twenty-three move bytes"*,
/// *"thirty-two move the post-state hash"* — and an English numeral is unparseable, so those
/// figures were structurally uncheckable however careful the author.
pub struct StatedFigure {
    pub site: &'static str,
    pub anchor: &'static str,
    pub quantity: Quantity,
}

pub const STATED_FIGURES: &[StatedFigure] = &[
    StatedFigure {
        site: "Abstract",
        anchor: "**Result: ",
        quantity: Quantity::Rows,
    },
    StatedFigure {
        site: "Abstract",
        anchor: "consensus-visible changes** — ",
        quantity: Quantity::Surface("N"),
    },
    StatedFigure {
        site: "Abstract",
        anchor: " on the F1r3node node itself, ",
        quantity: Quantity::Surface("L"),
    },
    StatedFigure {
        site: "§4.1 totals",
        anchor: "**Totals — ",
        quantity: Quantity::Rows,
    },
    StatedFigure {
        site: "§4.1 totals",
        anchor: "recounted from the rows above rather than adjusted: **",
        quantity: Quantity::Surface("N"),
    },
    StatedFigure {
        site: "§4.1 totals",
        anchor: "on Surface N, ",
        quantity: Quantity::Surface("L"),
    },
    StatedFigure {
        site: "§4.1 totals",
        anchor: "By evidence grade: **",
        quantity: Quantity::Grade("WITNESSED"),
    },
    StatedFigure {
        site: "§4.1 totals",
        anchor: "By direction: **",
        quantity: Quantity::Direction("CORRECTIVE"),
    },
    StatedFigure {
        site: "§4.1 totals",
        anchor: "Axis cells reading `UNVERIFIED`: **",
        quantity: Quantity::UnverifiedCells,
    },
    StatedFigure {
        site: "§6.4 budget",
        anchor: "The budget is **",
        quantity: Quantity::UnverifiedCells,
    },
    StatedFigure {
        site: "§8 conclusion 1",
        anchor: "The register holds **",
        quantity: Quantity::Rows,
    },
    StatedFigure {
        site: "§8 conclusion 1",
        anchor: "derived from the campaign record: **",
        quantity: Quantity::Surface("N"),
    },
    StatedFigure {
        site: "§8 conclusion 1",
        anchor: "on the F1r3node node, ",
        quantity: Quantity::Surface("L"),
    },
];

fn projected(p: &Projection, q: Quantity) -> usize {
    match q {
        Quantity::Rows => p.rows,
        Quantity::Surface(s) => p.surface.get(s).copied().unwrap_or(0),
        Quantity::Direction(d) => p.direction.get(d).copied().unwrap_or(0),
        Quantity::Grade(g) => p.grade.get(g).copied().unwrap_or(0),
        Quantity::UnverifiedCells => p.unverified_cells,
    }
}

fn quantity_name(q: Quantity) -> String {
    match q {
        Quantity::Rows => "entry count".to_string(),
        Quantity::Surface(s) => format!("Surface-{s} count"),
        Quantity::Direction(d) => format!("{d} count"),
        Quantity::Grade(g) => format!("{g} count"),
        Quantity::UnverifiedCells => "UNVERIFIED axis cells".to_string(),
    }
}

/// The document with every run of whitespace collapsed to one space.
///
/// ⚠ **Load-bearing.** The register is hard-wrapped at about a hundred columns, so an anchor
/// of any useful length straddles a newline — `"By evidence grade: **"` is literally split
/// across two lines. Searching the raw text found zero occurrences and the clause reported an
/// ambiguous anchor rather than a stale figure, which is the wrong diagnosis. Normalising
/// makes an anchor a property of the *prose* rather than of the *line width*.
fn unwrapped(document: &str) -> String { document.split_whitespace().collect::<Vec<_>>().join(" ") }

/// The integer that follows the unique occurrence of `anchor`, skipping `*` and whitespace.
fn figure_after(document: &str, anchor: &str) -> Result<i64, usize> {
    let document = &unwrapped(document);
    let anchor = &unwrapped(anchor);
    let anchor = anchor.as_str();
    let hits = document.matches(anchor).count();
    if hits != 1 {
        return Err(hits);
    }
    let tail = &document[document.find(anchor).expect("counted one") + anchor.len()..];
    let digits: String = tail
        .chars()
        .skip_while(|c| *c == '*' || c.is_whitespace())
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().map_err(|_| 0)
}

/// ★★ **DRIFT CLASS 3.** Every figure in [`STATED_FIGURES`] must equal its projection.
///
/// The witness: the 2026-07-29 recount found §5.1 still reading *"Share of the 40"* with
/// Lane B at 19 and the post-state hash at 28, and §5.3 still reading 23 CORRECTIVE — all
/// computed at 40 entries and never re-projected, **while the adjacent paragraph had been.**
pub fn check_stated_figures(document: &str, p: &Projection) -> Result<(), DriftBreach> {
    for f in STATED_FIGURES {
        match figure_after(document, f.anchor) {
            Ok(stated) => {
                let want = projected(p, f.quantity) as i64;
                if stated != want {
                    return Err(DriftBreach::FigureDisagrees {
                        site: f.site.to_string(),
                        quantity: quantity_name(f.quantity),
                        stated,
                        projected: want,
                    });
                }
            }
            Err(count) => {
                return Err(DriftBreach::AnchorNotUnique {
                    site: f.site.to_string(),
                    anchor: f.anchor.to_string(),
                    count,
                })
            }
        }
    }
    Ok(())
}

/// Rows of a `| label | **n** | … |` table between two headings.
fn counted_table(document: &str, from: &str, to: &str) -> Vec<(String, i64)> {
    let start = document.find(from).map(|i| i + from.len()).unwrap_or(0);
    let end = document[start..]
        .find(to)
        .map(|i| start + i)
        .unwrap_or(document.len());
    let mut out = Vec::new();
    for line in document[start..end].lines() {
        if !line.starts_with("| ") {
            continue;
        }
        let cells: Vec<&str> = line
            .trim()
            .trim_matches('|')
            .split('|')
            .map(str::trim)
            .collect();
        if cells.len() < 2 {
            continue;
        }
        let label = cells[0].replace(['*', '`'], "").trim().to_string();
        let value = cells[1].replace('*', "").trim().to_string();
        if let Ok(n) = value.parse::<i64>() {
            out.push((label, n));
        }
    }
    out
}

/// ★ **§5.1's axis-exposure table, projected structurally rather than by anchor.**
///
/// §5.1 is a table, so it is read as one: the seven axis rows are matched to the seven
/// columns of §4.1 **by position**, which is the same order the register's own legend
/// declares (V · T · B · P · H · A · M).
pub fn check_axis_exposure(document: &str, p: &Projection) -> Result<(), DriftBreach> {
    let rows = counted_table(document, "### 5.1 Aggregate axis exposure", "### 5.2 ");
    if rows.len() != 7 {
        return Err(DriftBreach::TableIdentityFailed {
            quantity: "§5.1 axis rows".to_string(),
            sum: rows.len(),
            rows: 7,
        });
    }
    for (k, (label, stated)) in rows.iter().enumerate() {
        let want = p.axis_moves[k] as i64;
        if *stated != want {
            return Err(DriftBreach::FigureDisagrees {
                site: format!("§5.1 row {} ({label})", k + 1),
                quantity: format!("{} MOVES", AXIS_KEYS[k]),
                stated: *stated,
                projected: want,
            });
        }
    }
    Ok(())
}

/// ★ **§5.3's direction profile, projected structurally.** Its `Total` row is the register's
/// own arithmetic check and is asserted against the row count.
pub fn check_direction_profile(document: &str, p: &Projection) -> Result<(), DriftBreach> {
    let rows = counted_table(document, "### 5.3 Direction profile", "### 5.4 ");
    if rows.is_empty() {
        return Err(DriftBreach::TableIdentityFailed {
            quantity: "§5.3 direction rows".to_string(),
            sum: 0,
            rows: p.direction.len(),
        });
    }
    for (label, stated) in &rows {
        let want = match label.as_str() {
            "Total" => p.rows as i64,
            "—" | "-" => projected(p, Quantity::Direction("NOT_APPLICABLE")) as i64,
            other => projected(p, Quantity::Direction(leak_direction(other))) as i64,
        };
        if *stated != want {
            return Err(DriftBreach::FigureDisagrees {
                site: format!("§5.3 row {label}"),
                quantity: format!("{label} count"),
                stated: *stated,
                projected: want,
            });
        }
    }
    Ok(())
}

/// The direction vocabulary is closed, so a label maps to a `&'static str` without leaking.
fn leak_direction(label: &str) -> &'static str {
    match label {
        "CORRECTIVE" => "CORRECTIVE",
        "PERMISSIVE" => "PERMISSIVE",
        "REGRESSIVE" => "REGRESSIVE",
        "NEUTRAL" => "NEUTRAL",
        "CONVERGENT" => "CONVERGENT",
        "DIVERGENT" => "DIVERGENT",
        _ => "UNKNOWN_DIRECTION",
    }
}

// ────────────────────────────────────────────────────────────────────────────────────────
// DRIFT CLASS 4 — a stale prose claim about the world
// ────────────────────────────────────────────────────────────────────────────────────────

/// ★★★ **The answer to drift class 4, and the reasoning, because a shrug is not an answer.**
///
/// The witness: CBR-L09's residual 3 recorded the `NaN` comparison divergence as *"FILED, NOT
/// FIXED"*; `19510082` ruled on it and fixed it **46 minutes** after the commit the row was
/// written against — *before the document was first saved carrying the row.* What went stale
/// was not a SHA, not a status field, not a line number and not an aggregate. It was a **prose
/// assertion about the state of the project**.
///
/// Three candidates were on the table. The answer is **(b) + (c), with (a) REJECTED on the
/// register's own criteria** — and the decomposition is the substance.
///
/// # (a) — *"name a work-item ID whose status the gate reads"* — REJECTED
///
/// Not on grounds of effort. On grounds the register already states: **nothing in this
/// repository reads the task tracker**. A clause that consulted it would need a network call
/// and would make CI's verdict a function of a mutable external database — which contradicts
/// §7.2's own load-bearing property, that no clause *"requires a build, a network call, or a
/// judgement"*. Worse, it would be **flaky**, and §7.6 finding 1 already establishes that a
/// flaky consensus gate is worse than none because *"it trains its readers to re-run it."*
///
/// # (b) — *"confine open-question prose to one register with its own freshness check"* — ADOPTED
///
/// §6.3 already **is** that register; what was missing is that nothing checked it. So every
/// §6.3 row now carries a **typed falsifier** in `register.toml`, and the falsifier vocabulary
/// is closed:
///
/// | falsifier | the claim stays live while … |
/// |---|---|
/// | `SYMBOL_PRESENT` | `token` occurs within ±[`CITATION_WINDOW`] lines of `line` in `path` at `HEAD` |
/// | `SYMBOL_ABSENT` | `token` does **not** occur there |
///
/// A row whose `state` is `OPEN` / `RULED` / `UNVERIFIED` must have its falsifier **hold**; a
/// row whose `state` is `CLOSED` must have it **fail**. ⇒ The clause fires in both
/// directions: a question answered by a change that never came back to update the row, and a
/// closure that did not actually happen.
///
/// ⚠ **(b) alone would NOT have caught the witness, and pretending otherwise would be the
/// same failure this gate exists to prevent.** CBR-L09 residual 3 *was* in §6.3 — as row 7 —
/// and the row went stale together with the entry body. What (b) buys is that the claim now
/// has a decision procedure at all; what it cannot buy is a decision procedure for a claim
/// whose subject is in another repository.
///
/// # (c) — *"accept it as uncheckable and SAY SO"* — ADOPTED for the residue, LOUDLY
///
/// The honest partition, measured: of §6.3's ten rows, **3** carry a decidable falsifier and
/// **7** are typed `UNDECIDABLE_HERE__*` with the reason — `FOREIGN_REPOSITORY` (4, including
/// the witness), `REQUIRES_EXPERIMENT` (1), `REQUIRES_OWNER_RULING` (1), and one
/// `NO_CITED_SITE` which is decidable *in principle* and is therefore an **actionable defect**
/// rather than a limit.
///
/// ⚠ 3 of 10 is a thin decidable fraction, and it is reported rather than rounded up: the
/// register's open questions are mostly about **another repository, the block store, or a
/// decision nobody has taken**, and no clause over this tree can settle any of those.
///
/// ★ The undecidable set's size is asserted **EXACTLY** by
/// [`check_open_question_freshness`], so it cannot grow quietly, and the register's §7.7.6
/// records the gap in §7 itself — which is the condition the brief attached to shipping (c) at
/// all. **(c) is shipped as a measured number with names attached, not as a shrug.**
const UNDECIDABLE_HERE: usize = 7;

const FALSIFIER_KINDS: [&str; 7] = [
    "SYMBOL_PRESENT",
    "SYMBOL_ABSENT",
    "UNDECIDABLE_HERE__FOREIGN_REPOSITORY",
    "UNDECIDABLE_HERE__CHAIN_HISTORY",
    "UNDECIDABLE_HERE__REQUIRES_EXPERIMENT",
    "UNDECIDABLE_HERE__REQUIRES_OWNER_RULING",
    "UNDECIDABLE_HERE__NO_CITED_SITE",
];

/// **DRIFT CLASS 4, the decidable part** — plus the exact assertion on the part that is not.
pub fn check_open_question_freshness(index: &Index) -> Result<(), DriftBreach> {
    let mut undecidable: Vec<String> = Vec::new();
    for row in index.rows("open_question") {
        let kind = row.get("falsifier").map(Val::as_str).unwrap_or("");
        let number = row.get("number").map(Val::as_int).unwrap_or(i64::MIN);
        if !FALSIFIER_KINDS.contains(&kind) {
            return Err(DriftBreach::UndecidableSetDrift {
                expected: UNDECIDABLE_HERE,
                found: undecidable.len(),
                kinds: vec![format!(
                    "question {number} carries unknown falsifier {kind:?}"
                )],
            });
        }
        if kind.starts_with("UNDECIDABLE_HERE__") {
            undecidable.push(format!("q{number}:{kind}"));
            continue;
        }
        let path = row.get("path").map(Val::as_str).unwrap_or("").to_string();
        // ⚠ `line` is a NAVIGATIONAL HINT for a reader, NOT part of the predicate. See below.
        let line = row.get("line").map(Val::as_int).unwrap_or(i64::MIN);
        let token = row.get("token").map(Val::as_str).unwrap_or("").to_string();
        let state = row.get("state").map(Val::as_str).unwrap_or("");
        // ★★ WHOLE-FILE, not a ±3 window around `line` — changed 2026-07-30, and it is a
        // FIX rather than a relaxation. A CITATION records a claim about a PAST state and is
        // pinned at a SHA so it stays checkable forever (register §7.7.5 rule 3). A
        // FALSIFIER records a claim about the PRESENT state — "the question stays open WHILE
        // that type is still there" — so it is evaluated at HEAD by construction, and the
        // register's `[[open_question]]` schema has no `at` field precisely because pinning
        // one at a SHA would make it hold forever and the question could never close.
        //
        // ⇒ Rule 3 cannot rescue a falsifier, and the LINE is the actual defect: it is
        // spurious precision. Question 10's token `HashMap<PublicKey, i64>` occurs at FOUR
        // lines of its file, so any single line was one arbitrary choice of four, and
        // `3fb4e21b` inserting 46 lines above it reddened three clauses over a coordinate
        // that was never load-bearing. §7.7.5 rule 1 already says the token is what makes a
        // coordinate falsifiable; for a falsifier that is the WHOLE of it.
        //
        // ★ The widening is CONSERVATIVE in the only direction that matters: §7.7.6 requires
        // a `CLOSED` row's falsifier to FAIL, so searching more text makes closure HARDER to
        // justify and never easier. Verified at the time of the change: questions 1 and 9
        // evaluate identically under both windows; only question 10 differs, and under the
        // whole file it correctly holds.
        let Some(body) = show("HEAD", &path) else {
            return Err(DriftBreach::OpenQuestionNoLongerOpen {
                number,
                path,
                line,
                token: format!("{token} (the file itself does not resolve at HEAD)"),
            });
        };
        let holds = body.lines().any(|l| l.contains(&token));
        let want = match kind {
            "SYMBOL_ABSENT" => !holds,
            _ => holds,
        };
        match (state, want) {
            ("CLOSED", true) => {
                return Err(DriftBreach::ClosedQuestionStillOpen {
                    number,
                    path,
                    line,
                    token,
                })
            }
            ("CLOSED", false) => {}
            (_, false) => {
                return Err(DriftBreach::OpenQuestionNoLongerOpen {
                    number,
                    path,
                    line,
                    token,
                })
            }
            (_, true) => {}
        }
    }
    if undecidable.len() != UNDECIDABLE_HERE {
        return Err(DriftBreach::UndecidableSetDrift {
            expected: UNDECIDABLE_HERE,
            found: undecidable.len(),
            kinds: undecidable,
        });
    }
    Ok(())
}

// ────────────────────────────────────────────────────────────────────────────────────────
// The remaining §7.2 clauses
// ────────────────────────────────────────────────────────────────────────────────────────

/// **§7.2 clause 4 — typed exemptions.** A free-text reason is a shrug; adding a reason
/// variant is a code change and therefore reviewed.
///
/// `evidence` must be non-empty **and** carry something a reader can go and check: a `.rs`
/// path, a digit, or a provenance tag. "It is fine" satisfies non-emptiness and discharges
/// nothing.
pub fn check_typed_exemptions(index: &Index) -> Result<(), DriftBreach> {
    for row in index.rows("exempt") {
        let commit = row.get("commit").map(Val::as_str).unwrap_or("").to_string();
        let reason = row.get("reason").map(Val::as_str).unwrap_or("");
        if !CLOSED_REASONS.contains(&reason) {
            return Err(DriftBreach::UntypedExemption {
                commit,
                reason: reason.to_string(),
            });
        }
        let evidence = row.get("evidence").map(Val::as_str).unwrap_or("");
        let discharges = evidence.contains(".rs")
            || evidence.contains("CITED")
            || evidence.contains("MEASURED")
            || evidence.contains("DERIVED")
            || evidence.chars().any(|c| c.is_ascii_digit());
        if evidence.trim().is_empty() || !discharges {
            return Err(DriftBreach::UndischargedExemption { commit });
        }
    }
    Ok(())
}

/// **§7.2 clause 5 — complete axis answers.** All seven cells present, all from §4.2's closed
/// vocabulary.
pub fn check_axis_completeness(index: &Index) -> Result<(), DriftBreach> {
    for row in index.rows("entry") {
        let id = row.get("id").map(Val::as_str).unwrap_or("?").to_string();
        for key in AXIS_KEYS {
            match row.get(key).map(Val::as_str) {
                None => {
                    return Err(DriftBreach::BadAxisCell {
                        id,
                        axis: key.to_string(),
                        value: "<absent>".to_string(),
                    })
                }
                Some(v) if !AXIS_VOCABULARY.contains(&v) => {
                    return Err(DriftBreach::BadAxisCell {
                        id,
                        axis: key.to_string(),
                        value: v.to_string(),
                    })
                }
                Some(_) => {}
            }
        }
    }
    Ok(())
}

/// **§7.2 clause 6 — the UNVERIFIED budget, asserted EXACTLY.**
///
/// ★ Exactly, in both directions. §6.4 records the budget going `1 → 2 → 1` in two days:
/// a cell became honest about being a design claim when its evidence evaporated, and returned
/// to `NO` when the evidence came back. *"A budget that can only rise is a ratchet, not a
/// measurement."* Both movements must be visible diffs, so `>` would be the wrong relation.
pub fn check_unverified_budget(index: &Index, p: &Projection) -> Result<(), DriftBreach> {
    let stated = index.int("unverified_budget");
    if stated != p.unverified_cells as i64 {
        return Err(DriftBreach::UnverifiedBudget {
            stated,
            counted: p.unverified_cells,
        });
    }
    Ok(())
}

/// ★★ **§7.2 clause 7b — the §4.1 ROW SET is the index's entry set.**
///
/// # The blind spot this closes
///
/// [`check_prose_index_agreement`] compares the entry **HEADINGS** (`### CBR-…`) with the
/// index, and then walks §4.1's rows comparing fields. Both halves are *conditional on a row
/// existing*: an entry with a heading, a body and an index row but **no glyph row in §4.1**
/// satisfies the heading-set test (its heading is present) and is simply never visited by the
/// field loop (there is no row to visit).
///
/// That is not hypothetical. **CBR-040 had no §4.1 row for an entire commit**, and nothing in
/// this gate could see it; it was found by a human reading the table. The failure is worse than
/// a missing line of prose, because §4.1 is *the single authoritative table* — [`project`]
/// derives every §5 count, share and percentage from these rows, so a missing row makes every
/// one of those figures quietly short, and the figure clause then *confirms* the wrong totals
/// because it is comparing the prose against the same deficient projection.
///
/// ⇒ the two sets must be **equal**, and the message must name the ids on each side.
pub fn check_summary_table_coverage(index: &Index, rows: &[SummaryRow]) -> Result<(), DriftBreach> {
    let ids: BTreeSet<String> = index
        .rows("entry")
        .iter()
        .map(|r| r.get("id").map(Val::as_str).unwrap_or("").to_string())
        .collect();
    let summary: BTreeSet<String> = rows.iter().map(|r| r.id.clone()).collect();
    let summary_only: Vec<String> = summary.difference(&ids).cloned().collect();
    let index_only: Vec<String> = ids.difference(&summary).cloned().collect();
    if !summary_only.is_empty() || !index_only.is_empty() {
        return Err(DriftBreach::SummaryTableDivergence {
            summary_only,
            index_only,
        });
    }
    Ok(())
}

/// **§7.2 clause 7 — prose ↔ index agreement**, strengthened.
///
/// §7.2 specified heading-set equality. That is necessary and **not sufficient**: an index row
/// can name the right entry and the wrong everything else. So every field the index shares
/// with §4.1's row — surface, direction, grade, and all seven axis cells — is compared too.
///
/// ★ This is the clause that makes the axis glyphs and the axis vocabulary one fact instead of
/// two: **57** rows × 7 cells of agreement, asserted rather than assumed.
///
/// ⚠ The row COUNT is not checked here and must not be inferred from this sentence — it is a
/// projection of §4.1 and is asserted by [`check_summary_table_coverage`] as a SET equality,
/// which is the checkable form. The figure above is prose and was stale at `45` for twelve
/// entries; it is corrected rather than deleted so the two clauses read as the pair they are.
pub fn check_prose_index_agreement(
    index: &Index,
    headings: &[String],
    rows: &[SummaryRow],
) -> Result<(), DriftBreach> {
    let ids: BTreeSet<String> = index
        .rows("entry")
        .iter()
        .map(|r| r.get("id").map(Val::as_str).unwrap_or("").to_string())
        .collect();
    let prose: BTreeSet<String> = headings.iter().cloned().collect();
    let prose_only: Vec<String> = prose.difference(&ids).cloned().collect();
    let index_only: Vec<String> = ids.difference(&prose).cloned().collect();
    if !prose_only.is_empty() || !index_only.is_empty() {
        return Err(DriftBreach::ProseIndexDivergence {
            prose_only,
            index_only,
        });
    }
    let by_id: BTreeMap<&str, &Row> = index
        .rows("entry")
        .iter()
        .map(|r| (r.get("id").map(Val::as_str).unwrap_or(""), r))
        .collect();
    for row in rows {
        let Some(ix) = by_id.get(row.id.as_str()) else {
            return Err(DriftBreach::ProseIndexDivergence {
                prose_only: vec![row.id.clone()],
                index_only: Vec::new(),
            });
        };
        for (field, prose_value) in [
            ("surface", row.surface.as_str()),
            ("direction", row.direction.as_str()),
            ("grade", row.grade.as_str()),
        ] {
            let index_value = ix.get(field).map(Val::as_str).unwrap_or("<absent>");
            if index_value != prose_value {
                return Err(DriftBreach::SummaryRowDisagrees {
                    id: row.id.clone(),
                    field: match field {
                        "surface" => "surface",
                        "direction" => "direction",
                        _ => "grade",
                    },
                    prose: prose_value.to_string(),
                    index: index_value.to_string(),
                });
            }
        }
        for (k, key) in AXIS_KEYS.iter().enumerate() {
            let index_value = ix.get(*key).map(Val::as_str).unwrap_or("<absent>");
            if index_value != row.axes[k] {
                return Err(DriftBreach::SummaryRowDisagrees {
                    id: row.id.clone(),
                    field: "axis",
                    prose: format!("{key} = {}", row.axes[k]),
                    index: index_value.to_string(),
                });
            }
        }
    }
    Ok(())
}

/// ★ **The cross-repository limit, asserted POSITIVELY.**
///
/// A Surface-L entry's commits belong to `mettail-rust`. The gate cannot check that such an
/// entry is *true* — but it can check that it is *foreign*: none of its SHAs may resolve here,
/// and it must name what does check it, or say `NOT_NAMED`.
///
/// ★ The `NOT_NAMED` count is asserted EXACTLY, which converts *"we cannot check Surface L"*
/// from a caveat into a number: 12 of 13.
pub fn check_foreign_rows(index: &Index) -> Result<(), DriftBreach> {
    const NOT_NAMED: usize = 12;
    let mut resolving = Vec::new();
    let mut missing_gate = Vec::new();
    let mut not_named = 0usize;
    for row in index.rows("entry") {
        if row.get("surface").map(Val::as_str) != Some("L") {
            continue;
        }
        let id = row.get("id").map(Val::as_str).unwrap_or("?").to_string();
        match row.get("source_gate").map(Val::as_str) {
            None | Some("") => missing_gate.push(id.clone()),
            Some("NOT_NAMED") => not_named += 1,
            Some(_) => {}
        }
        for sha in row.get("commits").map(Val::as_list).unwrap_or(&[]) {
            if resolves(sha) {
                resolving.push((id.clone(), sha.clone()));
            }
        }
    }
    if !missing_gate.is_empty() {
        return Err(DriftBreach::ForeignRowWithoutSourceGate { ids: missing_gate });
    }
    if !resolving.is_empty() {
        return Err(DriftBreach::ForeignShaResolvesLocally {
            offenders: resolving,
        });
    }
    if not_named != NOT_NAMED {
        return Err(DriftBreach::UndecidableSetDrift {
            expected: NOT_NAMED,
            found: not_named,
            kinds: vec!["Surface-L entries with source_gate = NOT_NAMED".to_string()],
        });
    }
    Ok(())
}

/// ★ **Every typed path exclusion must exclude something.**
///
/// The exclusion table is what stops the derived path set from quietly becoming the hand-list
/// it replaced, so each row carries its own falsifier: removing it must make the obligation
/// set **strictly larger**. A row that excludes nothing is dead weight and fails here.
/// ⚠ **The falsifier is over FILES, not commits — and the first run is why.**
///
/// The obvious form, *"removing the row must add a commit to the obligation set"*, was tried
/// and **REFUTED for `benches`**: over `7293d57c..HEAD` every bench-touching commit also
/// touches an included path (typically its own `[[bench]]` declaration in `Cargo.toml`), so
/// dropping the exclusion adds no *commits* while still changing which *paths* are in scope.
/// The path set is a statement about paths, so the obligation is measured over paths: the row
/// must exclude at least one **tracked file** in at least one closure crate.
pub fn check_path_exclusions_bite(_range: &str) -> Result<(), DriftBreach> {
    let crates = closure_crates();
    for x in PATH_EXCLUSIONS {
        let mut excluded_files = 0usize;
        for c in &crates {
            let listing = git(&[
                "ls-tree",
                "-r",
                "--name-only",
                "HEAD",
                "--",
                &format!("{c}/{}/", x.subdir),
            ]);
            if let Ok(text) = listing {
                excluded_files += text.lines().filter(|l| !l.trim().is_empty()).count();
            }
        }
        if excluded_files == 0 {
            return Err(DriftBreach::VacuousPathExclusion { subdir: x.subdir });
        }
    }
    Ok(())
}

// ════════════════════════════════════════════════════════════════════════════════════════
// 7 · The guards — every clause watched RED at the value it refuses, plus an ACCEPT cell
// ════════════════════════════════════════════════════════════════════════════════════════
//
// ★ Two rules govern this section, and both come from §7.4.
//
// 1. **The RED must fail on ITS OWN clause, not a neighbour's.** A check that goes red on the
//    wrong thing has not been shown to work, so every RED below asserts the *variant* it
//    expects — never merely `is_err()`.
// 2. **The ACCEPT cell is a guard too.** A gate that rejects every input is a wall, not a
//    floor. [`the_register_as_committed_passes_every_clause`] is what makes the refusals
//    meaningful.

/// A mutated copy of the index, so a RED cell perturbs exactly one value.
fn mutated(index: &Index, f: impl FnOnce(&mut Index)) -> Index {
    let mut copy = index.clone();
    f(&mut copy);
    copy
}

fn row_mut<'a>(index: &'a mut Index, table: &str, key: &str, value: &str) -> &'a mut Row {
    index
        .tables
        .get_mut(table)
        .expect("table present")
        .iter_mut()
        .find(|r| r.get(key).map(Val::as_str) == Some(value))
        .unwrap_or_else(|| panic!("no {table} row with {key} = {value}"))
}

struct Fixture {
    register: String,
    index: Index,
    rows: Vec<SummaryRow>,
    projection: Projection,
    headings: Vec<String>,
    coordinates: BTreeSet<Coordinate>,
    partition: Vec<String>,
    frontier: Vec<String>,
    entries_n: BTreeSet<String>,
    exempt_partition: BTreeSet<String>,
    exempt_all: BTreeSet<String>,
    head_time: i64,
}

fn fixture() -> Fixture {
    let register = read_register();
    let index = read_index();
    let rows = summary_rows(&register);
    let projection = project(&rows).expect("the summary table must project");
    let base = index.str("register_base").to_string();
    let part = index.str("partition_head").to_string();
    let spec = obligation_pathspec();
    Fixture {
        headings: entry_headings(&register),
        coordinates: document_coordinates(&register),
        partition: obligation_set(&format!("{base}..{part}"), &spec),
        frontier: obligation_set(&format!("{part}..HEAD"), &spec),
        entries_n: entry_commits(&index, Some("N")),
        exempt_partition: exempt_commits(&index, Some("PARTITION")),
        exempt_all: exempt_commits(&index, None),
        head_time: commit_time("HEAD"),
        register,
        index,
        rows,
        projection,
    }
}

// ────────────────────────────────────────────────────────────────────────────────────────
// The ACCEPT cell
// ────────────────────────────────────────────────────────────────────────────────────────

/// ★★ **ACCEPT.** The register as committed satisfies every clause.
///
/// Without this, the refusals below are a wall rather than a floor.
#[test]
fn the_register_as_committed_passes_every_clause() {
    let f = fixture();
    let checked = f
        .index
        .rows("citation")
        .iter()
        .filter(|r| r.contains_key("path"))
        .count();
    let decidable = f
        .index
        .rows("open_question")
        .iter()
        .filter(|r| {
            !r.get("falsifier")
                .map(Val::as_str)
                .unwrap_or("")
                .starts_with("UNDECIDABLE_HERE__")
        })
        .count();

    assert_eq!(
        check_non_vacuity(f.partition.len(), &f.index, checked, decidable),
        Ok(()),
        "★★ the non-vacuity floor must ADMIT a real run, or every refusal below is untestable"
    );
    assert_eq!(
        check_partition_coverage(&f.partition, &f.entries_n, &f.exempt_partition),
        Ok(())
    );
    assert_eq!(
        check_frontier_fuse(
            &f.frontier,
            &f.entries_n,
            &f.exempt_all,
            f.head_time,
            f.index.int("frontier_grace_days")
        ),
        Ok(())
    );
    assert_eq!(
        check_row_liveness(&f.entries_n, &f.exempt_all, &|s| resolves(s), &|s| {
            is_ancestor(s, "HEAD")
        }),
        Ok(())
    );
    assert_eq!(check_in_flight_staleness(&f.index), Ok(()), "class 1");
    assert_eq!(check_citation_rederivability(&f.index), Ok(()), "class 2");
    assert_eq!(check_citation_corpus(&f.index, &f.coordinates), Ok(()));
    assert_eq!(
        check_stated_figures(&f.register, &f.projection),
        Ok(()),
        "class 3"
    );
    assert_eq!(
        check_axis_exposure(&f.register, &f.projection),
        Ok(()),
        "class 3 · §5.1"
    );
    assert_eq!(
        check_direction_profile(&f.register, &f.projection),
        Ok(()),
        "class 3 · §5.3"
    );
    assert_eq!(check_open_question_freshness(&f.index), Ok(()), "class 4");
    assert_eq!(check_typed_exemptions(&f.index), Ok(()));
    assert_eq!(check_axis_completeness(&f.index), Ok(()));
    assert_eq!(check_unverified_budget(&f.index, &f.projection), Ok(()));
    assert_eq!(
        check_prose_index_agreement(&f.index, &f.headings, &f.rows),
        Ok(())
    );
    assert_eq!(
        check_summary_table_coverage(&f.index, &f.rows),
        Ok(()),
        "clause 7b — every index entry must have a §4.1 GLYPH ROW and vice versa"
    );
    assert_eq!(check_foreign_rows(&f.index), Ok(()));
}

// ────────────────────────────────────────────────────────────────────────────────────────
// ★★ The non-vacuity floor — a run that finds nothing must FAIL
// ────────────────────────────────────────────────────────────────────────────────────────

/// ★★ RED, the floor. Make the scanner match nothing and the gate must refuse.
#[test]
fn the_floor_refuses_an_empty_obligation_set() {
    let f = fixture();
    let breach = check_non_vacuity(0, &f.index, 1, 1)
        .expect_err("★★ a gate with nothing to check must FAIL, not pass");
    assert_eq!(breach, DriftBreach::EmptyObligation {
        found: 0,
        floor: 78
    });
    assert!(
        breach.to_string().starts_with("non-vacuity:"),
        "the refusal must name its reason first; got {breach}"
    );

    // ★ And the floor is a MEASURED lower bound, not merely "> 0": 77 must also fail,
    // because §3.1 measured 78 commits in the partition window by hand.
    assert!(matches!(
        check_non_vacuity(77, &f.index, 1, 1),
        Err(DriftBreach::EmptyObligation { found: 77, .. })
    ));
    // The controlled comparison: the real count, and nothing else changed, is admitted.
    assert_eq!(check_non_vacuity(f.partition.len(), &f.index, 1, 1), Ok(()));
}

/// ★ RED, the floor, per index table. An empty table makes every assertion over it vacuous.
#[test]
fn the_floor_refuses_an_index_table_that_lost_its_rows() {
    let f = fixture();
    for table in ["entry", "exempt", "citation", "open_question"] {
        let emptied = mutated(&f.index, |i| {
            i.tables.insert(table.to_string(), Vec::new());
        });
        let breach = check_non_vacuity(f.partition.len(), &emptied, 1, 1)
            .expect_err("an empty table must FAIL");
        assert_eq!(breach, DriftBreach::EmptyIndexTable { table: leak(table) });
    }
}

/// ★ RED, the floor, for the two derived corpora that could silently become empty: a citation
/// set in which nothing is checkable, and an open-question set in which nothing is decidable.
///
/// ⚠ Both are the *real* failure mode here, not a hypothetical: retyping every citation as
/// `FOREIGN_REPOSITORY` and every question as `UNDECIDABLE_HERE__*` would leave classes 2 and
/// 4 passing forever while checking nothing at all.
#[test]
fn the_floor_refuses_a_corpus_in_which_nothing_is_checkable() {
    let f = fixture();
    let breach = check_non_vacuity(f.partition.len(), &f.index, 0, 1)
        .expect_err("a citation corpus with no checkable row must FAIL");
    assert_eq!(breach, DriftBreach::EmptyIndexTable {
        table: "citation (all unchecked)"
    });

    let breach = check_non_vacuity(f.partition.len(), &f.index, 1, 0)
        .expect_err("an open-question set with no decidable row must FAIL");
    assert_eq!(breach, DriftBreach::EmptyIndexTable {
        table: "open_question (all undecidable)"
    });
}

/// ★★ **NO FILE CAN IMPERSONATE A COMMIT — asserted on the argument vectors, not on the prose.**
///
/// The failure this closes: on 2026-07-30 a commit hook wrote its ledger to a file literally
/// named `$sha` in the repository root, and `git log -1 --format=%ct ff244c69` — no `--` —
/// became *"fatal: ambiguous argument 'ff244c69': both revision and filename"*. The gate turned
/// that into **"★ HISTORY UNAVAILABLE … the checkout is shallow — set `fetch-depth: 0`"**, which
/// is a confident diagnosis of the wrong cause. History was complete.
///
/// Two legs, and the second is the one that keeps this fixed:
///
/// | leg | asserts | what it refuses |
/// |---|---|---|
/// | 1 | [`fenced_args`] ends every vector with `--` | the fence being dropped from the helper |
/// | 2 | **every** `git` call in this file either carries `"--"` or names a subcommand in [`UNFENCED_SUBCOMMANDS`] | a NEW unfenced rev call being added elsewhere in the file |
///
/// Leg 2 reads this file's own source, because the property is about *every call site* and a
/// property about every call site cannot be checked by exercising one of them. The exception
/// table is asserted EXACTLY in both directions, so a row that no longer describes any call is
/// dead weight and fails, exactly as [`PATH_EXCLUSIONS`] is treated.
#[test]
fn no_unfenced_git_call_can_be_shadowed_by_a_path() {
    // ── leg 1 ────────────────────────────────────────────────────────────────────────────
    assert_eq!(
        fenced_args(&["log", "-1", "--format=%ct", "deadbeef"]),
        vec!["log", "-1", "--format=%ct", "deadbeef", "--"],
        "★ the rev fence is gone from `fenced_args`, so any file named like a SHA prefix can \
         change this gate's verdict"
    );

    // ── leg 2: every call site in this file ──────────────────────────────────────────────
    //
    // ⚠ THE OPENERS ARE ASSEMBLED AT RUNTIME, AND THAT IS NOT STYLE. Written as literals they
    // would appear in this file's own source and the scanner would match ITSELF: the first run
    // reported `git , ()), ( …` as an unfenced subcommand, having found the array of openers
    // instead of a call. Nothing below may spell an opener contiguously — including in a panic
    // message or a doc comment.
    let source = include_str!("consensus_change_register_gate.rs");
    let array_open = format!("{}{}", "(&", "[");
    let openers = [
        format!("{}{array_open}", "git"),
        format!("{}{array_open}", "git_expect"),
    ];

    let mut inspected = 0usize;
    let mut used_exceptions: BTreeSet<&str> = BTreeSet::new();

    for opener in &openers {
        let mut rest = source;
        while let Some(at) = rest.find(opener.as_str()) {
            let after = &rest[at + opener.len()..];
            let close = after
                .find(']')
                .expect("a git call site must close its argument bracket");
            let call = &after[..close];
            rest = &after[close..];

            // The subcommand is the first string literal in the vector. A call built from a
            // `Vec` rather than a literal array (`obligation_set`) has none, and is skipped by
            // this scan — it fences explicitly and is covered by leg 1's sibling `--` push.
            let Some(first) = call.split('"').nth(1) else {
                continue;
            };
            inspected += 1;

            if call.contains("\"--\"") {
                continue;
            }
            match UNFENCED_SUBCOMMANDS.iter().find(|x| x.subcommand == first) {
                Some(x) => {
                    used_exceptions.insert(x.subcommand);
                }
                None => panic!(
                    "★★ UNFENCED REVISION ARGUMENT. `git {first} …` is invoked without a `--` \
                     fence and `{first}` is not in `UNFENCED_SUBCOMMANDS`. A file named like a \
                     SHA prefix then makes git refuse with 'ambiguous argument: both revision \
                     and filename', and this gate reports it as a shallow checkout. Route the \
                     call through `rev_only`, or add a row to `UNFENCED_SUBCOMMANDS` stating \
                     why the subcommand cannot take a pathspec.\n  arguments: {call}"
                ),
            }
        }
    }

    assert!(
        inspected >= 4,
        "VACUOUS SCAN: only {inspected} git call sites were found in this file, so the scanner \
         is not reading the calls it claims to check"
    );

    let declared: BTreeSet<&str> = UNFENCED_SUBCOMMANDS.iter().map(|x| x.subcommand).collect();
    assert_eq!(
        used_exceptions, declared,
        "★ the unfenced-subcommand table is asserted EXACTLY. A declared row that matches no \
         call site is dead weight and must be deleted; a call site that matched none would have \
         panicked above."
    );
    for x in UNFENCED_SUBCOMMANDS {
        assert!(
            x.why.len() > 60,
            "★ the `{}` row must STATE why the subcommand cannot confuse a path for a revision; \
             an exception without an argument is an assumption",
            x.subcommand
        );
    }
}

/// ★ RED, the floor, on the path-exclusion table: every row must exclude something.
///
/// ★ And every row of both hand-declared tables must carry a **stated obligation**. A root
/// without an argument is an assumption; an exclusion without a reason is a hole. The strings
/// are asserted rather than merely present, so the compiler cannot report them as dead code
/// and a future author cannot add an empty one.
#[test]
fn the_path_exclusions_each_exclude_something() {
    let f = fixture();
    let range = format!("{}..HEAD", f.index.str("register_base"));
    assert_eq!(
        check_path_exclusions_bite(&range),
        Ok(()),
        "a path exclusion that excludes nothing is dead weight and must be deleted"
    );

    assert_eq!(
        ROOT_CRATES.len(),
        1,
        "★ one hand-declared input to the path set, and one only"
    );
    for root in ROOT_CRATES {
        assert!(
            repo_root().join(root.dir).join("Cargo.toml").is_file(),
            "root crate {} must exist",
            root.dir
        );
        assert!(
            root.obligation.len() > 60,
            "★ root {} must STATE why it is a root; a root without an argument is an assumption",
            root.dir
        );
    }
    for x in PATH_EXCLUSIONS {
        assert!(
            x.why.len() > 40,
            "★ exclusion {:?} must state the reason cargo does not compile it into the library",
            x.subdir
        );
    }
    assert!(
        PATH_EXCLUSIONS
            .iter()
            .any(|x| x.subdir == "src/test" && x.why.contains("src/main")),
        "★ the `src/test` row must record that `casper/src/main/resources/` is DELIBERATELY not \
         excluded — CBR-030 is a genesis contract living there"
    );
}

// ────────────────────────────────────────────────────────────────────────────────────────
// DRIFT CLASS 1 — in-flight staleness
// ────────────────────────────────────────────────────────────────────────────────────────

/// ★★ RED, class 1, **at exactly the value CBR-027 held**: an entry that says it has not
/// landed while naming a SHA that has.
///
/// The mutation is one field on one row — CBR-027's status, back to `IN_FLIGHT` — and nothing
/// else. `6ff46f8a` is an ancestor of `HEAD`, which is the fact a human had to notice by
/// reading `git log`.
#[test]
fn the_in_flight_clause_refuses_an_entry_whose_commit_has_landed() {
    let f = fixture();
    let regressed = mutated(&f.index, |i| {
        row_mut(i, "entry", "id", "CBR-027").insert("status".into(), Val::Str("IN_FLIGHT".into()));
    });
    let breach = check_in_flight_staleness(&regressed)
        .expect_err("★★ IN FLIGHT with a landed SHA must be REFUSED");
    assert_eq!(
        breach,
        DriftBreach::InFlightButLanded {
            offenders: vec![
                ("CBR-027".to_string(), "6ff46f8a".to_string()),
                ("CBR-027".to_string(), "fd5474ab".to_string()),
            ],
        },
        "the refusal must name the entry AND the landed SHAs, not merely that something is wrong"
    );
    assert!(breach
        .to_string()
        .starts_with("class 1 — IN-FLIGHT STALENESS:"));

    // ★ THE CONTROLLED COMPARISON. Flip that one field back and nothing else, and the clause
    // admits it — which is what shows the refusal is attributable to the STATUS and not to
    // anything else in the row.
    assert_eq!(check_in_flight_staleness(&f.index), Ok(()));
}

/// ★ The control that shows class 1 is **not vacuous, and not over-eager**.
///
/// CBR-L08 is genuinely `IN_FLIGHT` and names no SHA; CBR-028 is `OPEN` and names five
/// commits that *characterise* an unrepaired hazard rather than implement it. §7.6 names
/// CBR-L08 as exactly this control: *"It would NOT catch CBR-L08, whose named change is
/// genuinely not in the tree — which is the control showing the check is not vacuous."*
#[test]
fn the_in_flight_clause_does_not_fire_on_a_genuinely_unlanded_entry() {
    let f = fixture();
    let l08 = f
        .index
        .rows("entry")
        .iter()
        .find(|r| r.get("id").map(Val::as_str) == Some("CBR-L08"))
        .expect("CBR-L08 is in the index");
    assert_eq!(l08.get("status").map(Val::as_str), Some("IN_FLIGHT"));
    assert!(
        l08.get("commits")
            .map(Val::as_list)
            .unwrap_or(&[])
            .is_empty(),
        "CBR-L08 must name no SHA, or it is no longer this control"
    );
    assert_eq!(check_in_flight_staleness(&f.index), Ok(()));
}

// ────────────────────────────────────────────────────────────────────────────────────────
// DRIFT CLASS 2 — transcribed `file:line` re-derivability
// ────────────────────────────────────────────────────────────────────────────────────────

/// ★★ RED, class 2, **reconstructing CBR-027's own second drift**: the `Files` cell cited
/// `wrapping_add` coordinates against a commit in which that call no longer exists.
///
/// The mutation repins CBR-027's citation from `61a53157` (where `wrapping_add` is at
/// `:3397`) to `fd5474ab` (where the checked form replaced it). One field. The token, the path
/// and the line are untouched — so a failure is attributable to the pin and to nothing else.
#[test]
fn the_citation_clause_refuses_a_coordinate_whose_token_is_gone() {
    let f = fixture();
    let regressed = mutated(&f.index, |i| {
        let row = i
            .tables
            .get_mut("citation")
            .expect("citations")
            .iter_mut()
            .find(|r| {
                r.get("owner").map(Val::as_str) == Some("CBR-027")
                    && r.get("token").map(Val::as_str) == Some("wrapping_add")
            })
            .expect("CBR-027's `wrapping_add` citation");
        row.insert("at".into(), Val::Str("fd5474ab".into()));
    });
    let breach = check_citation_rederivability(&regressed)
        .expect_err("★★ a coordinate whose token is gone must be REFUSED");
    let DriftBreach::StaleCitation { offenders } = &breach else {
        panic!("class 2 must fail on ITS OWN clause; got {breach:?}");
    };
    assert_eq!(offenders.len(), 1, "exactly one citation was perturbed");
    assert_eq!(offenders[0].owner, "CBR-027");
    assert_eq!(offenders[0].token, "wrapping_add");
    assert_eq!(offenders[0].at, "fd5474ab");
    assert!(
        !offenders[0].found.contains("wrapping_add"),
        "the refusal must show what is THERE, so the message names the fix; got {:?}",
        offenders[0].found
    );
    assert!(breach.to_string().contains("NON-RE-DERIVABLE citation"));

    // ★ The controlled comparison: the real pin, everything else identical, is admitted.
    assert_eq!(check_citation_rederivability(&f.index), Ok(()));
}

/// ★ RED, class 2, second form: the ±window is a **window**, not a licence. A token moved
/// four lines away is still a failure.
#[test]
fn the_citation_window_is_three_lines_and_not_more() {
    let f = fixture();
    let regressed = mutated(&f.index, |i| {
        let row = i
            .tables
            .get_mut("citation")
            .expect("citations")
            .iter_mut()
            .find(|r| r.get("token").map(Val::as_str) == Some("is_slashable"))
            .expect("the `is_slashable` citation");
        let line = row.get("line").map(Val::as_int).expect("a line");
        row.insert("line".into(), Val::Int(line + CITATION_WINDOW + 1));
    });
    assert!(
        matches!(
            check_citation_rederivability(&regressed),
            Err(DriftBreach::StaleCitation { .. })
        ),
        "a coordinate {} lines off must FAIL — otherwise the window is unbounded",
        CITATION_WINDOW + 1
    );

    // …and exactly ±CITATION_WINDOW away must still PASS, which is what makes the number a
    // decision rather than an accident.
    let tolerated = mutated(&f.index, |i| {
        let row = i
            .tables
            .get_mut("citation")
            .expect("citations")
            .iter_mut()
            .find(|r| r.get("token").map(Val::as_str) == Some("is_slashable"))
            .expect("the `is_slashable` citation");
        let line = row.get("line").map(Val::as_int).expect("a line");
        row.insert("line".into(), Val::Int(line + CITATION_WINDOW));
    });
    assert_eq!(check_citation_rederivability(&tolerated), Ok(()));
}

/// ★ RED on the corpus itself: the citation set is DERIVED from the prose, so deleting a row
/// cannot narrow it.
#[test]
fn the_citation_corpus_cannot_be_narrowed_by_deleting_a_row() {
    let f = fixture();
    let regressed = mutated(&f.index, |i| {
        i.tables.get_mut("citation").expect("citations").pop();
    });
    let breach = check_citation_corpus(&regressed, &f.coordinates)
        .expect_err("★ a coordinate in the prose with no pin must be REFUSED");
    let DriftBreach::CitationCorpusDrift {
        missing_rows,
        extra_rows,
    } = &breach
    else {
        panic!("must fail on the corpus clause; got {breach:?}");
    };
    assert_eq!(missing_rows.len(), 1);
    assert!(
        extra_rows.is_empty(),
        "nothing extra was added, so nothing extra may be reported"
    );
    assert_eq!(check_citation_corpus(&f.index, &f.coordinates), Ok(()));
}

// ────────────────────────────────────────────────────────────────────────────────────────
// DRIFT CLASS 3 — a stated aggregate that is not a projection
// ────────────────────────────────────────────────────────────────────────────────────────

/// ★★ RED, class 3, **reproducing the 2026-07-29 recount's own finding**: a figure computed at
/// an earlier entry count, left in place while the paragraph beside it was updated.
///
/// The mutation edits the DOCUMENT, not the table — the register keeps its rows and one stated
/// figure regresses to the value it held before the last entry landed. That is precisely the
/// shape of the witness: §5.1 read *"Share of the 40"* with Lane B at 19 while §4.1 said 44.
#[test]
fn the_figure_clause_refuses_a_total_that_is_not_the_projection() {
    let f = fixture();
    let anchor = "**Totals — ";
    let truth = format!("{anchor}{}", f.projection.rows);
    let stale = format!("{anchor}{}", f.projection.rows - 1);
    assert!(
        f.register.contains(&truth),
        "the §4.1 totals anchor must be present and carry a DIGIT; the anchor table cannot \
         check an English numeral"
    );
    let regressed = f.register.replacen(&truth, &stale, 1);

    let breach = check_stated_figures(&regressed, &f.projection)
        .expect_err("★★ a stated total that is not the projection must be REFUSED");
    assert_eq!(
        breach,
        DriftBreach::FigureDisagrees {
            site: "§4.1 totals".to_string(),
            quantity: "entry count".to_string(),
            stated: (f.projection.rows - 1) as i64,
            projected: f.projection.rows as i64,
        },
        "the refusal must name the SITE and both numbers"
    );
    assert!(breach
        .to_string()
        .contains("class 3 — PARTIAL-UPDATE DRIFT at §4.1 totals"));

    assert_eq!(check_stated_figures(&f.register, &f.projection), Ok(()));
}

/// ★ RED, class 3, on §5.1 — the table that actually went stale, checked structurally.
#[test]
fn the_axis_exposure_table_must_be_the_projection() {
    let f = fixture();
    // Perturb the post-state-hash row, which is the cell the recount found under by one.
    let truth = format!("| **{}** |", f.projection.axis_moves[4]);
    assert!(
        f.register.contains(&truth),
        "the §5.1 H row must carry its projected count"
    );
    let regressed = f.register.replacen(&truth, "| **28** |", 1);
    let breach = check_axis_exposure(&regressed, &f.projection)
        .expect_err("★ a §5.1 count that is not the projection must be REFUSED");
    let DriftBreach::FigureDisagrees {
        site,
        stated,
        projected,
        ..
    } = &breach
    else {
        panic!("must fail on the §5.1 clause; got {breach:?}");
    };
    assert_eq!(*stated, 28);
    assert_eq!(*projected, f.projection.axis_moves[4] as i64);
    assert!(
        site.starts_with("§5.1 row 5"),
        "the refusal must name the row; got {site}"
    );

    assert_eq!(check_axis_exposure(&f.register, &f.projection), Ok(()));
}

/// ★ RED, class 3, on §5.3 — including its own `Total` row, which is the register's arithmetic
/// check on itself.
#[test]
fn the_direction_profile_must_be_the_projection() {
    let f = fixture();
    let rows = counted_table(&f.register, "### 5.3 Direction profile", "### 5.4 ");
    assert!(
        rows.iter().any(|(l, _)| l == "Total"),
        "§5.3 must keep its Total row — it is the check that no row was double-counted"
    );
    let corrective = f
        .projection
        .direction
        .get("CORRECTIVE")
        .copied()
        .unwrap_or(0);
    let truth = format!("| CORRECTIVE | **{corrective}** |");
    assert!(f.register.contains(&truth));
    let regressed = f.register.replacen(&truth, "| CORRECTIVE | **23** |", 1);
    let breach = check_direction_profile(&regressed, &f.projection)
        .expect_err("★ a §5.3 count that is not the projection must be REFUSED");
    assert_eq!(
        breach,
        DriftBreach::FigureDisagrees {
            site: "§5.3 row CORRECTIVE".to_string(),
            quantity: "CORRECTIVE count".to_string(),
            stated: 23,
            projected: corrective as i64,
        },
        "23 is the value the previous revision actually carried, at 40 entries"
    );
    assert_eq!(check_direction_profile(&f.register, &f.projection), Ok(()));
}

/// ★ RED on the INSTRUMENT rather than the measurement: a malformed axis cell must fail the
/// arithmetic identity **before** any figure is compared, so the prose is not blamed for a
/// broken parser.
#[test]
fn a_malformed_axis_cell_fails_the_identity_not_the_prose() {
    let f = fixture();
    let mut rows = f.rows.clone();
    rows[3].axes[2] = "GARBAGE".to_string();
    let breach = project(&rows).expect_err("★ a cell outside the vocabulary must be REFUSED");
    assert_eq!(breach, DriftBreach::BadAxisCell {
        id: rows[3].id.clone(),
        axis: "axis_bytes_bincode".to_string(),
        value: "GARBAGE".to_string(),
    });

    // And a dropped row is caught by the split identity rather than by a figure comparison.
    let mut short = f.rows.clone();
    short.pop();
    let p = project(&short).expect("a shorter table still projects");
    assert_eq!(p.rows, f.projection.rows - 1);
    let breach = check_stated_figures(&f.register, &p)
        .expect_err("a projection over a truncated table must disagree with the prose");
    assert!(matches!(breach, DriftBreach::FigureDisagrees { .. }));
}

// ────────────────────────────────────────────────────────────────────────────────────────
// DRIFT CLASS 4 — the decidable part, and the exactly-asserted part that is not
// ────────────────────────────────────────────────────────────────────────────────────────

/// ★★ RED, class 4: an open question recorded as still open whose falsifier no longer holds.
///
/// This is the shape of the CBR-L09 incident *for the claims whose subject is in this
/// repository*: the question was answered by a change that never came back to the row.
#[test]
fn the_open_question_clause_refuses_a_claim_its_falsifier_no_longer_supports() {
    let f = fixture();
    // `row_mut` keys on string fields; `number` is an integer, so this lookup is explicit.
    let regressed = mutated(&f.index, |i| {
        let row = i
            .tables
            .get_mut("open_question")
            .expect("questions")
            .iter_mut()
            .find(|r| r.get("number").map(Val::as_int) == Some(10))
            .expect("open question 10");
        row.insert("token".into(), Val::Str("a_token_that_is_not_there".into()));
    });
    let breach = check_open_question_freshness(&regressed)
        .expect_err("★★ an OPEN claim whose falsifier fails must be REFUSED");
    assert_eq!(breach, DriftBreach::OpenQuestionNoLongerOpen {
        number: 10,
        path: "casper/src/rust/test_utils/util/genesis_builder.rs".to_string(),
        line: 174,
        token: "a_token_that_is_not_there".to_string(),
    });
    assert!(breach.to_string().starts_with("class 4 — open question 10"));
    assert_eq!(check_open_question_freshness(&f.index), Ok(()));
}

/// ★ RED, class 4, the **other direction**: a question recorded `CLOSED` whose falsifier still
/// holds. A closure that did not happen is worse than an open question, because nobody looks.
#[test]
fn the_open_question_clause_refuses_a_closure_that_did_not_happen() {
    let f = fixture();
    let regressed = mutated(&f.index, |i| {
        let row = i
            .tables
            .get_mut("open_question")
            .expect("questions")
            .iter_mut()
            .find(|r| r.get("number").map(Val::as_int) == Some(9))
            .expect("open question 9");
        row.insert("state".into(), Val::Str("CLOSED".into()));
    });
    let breach = check_open_question_freshness(&regressed)
        .expect_err("★ a CLOSED question whose falsifier still holds must be REFUSED");
    assert!(
        matches!(breach, DriftBreach::ClosedQuestionStillOpen {
            number: 9,
            ..
        }),
        "must fire on the closure clause, naming question 9; got {breach:?}"
    );
}

/// ★★ RED on the gate's own **honesty about its coverage gap**: the set of questions it
/// declines to check is asserted EXACTLY, so it cannot grow quietly.
///
/// ⚠ This is the clause that makes answer (c) shippable. Retyping one decidable question as
/// undecidable — the cheapest way to make class 4 pass by checking less — fails here.
#[test]
fn the_undecidable_set_is_asserted_exactly_and_cannot_grow() {
    let f = fixture();
    let regressed = mutated(&f.index, |i| {
        let row = i
            .tables
            .get_mut("open_question")
            .expect("questions")
            .iter_mut()
            .find(|r| r.get("number").map(Val::as_int) == Some(1))
            .expect("open question 1");
        row.insert(
            "falsifier".into(),
            Val::Str("UNDECIDABLE_HERE__REQUIRES_OWNER_RULING".into()),
        );
    });
    let breach = check_open_question_freshness(&regressed)
        .expect_err("★★ growing the undecidable set must be REFUSED");
    let DriftBreach::UndecidableSetDrift {
        expected, found, ..
    } = &breach
    else {
        panic!("must fail on the exactness clause; got {breach:?}");
    };
    assert_eq!(
        (*expected, *found),
        (UNDECIDABLE_HERE, UNDECIDABLE_HERE + 1)
    );
    assert!(breach.to_string().contains("honest coverage gap"));

    // And an unknown falsifier kind is refused rather than ignored.
    let unknown = mutated(&f.index, |i| {
        let row = i.tables.get_mut("open_question").expect("questions")[0].clone();
        let mut row = row;
        row.insert("falsifier".into(), Val::Str("PROBABLY_FINE".into()));
        i.tables.get_mut("open_question").expect("questions")[0] = row;
    });
    assert!(matches!(
        check_open_question_freshness(&unknown),
        Err(DriftBreach::UndecidableSetDrift { .. })
    ));
}

// ────────────────────────────────────────────────────────────────────────────────────────
// Coverage, exactness, liveness, typed exemptions, axes, budget, prose ↔ index, foreign rows
// ────────────────────────────────────────────────────────────────────────────────────────

/// ★★ §7.4 cell 1, verbatim: *"Remove one SHA from `register.toml`. The gate must fail
/// **naming that SHA**."*
#[test]
fn the_coverage_clause_names_the_sha_it_lost() {
    let f = fixture();
    let dropped = "6bc58743";
    let mut entries = f.entries_n.clone();
    assert!(
        entries.remove(dropped),
        "CBR-001's fix commit must be in the index"
    );
    let breach = check_partition_coverage(&f.partition, &entries, &f.exempt_partition)
        .expect_err("★★ an unregistered consensus-path commit must be REFUSED");
    let DriftBreach::Unregistered { window, commits } = &breach else {
        panic!("must fail on coverage; got {breach:?}");
    };
    assert_eq!(*window, "PARTITION");
    assert_eq!(commits.len(), 1, "exactly one SHA was removed");
    assert_eq!(commits[0].0, dropped, "the refusal must NAME the SHA");
    assert!(
        !commits[0].1.is_empty(),
        "and its subject, so a reviewer can classify it without a git call"
    );
    assert_eq!(
        check_partition_coverage(&f.partition, &f.entries_n, &f.exempt_partition),
        Ok(())
    );
}

/// ★ §7.4 cell 2's real intent — *catch a rebase* — at the clause that actually decides it.
///
/// §7.2 specified this as `E ⊎ X ⊆ O`, and that form is **measurably wrong**: `719f2432` is
/// CBR-030's evidence commit and touches only `casper/tests/`, so it is not in the obligation
/// set and a `⊆` test would refuse a correct row. The property wanted is ancestry.
#[test]
fn the_liveness_clause_refuses_a_commit_that_is_not_an_ancestor() {
    let f = fixture();
    let phantom = "deadbeef";
    let mut entries = f.entries_n.clone();
    entries.insert(phantom.to_string());
    let breach = check_row_liveness(
        &entries,
        &f.exempt_all,
        &|s| s == phantom || resolves(s),
        &|s| s != phantom && is_ancestor(s, "HEAD"),
    )
    .expect_err("★ a row naming a commit outside HEAD's history must be REFUSED");
    assert_eq!(breach, DriftBreach::AbandonedRow {
        commits: vec![phantom.to_string()]
    });
    assert!(breach.to_string().starts_with("stale row:"));

    // ★ THE CONTROL that shows §7.2's specified form was wrong: `719f2432` is a real entry
    // SHA, is an ancestor of HEAD, and is NOT in the obligation set — and it must pass.
    assert!(
        f.entries_n.contains("719f2432"),
        "CBR-030's evidence commit is an entry SHA"
    );
    assert!(
        !f.partition.contains(&"719f2432".to_string())
            && !f.frontier.contains(&"719f2432".to_string()),
        "…and it touches only `casper/tests/`, so it is NOT an obligation — which is exactly \
         why the clause is ancestry and not `⊆`"
    );
    assert_eq!(
        check_row_liveness(&f.entries_n, &f.exempt_all, &|s| resolves(s), &|s| {
            is_ancestor(s, "HEAD")
        }),
        Ok(())
    );
}

/// ★ RED: a SHA that is both explained and exempted.
#[test]
fn the_exactness_clause_refuses_a_double_listed_sha() {
    let f = fixture();
    let mut exempt = f.exempt_partition.clone();
    exempt.insert("6bc58743".to_string());
    let breach = check_partition_coverage(&f.partition, &f.entries_n, &exempt)
        .expect_err("★ a SHA both explained and exempted must be REFUSED");
    assert_eq!(breach, DriftBreach::DoubleListed {
        commits: vec!["6bc58743".to_string()]
    });
}

/// ★★ §7.4 cell 3, verbatim: *"Blank one `evidence` field. The gate must fail on the
/// *undischarged exemption* clause."* — and its sibling, an untyped reason.
#[test]
fn the_exemption_clause_refuses_a_shrug() {
    let f = fixture();
    let blanked = mutated(&f.index, |i| {
        row_mut(i, "exempt", "commit", "f0eb7e5f")
            .insert("evidence".into(), Val::Str(String::new()));
    });
    let breach = check_typed_exemptions(&blanked)
        .expect_err("★★ an exemption with no evidence must be REFUSED");
    assert_eq!(breach, DriftBreach::UndischargedExemption {
        commit: "f0eb7e5f".to_string()
    });
    assert!(breach.to_string().starts_with("undischarged exemption:"));

    // ★ Non-empty is not enough: evidence must name something a reader can go and check.
    let shrug = mutated(&f.index, |i| {
        row_mut(i, "exempt", "commit", "f0eb7e5f")
            .insert("evidence".into(), Val::Str("it is fine".into()));
    });
    assert_eq!(
        check_typed_exemptions(&shrug),
        Err(DriftBreach::UndischargedExemption {
            commit: "f0eb7e5f".to_string()
        }),
        "★ 'it is fine' satisfies non-emptiness and discharges nothing"
    );

    let untyped = mutated(&f.index, |i| {
        row_mut(i, "exempt", "commit", "f0eb7e5f")
            .insert("reason".into(), Val::Str("PROBABLY_HARMLESS".into()));
    });
    assert_eq!(
        check_typed_exemptions(&untyped),
        Err(DriftBreach::UntypedExemption {
            commit: "f0eb7e5f".to_string(),
            reason: "PROBABLY_HARMLESS".to_string(),
        })
    );
    assert_eq!(check_typed_exemptions(&f.index), Ok(()));
}

/// ★ RED: an axis cell that is absent, and one outside the closed vocabulary.
#[test]
fn the_axis_clause_refuses_an_incomplete_answer() {
    let f = fixture();
    let absent = mutated(&f.index, |i| {
        row_mut(i, "entry", "id", "CBR-001").remove("axis_metering");
    });
    assert_eq!(
        check_axis_completeness(&absent),
        Err(DriftBreach::BadAxisCell {
            id: "CBR-001".to_string(),
            axis: "axis_metering".to_string(),
            value: "<absent>".to_string(),
        })
    );
    let bad = mutated(&f.index, |i| {
        row_mut(i, "entry", "id", "CBR-001")
            .insert("axis_metering".into(), Val::Str("PROBABLY_NOT".into()));
    });
    assert_eq!(
        check_axis_completeness(&bad),
        Err(DriftBreach::BadAxisCell {
            id: "CBR-001".to_string(),
            axis: "axis_metering".to_string(),
            value: "PROBABLY_NOT".to_string(),
        })
    );
    assert_eq!(check_axis_completeness(&f.index), Ok(()));
}

/// ★ RED, **in both directions**, on §6.4's budget. A budget that can only rise is a ratchet.
#[test]
fn the_unverified_budget_is_exact_in_both_directions() {
    let f = fixture();
    let counted = f.projection.unverified_cells;
    for stated in [counted as i64 - 1, counted as i64 + 1] {
        let regressed = mutated(&f.index, |i| {
            i.header
                .insert("unverified_budget".into(), Val::Int(stated));
        });
        assert_eq!(
            check_unverified_budget(&regressed, &f.projection),
            Err(DriftBreach::UnverifiedBudget { stated, counted }),
            "★ the budget must be asserted EXACTLY — {stated} against a projected {counted}"
        );
    }
    assert_eq!(check_unverified_budget(&f.index, &f.projection), Ok(()));
}

/// ★ RED: the prose and the index disagree — first on membership, then on a field, which is
/// the case §7.2's heading-set clause could not see.
#[test]
fn the_prose_index_clause_refuses_a_row_that_lies_about_its_entry() {
    let f = fixture();
    let dropped = mutated(&f.index, |i| {
        let rows = i.tables.get_mut("entry").expect("entries");
        rows.retain(|r| r.get("id").map(Val::as_str) != Some("CBR-013"));
    });
    let breach = check_prose_index_agreement(&dropped, &f.headings, &f.rows)
        .expect_err("★ a heading with no index row must be REFUSED");
    assert_eq!(breach, DriftBreach::ProseIndexDivergence {
        prose_only: vec!["CBR-013".to_string()],
        index_only: Vec::new(),
    });

    // ★ The strengthening: membership was right and the CONTENT was wrong.
    let lying = mutated(&f.index, |i| {
        row_mut(i, "entry", "id", "CBR-013").insert("grade".into(), Val::Str("WITNESSED".into()));
    });
    let breach = check_prose_index_agreement(&lying, &f.headings, &f.rows)
        .expect_err("★ an index row that disagrees with its §4.1 row must be REFUSED");
    assert_eq!(
        breach,
        DriftBreach::SummaryRowDisagrees {
            id: "CBR-013".to_string(),
            field: "grade",
            prose: "MECHANISM_ONLY".to_string(),
            index: "WITNESSED".to_string(),
        },
        "§7.2's clause 7 checked ids only; an index row can name the right entry and the wrong \
         everything else"
    );

    // …and an axis cell edited on one side only.
    let skewed = mutated(&f.index, |i| {
        row_mut(i, "entry", "id", "CBR-013").insert("axis_value".into(), Val::Str("NO".into()));
    });
    assert!(matches!(
        check_prose_index_agreement(&skewed, &f.headings, &f.rows),
        Err(DriftBreach::SummaryRowDisagrees { field: "axis", .. })
    ));
    assert_eq!(
        check_prose_index_agreement(&f.index, &f.headings, &f.rows),
        Ok(())
    );
}

/// ★★ RED for clause 7b — **the missing GLYPH ROW**, which is the shape CBR-040 shipped in.
///
/// The mutation is deliberately the *narrow* one: the entry keeps its heading, its body and its
/// `[[entry]]` row, and loses only its line in §4.1. That is precisely what
/// [`check_prose_index_agreement`] cannot see — its heading-set test passes (the heading is
/// still there) and its field loop never visits a row that does not exist — so this test also
/// asserts that the OLD clause stays green on the same input. Without that second half, a
/// future refactor could fold 7b back into 7 and nobody would learn that 7 alone was blind.
#[test]
fn the_summary_table_clause_names_the_glyph_row_it_lost() {
    let f = fixture();

    // ── The mutation: drop ONE glyph row, change nothing else. ──────────────────────────
    let mut rows_without = f.rows.clone();
    rows_without.retain(|r| r.id != "CBR-013");
    assert_eq!(
        rows_without.len() + 1,
        f.rows.len(),
        "★ the mutation must actually remove exactly one row, or the RED below is vacuous"
    );

    let breach = check_summary_table_coverage(&f.index, &rows_without)
        .expect_err("★ an index entry with no §4.1 glyph row must be REFUSED");
    assert_eq!(
        breach,
        DriftBreach::SummaryTableDivergence {
            summary_only: Vec::new(),
            index_only: vec!["CBR-013".to_string()],
        },
        "the message must NAME the id that lost its row — CBR-040 was found by a human \
         precisely because nothing named it"
    );

    // ★★ THE SECOND HALF: clause 7 is BLIND to this exact mutation. This is the evidence
    // that 7b earns its place rather than restating its neighbour.
    assert_eq!(
        check_prose_index_agreement(&f.index, &f.headings, &rows_without),
        Ok(()),
        "★ clause 7 must PASS on the very input clause 7b refuses. If it ever starts failing \
         here, the two clauses have converged and this one can be reconsidered — until then, \
         removing 7b re-opens the CBR-040 hole."
    );

    // …and the OTHER direction: a §4.1 row for an entry the index does not have.
    let mut rows_extra = f.rows.clone();
    let mut ghost = f.rows[0].clone();
    ghost.id = "CBR-999".to_string();
    rows_extra.push(ghost);
    assert_eq!(
        check_summary_table_coverage(&f.index, &rows_extra),
        Err(DriftBreach::SummaryTableDivergence {
            summary_only: vec!["CBR-999".to_string()],
            index_only: Vec::new(),
        }),
        "★ the clause is a SET EQUALITY, so a glyph row with no `[[entry]]` must be refused too"
    );

    // ★ ACCEPT: the register as committed satisfies it.
    assert_eq!(check_summary_table_coverage(&f.index, &f.rows), Ok(()));
}

/// ★ RED on the cross-repository limit, asserted positively: a Surface-L row whose SHA
/// resolves here.
#[test]
fn a_surface_l_row_may_not_name_a_local_commit() {
    let f = fixture();
    let regressed = mutated(&f.index, |i| {
        row_mut(i, "entry", "id", "CBR-L10")
            .insert("commits".into(), Val::List(vec!["6bc58743".to_string()]));
    });
    let breach = check_foreign_rows(&regressed)
        .expect_err("★ a Surface-L row naming a local SHA must be REFUSED");
    assert_eq!(breach, DriftBreach::ForeignShaResolvesLocally {
        offenders: vec![("CBR-L10".to_string(), "6bc58743".to_string())],
    });
    let no_gate = mutated(&f.index, |i| {
        row_mut(i, "entry", "id", "CBR-L10").remove("source_gate");
    });
    assert_eq!(
        check_foreign_rows(&no_gate),
        Err(DriftBreach::ForeignRowWithoutSourceGate {
            ids: vec!["CBR-L10".to_string()]
        }),
        "a row this gate cannot check must at least NAME what does, or say NOT_NAMED"
    );
    assert_eq!(check_foreign_rows(&f.index), Ok(()));
}

/// ★ RED on the frontier fuse: an unregistered frontier commit is REFUSED once it is older
/// than `frontier_grace_days`, and ADMITTED while it is younger — probed at the exact
/// boundary the configured value names.
///
/// ⚠ **Three defects this guard used to carry, all of one shape: the guard asserted things
/// about the LIVE frontier that only held by luck.**
///
/// 1. **The un-fused floor.** It asserted `unregistered.is_empty()` over the live frontier
///    before probing anything — an assertion with **no grace window at all**.
///    [`check_frontier_fuse`] deliberately tolerates a *young* unregistered commit, and this
///    line re-imposed the very obligation the fuse exists to relax. ⇒ the fuse was *a fuse in
///    the function and a tripwire in its own guard*: it fired on whoever committed next
///    rather than on drift, which is the exact failure mode [`check_frontier_fuse`]'s own
///    doc-comment argues against ("a gate that fires on the wrong thing gets disabled").
///
/// 2. **★ The sibling — deleting that line is NOT the repair.** `commits.len() == 1` below
///    carried the *same* coupling: at a fuse of `-1` **every** unregistered frontier commit
///    blows, so the count equalled one only while the live frontier happened to be otherwise
///    fully registered. A concurrent agent landing one consensus-path commit turned a
///    `len() == 1` into `len() == 2`. Both cells had to move onto a CONTROLLED frontier;
///    fixing only the first would have left the tripwire in place one line down.
///
/// 3. **A dated time bomb.** The admit cell read the exemplar's age off the live `HEAD`, so it
///    held only while `HEAD` stayed within `frontier_grace_days` of `53e78427` (committed
///    2026-07-29). It would have gone red on **2026-08-01** for a reason wholly unconnected
///    to drift. `head_time` is a *parameter* of the fuse, so both ages are now synthesised
///    from the exemplar's own commit date and the verdict is invariant under the passage of
///    time — the same "pure function of the checkout" property the fuse's doc-comment claims.
///
/// **A floor may assert NON-VACUITY; it may never assert the obligation under test.** That is
/// the rule the three defects above each broke, and the floors below are written to it: they
/// establish that the probe is *about* something, and nothing more.
///
/// **Where the live-frontier obligation lives.** Unchanged, and asserted with the fuse
/// HONOURED, in [`the_register_as_committed_passes_every_clause`]. No coverage is lost here;
/// what is removed is a *duplicate* of that obligation with the fuse stripped off.
#[test]
fn the_frontier_fuse_admits_a_fresh_commit_and_refuses_a_stale_one() {
    const DAY: i64 = 86_400;
    // An unregistered-but-EXEMPTED commit on the living frontier. Removing its exemption is
    // what makes it the single unregistered member of the controlled frontier below.
    const EXEMPLAR: &str = "53e78427";

    let f = fixture();
    let grace = f.index.int("frontier_grace_days");

    // ── FLOORS. Non-vacuity only: each says the probe is ABOUT something. ──
    assert!(
        !f.frontier.is_empty(),
        "★ FLOOR for this guard: the frontier must be non-empty, or the cells below compare \
         two empty sets"
    );
    assert!(
        f.frontier.iter().any(|c| c.as_str() == EXEMPLAR),
        "★ FLOOR: the exemplar {EXEMPLAR} must be ON the living frontier, or the probe below \
         is about a commit this clause never examines. If `partition_head` has moved past it, \
         choose a new exemplar from the frontier — do NOT relax the probe."
    );
    assert!(
        f.exempt_all.contains(EXEMPLAR),
        "★ FLOOR: the exemplar must be EXEMPTED in the committed register, so that removing \
         its exemption below is what makes it unregistered"
    );
    assert!(
        grace >= 0,
        "★ FLOOR: `frontier_grace_days` must be non-negative, or the fuse forgives nothing and \
         the admit cell below is vacuous"
    );

    // ── A CONTROLLED frontier: every member registered or exempted, EXCEPT the exemplar. ──
    // The live frontier's incidental registration state cannot reach the cells below, so a
    // concurrent agent's fresh commit can no longer redden this guard.
    let mut exempt = f.exempt_all.clone();
    assert!(
        exempt.remove(EXEMPLAR),
        "the sorter conversion must be exempted"
    );
    let frontier: Vec<String> = f
        .frontier
        .iter()
        .filter(|c| {
            c.as_str() == EXEMPLAR
                || f.entries_n.contains(c.as_str())
                || exempt.contains(c.as_str())
        })
        .cloned()
        .collect();
    let unregistered: Vec<&str> = frontier
        .iter()
        .map(String::as_str)
        .filter(|c| !f.entries_n.contains(*c) && !exempt.contains(*c))
        .collect();
    assert_eq!(
        unregistered,
        vec![EXEMPLAR],
        "★ FLOOR: the controlled frontier must leave EXACTLY the exemplar unregistered, or the \
         two cells below are not measuring the fuse"
    );

    let born = commit_time(EXEMPLAR);

    // ── REFUSE: one day PAST the configured fuse. ──
    let breach = check_frontier_fuse(
        &frontier,
        &f.entries_n,
        &exempt,
        born + (grace + 1) * DAY,
        grace,
    )
    .expect_err("★ an unregistered frontier commit past its fuse must be REFUSED");
    let DriftBreach::FrontierFuseBlown {
        commits,
        grace_days,
    } = &breach
    else {
        panic!("must fail on the frontier clause; got {breach:?}");
    };
    assert_eq!(*grace_days, grace);
    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0].0, EXEMPLAR);
    assert_eq!(
        commits[0].2,
        grace + 1,
        "the refusal must report the age that blew the fuse, not merely that one blew"
    );

    // ── ADMIT: exactly AT the fuse — the boundary the configured value actually names, which
    // the old `-1` probe never reached. ★ This is what makes the fuse a fuse: the same missing
    // row, inside the real grace window, is tolerated — the difference between a ratchet with
    // a fuse and a tripwire that fires on whoever commits next.
    assert_eq!(
        check_frontier_fuse(&frontier, &f.entries_n, &exempt, born + grace * DAY, grace),
        Ok(()),
        "★ an unregistered frontier commit AT its fuse must be ADMITTED"
    );
}

// ────────────────────────────────────────────────────────────────────────────────────────
// The parser, and the classes the gate cannot cover
// ────────────────────────────────────────────────────────────────────────────────────────

/// ★ The parser REFUSES rather than skips. A permissive parser would turn an unrecognised key
/// into an unasserted pin, which is the failure mode the whole gate exists to prevent.
#[test]
fn the_index_parser_refuses_everything_outside_its_grammar() {
    let cases: [(&str, &str); 5] = [
        ("[entry]\nid = \"CBR-001\"\n", "a single-bracket"),
        ("[[entry]]\n  id = \"CBR-001\"\n", "indented"),
        ("[[entry]]\nid=\"CBR-001\"\n", "not `key = value`"),
        ("[[entry]]\nid = \"a\"\nid = \"b\"\n", "duplicate key"),
        (
            "[[entry]]\naxes = { value = \"MOVES\" }\n",
            "not a quoted string",
        ),
    ];
    for (text, expected_why) in cases {
        let breach = parse_index(text).expect_err("★ the parser must REFUSE, never skip");
        let DriftBreach::Malformed { why, line, .. } = &breach else {
            panic!("the parser must fail as Malformed; got {breach:?}");
        };
        assert!(
            why.contains(expected_why),
            "the refusal must name WHY {line:?} is not in the grammar; wanted {expected_why:?}, \
             got {why:?}"
        );
    }
    // The controlled comparison: the grammar's own shapes parse.
    let ok = parse_index(
        "schema_version = 1\n\n[[entry]]\nid = \"CBR-001\"\ncommits = [\"a\", \"b\"]\n",
    )
    .expect("the declared grammar must parse");
    assert_eq!(ok.header.get("schema_version"), Some(&Val::Int(1)));
    assert_eq!(ok.rows("entry").len(), 1);
    assert_eq!(
        ok.rows("entry")[0].get("commits"),
        Some(&Val::List(vec!["a".into(), "b".into()]))
    );
}

/// ★ The derived path set must RECOVER what the hand-listed one lost, and must not lose what
/// the hand list had.
///
/// Two named witnesses, both from the register's own §3.4:
/// * `rho-pure-eval` — omitted by the first hand list, and the component that decides `where`
///   verdicts. It must be in the closure **by derivation**, not by memory.
/// * `models/build.rs` — the wire-table GENERATOR, which a `<crate>/src/`-rooted derivation
///   silently drops. `903cefb3` already proved a stale generated table can ship while the
///   build reports success.
///
/// And one exclusion by derivation: `node` is not in the closure, because `casper` does not
/// depend on it.
#[test]
fn the_derived_path_set_recovers_what_the_hand_list_lost() {
    let crates = closure_crates();
    assert!(
        crates.contains("rho-pure-eval"),
        "★ the guard evaluator must be DERIVED into the path set — the hand list omitted it, \
         and two entries live there ({:?})",
        crates
    );
    assert!(crates.contains("casper"));
    assert!(crates.contains("models"));
    assert!(crates.contains("rholang"));
    assert!(crates.contains("rspace++"));
    assert!(crates.contains("shared"));
    assert!(
        !crates.contains("node"),
        "`node` is excluded BY DERIVATION — `casper` does not depend on it — not by a rule"
    );
    let spec = obligation_pathspec();
    assert!(
        spec.contains(&"models/".to_string()),
        "the pathspec must be crate-rooted so `models/build.rs` is inside it; got {spec:?}"
    );
    assert!(
        spec.contains(&"Cargo.lock".to_string()),
        "★ §7.5 extension 2: CBR-L06 proves a dependency bump alone can move published bytes"
    );
    // ★ And the pathspec really does reach the generator, which is the point of crate-rooting.
    let generator = obligation_set("7293d57c..HEAD", &["models/build.rs".to_string()]);
    assert!(
        !generator.is_empty(),
        "the wire-table generator has commits in range; a `src/`-rooted derivation would see none"
    );
    for sha in &generator {
        assert!(
            obligation_set("7293d57c..HEAD", &spec).contains(sha),
            "the derived pathspec must include the generator commit {sha}"
        );
    }
}

/// ★★ **The gate states the classes it cannot cover — and that statement is itself asserted.**
///
/// A gate whose advertised coverage exceeds its real coverage is worse than no gate, so the
/// two limits and the one undecidable class are not left to a reader's goodwill: the register
/// must say them, and this test fails if it stops.
#[test]
fn the_gate_states_the_classes_it_cannot_cover() {
    let register = read_register();
    for (what, needle) in [
        (
            "the task-tracker limit",
            "in this repository reads the task tracker",
        ),
        (
            "the cross-repository limit",
            "A test in f1r3node cannot read",
        ),
        (
            "class 5 — a justification wrong when written",
            "wrong when written",
        ),
        (
            "the genesis-instability constraint",
            "no artefact of the genesis build",
        ),
        (
            "the gate's built status",
            "casper/tests/consensus_change_register_gate.rs",
        ),
    ] {
        assert!(
            register.contains(needle),
            "§7 must state {what}: the register no longer contains {needle:?}. ⚠ A gate that \
             stops disclosing a gap has started overstating its coverage."
        );
    }
}
