//! # A stuck term that carries its reason
//!
//! In the rho-calculus a send with no matching receive **rests**. That is not an
//! error, it is the calculus: `c!(1)` with nobody reading `c` is a perfectly
//! well-formed process whose normal form is itself. Resting is therefore
//! **correct semantics and this module does not change it** — changing an
//! arity-mismatched send from *rests* to *errors* would change which programs
//! are accepted, which is a consensus break.
//!
//! What resting lacks is not correctness but **voice**. Its virtues are real —
//! the term survives, the operands stay recoverable, a stuck subterm does not
//! destroy its context — and its single vice is silence. An arity-mismatched
//! send is indistinguishable, from outside, from a send that is legitimately
//! waiting for a receive that has not arrived yet. That silence concealed a
//! genesis contract invocation that was **two parameters out of date for the
//! entire life of the harness**.
//!
//! This module supplies the missing voice, and it supplies it as a **value**:
//! [`RestReason`] is a disposition, not an absence. The design is the
//! generalisation of `matcher::r#match::GuardDisposition`, which replaced an
//! `Option<bool>` — where `None` meant *"something went wrong, and the something
//! is gone"* — with `{Admits, Refutes, NotABoolean, Undecidable, Failed}`. The
//! finding that produced both is the same: **a disposition is a value, not an
//! absence.**
//!
//! ---
//!
//! ## §1 Upstream is a floor on SEMANTICS, not a ceiling on DIAGNOSTICS
//!
//! | binding | free |
//! |---|---|
//! | a program upstream **accepts** must be accepted | **how** a failure is reported |
//! | it must compute the **same value** | how **specific** the message is |
//! | — | what **provenance** an error carries |
//! | — | whether a disposition is **richer** than upstream's |
//!
//! Owner ruling, verbatim (2026-07-29): *"We can handle errors better than
//! upstream Rholang, do not necessarily restrict your options to what upstream
//! supports. We should support everything upstream supports correctly, but
//! should fix any bugs that upstream has and make it more debuggable (e.g.
//! better error handling, more specific and clearer error messages, etc.)"*
//!
//! ⚠ The diagnostic therefore may not reach the consensus byte path. §4 states
//! where it does reach and why that is off it.
//!
//! ## §2 The instrument, and what it can and cannot see
//!
//! The analysis is a **pure function of a hot-store snapshot** —
//! `HashMap<Vec<Par>, Row<BindPattern, ListParWithRandom, TaggedContinuation>>`,
//! which is what `RhoRuntime::get_hot_changes` returns.
//!
//! ★ That snapshot is produced by `HotStore::to_map`
//! (`rspace++/src/rspace/hot_store.rs`), which takes a **read** lock and clones.
//! It performs no history fill and no write. This matters and it is not
//! incidental: the sibling reader `HotStore::get_data` takes a **write** lock and
//! inserts a history fill into the hot state, which `changes()` then emits as a
//! store action. So the *obvious* per-channel API mutates the state that becomes
//! the checkpoint, and the whole-map API does not. This module uses only the
//! whole-map API, and that is the first half of its consensus-invisibility
//! argument.
//!
//! ⚠ **The instrument has one blind spot, and it is structural.** `to_map`
//! iterates the **data** keys and looks up continuations at the same key, so:
//!
//! * a continuation resting on a channel that carries **no data** is absent from
//!   the snapshot entirely;
//! * a continuation whose join is **multi-channel** (`for (x <- a & y <- b)`) is
//!   keyed under `[a, b]` and is therefore never found from the single-channel
//!   data key `[a]`.
//!
//! ⇒ This module diagnoses **resting DATA**. The two rest classes that concern
//! resting *continuations* — a `for` nobody sends to, and a partially satisfied
//! join — are **not observable through this instrument**, and are named in §3
//! rather than guessed at. Making them observable is a change to
//! `HotStore::to_map`, which this module does not own.
//!
//! ## §3 The rest classes, DERIVED rather than listed
//!
//! The taxonomy is the case analysis of one predicate over a snapshot row, so a
//! case cannot be forgotten by omission — [`diagnose_row`]'s `match` is total
//! over the same product:
//!
//! ```text
//!   (data present?) × (continuation present at this key?) × (some bind admits the arity?)
//!
//!   ┌───────────┬──────────────────┬────────────────────┬──────────────────────────────┐
//!   │ data      │ continuation     │ arity admitted     │ reason                       │
//!   ├───────────┼──────────────────┼────────────────────┼──────────────────────────────┤
//!   │ absent    │ (either)         │ —                  │ nothing to diagnose (§2)      │
//!   │ present   │ absent           │ —                  │ NoReader                     │
//!   │ present   │ present          │ no                 │ ArityMismatch  ← work item #169│
//!   │ present   │ present          │ yes                │ ShapeOrGuardRefused          │
//!   │ present   │ present          │ unanswerable       │ MalformedRow                 │
//!   └───────────┴──────────────────┴────────────────────┴──────────────────────────────┘
//! ```
//!
//! `MalformedRow` exists so that a row whose join arity disagrees with its
//! pattern-list length is **reported** rather than indexed into. ⚠ This module
//! must never panic: it runs against store contents an adversary can influence,
//! and a `panic!` in a consensus interpreter is remotely triggerable. There is
//! no `unwrap`, no `expect` on store data, and no slice index in it.
//!
//! ## §4 ★ Consensus invisibility, by construction
//!
//! 1. **No consensus-path call site.** The analysis is *pull-based*. Nothing in
//!    `Interpreter::inj_attempt`, `Reduce::eval`, `RhoRuntimeImpl::evaluate` or
//!    the `casper` block pipeline calls it. Its only in-tree callers are
//!    `storage::storage_printer::pretty_print_rest_diagnosis` and the tests.
//! 2. **The surface it joins is already off the path**, and its callers are a
//!    closed, checkable set: `rholang/src/rholang_cli.rs` (the developer CLI),
//!    `node/src/rust/api/repl_grpc_service.rs` (the REPL service), and tests.
//!    No `casper` caller exists.
//! 3. **It cannot write.** It takes `&`-references to a cloned snapshot and
//!    returns owned values. It does not hold the runtime.
//! 4. **It cannot change control flow.** Every entry point returns data or a
//!    `String`; none returns `Result`, so no caller can `?` on it, and none can
//!    make a deploy fail.
//! 5. **It does not touch `EvaluateResult`.** `errors` decides whether a deploy
//!    is recorded as failed and `cost` is metering; both are consensus-visible,
//!    and neither is written here. This is the clause that rules out the
//!    tempting wiring.
//! 6. **It levies no charge.** No `Cost`, no `charge`, no phlogiston.
//!
//! ⇒ On every consensus axis the answer is `N/A`: there is no execution of this
//! code on the consensus path to have an effect. The precedent for insisting on
//! the *structural* form of this argument rather than an intentional one is
//! `ca84d535`/`4290303c`, which moved an environment variable **out of** the
//! consensus byte path by splitting the renderer — the same shape of fix, driven
//! by the same distinction between "does not, today" and "cannot".

use std::collections::{BTreeMap, HashMap};
use std::fmt;

use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use rspace_plus_plus::rspace::internal::{Datum, Row, WaitingContinuation};

use super::pretty_printer::PrettyPrinter;

/// The snapshot this module analyses — exactly what `RhoRuntime::get_hot_changes`
/// returns.
pub type StoreSnapshot = HashMap<Vec<Par>, Row<BindPattern, ListParWithRandom, TaggedContinuation>>;

/// What a bind will accept, in payload count.
///
/// A bind without a remainder admits **one** arity; a bind with a remainder
/// (`for (a, b ... rest <- c)`) admits every arity from its explicit pattern
/// count upwards, because the remainder absorbs the surplus.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Admits {
    /// Exactly `n` payloads.
    Exactly(usize),
    /// `n` or more payloads — the bind carries a remainder.
    AtLeast(usize),
}

impl Admits {
    /// Whether a datum of arity `sent` can be delivered to this bind.
    ///
    /// ⚠ Arity admissibility is **necessary, not sufficient**: the pattern
    /// shapes and any `where` guard still decide. That is precisely why
    /// [`RestReason::ShapeOrGuardRefused`] exists as a separate answer rather
    /// than being folded into "matched".
    pub fn accepts(&self, sent: usize) -> bool {
        match self {
            Admits::Exactly(n) => *n == sent,
            Admits::AtLeast(n) => sent >= *n,
        }
    }
}

impl fmt::Display for Admits {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Admits::Exactly(n) => write!(f, "{n}"),
            Admits::AtLeast(n) => write!(f, "{n} or more"),
        }
    }
}

/// ★ Why a term rests. **A disposition is a value, not an absence.**
///
/// Every variant carries enough to act on without re-deriving anything: the
/// arity that was sent, and what the installed continuations would have taken.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RestReason {
    /// Payloads rest here and **no continuation is installed at this channel**.
    ///
    /// ⚠ Read this against §2's blind spot: a multi-channel join listening on
    /// this channel is invisible to a hot-store snapshot, so this reason means
    /// *"nothing is installed on this channel alone"* and not *"nothing anywhere
    /// could ever read it"*. The variant is named for what the instrument sees.
    NoReader {
        /// How many payloads the resting send carried.
        sent: usize,
        /// How many such data rest here.
        count: usize,
    },

    /// ★ **Work item #169.** Payloads and continuations are both resting here
    /// and **no installed bind will take this many payloads**. No COMM can ever
    /// fire, however long the program runs: the arity is a static property of
    /// both sides.
    ArityMismatch {
        sent: usize,
        count: usize,
        /// What the installed continuations would take, ascending, deduplicated.
        admitted: Vec<Admits>,
    },

    /// Payloads and continuations are both resting here, some bind **would**
    /// take this many payloads, and the COMM still has not fired. So the refusal
    /// is in the pattern **shapes** or in a `where` **guard** — not in the arity.
    ///
    /// This is a weaker statement than the two above and is reported anyway: it
    /// tells a developer to stop counting arguments and start reading patterns.
    ShapeOrGuardRefused {
        sent: usize,
        count: usize,
        admitted: Vec<Admits>,
    },

    /// The row's join arity and a continuation's pattern-list length disagree, so
    /// the question *"what does the bind at this channel admit?"* has no answer.
    ///
    /// ★ Reported rather than indexed into. This is the variant that makes the
    /// module total without a panic.
    MalformedRow {
        /// Channels in the row's key.
        join_arity: usize,
        /// `patterns.len()` of the continuation that disagrees.
        pattern_lists: usize,
    },
}

/// A reason, located at the channel it is about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RestSite {
    /// The row's key. One channel for an ordinary send; several for a join.
    pub channels: Vec<Par>,
    pub reason: RestReason,
}

impl RestSite {
    /// ★ The developer-facing message. Names the channel, the arities on both
    /// sides, and — for the case that cannot ever fire — says so in those words.
    ///
    /// The channel is rendered with the interpreter's own `PrettyPrinter`, so the
    /// message quotes Rholang rather than `Debug`.
    pub fn describe(&self) -> String {
        let mut printer = PrettyPrinter::new();
        let where_ = match self.channels.as_slice() {
            [one] => printer.build_string_from_message(one),
            many => {
                let rendered: Vec<String> = many
                    .iter()
                    .map(|channel| PrettyPrinter::new().build_string_from_message(channel))
                    .collect();
                format!("the join ({})", rendered.join(" & "))
            }
        };
        match &self.reason {
            RestReason::NoReader { sent, count } => format!(
                "{count} send(s) of {sent} payload(s) rest on `{where_}` and no continuation is \
                 installed on that channel. Nothing will read them unless a `for` or `contract` \
                 is installed there, or unless a multi-channel join is listening (a join is not \
                 visible in a hot-store snapshot)."
            ),
            RestReason::ArityMismatch {
                sent,
                count,
                admitted,
            } => format!(
                "★ ARITY MISMATCH on `{where_}`: {count} send(s) carry {sent} payload(s), and the \
                 continuation(s) installed there take {}. No COMM can EVER fire — arity is static \
                 on both sides, so waiting will not help. The send rests, which is the calculus; \
                 this is why.",
                render_admitted(admitted)
            ),
            RestReason::ShapeOrGuardRefused {
                sent,
                count,
                admitted,
            } => format!(
                "{count} send(s) of {sent} payload(s) rest on `{where_}` although a continuation \
                 installed there takes {}. The arities agree, so the refusal is in the pattern \
                 SHAPES or in a `where` guard — stop counting arguments and read the patterns.",
                render_admitted(admitted)
            ),
            RestReason::MalformedRow {
                join_arity,
                pattern_lists,
            } => format!(
                "the row for `{where_}` is malformed: its key names {join_arity} channel(s) but a \
                 waiting continuation carries {pattern_lists} pattern list(s). No arity question \
                 can be answered here; reported rather than assumed."
            ),
        }
    }
}

impl fmt::Display for RestSite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(&self.describe()) }
}

/// `Exactly(2)`, `AtLeast(3)` → `"2 payload(s), or 3 or more payload(s)"`.
fn render_admitted(admitted: &[Admits]) -> String {
    match admitted {
        [] => "no payloads at all".to_string(),
        [only] => format!("{only} payload(s)"),
        many => {
            let rendered: Vec<String> = many.iter().map(Admits::to_string).collect();
            format!("{} payload(s)", rendered.join(" or "))
        }
    }
}

/// What the bind at position `index` of a continuation's join will accept.
///
/// `None` when the continuation's pattern list is too short for the position —
/// the [`RestReason::MalformedRow`] case. Never indexes.
fn admits_at(
    continuation: &WaitingContinuation<BindPattern, TaggedContinuation>,
    index: usize,
) -> Option<Admits> {
    continuation
        .patterns
        .get(index)
        .map(|bind| match bind.remainder {
            None => Admits::Exactly(bind.patterns.len()),
            Some(_) => Admits::AtLeast(bind.patterns.len()),
        })
}

/// The distinct payload counts resting in a row, ascending, each with how many
/// data carry it.
///
/// A channel may hold several sends of different arities at once, and each is a
/// separate question, so the analysis groups by arity rather than reporting the
/// first.
fn resting_arities(data: &[Datum<ListParWithRandom>]) -> BTreeMap<usize, usize> {
    let mut counts = BTreeMap::new();
    for datum in data {
        *counts.entry(datum.a.pars.len()).or_insert(0usize) += 1;
    }
    counts
}

/// ★ One row's diagnosis. The `match` is **total** over §3's product, which is
/// what makes the taxonomy derived rather than maintained.
pub fn diagnose_row(
    channels: &[Par],
    row: &Row<BindPattern, ListParWithRandom, TaggedContinuation>,
) -> Vec<RestSite> {
    // (data absent) — nothing this instrument can diagnose. A resting
    // continuation with no data is not in the snapshot at all (§2).
    if row.data.is_empty() {
        return Vec::new();
    }

    let arities = resting_arities(&row.data);
    let mut sites = Vec::with_capacity(arities.len());
    let at = |reason| RestSite {
        channels: channels.to_vec(),
        reason,
    };

    // (data present, continuation absent at this key)
    if row.wks.is_empty() {
        for (&sent, &count) in &arities {
            sites.push(at(RestReason::NoReader { sent, count }));
        }
        return sites;
    }

    // (data present, continuation present) — the arity question. Which leg of the
    // join is this channel? The snapshot keys data by a one-channel vector, so the
    // position is 0; the general form is written anyway so a richer snapshot needs
    // no second reading of the law.
    let index = 0usize;
    let mut admitted: Vec<Admits> = Vec::with_capacity(row.wks.len());
    for continuation in &row.wks {
        match admits_at(continuation, index) {
            Some(admits) => admitted.push(admits),
            // The unanswerable case, reported and not indexed.
            None => {
                return vec![at(RestReason::MalformedRow {
                    join_arity: channels.len(),
                    pattern_lists: continuation.patterns.len(),
                })]
            }
        }
    }
    admitted.sort_unstable();
    admitted.dedup();

    for (&sent, &count) in &arities {
        let reason = match admitted.iter().any(|admits| admits.accepts(sent)) {
            false => RestReason::ArityMismatch {
                sent,
                count,
                admitted: admitted.clone(),
            },
            true => RestReason::ShapeOrGuardRefused {
                sent,
                count,
                admitted: admitted.clone(),
            },
        };
        sites.push(at(reason));
    }
    sites
}

/// ★ Every resting datum in a snapshot, with why it rests.
///
/// The order is **deterministic** — sorted on the rendered channel and then on
/// the payload count — because a `HashMap` iteration order is not, and a
/// diagnostic that reorders between runs is one a developer stops reading. It is
/// not a consensus requirement (§4); it is a usability one.
pub fn diagnose(snapshot: &StoreSnapshot) -> Vec<RestSite> {
    let mut sites: Vec<RestSite> = snapshot
        .iter()
        .flat_map(|(channels, row)| diagnose_row(channels, row))
        .collect();
    sites.sort_by_cached_key(|site| {
        let rendered: Vec<String> = site
            .channels
            .iter()
            .map(|channel| PrettyPrinter::new().build_string_from_message(channel))
            .collect();
        (rendered, sort_key(&site.reason))
    });
    sites
}

/// A total order on reasons within one channel: by payload count, then by
/// variant, so two reasons at one channel never swap between runs.
fn sort_key(reason: &RestReason) -> (usize, u8) {
    match reason {
        RestReason::NoReader { sent, .. } => (*sent, 0),
        RestReason::ArityMismatch { sent, .. } => (*sent, 1),
        RestReason::ShapeOrGuardRefused { sent, .. } => (*sent, 2),
        RestReason::MalformedRow { .. } => (0, 3),
    }
}

/// ★ The reasons, as a report — and emitted through `tracing` as a side effect
/// of asking, so a caller that only wants the log does not have to format one.
///
/// `tracing` is out-of-band by construction: it writes to a subscriber, never to
/// the store, never to `EvaluateResult`, and never to the event log (§4).
///
/// ⚠ Returns a `String` and not a `Result`, so no caller can `?` on it and no
/// caller can make a deploy fail with it.
pub fn report(sites: &[RestSite]) -> String {
    match sites {
        [] => "No resting data. Nothing is stuck on a channel this snapshot can see.".to_string(),
        some => {
            let mut out = String::with_capacity(96 * some.len());
            for site in some {
                let line = site.describe();
                match &site.reason {
                    // The one class that can never make progress is a warning;
                    // the rest are ordinary information about a resting term.
                    RestReason::ArityMismatch { .. } | RestReason::MalformedRow { .. } => {
                        tracing::warn!(target: "rholang::rest", "{line}")
                    }
                    _ => tracing::debug!(target: "rholang::rest", "{line}"),
                }
                out.push_str(&line);
                out.push('\n');
            }
            out
        }
    }
}
