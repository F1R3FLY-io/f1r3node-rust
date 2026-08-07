// See rholang/src/main/scala/coop/rchain/rholang/interpreter/PrettyPrinter.scala

use models::rhoapi::connective::ConnectiveInstance;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{
    Bundle, Connective, EAnd, EDiv, EEq, EGt, EGte, EList, ELt, ELte, EMatches, EMinus,
    EMinusMinus, EMod, EMult, ENeg, ENeq, ENot, EOr, EPathMap, EPercentPercent, EPlus, EPlusPlus,
    ETuple, EVar, EZipper, Expr, GUnforgeable, Match, MatchCase, New, Par, Receive, Var,
};
use models::rust::bundle_ops::BundleOps;
use models::rust::epathmap_trie_codec::EPathMapMode;
use models::rust::par_map_type_mapper::ParMapTypeMapper;
use models::rust::par_set_type_mapper::ParSetTypeMapper;
use models::rust::pathmap_integration::render_cursor_position;
use shared::rust::shared::printer::{Audience, Printer};
use shared::rust::shared::string_ops::wrap_with_braces;
use typed_arena::Arena;

use super::errors::InterpreterError;

/// The message text the `&dyn Any` dispatch produced for a value it did not
/// recognise. Preserved **verbatim** — see [`PpNode::Unprintable`].
///
/// ⚠ No `Par` reaches this any more. The one production construction site was
/// the `Match` target defect, fixed here; what remains is the explicit
/// `build_string_from_node(PpNode::Unprintable(UNPRINTABLE_ANY))` escape for a
/// caller that has something this printer has no rendering for, plus the
/// `cfg(test)` seeds that exercise the unwind.
pub const UNPRINTABLE_ANY: &str = "Attempt to print unknown prost::Message type: Any { .. }";

/// The **closed** set of nodes the printer can render.
///
/// ## ★ Why this replaces `&dyn std::any::Any`
///
/// `_build_string_from_message` used to take `&dyn Any` and dispatch through a
/// chain of `downcast_ref`s ending in an `else` that returned an error. `Any`
/// **cannot be made exhaustive**: a caller passing a type the chain does not
/// know gets no compile error, no panic, and no test failure — it gets an error
/// string spliced silently into the output. The printer's input alphabet was
/// therefore not finite, which is also why it could not be a machine.
///
/// This enum makes the alphabet finite and makes an unhandled node a **compile
/// error**. Adding a printable type to the family now fails to build here
/// rather than degrading at runtime.
///
/// ## ★ There was a live instance. It is now FIXED (see the byte delta below)
///
/// `_build_string_from_message`'s `Match` arm called
/// `self.build_string_from_message(&m.target)`, and `m.target` is an
/// `Option<Par>`, **not** a `Par` — `RhoTypes.proto` declares
/// `Par target = 1 [(scalapb.field).no_box = true]`, which prost generates as
/// `Option<Par>`. `Option<Par>` implements no `AsPpNode`, matched no
/// `downcast_ref` arm before that, and so **every** `match` term this printer
/// rendered showed its target as
///
/// ```text
/// match <unprintable: Bug found: Attempt to print unknown prost::Message type: Any { .. }> {
/// ```
///
/// Making the dispatch closed turned that call site into a type error, which
/// is exactly what it is for. The correction is now applied: the arm renders
/// `m.target` through the *same* entry point the author wrote —
/// `build_string_from_message`, i.e. catching, capping, indent 0 — with the
/// `Option` projected by the same `.expect` discipline every other required
/// `Option<Par>` field in this printer uses (`Send::chan`, `Receive::body`,
/// `Bundle::body`, `New::p`, `MatchCase::pattern`, `MatchCase::source`,
/// `EMethod::target`).
///
/// ### ★ The bytes that changed, and why that needed saying out loud
///
/// `build_channel_string` reaches
/// `SystemDeployPlatformFailure::UnexpectedResult` -> `Display` -> `error_msg`
/// -> `ProcessedSystemDeploy::Failed`, which is serialized into the block and
/// compared byte-for-byte in replay validation
/// (`casper/src/rust/rholang/replay_runtime.rs:745-758`), and it is reachable
/// from untrusted input through `rho:io:stdout`. So this is a
/// consensus-observable change, and the exact substitution is:
///
/// ```text
/// -  match <unprintable: Bug found: Attempt to print unknown prost::Message type: Any { .. }> {
/// +  match <the target, rendered as any other `Par` in a value position> {
/// ```
///
/// Nothing else in a `match` rendering moves: the `{`, the per-case indent,
/// the case bodies and the closing `}` are produced by the same code as
/// before. `tests::a_match_target_renders_as_its_target` pins the new bytes,
/// and `differential::a_match_nested_in_a_send` pins them mid-traversal.
///
/// ### ⚠ Consequence: no `Par` can make this printer return `Err` any more
///
/// [`PpNode::Unprintable`] was the printer's only error source, and the
/// `Match` arm was the only thing that constructed one. After this fix nothing
/// reachable from a `Par` does, so the three `<unprintable…>` fallbacks and
/// every `EndCatch` unwind are exercised **only** through
/// `build_string_from_node(PpNode::Unprintable(..))` — which is public and
/// still supported — and through the `cfg(test)` seeds in [`drive`]. The catch
/// frames remain the faithful model of the recursive form's `?` and are tested
/// as such; they are simply no longer reachable from a term.
pub enum PpNode<'a> {
    Var(&'a Var),
    Send(&'a models::rhoapi::Send),
    Receive(&'a Receive),
    Bundle(&'a Bundle),
    New(&'a New),
    Expr(&'a Expr),
    Match(&'a Match),
    Unforgeable(&'a GUnforgeable),
    Connective(&'a Connective),
    Par(&'a Par),
    /// ⚠ A call site that hands the printer something it has no rendering for.
    /// Carries the message the `&dyn Any` dispatch used to synthesise, so the
    /// emitted bytes are unchanged. See the type documentation.
    Unprintable(&'static str),
}

/// Lets `build_string_from_message` keep its by-reference call shape at every
/// existing call site while the dispatch underneath becomes closed.
///
/// A type that is not printable simply has no impl, so passing one is a
/// **compile error** rather than an `<unprintable: …>` string at runtime.
pub trait AsPpNode {
    fn as_pp_node(&self) -> PpNode<'_>;
}

macro_rules! as_pp_node {
    ($ty:ty, $variant:ident) => {
        impl AsPpNode for $ty {
            fn as_pp_node(&self) -> PpNode<'_> { PpNode::$variant(self) }
        }
    };
}

as_pp_node!(Var, Var);
as_pp_node!(models::rhoapi::Send, Send);
as_pp_node!(Receive, Receive);
as_pp_node!(Bundle, Bundle);
as_pp_node!(New, New);
as_pp_node!(Expr, Expr);
as_pp_node!(Match, Match);
as_pp_node!(GUnforgeable, Unforgeable);
as_pp_node!(Connective, Connective);
as_pp_node!(Par, Par);

/// The `where`-clause guard this receive renders, if it renders one.
///
/// # Why this predicate and not `condition.is_some()`
///
/// `Receive.condition` has **two** spellings of "no guard", and both of them
/// are decided that way at every site that decides anything:
///
/// | site | code | treats as "commit unconditionally" |
/// |---|---|---|
/// | the matcher | `matcher::r#match::Matcher::check_commit` | `None`, **and** `Some(g)` with `g == Par::default()` |
/// | the reducer | `Reduce::eval_receive` | the same pair — it collapses `Some(Par::default())` to `None` before the continuation is registered |
///
/// So an empty `Par` in `condition` is not a weaker guard than the author
/// wrote; it *is* what they wrote, and it admits exactly what no guard admits.
/// Rendering it as ` where Nil` would attach a clause to a receive whose
/// behaviour has none.
///
/// ★ Reusing the decision sites' own predicate also **confines the byte
/// movement**. These bytes are block-resident and replay-compared
/// (`build_channel_string` -> `SystemDeployPlatformFailure::UnexpectedResult`
/// -> `ProcessedSystemDeploy::Failed`, compared at
/// `casper/src/rust/rholang/replay_runtime.rs`), so *which* terms move matters.
/// Under this predicate the set that moves is exactly the set of receives whose
/// COMM is actually gated: no term that behaves as unguarded moves a byte.
///
/// # Why it is SHARED between the driver and the oracle
///
/// It is a true leaf — a pure predicate on the term, with no printer state and
/// no recursion — so it sits alongside `build_remainder_string`,
/// `build_string_from_var` and `render_cursor_position` in the set
/// `pretty_printer_oracle`'s documentation calls "the true LEAVES deliberately
/// SHARED with production". Duplicating it would test the copy rather than the
/// driving, and would let the two printers disagree about *whether* there is a
/// guard while agreeing about how to print one.
pub(crate) fn receive_guard(r: &Receive) -> Option<&Par> {
    match &r.condition {
        Some(condition) if condition != &Par::default() => Some(condition),
        _ => None,
    }
}

/// The set of shift indices ONE `New` introduces, held as its two endpoints
/// rather than materialised element by element.
///
/// # Why an interval and not a `Vec<i32>`
///
/// `New::bind_count` is a `sint32` an attacker controls in a hand-built `Par`,
/// and the printer is reachable from untrusted input through `rho:io:stdout` ->
/// [`PrettyPrinter::build_channel_string`], whose output is replay-compared
/// (`casper/src/rust/rholang/replay_runtime.rs`). The indices a `New`
/// introduces are, by construction, the **contiguous** run
/// `[bound_shift, bound_shift + bind_count)` at the pre-mutation `bound_shift`
/// — so materialising them costs `Θ(bind_count)` words to represent
/// information that two words already carry. At `bind_count = i32::MAX` that is
/// an ~8 GiB request; at any `bind_count` it is also `Θ(bind_count)` work per
/// membership test, and the printer performs one membership test **per bound
/// variable rendered**.
///
/// Holding the interval closes both at the root: `Θ(1)` memory per `New`,
/// `Θ(1)` per membership test, and — because [`NewBindRange::contains`] is
/// *exact* — not one byte of rendered output moves.
///
/// # The membership obligation
///
/// This type replaces a `Vec<i32>` that was consulted with `Vec::contains`, on
/// a byte-for-byte replay-compared path, so `contains` must answer **exactly**
/// what that vector answered. The vector held
///
/// ```text
///     S(start, count) = { start (+) i : i in [0, count) }
/// ```
///
/// where `(+)` is the `i32` addition the printer actually performed — which is
/// *wrapping* in the `release` profile the node ships (`overflow-checks` off).
/// [`NewBindRange::contains`] reproduces `S` including the wrap; see its own
/// documentation for the derivation and
/// `differential::a_bind_range_answers_membership_exactly_as_a_vector_did` for
/// the differential against a materialised `S`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NewBindRange {
    /// The pre-mutation `bound_shift` at the `New` — the first index in the run.
    pub start: i32,
    /// The `New`'s RAW `bind_count`. May be zero or negative; see
    /// [`NewBindRange::is_empty`].
    pub count: i32,
}

impl NewBindRange {
    /// `true` when this `New` introduces no index at all.
    ///
    /// ⚠ A negative `bind_count` is representable — the proto declares
    /// `sint32 bindCount` and prost generates `i32` — and `0..negative` is the
    /// EMPTY range, so a negative count binds nothing. That is the value the
    /// materialised form produced, and it is a decision rather than an accident:
    /// a `.max(0)` or an `as usize` on `count` would silently turn it into a
    /// 4 GiB allocation or a panic instead.
    pub fn is_empty(&self) -> bool { self.count <= 0 }

    /// Membership in `S(start, count)`, exactly as `Vec::contains` answered it
    /// over the materialised form.
    ///
    /// # Derivation
    ///
    /// * **`count <= 0`.** `0..count` is empty, so `S` is empty and no index is
    ///   a member. The `is_empty` guard is load-bearing: without it the
    ///   `count as u32` below would read `-1` as `4_294_967_295` and report
    ///   every index as a member.
    /// * **`count > 0`.** Write `⊕`/`⊖` for wrapping `i32` add/subtract. The
    ///   vector held `start ⊕ i` for each `i` in `[0, count)`, and those values
    ///   are pairwise distinct because `count <= i32::MAX < 2^32`, so
    ///
    ///   ```text
    ///       idx ∈ S  ⟺  ∃ i ∈ [0, count).  idx = start ⊕ i
    ///                ⟺  (idx ⊖ start) mod 2^32  ∈  [0, count)
    ///   ```
    ///
    ///   `idx.wrapping_sub(start) as u32` **is** `(idx ⊖ start) mod 2^32`, and
    ///   `count as u32` is exact for `0 < count <= i32::MAX`. So the comparison
    ///   below is the right-hand side, verbatim.
    ///
    /// ⚠ Deliberately **wrapping**, not saturating and not widened to `i64`.
    /// The node ships `release`, where the materialised form wrapped; an `i64`
    /// widening would be the mathematically contiguous interval and would
    /// therefore *differ* from the shipped bytes in exactly the regime
    /// (`start + count` past `i32::MAX`) where the two can be told apart.
    /// ★ The two mutations of this body — widening to `i64`, and dropping the
    /// [`Self::is_empty`] guard — are EXECUTED, not recorded:
    /// `representation_mutations::the_recorded_representation_table_is_executable`.
    pub fn contains(&self, idx: i32) -> bool {
        if mutating_bind_range(BindRangeMutation::ContainsWidenedToI64) {
            let (idx, start, count) = (idx as i64, self.start as i64, self.count as i64);
            return count > 0 && idx >= start && idx < start + count;
        }
        if mutating_bind_range(BindRangeMutation::ContainsDropsEmptyGuard) {
            return (idx.wrapping_sub(self.start) as u32) < (self.count as u32);
        }
        !self.is_empty() && (idx.wrapping_sub(self.start) as u32) < (self.count as u32)
    }
}

// ===========================================================================
// ★ The SECOND recorded mutation table, made EXECUTABLE
// ===========================================================================

/// One deliberate defect in how a `New`'s bound names are REPRESENTED,
/// addressable by name.
///
/// # Why this exists
///
/// [`drive`]'s module documentation carries two mutation tables. The first is
/// about the *traversal* and became executable at `a8ec319f`
/// ([`drive::DriveMutation`]). The second — five rows about *this*
/// representation: the recorded interval, its membership predicate, and the
/// display cap — was still a record. Every one of its rows had been produced by
/// editing this file by hand, running the suite, writing the outcome down and
/// reverting the edit, so "these tests stand between that defect and a
/// regression" was a claim about an afternoon rather than a property of the
/// code.
///
/// ⚠ Executing the *first* table falsified one of its rows: `New` mutating
/// `bound_shift` before `build_variables` was recorded as caught by 11 tests,
/// and `a4c23a58` had since made the two orderings observationally identical
/// without anyone noticing, because nothing re-ran the table. A reader
/// consulting it would have believed eleven tests stood where none did. That is
/// the whole argument for executing a table rather than recording one, and it
/// applies verbatim to the rows below.
///
/// # Why these are not [`drive::DriveMutation`]s
///
/// A `DriveMutation` perturbs the *worklist driver*, so the recursive twin
/// still computes the unperturbed answer and the differential separates the two.
/// The mutations here perturb PURE FUNCTIONS that the driver and the twin
/// **share** — `pretty_printer_oracle.rs` calls
/// [`PrettyPrinter::new_bind_range`] and [`PrettyPrinter::build_variables`]
/// directly — so both forms move together and *no output differential can see
/// them at all*. They are separated by property assertions instead, which is
/// exactly why their table has a different shape: it names the assertion that
/// rejects rather than the witness term that separates.
///
/// # Why it costs production nothing
///
/// [`mutating_bind_range`] is a `const fn` returning `false` outside
/// `cfg(test)`, so every site is `if false { … }` in a production build and
/// folds away before optimisation. There is no runtime switch, no environment
/// variable and no second code path in a shipped binary — the alternative
/// branches do not exist in one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(not(test), allow(dead_code))]
enum BindRangeMutation {
    /// `bd7cb45f`'s form: the RECORDED interval is re-clamped to
    /// `max_var_count`, so the display cap doubles as an allocation bound and a
    /// slot past the cap stops being marked new-bound.
    RecordedIntervalReClamped,
    /// The pre-`bd7cb45f` form: the interval is MATERIALISED again — the same
    /// `Θ(bind_count)` `Vec<i32>`, built at the same site with the same
    /// wrapping arithmetic, and RETAINED for the printer's lifetime as that
    /// form retained it.
    ///
    /// ⚠ This mutant is **byte-equivalent** (see
    /// [`retain_materialised_interval`]) and is bounded by
    /// [`MAX_MATERIALISED_BIND_COUNT`]; it is a pure-COST defect, which is why
    /// no byte-level check in the suite can see it and why the row that records
    /// it is a resource measurement rather than a test outcome.
    IntervalMaterialisedAgain,
    /// [`NewBindRange::contains`] widened to `i64` — the mathematically
    /// contiguous interval instead of the wrap the shipped `release` profile
    /// performed.
    ContainsWidenedToI64,
    /// [`NewBindRange::contains`] with its [`NewBindRange::is_empty`] guard
    /// dropped, so a negative `count` is read as `count as u32` and reports
    /// every index as a member.
    ContainsDropsEmptyGuard,
    /// [`PrettyPrinter::printed_bind_extent`] off by one, printing one name
    /// MORE than the display cap allows — the direction that loosens the bound.
    PrintedExtentOneTooMany,
    /// The same off-by-one in the other direction, printing one name FEWER.
    /// Enumerated separately because "off by one" is two mutants, and a cap
    /// that is checked only from above is checked on one side.
    PrintedExtentOneTooFew,
}

/// The largest `bind_count` at which [`BindRangeMutation::IntervalMaterialisedAgain`]
/// will reproduce the historical allocation.
///
/// ⚠ **This is a refusal, not a convenience.** The recorded row for that
/// mutation is a RESOURCE measurement — ">300 s wall, 7.31 GiB peak RSS,
/// thrashing against an 8 GiB cgroup with `oom_kill 0`" — i.e. the mutant does
/// not get killed, it *hangs*, and a suite that ran it would hang with it. The
/// fixtures reach `bind_count = i32::MAX`, where the materialised form is a
/// `(2^31 - 1) x 4 B` = ~8 GiB request, so the guard converts an unbounded
/// allocation into an immediate, explanatory panic. A fixture that grows past
/// this bound therefore fails loudly in milliseconds instead of taking the
/// machine down.
#[cfg(test)]
const MAX_MATERIALISED_BIND_COUNT: i32 = 100_000;

/// The marker every refusal panic carries, so a harness can tell "this mutation
/// was declined on this fixture" from "this assertion rejected".
#[cfg(test)]
const MATERIALISATION_REFUSED: &str = "REFUSED as a resource hazard";

#[cfg(test)]
thread_local! {
    /// The representation mutation this thread's printers should perform, if
    /// any. Thread-local so the harness cannot perturb a concurrently running
    /// test.
    static ACTIVE_BIND_RANGE_MUTATION: std::cell::Cell<Option<BindRangeMutation>> =
        const { std::cell::Cell::new(None) };

    /// What [`BindRangeMutation::IntervalMaterialisedAgain`] has materialised
    /// and RETAINED on this thread. Retention is the point: the form this
    /// mutant reproduces held its `Vec<i32>` in `news_shift_indices` for the
    /// printer's lifetime, so a mutant that allocated and dropped would model
    /// its throughput cost and not its FOOTPRINT, which is the cost that made
    /// it a denial of service.
    static MATERIALISED_INTERVALS: std::cell::RefCell<Vec<Vec<i32>>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Is `m` the representation mutation currently in force?
#[cfg(test)]
fn mutating_bind_range(m: BindRangeMutation) -> bool {
    ACTIVE_BIND_RANGE_MUTATION.with(|active| active.get() == Some(m))
}

/// Production: never. `const` so the mutant branches are eliminated rather than
/// merely untaken.
#[cfg(not(test))]
#[inline(always)]
const fn mutating_bind_range(_m: BindRangeMutation) -> bool { false }

/// ⚠ TEST ONLY. Run `f` with `m` in force, restoring the previous setting — and
/// releasing anything the mutant retained — even if `f` panics.
#[cfg(test)]
fn with_bind_range_mutation<T>(m: BindRangeMutation, f: impl FnOnce() -> T) -> T {
    struct Restore(Option<BindRangeMutation>);
    impl Drop for Restore {
        fn drop(&mut self) {
            ACTIVE_BIND_RANGE_MUTATION.with(|active| active.set(self.0));
            MATERIALISED_INTERVALS.with(|held| held.borrow_mut().clear());
        }
    }
    let _restore = ACTIVE_BIND_RANGE_MUTATION.with(|active| Restore(active.replace(Some(m))));
    MATERIALISED_INTERVALS.with(|held| held.borrow_mut().clear());
    f()
}

/// ⚠ TEST ONLY. The pre-`bd7cb45f` materialisation, performed at the production
/// site for its COST and bounded by [`MAX_MATERIALISED_BIND_COUNT`].
///
/// **This mutant is byte-equivalent, by construction.** It returns nothing, is
/// called for effect only, and the effect is a `Vec<i32>` no printer reads:
/// [`PrettyPrinter::new_bind_range`] still returns the same interval, so every
/// byte of every render is unchanged. A defect invisible to output is invisible
/// to an output differential — which is why the row recording it cites a
/// resource measurement, and why the property that stands in its place
/// (`differential::the_bytes_a_new_retains_do_not_depend_on_bind_count`) is a
/// FOOTPRINT assertion rather than a string comparison.
#[cfg(test)]
fn retain_materialised_interval(start: i32, bind_count: i32) {
    assert!(
        bind_count <= MAX_MATERIALISED_BIND_COUNT,
        "{MATERIALISATION_REFUSED}: a fixture asked the materialising mutant for {bind_count} \
         names, past the {MAX_MATERIALISED_BIND_COUNT} bound. The historical form allocated 4 \
         bytes per name and held them, so this request is {} bytes; at `i32::MAX` it is the ~8 \
         GiB that was measured to take >300 s and 7.31 GiB of RSS while thrashing an 8 GiB \
         cgroup WITHOUT being OOM-killed. Executing it would hang the suite rather than fail \
         it, so it is refused here instead.",
        (bind_count as i64) * 4
    );
    let materialised: Vec<i32> = (0..bind_count).map(|i| start.wrapping_add(i)).collect();
    MATERIALISED_INTERVALS.with(|held| held.borrow_mut().push(materialised));
}

/// Production: a no-op the `if false` above folds away.
#[cfg(not(test))]
#[inline(always)]
fn retain_materialised_interval(_start: i32, _bind_count: i32) {}

/// ⚠ TEST ONLY. Bytes a printer is holding to record which indices its `New`s
/// introduced — the interval vector itself, plus whatever the materialising
/// mutant retained.
///
/// This is the observable the interval representation exists to bound. It is
/// measured from REAL printer state (`news_shift_indices`'s own capacity), so
/// the unmutated reading is a measurement of the shipped implementation rather
/// than of the harness.
#[cfg(test)]
fn bytes_retained_for_recorded_bounds(printer: &PrettyPrinter) -> usize {
    printer.news_shift_indices.capacity() * std::mem::size_of::<NewBindRange>()
        + MATERIALISED_INTERVALS.with(|held| {
            held.borrow()
                .iter()
                .map(|v| v.capacity() * std::mem::size_of::<i32>())
                .sum::<usize>()
        })
}

#[derive(Clone)]
pub struct PrettyPrinter {
    pub free_shift: i32,
    pub bound_shift: i32,
    /// One entry per `New` **entered**, not per name bound — see
    /// [`NewBindRange`]. `Θ(number of `New` nodes)`, independent of any
    /// `bind_count`.
    pub news_shift_indices: Vec<NewBindRange>,
    pub free_id: String,
    pub base_id: String,
    pub rotation: i32,
    pub max_var_count: i32,
    pub is_building_channel: bool,
    /// Whose budget governs **every** trim this printer performs.
    ///
    /// ⚠ It has to be a property of the printer, not an argument to a final
    /// trim, because [`Self::cap`] is called at INTERIOR nodes of a render as
    /// well as at the end: the drive machine's `PpKont::EndCatch` frame caps
    /// every successful sub-render inside a catching scope. Trimming only the
    /// finished string would leave an operator's budget deciding the bytes of
    /// every nested sub-render — and that is not hypothetical, it is what
    /// `casper/tests/system_deploy_error_message_determinism.rs` caught when
    /// this split was first written as a final trim: `@{"abc...}`, truncated
    /// *inside* the channel braces, still varying with the environment.
    pub audience: Audience,
}

impl PrettyPrinter {
    /// A printer for **operator** output — logs, stdout, the REPL,
    /// `rnode eval`. Trims to `PRETTY_PRINTER_OUTPUT_TRIM_AFTER`.
    pub fn new() -> Self { PrettyPrinter::create(0, 0) }

    /// A printer whose bytes will reach a block and be compared by replay.
    ///
    /// Trims to the compile-time [`Printer::CONSENSUS_TRIM_AFTER`] and never
    /// reads the process environment, at the outermost render or at any
    /// interior node. See [`Audience`] for why that distinction is a type.
    ///
    /// The only production caller is `casper`'s `show_seq_par`, the render
    /// behind `SystemDeployUserError::error_message`.
    pub fn for_consensus() -> Self {
        PrettyPrinter {
            audience: Audience::Consensus,
            ..PrettyPrinter::create(0, 0)
        }
    }

    fn create(free_shift: i32, bound_shift: i32) -> Self {
        PrettyPrinter {
            free_shift,
            bound_shift,
            news_shift_indices: Vec::new(),
            free_id: String::from("free"),
            base_id: String::from("a"),
            rotation: 23,
            max_var_count: 128,
            is_building_channel: false,
            audience: Audience::Operator,
        }
    }

    /// Trim a render — finished or intermediate — to [`Self::audience`]'s
    /// budget.
    ///
    /// The policy lives in [`Printer::cap`], next to the environment variable it
    /// reads: which budget applies, the panic when an *operator* budget exceeds
    /// the string (which `the_capping_call_sites_are_reproduced` depends on to
    /// locate this function's call sites), and the char-boundary flooring that
    /// removed a deploy-triggerable panic.
    pub fn cap(&self, str: &str) -> String { Printer::cap(self.audience, str) }

    pub(super) fn indent_string(&self) -> String { String::from("  ") }

    pub(super) fn bound_id(&self) -> String { self.rotate(self.base_id.clone()) }

    pub(super) fn set_base_id(&self) -> String { self.increment(self.base_id.clone()) }

    pub fn build_string_from_expr(&mut self, e: &Expr) -> String {
        // Instead of panicking on errors, return a fallback string
        // This matches Scala behavior where errors are handled gracefully
        match self._build_string_from_expr(e) {
            Ok(str) => self.cap(&str),
            Err(err) => {
                // Return a fallback message instead of panicking
                format!("<unprintable expr: {}>", err)
            }
        }
    }

    pub fn build_string_from_var(&self, v: &Var) -> String {
        self.cap(&self._build_string_from_var(v))
    }

    /// ⚠ Note the two things this does BESIDES printing, both of which the
    /// driver has to reproduce: it resets `indent` to **0**, and it **caps**
    /// the result. Its `_`-prefixed twin does neither. Which of the two a call
    /// site uses is part of the output, not a style choice.
    pub fn build_string_from_message<T: AsPpNode + ?Sized>(&mut self, m: &T) -> String {
        self.build_string_from_node(m.as_pp_node())
    }

    /// [`build_string_from_message`] for a node that has no `AsPpNode` impl —
    /// in practice only [`PpNode::Unprintable`].
    pub fn build_string_from_node(&mut self, node: PpNode<'_>) -> String {
        // Instead of panicking on unknown types, return a fallback string
        // This matches Scala behavior where errors are handled gracefully
        match self._build_string_from_message(node, 0) {
            Ok(str) => self.cap(&str),
            // ⚠ The fallback is NOT capped. That asymmetry is the pre-existing
            // behaviour and the driver's `Catch` frame reproduces it.
            Err(err) => format!("<unprintable: {}>", err),
        }
    }

    /// Render a channel, trimming every node to [`Self::audience`]'s budget.
    ///
    /// # ★ The consensus caller
    ///
    /// `SystemDeployPlatformFailure::UnexpectedResult` renders a `Par` through
    /// `casper`'s `show_seq_par` into `SystemDeployUserError::error_message`;
    /// that string is written into the block as
    /// `ProcessedSystemDeploy::Failed { error_msg }` and compared, byte for
    /// byte, by every validator that replays the block
    /// (`ReplayRuntimeOps::replay_system_deploy_internal`). While that render
    /// used a printer built by [`Self::new`], two validators with different
    /// settings of an operator's `PRETTY_PRINTER_OUTPUT_TRIM_AFTER` computed
    /// different bytes for the same failing system deploy, and replay reported
    /// `ReplayFailure::system_deploy_error_mismatch` for a deploy that had
    /// executed identically on both.
    ///
    /// That caller now builds its printer with [`Self::for_consensus`].
    /// Splitting the renderer — rather than, say, clamping the variable's range
    /// — is what removes the operator from the byte path instead of narrowing
    /// the window in which the operator can move it.
    ///
    /// The error arm is not trimmed by either audience: that fallback was never
    /// capped, so it was already environment-independent, and it is unchanged.
    pub fn build_channel_string(&mut self, m: &Par) -> String {
        // Instead of panicking on errors, return a fallback string
        // This matches Scala behavior where errors are handled gracefully
        match self._build_channel_string(m, 0) {
            Ok(str) => self.cap(&str),
            Err(err) => {
                // Return a fallback message instead of panicking
                format!("<unprintable channel: {}>", err)
            }
        }
    }

    pub(super) fn build_string_from_unforgeable(
        &self,
        u: &GUnforgeable,
    ) -> Result<String, InterpreterError> {
        match &u.unf_instance {
            Some(instance) => match instance {
                UnfInstance::GPrivateBody(p) => {
                    Ok(format!("Unforgeable(0x{})", hex::encode(p.id.clone())))
                }
                UnfInstance::GDeployIdBody(id) => {
                    Ok(format!("DeployId(0x{})", hex::encode(id.sig.clone())))
                }
                UnfInstance::GDeployerIdBody(id) => Ok(format!(
                    "DeployerId(0x{})",
                    hex::encode(id.public_key.clone())
                )),
                UnfInstance::GSysAuthTokenBody(value) => {
                    Ok(format!("GSysAuthTokenBody({:?})", value))
                }
            },
            // TODO: Figure out if we can prevent prost from generating - OLD
            None => Ok(String::from("Nil")),
        }
    }

    /// The machine form of `_build_string_from_expr`.
    ///
    /// ⚠ The recursive body this replaced is preserved verbatim as
    /// `_oracle_build_string_from_expr`, and `differential` asserts the two
    /// print identical bytes. See [`drive`] for the derivation.
    fn _build_string_from_expr(&mut self, e: &Expr) -> Result<String, InterpreterError> {
        drive::drive_expr(self, e)
    }

    /*
      I change this code, because we should properly work with option remainder, based on "list_should_print" test,
      without a detailed treatment of each case, we will have the following result:
      "[x0, x1, 7...Var { var_instance: Some(FreeVar(0)) }]" instead of "[x0, x1, 7...free0]"

      So,  format!("...{:?}", v) not enough for all cases.
    */
    pub(super) fn build_remainder_string(&self, remainder: &Option<Var>) -> String {
        // match remainder {
        //     Some(v) => {
        //         format!("...{:?}", v)
        //     }
        //     None => format!(""),
        // }

        match remainder {
            Some(v) => match &v.var_instance {
                Some(VarInstance::FreeVar(level)) => {
                    format!("...free{}", self.free_shift + level)
                }
                Some(VarInstance::BoundVar(level)) => {
                    format!("...bound{}", self.bound_shift + level)
                }
                Some(VarInstance::Wildcard(_)) => String::from("..._"),
                None => String::from("...Nil"),
            },
            None => String::new(),
        }
    }

    pub(super) fn _build_string_from_var(&self, v: &Var) -> String {
        match &v.var_instance {
            Some(instance) => match instance {
                VarInstance::FreeVar(level) => {
                    format!("{}{}", self.free_id, self.free_shift + level)
                }
                VarInstance::BoundVar(level) => {
                    let prefix = if PrettyPrinter::is_new_var(
                        level,
                        &self.news_shift_indices,
                        self.bound_shift,
                    ) && !self.is_building_channel
                    {
                        "*".to_string()
                    } else {
                        "".to_string()
                    };

                    format!(
                        "{}{}",
                        prefix,
                        self.bound_id() + &(self.bound_shift - level - 1).to_string()
                    )
                }
                VarInstance::Wildcard(_) => String::from("_"),
            },
            None => String::from("@Nil"),
        }
    }

    /// The machine form of `_build_channel_string`.
    ///
    /// ⚠ Sets `is_building_channel` and never resets it — see [`drive`]. The
    /// recursive body is preserved verbatim as `_oracle_build_channel_string`.
    fn _build_channel_string(
        &mut self,
        p: &Par,
        indent: usize,
    ) -> Result<String, InterpreterError> {
        drive::drive_channel(self, p, indent)
    }

    /// The machine form of `_build_string_from_message`.
    ///
    /// ⚠ The recursive body is preserved verbatim as
    /// `_oracle_build_string_from_message`. See [`drive`] for why the
    /// interleaving of mutation and descent — not merely the post-order — is
    /// what had to be reproduced.
    fn _build_string_from_message(
        &mut self,
        node: PpNode<'_>,
        indent: usize,
    ) -> Result<String, InterpreterError> {
        drive::drive_message(self, node, indent)
    }

    pub(super) fn increment(&self, id: String) -> String {
        fn inc_char(char_id: char) -> char { ((char_id as u8 + 1 - b'a') % 26 + b'a') as char }

        let mut chars: Vec<char> = id.chars().collect();
        for index in (0..chars.len()).rev() {
            chars[index] = inc_char(chars[index]);
            if chars[index] != 'a' {
                return chars.into_iter().collect();
            }
        }
        assert!(!chars.is_empty());
        chars.insert(0, 'a');
        chars.into_iter().collect()
    }

    pub(super) fn rotate(&self, id: String) -> String {
        id.chars()
            .map(|char| ((char as u8 + self.rotation as u8 - b'a') % 26 + b'a') as char)
            .collect()
    }

    /// ★ THE SINGLE SPELLING of "which shift indices does this `New`
    /// introduce": the half-open interval `[bound_shift, bound_shift + count)`
    /// taken at the **pre-mutation** `bound_shift`.
    ///
    /// Everything downstream reads this one value. The descend handler records
    /// it in `news_shift_indices` (so [`PrettyPrinter::is_new_var`] consults
    /// it), and [`PrettyPrinter::build_variables`] renders a prefix of it (so
    /// the printed names come from the same interval, at the same `start`, in
    /// the same order). There is no second derivation of the interval that
    /// could drift from the first.
    ///
    /// ⚠ `count` is the RAW `bind_count`, unclamped and possibly negative. The
    /// clamp belongs to *rendering* — see
    /// [`PrettyPrinter::printed_bind_extent`] — because rendering is what
    /// allocates per name; recording the interval does not.
    ///
    /// ★ Both defects this body can carry — re-clamping the recorded interval
    /// and materialising it again — are EXECUTED at
    /// `representation_mutations::the_recorded_representation_table_is_executable`
    /// rather than recorded.
    pub(super) fn new_bind_range(&self, bind_count: i32) -> NewBindRange {
        if mutating_bind_range(BindRangeMutation::IntervalMaterialisedAgain) {
            retain_materialised_interval(self.bound_shift, bind_count);
        }
        if mutating_bind_range(BindRangeMutation::RecordedIntervalReClamped) {
            return NewBindRange {
                start: self.bound_shift,
                count: std::cmp::min(self.max_var_count, bind_count),
            };
        }
        NewBindRange {
            start: self.bound_shift,
            count: bind_count,
        }
    }

    /// How many of a `New`'s names this printer will PRINT — the display cap,
    /// and now the only clamp in the `New` path.
    ///
    /// `bind_count` is an attacker-controllable `i32` on a path reachable from
    /// untrusted input (`rho:io:stdout` -> `build_channel_string`), and
    /// rendering a name costs a `String`, so the rendered count must be
    /// bounded. Its ONE caller is [`PrettyPrinter::build_variables`].
    ///
    /// ⚠ This is deliberately **not** the extent of
    /// [`PrettyPrinter::new_bind_range`]. `bd7cb45f` briefly made the two equal
    /// — clamping the recorded indices to `max_var_count` as well — which did
    /// bound the allocation, but at the cost of moving bytes on a
    /// replay-compared path: a variable in slot `>= start + 128` stopped being
    /// marked new-bound and lost its `*` prefix (7 distinct deltas were
    /// measured, including four in a `New`-inside-a-`New`). Holding the
    /// interval instead bounds the allocation *strictly harder* — `Θ(1)` rather
    /// than `Θ(max_var_count)` per `New` — while leaving the marking exact, so
    /// the display clamp no longer has to double as an allocation bound and can
    /// be what its name says. `differential::the_star_prefix_survives_past_the_\
    /// display_cap` pins the marking; `differential::a_new_costs_one_range_\
    /// however_many_names_it_binds` pins the bound.
    ///
    /// ⚠ The result may be **negative**, and that is deliberate: `0..negative`
    /// is the empty range, so a negative count prints nothing. Returning `0`
    /// instead would be the same value by a longer route; returning
    /// `max_var_count` would silently invent names.
    ///
    /// ★ The off-by-one is EXECUTED in **both** directions
    /// ([`BindRangeMutation::PrintedExtentOneTooMany`] and
    /// [`BindRangeMutation::PrintedExtentOneTooFew`]); the recorded row
    /// enumerated one, and a cap checked only from above is checked on one side.
    pub(super) fn printed_bind_extent(&self, bind_count: i32) -> i32 {
        // ⚠ `saturating_*`, so that a mutant near `i32::MIN` rejects for the
        // property it perturbs rather than for an overflow the production body
        // cannot reach. No fixture is within `1` of either extreme, so the
        // saturation never fires on the executed corpus.
        if mutating_bind_range(BindRangeMutation::PrintedExtentOneTooMany) {
            return std::cmp::min(self.max_var_count, bind_count).saturating_add(1);
        }
        if mutating_bind_range(BindRangeMutation::PrintedExtentOneTooFew) {
            return std::cmp::min(self.max_var_count, bind_count).saturating_sub(1);
        }
        std::cmp::min(self.max_var_count, bind_count)
    }

    /// The names a `New` declares between `new` and `in`: a display-capped
    /// PREFIX of `introduced`.
    ///
    /// ⚠ `introduced.start`, not `self.bound_shift`. At every call site the two
    /// are the same value — both handlers compute the interval before the
    /// `bound_shift` mutation — but taking it from the interval is what makes
    /// "the names printed" and "the indices marked" *the same authority* rather
    /// than two reads that happen to agree today. The `i32` addition is
    /// unchanged, so a `bound_shift` near `i32::MAX` still overflows here
    /// exactly when it used to.
    pub(super) fn build_variables(&self, introduced: NewBindRange) -> String {
        (0..self.printed_bind_extent(introduced.count))
            .map(|i| format!("{}{}", self.bound_id(), introduced.start + i))
            .collect::<Vec<String>>()
            .join(", ")
    }

    fn build_vec(&mut self, s: &Vec<Par>) -> String {
        s.iter().enumerate().fold(String::new(), |string, (i, p)| {
            let mut result = string;

            result.push_str(&self.build_string_from_message(p));

            if i != s.len() - 1 {
                result.push_str(", ");
            }

            result
        })
    }

    // ⚠ `build_pattern` and `build_match_case` used to sit here. They are not
    // deleted and they are not commented out: they were the two folds whose
    // *interleaving* with the printer's mutable state is the whole difficulty
    // of this conversion, so they live on — verbatim, still executed — as
    // `oracle_build_pattern` and `oracle_build_match_case` further down, where
    // `differential` runs them against their machine forms
    // (`PpWork::RecvBindStep` + `PpKont::RecvBindJoin`, and `PpWork::CaseStep` +
    // `PpKont::CaseJoin`). Production reaches those folds only through
    // [`drive`]; keeping a second production copy would be a dual path.
    //
    // `build_vec` above is NOT in that position: the three re-entrant `Expr`
    // arms (`ESet`, `EMap`, `EZipper`) still call it, so it stays.

    pub(super) fn is_empty_par(&self, p: &Par) -> bool {
        p.sends.is_empty()
            && p.receives.is_empty()
            && p.news.is_empty()
            && p.exprs.is_empty()
            && p.matches.is_empty()
            && p.unforgeables.is_empty()
            && p.bundles.is_empty()
            && p.connectives.is_empty()
    }

    /// Does the variable at de Bruijn `level` name something a `New` bound?
    ///
    /// ⚠ Takes the intervals by **slice**. It used to take a `Vec<i32>` by
    /// value, which meant every call site cloned the whole vector — once per
    /// bound variable rendered, i.e. `Θ(K)` clones of a `Θ(K)` vector for a
    /// term with `K` `New`-bound names. Nothing here mutates, so a borrow is
    /// sufficient and the `Θ(K²)` term disappears.
    ///
    /// The shift-index computation `bound_shift - level - 1` is unchanged and
    /// is still performed unconditionally — including when there are no
    /// intervals at all — so a malformed `level` still overflows exactly where
    /// it used to.
    pub(super) fn is_new_var(
        level: &i32,
        news_shift_indices: &[NewBindRange],
        bound_shift: i32,
    ) -> bool {
        let shift_idx = bound_shift - level - 1;
        news_shift_indices
            .iter()
            .any(|introduced| introduced.contains(shift_idx))
    }
}

/// ⚠ `pub(super)` for the oracle twin, which lives in
/// [`super::pretty_printer_oracle`] and shares this leaf rather than copying it.
pub(super) fn twos_complement_to_decimal(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return "0".to_string();
    }
    num_bigint::BigInt::from_signed_bytes_be(bytes).to_string()
}

// ===========================================================================
// Leg-2 Stage D: the pretty printer as an explicit pushdown machine
// ===========================================================================

mod drive {
    //! # The pretty printer as an explicit pushdown machine
    //!
    //! `PrettyPrinter::_build_string_from_message`, `_build_string_from_expr`
    //! and `_build_channel_string` were mutually recursive over the whole `Par`
    //! family. Measured by direct bisection of `stack_depth_gate`'s `pretty`
    //! subject immediately before this conversion: **41,984 bytes of native
    //! stack per nesting level in debug** (724,992 B at depth 16, 2,740,224 B
    //! at depth 64) and **4,266 B/level in release** (81,920 and 286,720). On
    //! the 2 MiB stack a tokio worker gets when `RUST_MIN_STACK` is unset that
    //! is a maximum nesting depth of ~49 (debug) / ~490 (release), after which
    //! the guard page is hit — and a stack overflow is a `SIGSEGV`, not a
    //! catchable error, so a printable term deep enough to overflow takes the
    //! node process with it.
    //!
    //! That matters here more than it would in a debugging utility, because
    //! this printer's output is **block-resident and replay-compared**:
    //! `build_channel_string` is reached from
    //! `SystemDeployPlatformFailure::UnexpectedResult` -> `Display` ->
    //! `error_msg` -> `ProcessedSystemDeploy::Failed`, which is serialized into
    //! the block and compared byte-for-byte in replay validation
    //! (`casper/src/rust/rholang/replay_runtime.rs:745-758`). It is also
    //! reachable from untrusted input through `rho:io:stdout`. So the printer
    //! is simultaneously a **liveness** surface (depth controls whether the
    //! process survives) and a **correctness** surface (its bytes must not
    //! move).
    //!
    //! This module replaces the call stack with an explicit heap worklist.
    //! Native stack becomes `O(1)` in nesting depth and in sibling width; the
    //! recursion lives in `work`. The pattern is the house standard — the
    //! evaluator SCC (`reduce.rs`, `EvVal`/`EvWork`/`EvKont`, `a929a2d6`) and
    //! the substitution SCC (`substitute_drive.rs`) already use it, and the
    //! recursive body survives as a `cfg(test)` oracle twin (`super::oracle`)
    //! that a differential compares against (`super::differential`).
    //!
    //! Full analysis, enumeration and measured constants:
    //! `docs/design/audits/theta-depth-traversals-2026-07-26.md`.
    //!
    //! ---
    //!
    //! # ★ Why this is the hardest member of the Θ(depth) family
    //!
    //! The other machines in this family fold a tree **post-order**: every
    //! child's value depends only on that child. This one **threads mutable
    //! printer state** — `free_shift`, `bound_shift`, `free_id`, `base_id`,
    //! `news_shift_indices`, `is_building_channel` — in *visit order*, and
    //! **never restores it**. There is no scoping, no save/restore, no
    //! environment: a sub-render leaves its mutations behind for whatever is
    //! rendered next.
    //!
    //! So the driver must reproduce the **interleaving** of mutation and
    //! descent, not merely the post-order:
    //!
    //! * a mutation that **precedes** a recursive call goes in the descend
    //!   handler, before that node's children are pushed;
    //! * a mutation **between** two child descents needs an interposed
    //!   continuation ([`PpWork::Mutate`]), pushed so that it pops between
    //!   them.
    //!
    //! Getting this wrong does not crash. It produces plausible-but-wrong
    //! variable names — `x0` where the recursive form said `x1` — which is why
    //! the differential below is the deliverable and not a formality.
    //!
    //! ## The work alphabet
    //!
    //! | item | stands for |
    //! |---|---|
    //! | `Node(node, indent)` | `_build_string_from_message(node, indent)` |
    //! | `ExprBody(e)` | `_build_string_from_expr(e)` |
    //! | `Channel(p, indent)` | `_build_channel_string(p, indent)` |
    //! | `OptPar { .. }` | an `Option<Par>` child projected at POP time (see below) |
    //! | `BeginCatch(kind)` | open a catching scope |
    //! | `Mutate(m)` | one state mutation, sequenced between children |
    //! | `RecvBindStep { .. }` | one `Receive` bind: its four mutations, then its children |
    //! | `CaseStep { .. }` | one `MatchCase`: its four mutations, then its children |
    //! | `Combine(k)` | run the post-order continuation `k` |
    //!
    //! The value stack is a plain `Vec<String>`. Every work item pushes exactly
    //! one value; every continuation pops the values of its children (which,
    //! because children are pushed in **reverse**, sit on the stack **in
    //! order**) and pushes exactly one.
    //!
    //! ---
    //!
    //! # ★ The catch frame — a MID-TRAVERSAL continuation, not a top-level wrapper
    //!
    //! Three entry points catch errors and substitute a fallback string, and
    //! they are called from **inside** the traversal. Each also **resets indent
    //! to 0** and **caps** its result, while the `_`-prefixed twins do none of
    //! those three things. Which of the two forms a call site uses is part of
    //! the output, not a style choice:
    //!
    //! | catching (`indent = 0`, capped, error -> fallback) | non-catching (`_` twins) |
    //! |---|---|
    //! | `Send`'s data elements and its channel | `Bundle`'s body |
    //! | `Receive`'s body | `New`'s body |
    //! | `Connective`'s `ps` (`ConnAnd`/`ConnOr`/`ConnNot`) | `Receive`'s bind **source** |
    //! | every `Expr` child of a `Par` | the eight `Par` categories |
    //! | every `build_vec` / `build_pattern` element | `build_match_case`'s two children |
    //!
    //! **Push order for a catching sub-render is `EndCatch`, then the work,
    //! then `BeginCatch`** — so `BeginCatch` pops first and records
    //!
    //! ```text
    //!   work_mark = work.len() - 1      // the index of the guarded item
    //!   vals_mark = vals.len()
    //! ```
    //!
    //! On error the machine truncates **both** stacks to the mark. That drops
    //! the failed sub-render *and everything it spawned*, while leaving
    //! `EndCatch` intact because it sits one slot **below** the mark — which is
    //! exactly what `?` does in the recursive form. It then pushes the fallback
    //! string and sets `failed`.
    //!
    //! `EndCatch` caps **only** if `!failed`: the `<unprintable: …>` fallback is
    //! deliberately NOT capped, which is the pre-existing asymmetry in
    //! `build_string_from_node`.
    //!
    //! Three kinds, one per entry point:
    //!
    //! | kind | entry point | fallback |
    //! |---|---|---|
    //! | `Message` | `build_string_from_message` / `build_string_from_node` | `<unprintable: {err}>` |
    //! | `Expr` | `build_string_from_expr` | `<unprintable expr: {err}>` |
    //! | `Channel` | `build_channel_string` | `<unprintable channel: {err}>` |
    //!
    //! ⚠ [`push_catch`] guards **exactly one** work item, and that is what makes
    //! `work_mark = work.len() - 1` correct. The three pushes are contiguous, so
    //! when `BeginCatch` pops, `work.len() - 1` is necessarily the index of the
    //! guarded item. Every catching site in this module goes through
    //! [`push_catch`], so the invariant is structural rather than reviewed.
    //!
    //! On error with **no** enclosing frame the drive returns `Err` — the
    //! recursive form's "`?` all the way out of `_build_string_from_message`".
    //! The innermost live frame is always `catches.last()`: a nested catch is
    //! pushed onto `catches` by its own `BeginCatch`, which pops *after* the
    //! outer one, and its `EndCatch` is removed by the same truncation that
    //! removes it, so `catches` and the pending `EndCatch`es cannot desynchronise.
    //!
    //! ---
    //!
    //! # ★ The three mid-traversal mutation sites
    //!
    //! ## 1. `Receive`'s bind fold
    //!
    //! Each bind performs four mutations before rendering its patterns:
    //!
    //! ```text
    //!   free_shift = bound_shift + previous_free
    //!   bound_shift = 0
    //!   free_id = bound_id()          // rotate(base_id) — reads the OLD base_id
    //!   base_id = set_base_id()       // increment(base_id)
    //! ```
    //!
    //! `previous_free_i` is the static prefix sum of `free_count` over binds
    //! `0..i`, so **no resumable fold is needed**: the descend handler pushes
    //! [`PpKont::ReceiveK`], the catch-wrapped body, `Mutate(AddBoundShift(
    //! totally_free))`, then `RecvBindStep{n-1}…{0}` in **reverse**. LIFO makes
    //! each step's children complete before the next step pops.
    //!
    //! ⚠ **`bound_shift` at step `i` is NOT statically known.** Only bind 0 sees
    //! the incoming `bound_shift`; every later bind sees whatever its
    //! predecessors' patterns and sources mutated it to (a `New` inside a
    //! pattern bumps it; `bound_shift = 0` resets it per bind). That is
    //! precisely why the steps must be **sequenced** rather than precomputed —
    //! and the failure mode is silent renaming, not a crash. The differential's
    //! `two_binds_that_bind_different_counts` corpus is the shape that
    //! distinguishes the two.
    //!
    //! ⚠ Precomputing the prefix sums moves an *arithmetic-overflow panic* on a
    //! malformed term (`free_count` outside the range a normalizer can produce)
    //! from "after bind `i` rendered" to "at descend". Same panic, same payload
    //! (`attempt to add with overflow`), earlier position. Normalized terms
    //! carry `free_count >= 0` bounded by term size, so this is unreachable in
    //! production; it is recorded because the differential's raw
    //! `generate_par` corpus draws `free_count` from `any::<i32>()` and
    //! therefore does reach it.
    //!
    //! ## 2. `New`
    //!
    //! `build_variables` and the introduced interval are computed in the
    //! **descend** handler, *before* the mutation — Rust evaluates `format!`
    //! arguments left to right, so the recursive form genuinely does this
    //! first, reading the *old* `bound_shift`. Then the mutation runs, then the
    //! single child is pushed. [`PpKont::NewK`] carries the pre-computed
    //! `variables` string.
    //!
    //! ★ Both reads go through the ONE value
    //! [`PrettyPrinter::new_bind_range`] returns — the half-open interval
    //! `[bound_shift, bound_shift + bind_count)`. `news_shift_indices` records
    //! that interval and [`PrettyPrinter::build_variables`] renders a
    //! display-capped prefix of it, so there is no second derivation that could
    //! drift.
    //!
    //! The interval used to be materialised, `(0..n.bind_count).map(|i| i +
    //! bound_shift).collect()` — unbounded, on a path reachable from untrusted
    //! input, so a `New` could ask for an ~8 GiB `Vec` behind a 128-name
    //! display cap. `bd7cb45f` bounded it by clamping the recorded indices to
    //! `max_var_count` too; that closed the DoS but **moved bytes**, because
    //! `is_new_var` reads those indices back to decide the `*` prefix. Holding
    //! the interval closes the DoS at the root (`Θ(1)` per `New`, no clamp
    //! needed) and keeps the marking exact, so no byte moves. See
    //! `super::differential::a_new_costs_one_range_however_many_names_it_binds`
    //! for the bound and `super::differential::the_star_prefix_survives_past_\
    //! the_display_cap` for the marking.
    //!
    //! ## 3. `build_match_case`
    //!
    //! [`PpWork::CaseStep`] applies its four mutations, then pushes
    //! [`PpKont::CaseJoin`], the source, `Mutate(AddBoundShift(free_count))`,
    //! and the pattern — so the interposed mutation pops **between** the two
    //! child descents. `open_brace` / `close_brace` depend only on `indent`, so
    //! [`PpKont::CaseJoin`] recomputes them rather than carrying them.
    //!
    //! ---
    //!
    //! # ★ `.expect` position: [`PpWork::OptPar`]
    //!
    //! Several children are `Option<Par>` fields whose `.expect` fires *after*
    //! a sibling has already been rendered — `Send::chan` (after the data),
    //! `Receive::body` (after the binds *and* after `bound_shift +=
    //! totally_free`), `ReceiveBind::source` (after the patterns),
    //! `MatchCase::source` (after the pattern and after `bound_shift +=
    //! free_count`), `EMatches::pattern`, `EMethod::target`, and `p2` of every
    //! binary `Expr` arm. Projecting those at descend time would fire the
    //! `.expect` **earlier** than the recursive form does, which on a malformed
    //! term changes *which* panic message the process dies with.
    //!
    //! [`PpWork::OptPar`] therefore carries the `&Option<Par>` and its message
    //! and performs the projection at **pop** time, restoring the original
    //! order. It is itself the guarded item of its catch frame, so a truncation
    //! removes it together with anything it spawned.
    //!
    //! ---
    //!
    //! # ★ Three arms stay verbatim (a nested, bounded drive)
    //!
    //! Same disposition as the sorter's three re-entrant arms
    //! (`sort_nested_set` / `sort_nested_map` in `stack_depth_gate`'s
    //! tripwire), and for the same reason — the values they traverse are
    //! **owned intermediates** that cannot be borrowed onto a worklist keyed by
    //! `'a`:
    //!
    //! * `ESetBody` — `ParSetTypeMapper::eset_to_par_set(eset.clone())` builds
    //!   an owned, *sorted* `SortedParHashSet`; `sorted_pars` lives in it.
    //! * `EMapBody` — likewise `sorted_list`.
    //! * `EZipperBody` — its rendered path segments are `Par`s **decoded** from
    //!   bytes (`decode_trie_path`), rendered through `build_channel_string`.
    //!
    //! Each runs the pre-conversion body unchanged inside [`inline_expr`], and
    //! the nested `build_vec` / `build_string_from_message` /
    //! `build_channel_string` calls it makes each start a **fresh bounded
    //! drive**. Native stack is therefore `O(nesting of set/map/zipper)` rather
    //! than `O(term depth)` — the same residual the sorter carries, tracked by
    //! the same tripwire subjects, and not reachable at all by the `EList`
    //! nesting the `pretty` gate subject probes.
    //!
    //! Worklisted because they borrow: `EListBody`, `ETupleBody`,
    //! `EPathmapBody`, `EMethodBody`, and every unary / binary arm.
    //!
    //! ---
    //!
    //! # ⚠ `is_building_channel` is set and NEVER reset — reproduce it exactly
    //!
    //! `_build_channel_string` sets `self.is_building_channel = true` and
    //! nothing ever sets it back. That is **pre-existing and load-bearing**: it
    //! suppresses the `*` prefix on bound-`new` variables in
    //! `_build_string_from_var` for the remainder of the run, so the first
    //! channel render in a printer's life permanently changes how every later
    //! variable prints. [`PpWork::Channel`] sets it at pop time and never
    //! clears it. Do not scope it, do not "fix" it: it is output.
    //!
    //! ---
    //!
    //! # ★ Measured outcome
    //!
    //! Bisected minimum thread stack (`rholang/tests/stack_depth_gate.rs`,
    //! subjects `pretty` and `pretty_wide`; 4,096-byte resolution), before and
    //! after this conversion:
    //!
    //! | profile | subject | before | after |
    //! |---|---|---|---|
    //! | debug | `pretty` (depth) | 724,992 B @ 16, 2,740,224 B @ 64 -> **41,984 B/level** | **45,056 B FLAT** @ 4 / 16 / 64 / 4,096 |
    //! | release | `pretty` (depth) | 81,920 B @ 16, 286,720 B @ 64 -> **4,266 B/level** | **12,288 B FLAT** @ 4 / 16 / 64 / 4,096 |
    //! | debug | `pretty_wide` (width) | 94,208 B flat (already O(1)) | **49,152 B FLAT** @ 4 / 64 / 1,024 / 65,536 |
    //! | release | `pretty_wide` (width) | 12,288 B flat (already O(1)) | **12,288 B FLAT** @ 4 / 64 / 1,024 / 65,536 |
    //!
    //! The width axis was already `O(1)` — `build_vec` and the eight `Par`
    //! category walks are `for` loops, not tail recursion — so what moved there
    //! is only the intercept (the debug figure fell because the per-element
    //! render no longer opens a native frame). The depth axis is the deliverable:
    //! a 4,096-level term now renders on a stack that a 4-level term needed.
    //!
    //! Confirmed independently by the *other* committed harness —
    //! `scripts/stack_depth_probe.sh` over `rholang/tests/stack_depth_probe.rs`,
    //! which fits a least-squares line rather than comparing two bisections:
    //!
    //! ```text
    //!   debug    pretty       S(N) = 65536 + 0*N   (N = 16, 64, 256, 1024)
    //!   debug    pretty_wide  S(N) = 65536 + 0*N   (N = 64, 256, 1024, 4096)
    //!   release  pretty       S(N) =  4096 + 0*N   (N = 16, 64, 256, 1024)
    //!   release  pretty_wide  S(N) =  4096 + 0*N   (N = 64, 256, 1024, 4096)
    //! ```
    //!
    //! The two harnesses report different INTERCEPTS (they run their fixtures on
    //! differently-loaded probe threads) and the same SLOPE, which is the claim.
    //! The pre-conversion figures also agree with the audit document's
    //! independently obtained 41,840 / 4,242 B per level to within 0.4% / 0.6%.
    //!
    //! # ★ The differential has teeth — EXECUTED, not remembered
    //!
    //! Two tables follow. The FIRST is about this traversal and is separated by
    //! the differential; the SECOND is about the `New` bound's representation
    //! and is separated by property assertions. Both are executed; neither is a
    //! record any more.
    //!
    //! This table used to be a record: each row had been produced by editing
    //! this file by hand, running the suite, writing down the outcome, and
    //! reverting the edit. Nothing re-ran any of them, so "the differential has
    //! teeth" was a claim about an afternoon.
    //!
    //! Every row is now a call. [`DriveMutation`] names the defect,
    //! [`with_mutation`] performs it, and
    //! `super::drive_mutations::the_recorded_mutation_table_is_executable`
    //! asserts — for each one — that the UNMUTATED drive agrees with the
    //! recursive twin on a witness (so the witness is a term the differential
    //! passes today) and that the MUTATED drive disagrees (so the comparison
    //! separates the defect). Both directions, or a comparator that rejected
    //! everything would pass.
    //!
    //! | mutation | caught | decided by |
    //! |---|---|---|
    //! | `RecvBindStep`'s `previous_free` forced to 0 (un-sequences the bind fold) | ✔ | `two_binds_that_bind_different_counts` |
    //! | `CaseStep` drops its interposed `Mutate(AddBoundShift)` | ✔ | `match_case_with_a_binding_pattern` |
    //! | `EndCatch` caps the fallback too (drops the asymmetry) | ✔ at all 5 trims | `the_capping_call_sites_are_reproduced`'s sweep |
    //! | `Send` renders its channel BEFORE its data | ✔ | `send_whose_data_bind` |
    //! | `New` takes its interval AFTER the `bound_shift` mutation | ✔ | 3 witnesses |
    //! | `Channel` resets `is_building_channel` after the sub-render | ✔ | `new_name_read_after_a_channel_render` |
    //! | `Par` pushes its `exprs` un-reversed | ✔ | 2 witnesses |
    //! | `RecvBindJoin` takes patterns before source | ✔ | 4 witnesses |
    //! | `Receive`'s `where` guard renders BEFORE the interposed `AddBoundShift` | ✔ | `receive_with_a_where_guard_over_its_binder` |
    //! | `New` mutates `bound_shift` before `build_variables` | **EQUIVALENT since `a4c23a58`** | proven + executed at `the_equivalent_mutants_really_are_equivalent` |
    //! | `ParK`'s category-length array permuted | **EQUIVALENT** | proven at [`render_par_categories`], executed over all 8! permutations |
    //!
    //! ## ⚠ Executing the table falsified one of its rows
    //!
    //! `New` mutating `bound_shift` before `build_variables` was recorded as
    //! caught by **11 tests**, and when the row was written it was:
    //! `build_variables` then read `self.bound_shift + i`. `a4c23a58` changed it
    //! to `introduced.start + i` — deliberately, so that "the names printed" and
    //! "the indices marked" have one authority — and in doing so made the two
    //! orderings observationally identical. Nothing noticed, because nothing
    //! re-ran the table, and a reader consulting it would have believed eleven
    //! tests stood between that ordering and a regression when none did.
    //!
    //! The row is now classified EQUIVALENT with a proof, and the property it
    //! was meant to protect is policed by the mutation that still has teeth:
    //! [`DriveMutation::NewIntervalReadsPostMutationShift`].
    //!
    //! # ★ The SECOND table — the `New` bound's REPRESENTATION — also EXECUTED
    //!
    //! The representation of a `New`'s bound names ([`super::NewBindRange`]) was
    //! measured the same way and recorded the same way, one mutation at a time,
    //! by hand. It is now run by
    //! `super::representation_mutations::the_recorded_representation_table_is_executable`,
    //! which performs each mutation inside the production function
    //! ([`super::BindRangeMutation`]), runs every deterministic test that
    //! observes the representation, and asserts the WHOLE verdict vector — which
    //! test rejects, which stays green, and **which assertion** did the
    //! rejecting.
    //!
    //! ⚠ These are not [`DriveMutation`]s and their table cannot have the same
    //! shape. A `DriveMutation` perturbs the driver, so the recursive twin still
    //! computes the unperturbed answer and the *differential* separates them.
    //! These perturb pure functions the driver and the twin **share**
    //! (`pretty_printer_oracle.rs` calls [`PrettyPrinter::new_bind_range`] and
    //! [`PrettyPrinter::build_variables`] directly), so both forms move together
    //! and no output differential can see them at all. Property assertions are
    //! what separate them, so each row names the clause that rejects.
    //!
    //! | mutation | recorded | EXECUTED — which assertion rejected |
    //! |---|---|---|
    //! | the recorded interval re-clamped to `max_var_count` (`bd7cb45f`'s form) | ✔ 3 tests | ✔ **exactly those 3** — `a_new_costs_one_range_…` at *"one entry with a truncated `count` means the marking was clamped"* (recorded `[{start: 0, count: 128}]` for a `New` binding 1'000), `the_star_prefix_survives_past_the_display_cap` at *"the recorded interval is being truncated at `max_var_count`"*, `the_star_prefix_is_exact_across_nested_news` at *"slot 202 (level 0) lost its `*` prefix"* |
    //! | the interval materialised again, unclamped (the pre-`bd7cb45f` form) | ✔ `the_two_forms_agree_even_where_the_arithmetic_overflows`; 6.92 s / 4 MiB peak becomes >300 s / 7.31 GiB peak under an 8 GiB cgroup | ⚠ **the ✔ was not an assertion rejecting** — see below. Executed as a SLOPE: `the_bytes_a_new_retains_do_not_depend_on_bind_count` rejects at *"MOVED with `bind_count`"* (32 B flat becomes 32/36/548/4'548/404'548 B), and `a_new_costs_one_range_…`'s `i32::MAX` fixture is REFUSED by [`super::MAX_MATERIALISED_BIND_COUNT`] |
    //! | `contains` widened to `i64` (the mathematical interval, not the shipped wrap) | ✔ `a_bind_range_answers_membership_exactly_as_a_vector_did` | ✔ **exactly that one**, at *"membership diverged at start = 2147483645, count = 5, idx = -2147483648"* — the wrap regime, as the row intends |
    //! | the `is_empty` guard dropped from `contains` (negative `count` read as unsigned) | ✔ `a_new_with_a_negative_bind_count_binds_nothing` | ✔ that one at *"is being read as unsigned somewhere"*, **and one the row did not name**: `a_bind_range_answers_membership_…` at *"start = 0, count = -1, idx = -2147483648: the interval says true, the vector it replaced says false"* |
    //! | `printed_bind_extent` off by one (the display cap alone) | ✔ `a_new_costs_one_range_however_many_names_it_binds` | ✔ that one at *"PRINTED 129 of them — `printed_bind_extent` is not bounding the rendered `Vec<String>`"*, **and one the row did not name**: `the_star_prefix_survives_past_the_display_cap` at *"the display cap is not in force"* |
    //! | ★ the same off-by-one in the OTHER direction (one name too FEW) | not enumerated | ✔ the same two tests, at *"PRINTED 127 of them"* and *"the header declared 127 names"* — a cap checked only from above is checked on one side, so both directions are now rows |
    //!
    //! ## ⚠ The materialisation row's ✔ is not the kind of ✔ the others are
    //!
    //! Every other row's ✔ means *an assertion rejected*. This one cannot:
    //! materialising the interval changes what the printer HOLDS, not what it
    //! prints, so the mutant is **byte-equivalent** — and that is executed, not
    //! assumed. `the_materialised_form_costs_four_bytes_per_name` renders the
    //! corpus with and without the mutant and requires byte-identical output,
    //! *and* compares the driver against the recursive twin under the mutant —
    //! which is the very comparison
    //! `the_two_forms_agree_even_where_the_arithmetic_overflows` performs — and
    //! requires them equal, because the twin calls the mutated function too. The
    //! row's ✔ therefore recorded the named test *exhausting memory*, not its
    //! `prop_assert_eq` rejecting. Those are different events and only one of
    //! them is a guard.
    //!
    //! ⚠ The record disagrees with itself about which event it was: the table
    //! said ">300 s / 7.31 GiB peak … thrashing" (a HANG) while the property
    //! test's own documentation says "SIGKILLed by an 8 GiB cgroup" (a KILL).
    //! Neither is re-run — deliberately: a hang is a worse thing for a suite to
    //! contain than an un-executed row — so both are retained as prior
    //! measurements, and what is executed instead is the slope that implies
    //! either of them: `4 B` per name against the interval's `0 B` per name,
    //! measured at `bind_count` = 0, 1, 128, 1'000, 10'000 and 100'000, which
    //! extrapolates to `(2^31 - 1) x 4 B` = 8'589'934'588 B (7.999999999 GiB) at
    //! the attacker's maximum. The extrapolation is computed; it is never
    //! allocated.
    //!
    //! ## What the second table's execution changed
    //!
    //! No row was falsified the way the `bound_shift` row was — every one of the
    //! five still has teeth. Two were **under-claimed**: they named one test
    //! where two reject, and a reader picking a fixture to keep would have had
    //! no way to know the second existed. One (`printed_bind_extent`) covered
    //! one direction of an off-by-one; the other direction is now a row of its
    //! own. And one (`materialised`) turns out to be a class the first table has
    //! no member of — a defect no output comparison can see — which is why the
    //! property standing in its place is a FOOTPRINT measurement rather than a
    //! string.
    //!
    //! ---
    //!
    //! # ★ `PpNode::Unprintable` was a live defect. It is fixed, and that
    //!   moved where the catch machinery is exercised
    //!
    //! The `Match` arm used to render its target as
    //! `build_string_from_node(PpNode::Unprintable(UNPRINTABLE_ANY))`, which
    //! always errors and is always caught — so **every** `match` term used to
    //! exercise the catch machinery for free. It now renders `m.target`
    //! properly (see [`super::PpNode`] for the byte delta), and the consequence
    //! is structural: `PpNode::Unprintable` is this drive's ONLY `Err`
    //! source — `descend_expr`'s single `Err` branch renders hex rather than
    //! propagating, and `build_string_from_unforgeable` is total — so **no
    //! `Par` can make this drive return `Err` any more**.
    //!
    //! What that costs, and how it is paid:
    //!
    //! | property | used to be witnessed by | now witnessed by |
    //! |---|---|---|
    //! | the drive returns `Err` with no enclosing frame | a bare `Match` | `build_string_from_node(PpNode::Unprintable(..))`, still public |
    //! | a catch frame truncates its spawned work + values | [`drive_truncation_probe`] | unchanged |
    //! | a fallback is **spliced** mid-render, not propagated | `differential::a_match_nested_in_a_send` | [`drive_splice_probe`] |
    //! | `EndCatch` does NOT cap a fallback | the `match` probe in `the_capping_call_sites_are_reproduced` | [`drive_splice_probe`] under the same `TRIM` sweep |
    //!
    //! ⚠ The last two are the reason [`drive_splice_probe`] exists. Deleting a
    //! defect that a test was standing on removes the test's subject; the
    //! replacement has to be built in the same change or the check silently
    //! becomes one that cannot fail.

    use models::rhoapi::EMethod;

    use super::*;

    // -----------------------------------------------------------------------
    // catching scopes
    // -----------------------------------------------------------------------

    /// Which fallback text a catching scope substitutes, and hence which entry
    /// point it stands for.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum CatchKind {
        /// `build_string_from_message` / `build_string_from_node`
        Message,
        /// `build_string_from_expr`
        Expr,
        /// `build_channel_string`
        Channel,
    }

    impl CatchKind {
        fn fallback(self, err: &InterpreterError) -> String {
            match self {
                CatchKind::Message => format!("<unprintable: {}>", err),
                CatchKind::Expr => format!("<unprintable expr: {}>", err),
                CatchKind::Channel => format!("<unprintable channel: {}>", err),
            }
        }
    }

    /// A pending catching scope and its unwind mark.
    struct CatchFrame {
        kind: CatchKind,
        /// Index in `work` of the guarded item. Truncating here removes the
        /// sub-render and everything it spawned, and leaves the matching
        /// `EndCatch` — which sits one slot BELOW — in place.
        work_mark: usize,
        vals_mark: usize,
        failed: bool,
    }

    // -----------------------------------------------------------------------
    // the work alphabet
    // -----------------------------------------------------------------------

    /// A state mutation interposed between two child descents.
    enum PpMutation {
        /// `self.bound_shift += n`
        AddBoundShift(i32),
    }

    /// Which entry point a lazily-projected [`PpWork::OptPar`] child feeds.
    #[derive(Clone, Copy)]
    enum OptRender {
        /// `_build_string_from_message(PpNode::Par(p), indent)`
        Message,
        /// `_build_channel_string(p, indent)`
        Channel,
    }

    // -----------------------------------------------------------------------
    // ★ The recorded mutation table, made EXECUTABLE
    // -----------------------------------------------------------------------

    /// One deliberate defect in this drive, addressable by name.
    ///
    /// # Why this exists
    ///
    /// The module documentation above carries a table of nine mutations with a
    /// "caught / not caught" column. Every one of them was performed BY HAND,
    /// observed, and reverted. Nothing re-ran them, so the differential's teeth
    /// were an assertion about an afternoon rather than a property of the code:
    /// a refactor that quietly stopped the differential from separating one of
    /// these would leave the table saying it still does.
    ///
    /// A guard's obligation is not to re-run history. It is to show that its
    /// decision procedure rejects the class of behaviour it excludes, on data
    /// actually in that class. The class here is *this drive, defective*, and
    /// nothing but the drive can supply a member of it — so the drive supplies
    /// them, under [`ACTIVE_MUTATION`].
    ///
    /// # Why it costs production nothing
    ///
    /// [`mutating`] is a `const fn` returning `false` outside `cfg(test)`, so
    /// every site below is `if false { … }` in a production build and folds
    /// away before optimisation. There is no runtime switch, no environment
    /// variable, and no second code path in a shipped binary — the alternative
    /// branches do not exist in one. What exists in production is exactly what
    /// existed before, with an `if false` around the mutant half.
    ///
    /// `ParK`'s permuted category-length array is deliberately absent from this
    /// enum. It is an EQUIVALENT mutant, and equivalence is a theorem rather
    /// than a test outcome; see [`render_par_categories`].
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) enum DriveMutation {
        /// `RecvBindStep`'s `previous_free` forced to 0 — un-sequences the bind
        /// fold, so a later bind no longer sees the free variables an earlier
        /// one introduced.
        RecvBindPreviousFreeZero,
        /// `CaseStep` drops its interposed `Mutate(AddBoundShift)`, so the
        /// case's source renders at the pattern's `bound_shift` instead of the
        /// shifted one.
        CaseStepDropsInterposedShift,
        /// `EndCatch` caps the fallback as well as the success value, dropping
        /// the asymmetry `build_string_from_node` has always had.
        EndCatchCapsFallback,
        /// `Send` renders its channel BEFORE its data, so the data elements no
        /// longer mutate printer state first.
        SendChannelBeforeData,
        /// ⚠ **EQUIVALENT since `a4c23a58` — see [`the_equivalent_mutants_really_are_equivalent`].**
        /// `New` mutates `bound_shift` before `build_variables` reads it. The
        /// table above recorded this as caught by 11 tests, and it was, until
        /// `build_variables` stopped reading `bound_shift`.
        NewMutatesBeforeVariables,
        /// ★ The mutation that replaces it. The interval is taken AFTER the
        /// `bound_shift` mutation, so the names printed and the indices marked
        /// both shift by `bind_count`. This is the ordering the `New` handler's
        /// "both of these read the PRE-mutation `bound_shift`" comment is about,
        /// and it is the one that still has teeth.
        NewIntervalReadsPostMutationShift,
        /// `Channel` resets `is_building_channel` after its sub-render, instead
        /// of leaving it set.
        ChannelResetsFlag,
        /// `Par` pushes its `exprs` un-reversed, so they pop in reverse order.
        ParPushesExprsUnreversed,
        /// `RecvBindJoin` takes its patterns off the value stack before its
        /// source.
        RecvBindJoinPatternsFirst,
        /// ★ The `Receive`'s `where` guard is rendered BEFORE the interposed
        /// `Mutate(AddBoundShift(totally_free))` instead of after it, so it
        /// prints its binders at the un-shifted level while the body prints
        /// them at the shifted one.
        ///
        /// This is the sequencing decision the guard's rendering rests on, and
        /// it is not a free choice: `p_input_normalizer`'s `InputPhase::Guard`
        /// normalizes the guard against `k.body_env` — *"guard and body see the
        /// same de Bruijn levels"* — so a guard rendered in a different
        /// environment from the body names the same binder two different ways
        /// in one term. The defect is quiet: the output is still well-formed
        /// Rholang, just about different variables.
        ReceiveConditionBeforeBoundShift,
    }

    #[cfg(test)]
    thread_local! {
        /// The mutation this thread's drives should perform, if any. Thread-local
        /// so the harness cannot perturb a concurrently running test.
        static ACTIVE_MUTATION: std::cell::Cell<Option<DriveMutation>> =
            const { std::cell::Cell::new(None) };
    }

    /// Is `m` the mutation currently in force?
    #[cfg(test)]
    fn mutating(m: DriveMutation) -> bool { ACTIVE_MUTATION.with(|active| active.get() == Some(m)) }

    /// Production: never. `const` so the mutant branches are eliminated rather
    /// than merely untaken.
    #[cfg(not(test))]
    #[inline(always)]
    const fn mutating(_m: DriveMutation) -> bool { false }

    /// ⚠ TEST ONLY. Run `f` with `m` in force, restoring the previous setting
    /// even if `f` panics.
    #[cfg(test)]
    pub(super) fn with_mutation<T>(m: DriveMutation, f: impl FnOnce() -> T) -> T {
        struct Restore(Option<DriveMutation>);
        impl Drop for Restore {
            fn drop(&mut self) { ACTIVE_MUTATION.with(|active| active.set(self.0)); }
        }
        let _restore = ACTIVE_MUTATION.with(|active| Restore(active.replace(Some(m))));
        f()
    }

    /// Every mutation, so the harness cannot silently cover fewer than it
    /// claims. A new variant that is not added here is a compile error at the
    /// exhaustive `match` below.
    #[cfg(test)]
    pub(super) fn mutations_the_differential_must_separate() -> Vec<DriveMutation> {
        vec![
            DriveMutation::RecvBindPreviousFreeZero,
            DriveMutation::CaseStepDropsInterposedShift,
            DriveMutation::SendChannelBeforeData,
            DriveMutation::NewIntervalReadsPostMutationShift,
            DriveMutation::ChannelResetsFlag,
            DriveMutation::ParPushesExprsUnreversed,
            DriveMutation::RecvBindJoinPatternsFirst,
            DriveMutation::ReceiveConditionBeforeBoundShift,
        ]
    }

    /// Mutations that produce byte-identical output, each with a proof. Asserted
    /// EQUAL rather than merely unlisted, because "the suite cannot see this"
    /// and "this is not a defect" have opposite consequences and look the same
    /// from outside.
    #[cfg(test)]
    pub(super) fn mutations_that_are_equivalent() -> Vec<DriveMutation> {
        vec![DriveMutation::NewMutatesBeforeVariables]
    }

    /// The one mutation whose witness needs a capping environment, so it is
    /// decided inside `the_capping_call_sites_are_reproduced`'s child process
    /// rather than here.
    #[cfg(test)]
    pub(super) fn capping_mutation() -> DriveMutation { DriveMutation::EndCatchCapsFallback }

    /// Every mutation exactly once, across the three dispositions. A new variant
    /// that is not classified is a compile error at the exhaustive `match`.
    #[cfg(test)]
    pub(super) fn every_mutation() -> Vec<DriveMutation> {
        let mut all = mutations_the_differential_must_separate();
        all.extend(mutations_that_are_equivalent());
        all.push(capping_mutation());
        for m in &all {
            match m {
                DriveMutation::RecvBindPreviousFreeZero
                | DriveMutation::CaseStepDropsInterposedShift
                | DriveMutation::EndCatchCapsFallback
                | DriveMutation::SendChannelBeforeData
                | DriveMutation::NewMutatesBeforeVariables
                | DriveMutation::NewIntervalReadsPostMutationShift
                | DriveMutation::ChannelResetsFlag
                | DriveMutation::ParPushesExprsUnreversed
                | DriveMutation::RecvBindJoinPatternsFirst
                | DriveMutation::ReceiveConditionBeforeBoundShift => {}
            }
        }
        all
    }

    /// A pending unit of work. Every reference borrows the input term (`'a`).
    enum PpWork<'a> {
        /// A pre-rendered leaf used when a zipper cursor segment is not a
        /// decodable channel value.
        Literal(String),
        /// `_build_string_from_message(node, indent)`
        Node(PpNode<'a>, usize),
        /// `_build_string_from_expr(e)`
        ExprBody(&'a Expr),
        /// `_build_channel_string(p, indent)`
        Channel(&'a Par, usize),
        /// An `Option<Par>` child projected at POP time, so that its `.expect`
        /// fires in the same order the recursive form fired it in.
        OptPar {
            field: &'a Option<Par>,
            message: &'static str,
            render: OptRender,
            indent: usize,
        },
        /// Open a catching scope. Pushed so it pops BEFORE the work it guards.
        BeginCatch(CatchKind),
        /// A state mutation, sequenced between children.
        Mutate(PpMutation),
        /// One `Receive` bind: applies its four mutations, then renders its
        /// patterns and source.
        RecvBindStep {
            recv: &'a Receive,
            index: usize,
            previous_free: i32,
            indent: usize,
        },
        /// One `MatchCase`: applies its mutations around pattern and source.
        CaseStep {
            case: &'a MatchCase,
            indent: usize,
        },
        Combine(PpKont<'a>),

        /// ⚠ TEST ONLY. Opens a catching scope around
        /// [`PpWork::TestSpawnThenFail`]. See
        /// [`drive_truncation_probe`].
        #[cfg(test)]
        TestCatchingFailure(&'a Par),
        /// ⚠ TEST ONLY. Pushes a value AND spawns work, then fails — the shape
        /// no production term can produce (the printer's only error source,
        /// `PpNode::Unprintable`, is a leaf), and therefore the only way to
        /// exercise a NON-TRIVIAL unwind.
        #[cfg(test)]
        TestSpawnThenFail(&'a Par),
        /// ⚠ TEST ONLY. A failing catching region with a rendered sibling on
        /// **each side**, joined by [`PpKont::TestJoinThree`].
        ///
        /// This is the shape `a_match_nested_in_a_send` used to get for free
        /// from the `Match` target defect: a fallback spliced into the MIDDLE
        /// of a larger value. Fixing the defect removed every `Par` that can
        /// produce it, so the shape is constructed directly. See
        /// [`drive_splice_probe`].
        #[cfg(test)]
        TestFailureBetweenSiblings {
            before: &'a Par,
            after: &'a Par,
        },
        /// ⚠ TEST ONLY. [`PpWork::TestFailureBetweenSiblings`] wrapped in ONE
        /// MORE catching scope, so that **two frames are open simultaneously**
        /// when the failure happens.
        ///
        /// ★ Without this, `catches.last_mut()` and `catches.first_mut()` are
        /// indistinguishable: every other probe here opens its frames in
        /// sequence, so there is only ever one on the stack and "unwind to the
        /// innermost" is a claim about a one-element list. Measured: mutating
        /// [`run`]'s `last_mut()` to `first_mut()` passed the entire suite
        /// until this variant existed. See
        /// `differential::the_unwind_stops_at_the_innermost_open_frame`.
        #[cfg(test)]
        TestNestedCatchingFailure {
            before: &'a Par,
            after: &'a Par,
        },
    }

    /// Post-order continuation: the borrowed shell plus whatever was computed
    /// *before* the children (and so cannot be recomputed after them).
    enum PpKont<'a> {
        /// Close a catching scope: cap on success, leave the fallback alone.
        EndCatch,
        SendK {
            send: &'a models::rhoapi::Send,
        },
        RecvBindJoin {
            recv: &'a Receive,
            index: usize,
        },
        ReceiveK {
            recv: &'a Receive,
            indent: usize,
        },
        /// `BundleOps::show` reads only the bundle, but it is arg 1 of the
        /// recursive `format!` and therefore runs before the child.
        BundleK {
            show: String,
            indent: usize,
        },
        /// `variables` reads the PRE-mutation `bound_shift`.
        NewK {
            variables: String,
            indent: usize,
        },
        CaseJoin {
            indent: usize,
        },
        MatchK {
            m: &'a Match,
            indent: usize,
        },
        ConnectiveK {
            conn: &'a Connective,
        },
        ParK {
            par: &'a Par,
            indent: usize,
        },
        ChannelK {
            par: &'a Par,
        },
        ExprK {
            expr: &'a Expr,
        },

        /// ⚠ TEST ONLY. Pops exactly three children and joins them with
        /// `" | "`. Exists so [`PpWork::TestFailureBetweenSiblings`] can put a
        /// fallback BETWEEN two rendered values without borrowing a production
        /// shell that no `Par` can supply.
        #[cfg(test)]
        TestJoinThree,
    }

    /// Render a `Par`'s already-rendered children, grouped into eight
    /// categories, separated by `separator`.
    ///
    /// # ★ The `lengths` array is PARTITION-INVARIANT, and that is a theorem
    ///
    /// The module's mutation table records `ParK`'s category-length array being
    /// permuted and **not caught**, calling it "an EQUIVALENT mutant". That was
    /// a claim about the mutant's semantics, asserted rather than established —
    /// which is worse than an unchecked positive claim, because "the test suite
    /// cannot see this defect" and "this is not a defect" look identical from
    /// the outside and have opposite consequences.
    ///
    /// It is a theorem, and here is the proof.
    ///
    /// Let the non-zero entries of `lengths`, in array order, be
    /// `l₁ … l_k`, and let `n = Σ lᵢ`. The loop below:
    ///
    /// * skips every zero-length group entirely, leaving `prev_non_empty`
    ///   untouched, so zero entries contribute neither text nor separators;
    /// * for the `j`-th non-zero group emits `children[next .. next+lⱼ]` in
    ///   order, interleaved with `lⱼ − 1` separators (the `index != length − 1`
    ///   guard suppresses the trailing one), and advances `next` by `lⱼ`;
    /// * emits exactly one separator before every non-zero group after the
    ///   first (`prev_non_empty`).
    ///
    /// So the emitted text is `children[0 .. n]` in index order, with
    /// `Σⱼ (lⱼ − 1) + (k − 1) = n − k + k − 1 = n − 1` separators — one between
    /// each adjacent pair and nowhere else. That is `children[0..n].join(sep)`,
    /// and the expression depends on `lengths` **only through `n`**.
    ///
    /// A permutation preserves both the multiset of entries and their sum, so
    /// it preserves `n`, so it preserves the output. Byte-identically, for every
    /// input. ∎
    ///
    /// Two corollaries worth stating, because they bound the claim:
    ///
    /// 1. The equivalence is a property of *this separator discipline* — one
    ///    string used both between categories and within one. Give any category
    ///    its own separator and the permutation becomes observable immediately.
    ///    The grouped form is kept for exactly that reason (it mirrors the
    ///    recursive body line for line and stays correct under such a change),
    ///    and this proof is what a future author must re-derive if they make it.
    /// 2. Equivalence does NOT extend to arbitrary edits of `lengths`. Anything
    ///    that changes `n` — reading a category twice, dropping one — changes
    ///    `take(vals, …)` and is caught loudly. Only permutations are equivalent.
    ///
    /// `drive_mutations::the_category_partition_is_not_observable` executes the
    /// theorem over **all 8! = 40,320 permutations** of a corpus of length
    /// vectors, so "equivalent mutant" is now a checked statement rather than a
    /// recorded one.
    pub(super) fn render_par_categories(
        children: &[String],
        lengths: [usize; 8],
        separator: &str,
    ) -> String {
        let mut prev_non_empty = false;
        let mut result = String::new();
        let mut next = 0usize;
        for length in lengths {
            if length != 0 {
                if prev_non_empty {
                    result.push_str(separator);
                }
                for index in 0..length {
                    result.push_str(&children[next]);
                    next += 1;
                    if index != length - 1 {
                        result.push_str(separator);
                    }
                }
                // ⚠ The recursive form omits this assignment in the LAST
                // (connectives) block only. Unobservable: nothing reads the flag
                // afterwards.
                prev_non_empty = true;
            }
        }
        result
    }

    /// Push a catching sub-render: `EndCatch`, then the guarded work, then
    /// `BeginCatch`.
    ///
    /// ⚠ Exactly ONE work item is guarded. The three pushes are contiguous, so
    /// when `BeginCatch` pops, `work.len() - 1` is the index of `guarded` —
    /// which is what [`step`] records as `work_mark`. Every catching site goes
    /// through here, so that invariant is structural.
    fn push_catch<'a>(work: &mut Vec<PpWork<'a>>, kind: CatchKind, guarded: PpWork<'a>) {
        work.push(PpWork::Combine(PpKont::EndCatch));
        work.push(guarded);
        work.push(PpWork::BeginCatch(kind));
    }

    // -----------------------------------------------------------------------
    // entry points
    // -----------------------------------------------------------------------

    /// The machine form of `_build_string_from_message`.
    pub(super) fn drive_message(
        pp: &mut PrettyPrinter,
        node: PpNode<'_>,
        indent: usize,
    ) -> Result<String, InterpreterError> {
        run(pp, PpWork::Node(node, indent))
    }

    /// The machine form of `_build_string_from_expr`.
    pub(super) fn drive_expr(pp: &mut PrettyPrinter, e: &Expr) -> Result<String, InterpreterError> {
        run(pp, PpWork::ExprBody(e))
    }

    /// The machine form of `_build_channel_string`.
    pub(super) fn drive_channel(
        pp: &mut PrettyPrinter,
        p: &Par,
        indent: usize,
    ) -> Result<String, InterpreterError> {
        run(pp, PpWork::Channel(p, indent))
    }

    /// Initial capacity for both stacks. A `Par` of any interesting size
    /// immediately pushes eight categories' worth of children plus their catch
    /// triples; preallocating removes the first several growth reallocations
    /// from every render.
    const STACK_HINT: usize = 64;

    /// The single LIFO loop.
    fn run<'root>(pp: &mut PrettyPrinter, seed: PpWork<'root>) -> Result<String, InterpreterError> {
        let decoded_entries = Arena::new();
        run_with_arena(pp, seed, &decoded_entries)
    }

    fn run_with_arena<'a>(
        pp: &mut PrettyPrinter,
        seed: PpWork<'a>,
        decoded_entries: &'a Arena<Par>,
    ) -> Result<String, InterpreterError> {
        let mut work: Vec<PpWork<'a>> = Vec::with_capacity(STACK_HINT);
        let mut vals: Vec<String> = Vec::with_capacity(STACK_HINT);
        let mut catches: Vec<CatchFrame> = Vec::new();

        work.push(seed);
        while let Some(item) = work.pop() {
            if let Err(err) = step(
                pp,
                item,
                decoded_entries,
                &mut work,
                &mut vals,
                &mut catches,
            ) {
                // The recursive form's `?`: unwind to the innermost catching
                // entry point on the call path, or out of the drive entirely.
                match catches.last_mut() {
                    Some(frame) => {
                        let fallback = frame.kind.fallback(&err);
                        work.truncate(frame.work_mark);
                        vals.truncate(frame.vals_mark);
                        frame.failed = true;
                        vals.push(fallback);
                    }
                    None => return Err(err),
                }
            }
        }

        let result = vals
            .pop()
            .expect("pretty-printer drive: the value stack was empty at completion");
        assert!(
            vals.is_empty(),
            "pretty-printer drive: {} value(s) left over at completion — a continuation \
             popped the wrong number of children",
            vals.len()
        );
        assert!(
            catches.is_empty(),
            "pretty-printer drive: {} catching scope(s) never closed",
            catches.len()
        );
        Ok(result)
    }

    /// One transition. `Err` means "the recursive form would have returned
    /// `Err` here"; [`run`] performs the unwind.
    fn step<'a>(
        pp: &mut PrettyPrinter,
        item: PpWork<'a>,
        decoded_entries: &'a Arena<Par>,
        work: &mut Vec<PpWork<'a>>,
        vals: &mut Vec<String>,
        catches: &mut Vec<CatchFrame>,
    ) -> Result<(), InterpreterError> {
        match item {
            PpWork::Literal(value) => vals.push(value),

            PpWork::Node(node, indent) => descend_node(pp, node, indent, work, vals)?,

            PpWork::ExprBody(e) => descend_expr(pp, e, decoded_entries, work, vals)?,

            PpWork::Channel(p, indent) => {
                // ⚠ Set, never reset. See the module documentation.
                pp.is_building_channel = true;
                work.push(PpWork::Combine(PpKont::ChannelK { par: p }));
                work.push(PpWork::Node(PpNode::Par(p), indent));
            }

            PpWork::OptPar {
                field,
                message,
                render,
                indent,
            } => {
                let p = field.as_ref().expect(message);
                match render {
                    OptRender::Message => work.push(PpWork::Node(PpNode::Par(p), indent)),
                    OptRender::Channel => work.push(PpWork::Channel(p, indent)),
                }
            }

            PpWork::BeginCatch(kind) => catches.push(CatchFrame {
                kind,
                work_mark: work.len() - 1,
                vals_mark: vals.len(),
                failed: false,
            }),

            PpWork::Mutate(PpMutation::AddBoundShift(n)) => pp.bound_shift += n,

            PpWork::RecvBindStep {
                recv,
                index,
                previous_free,
                indent,
            } => {
                let bind = &recv.binds[index];
                // ⚠ `previous_free` is what SEQUENCES the bind fold: bind `i`
                // renders at the free level bind `i-1` left behind.
                pp.free_shift = match mutating(DriveMutation::RecvBindPreviousFreeZero) {
                    true => pp.bound_shift,
                    false => pp.bound_shift + previous_free,
                };
                pp.bound_shift = 0;
                pp.free_id = pp.bound_id();
                pp.base_id = pp.set_base_id();

                work.push(PpWork::Combine(PpKont::RecvBindJoin { recv, index }));
                // `_build_channel_string` — NOT the catching twin: an error in
                // a bind source propagates out of the whole `Receive`.
                work.push(PpWork::OptPar {
                    field: &bind.source,
                    message: "source field on bind was None, should be Some",
                    render: OptRender::Channel,
                    indent,
                });
                // `build_pattern`: each element through the CATCHING
                // `build_channel_string`, i.e. indent 0 and capped.
                for pattern in bind.patterns.iter().rev() {
                    push_catch(work, CatchKind::Channel, PpWork::Channel(pattern, 0));
                }
            }

            PpWork::CaseStep { case, indent } => {
                let pattern_free = case.free_count;
                pp.free_shift = pp.bound_shift;
                pp.bound_shift = 0;
                pp.free_id = pp.bound_id();
                pp.base_id = pp.set_base_id();

                work.push(PpWork::Combine(PpKont::CaseJoin { indent }));
                work.push(PpWork::OptPar {
                    field: &case.source,
                    message: "source field on MatchCase was None, should be Some",
                    render: OptRender::Message,
                    indent: indent + 1,
                });
                if !mutating(DriveMutation::CaseStepDropsInterposedShift) {
                    work.push(PpWork::Mutate(PpMutation::AddBoundShift(pattern_free)));
                }
                work.push(PpWork::Node(
                    PpNode::Par(
                        case.pattern
                            .as_ref()
                            .expect("pattern field on MatchCase was None, should be Some"),
                    ),
                    indent,
                ));
            }

            PpWork::Combine(kont) => combine(pp, kont, vals, catches)?,

            #[cfg(test)]
            PpWork::TestCatchingFailure(p) => {
                push_catch(work, CatchKind::Message, PpWork::TestSpawnThenFail(p))
            }

            #[cfg(test)]
            PpWork::TestSpawnThenFail(p) => {
                vals.push(String::from("<must be truncated>"));
                // Pops SECOND — so the unwind has to drop it.
                work.push(PpWork::Node(PpNode::Par(p), 0));
                // Pops FIRST, and fails.
                work.push(PpWork::Node(PpNode::Unprintable(UNPRINTABLE_ANY), 0));
            }

            #[cfg(test)]
            PpWork::TestFailureBetweenSiblings { before, after } => {
                work.push(PpWork::Combine(PpKont::TestJoinThree));
                // Pops THIRD. Guarded, so `EndCatch` caps it on success —
                // which is what makes this probe decide the SUCCESS side of
                // the capping asymmetry as well as the fallback side.
                push_catch(
                    work,
                    CatchKind::Message,
                    PpWork::Node(PpNode::Par(after), 0),
                );
                // Pops SECOND, and fails: its fallback must land in the MIDDLE
                // of the three values, not replace them.
                push_catch(work, CatchKind::Message, PpWork::TestSpawnThenFail(after));
                // Pops FIRST.
                push_catch(
                    work,
                    CatchKind::Message,
                    PpWork::Node(PpNode::Par(before), 0),
                );
            }

            #[cfg(test)]
            PpWork::TestNestedCatchingFailure { before, after } => push_catch(
                work,
                CatchKind::Message,
                PpWork::TestFailureBetweenSiblings { before, after },
            ),
        }
        Ok(())
    }

    /// ⚠ TEST ONLY. Drive a catching region that pushes a value and spawns work
    /// **before** it fails, so that the unwind has something to truncate.
    ///
    /// No production term can reach this shape — `PpNode::Unprintable` is the
    /// printer's only error source and it is a leaf, so every catch in a real
    /// render truncates zero work items and zero values. The frame's semantics
    /// are still what make the conversion faithful to `?`, so they are tested
    /// rather than argued. See
    /// `differential::a_failing_region_takes_its_spawned_work_with_it`.
    #[cfg(test)]
    pub(super) fn drive_truncation_probe(
        pp: &mut PrettyPrinter,
        sibling: &Par,
    ) -> Result<String, InterpreterError> {
        run(pp, PpWork::TestCatchingFailure(sibling))
    }

    /// ⚠ TEST ONLY. Drive `before`, then a **failing** catching region, then
    /// `after`, joined into one value.
    ///
    /// ## Why this exists
    ///
    /// Two properties were witnessed, before the `Match` target defect was
    /// fixed, by an ordinary term (`differential::a_match_nested_in_a_send`):
    ///
    /// 1. a catch frame's fallback is **spliced in place** — the surrounding
    ///    render survives and the unwind does not run to the top;
    /// 2. `EndCatch` caps a **successful** sub-render and does **not** cap a
    ///    fallback (`differential::the_capping_call_sites_are_reproduced`).
    ///
    /// No `Par` can produce a failing catch any more, so both would have become
    /// checks that cannot fail. This seed reconstructs the shape directly, and
    /// the two tests above now stand on it.
    ///
    /// ⚠ There is deliberately no oracle twin: the recursive form cannot
    /// express this shape either (that is the same reason the defect was the
    /// only witness). What the two tests assert against it is therefore
    /// absolute — the exact spliced string, and the panic disposition under a
    /// `TRIM` longer than the fallback — not a driver-versus-twin comparison.
    #[cfg(test)]
    pub(super) fn drive_splice_probe(
        pp: &mut PrettyPrinter,
        before: &Par,
        after: &Par,
    ) -> Result<String, InterpreterError> {
        run(pp, PpWork::TestFailureBetweenSiblings { before, after })
    }

    /// ⚠ TEST ONLY. [`drive_splice_probe`]'s shape with **one more catching
    /// scope around it**, so that two frames are open when the failure fires.
    ///
    /// ## ★ Why the suite needed this, stated as the measurement that found it
    ///
    /// [`run`] unwinds to `catches.last_mut()` — the INNERMOST open frame,
    /// which is what the recursive form's `?` does. Mutating that to
    /// `catches.first_mut()` passed all 29 tests: every other probe opens its
    /// catching scopes in *sequence* (`push_catch` pushes `EndCatch`, the
    /// guarded item, `BeginCatch`, and the frame is popped by its own
    /// `EndCatch` before the next `BeginCatch` runs), so `catches` never held
    /// more than one frame and `first` and `last` were the same element. The
    /// claim "unwind to the innermost" was a claim about a one-element list.
    ///
    /// With two frames open the two spellings separate cleanly:
    ///
    /// * `last_mut()` — the inner frame absorbs, its siblings survive, the
    ///   outer frame closes normally, `catches` empties.
    /// * `first_mut()` — the OUTER frame absorbs. It truncates the inner
    ///   frame's `EndCatch` off the work stack, so the inner frame is never
    ///   closed; the surviving `EndCatch` pops the *wrong* frame, the siblings
    ///   are lost, and [`run`]'s "catching scope(s) never closed" assertion
    ///   fires.
    ///
    /// ⚠ Nesting also means the outer `EndCatch` sees a **successful** region
    /// and caps it, so this probe cannot double as the capping decider — that
    /// stays with the single-layer [`drive_splice_probe`]. One property per
    /// probe.
    #[cfg(test)]
    pub(super) fn drive_nested_catch_probe(
        pp: &mut PrettyPrinter,
        before: &Par,
        after: &Par,
    ) -> Result<String, InterpreterError> {
        run(pp, PpWork::TestNestedCatchingFailure { before, after })
    }

    // -----------------------------------------------------------------------
    // descend: `_build_string_from_message`
    // -----------------------------------------------------------------------

    fn descend_node<'a>(
        pp: &mut PrettyPrinter,
        node: PpNode<'a>,
        indent: usize,
        work: &mut Vec<PpWork<'a>>,
        vals: &mut Vec<String>,
    ) -> Result<(), InterpreterError> {
        match node {
            PpNode::Var(v) => vals.push(pp.build_string_from_var(v)),

            PpNode::Unprintable(msg) => {
                return Err(InterpreterError::BugFoundError(msg.to_string()))
            }

            PpNode::Send(s) => {
                work.push(PpWork::Combine(PpKont::SendK { send: s }));
                // ⚠ `data_str` is bound BEFORE the channel is rendered, so the
                // data elements mutate printer state first.
                let channel = |work: &mut Vec<PpWork<'a>>| {
                    push_catch(work, CatchKind::Message, PpWork::OptPar {
                        field: &s.chan,
                        message: "channel field on Send was None, should be Some",
                        render: OptRender::Message,
                        indent: 0,
                    })
                };
                let channel_first = mutating(DriveMutation::SendChannelBeforeData);
                if !channel_first {
                    channel(work);
                }
                for p in s.data.iter().rev() {
                    push_catch(work, CatchKind::Message, PpWork::Node(PpNode::Par(p), 0));
                }
                if channel_first {
                    channel(work);
                }
            }

            PpNode::Receive(r) => {
                work.push(PpWork::Combine(PpKont::ReceiveK { recv: r, indent }));
                push_catch(work, CatchKind::Message, PpWork::OptPar {
                    field: &r.body,
                    message: "body field on receive was None, should be Some",
                    render: OptRender::Message,
                    indent: 0,
                });
                // ★ The `where` guard, if this receive renders one. Three
                // things about its position are decisions, not conveniences:
                //
                // * it renders **after** `Mutate(AddBoundShift(totally_free))`,
                //   i.e. in the SAME de Bruijn environment as the body, because
                //   that is the environment it was normalized in —
                //   `p_input_normalizer`'s `InputPhase::Guard` normalizes the
                //   guard against `k.body_env`, so "guard and body see the same
                //   de Bruijn levels". Rendering it before the shift prints
                //   different names for the same binder;
                // * it renders **before** the body, which is the order the
                //   bytes come out in (`for( binds where G ) { body }`), so a
                //   guard that itself moves printer state — one containing a
                //   `New`, say — moves it for the body exactly as the source
                //   order says it should;
                // * it is catch-wrapped at indent 0, which is what makes this
                //   the machine form of `oracle_build_string_from_message(g)`
                //   rather than of `_oracle_build_string_from_message(g,
                //   indent)`: catching, capping, indent reset. The body's
                //   `OptPar { render: Message, indent: 0 }` reduces to the same
                //   thing; it takes the `OptPar` route only because its field is
                //   a required `Option` whose `None` must still panic.
                //
                // The first of the three is policed by
                // `DriveMutation::ReceiveConditionBeforeBoundShift`.
                let guard_before_shift = mutating(DriveMutation::ReceiveConditionBeforeBoundShift);
                let push_guard = |work: &mut Vec<PpWork<'a>>| {
                    if let Some(condition) = receive_guard(r) {
                        push_catch(
                            work,
                            CatchKind::Message,
                            PpWork::Node(PpNode::Par(condition), 0),
                        );
                    }
                };
                if !guard_before_shift {
                    push_guard(work);
                }
                // The fold's accumulator, as prefix sums. See the module
                // documentation for why this is static and what it costs.
                let mut previous_free: Vec<i32> = Vec::with_capacity(r.binds.len());
                let mut totally_free: i32 = 0;
                for bind in &r.binds {
                    previous_free.push(totally_free);
                    // `+=`, and the operand order flips with it. `i32` addition
                    // is commutative and its overflow condition is symmetric,
                    // so this is the same value AND the same panic in the
                    // `dev` profile — which matters, because the raw property
                    // corpus compares the two forms by *disposition*.
                    totally_free += bind.free_count;
                }
                work.push(PpWork::Mutate(PpMutation::AddBoundShift(totally_free)));
                if guard_before_shift {
                    push_guard(work);
                }
                for index in (0..r.binds.len()).rev() {
                    work.push(PpWork::RecvBindStep {
                        recv: r,
                        index,
                        previous_free: previous_free[index],
                        indent,
                    });
                }
            }

            PpNode::Bundle(b) => {
                work.push(PpWork::Combine(PpKont::BundleK {
                    show: BundleOps::show(b),
                    indent,
                }));
                work.push(PpWork::Node(
                    PpNode::Par(
                        b.body
                            .as_ref()
                            .expect("body field on bundle was None, should be Some"),
                    ),
                    indent + 1,
                ));
            }

            PpNode::New(n) => {
                // Both of these read the PRE-mutation `bound_shift`, which is
                // why the interval is taken here and then handed to
                // `build_variables` rather than each of them reading `pp`.
                //
                // ★ `Θ(1)`. `bind_count` is an attacker-controllable `i32` in a
                // hand-built `Par`, and this used to be
                // `(0..n.bind_count).map(|i| i + pp.bound_shift).collect()` — a
                // `Vec<i32>` of up to 2^31-1 elements, i.e. an ~8 GiB request,
                // on a path reachable from untrusted input via `rho:io:stdout`.
                // The indices are contiguous by construction, so the interval
                // IS the information; recording it costs two words per `New`
                // and no allocation scales with `bind_count` any more.
                //
                // ⚠ NEGATIVE `bind_count` is representable and is NOT an error
                // here: the interval is empty (`NewBindRange::is_empty`), so it
                // binds nothing — the same value the materialised form
                // produced. Explicit, not incidental;
                // `a_new_with_a_negative_bind_count_binds_nothing` pins it, and
                // the `bound_shift` mutation below still takes the RAW count.
                if mutating(DriveMutation::NewIntervalReadsPostMutationShift) {
                    pp.bound_shift += n.bind_count;
                }
                let introduced = pp.new_bind_range(n.bind_count);
                if mutating(DriveMutation::NewMutatesBeforeVariables) {
                    pp.bound_shift += n.bind_count;
                }
                let variables = pp.build_variables(introduced);
                work.push(PpWork::Combine(PpKont::NewK { variables, indent }));

                if !mutating(DriveMutation::NewMutatesBeforeVariables)
                    && !mutating(DriveMutation::NewIntervalReadsPostMutationShift)
                {
                    pp.bound_shift += n.bind_count;
                }
                pp.news_shift_indices.push(introduced);

                work.push(PpWork::Node(
                    PpNode::Par(
                        n.p.as_ref()
                            .expect("p field on New was None, should be Some"),
                    ),
                    indent + 1,
                ));
            }

            PpNode::Expr(e) => push_catch(work, CatchKind::Expr, PpWork::ExprBody(e)),

            PpNode::Match(m) => {
                work.push(PpWork::Combine(PpKont::MatchK { m, indent }));
                for case in m.cases.iter().rev() {
                    work.push(PpWork::CaseStep {
                        case,
                        indent: indent + 1,
                    });
                }
                // ★ FIXED (was the `Unprintable` defect): the target renders
                // through the entry point the recursive form named —
                // `build_string_from_message`, i.e. CATCHING, CAPPED, indent 0
                // — which is what `push_catch(Message, …)` around a
                // non-capping `OptPar` is.
                //
                // ⚠ `OptPar` and not an eager `.expect` at descend: the
                // module's `.expect`-position rule. Here the two coincide (the
                // target is the first thing a `Match` renders, and `OptPar`
                // pops before every `CaseStep`), so the rule costs nothing and
                // keeps the site uniform with the other six `Option<Par>`
                // children.
                push_catch(work, CatchKind::Message, PpWork::OptPar {
                    field: &m.target,
                    message: "target field on Match was None, should be Some",
                    render: OptRender::Message,
                    indent: 0,
                });
            }

            PpNode::Unforgeable(u) => vals.push(pp.build_string_from_unforgeable(u)?),

            PpNode::Connective(c) => match &c.connective_instance {
                Some(ConnectiveInstance::ConnAndBody(value))
                | Some(ConnectiveInstance::ConnOrBody(value)) => {
                    work.push(PpWork::Combine(PpKont::ConnectiveK { conn: c }));
                    for p in value.ps.iter().rev() {
                        push_catch(work, CatchKind::Message, PpWork::Node(PpNode::Par(p), 0));
                    }
                }
                Some(ConnectiveInstance::ConnNotBody(value)) => {
                    work.push(PpWork::Combine(PpKont::ConnectiveK { conn: c }));
                    push_catch(
                        work,
                        CatchKind::Message,
                        PpWork::Node(PpNode::Par(value), 0),
                    );
                }
                Some(ConnectiveInstance::VarRefBody(value)) => vals.push(format!(
                    "={}{}",
                    pp.free_id,
                    pp.free_shift - value.index - 1
                )),
                Some(ConnectiveInstance::ConnBool(_)) => vals.push(String::from("Bool")),
                Some(ConnectiveInstance::ConnInt(_)) => vals.push(String::from("Int")),
                Some(ConnectiveInstance::ConnString(_)) => vals.push(String::from("String")),
                Some(ConnectiveInstance::ConnUri(_)) => vals.push(String::from("Uri")),
                Some(ConnectiveInstance::ConnByteArray(_)) => vals.push(String::from("ByteArray")),
                None => vals.push(String::new()),
            },

            PpNode::Par(p) => {
                if pp.is_empty_par(p) {
                    vals.push(String::from("Nil"));
                } else {
                    work.push(PpWork::Combine(PpKont::ParK { par: p, indent }));
                    // The eight categories, in the order the recursive form
                    // walks them, pushed in REVERSE so they pop in order. All
                    // non-catching, all at the SAME indent.
                    for c in p.connectives.iter().rev() {
                        work.push(PpWork::Node(PpNode::Connective(c), indent));
                    }
                    for u in p.unforgeables.iter().rev() {
                        work.push(PpWork::Node(PpNode::Unforgeable(u), indent));
                    }
                    for m in p.matches.iter().rev() {
                        work.push(PpWork::Node(PpNode::Match(m), indent));
                    }
                    match mutating(DriveMutation::ParPushesExprsUnreversed) {
                        true => {
                            for e in p.exprs.iter() {
                                work.push(PpWork::Node(PpNode::Expr(e), indent));
                            }
                        }
                        false => {
                            for e in p.exprs.iter().rev() {
                                work.push(PpWork::Node(PpNode::Expr(e), indent));
                            }
                        }
                    }
                    for n in p.news.iter().rev() {
                        work.push(PpWork::Node(PpNode::New(n), indent));
                    }
                    for r in p.receives.iter().rev() {
                        work.push(PpWork::Node(PpNode::Receive(r), indent));
                    }
                    for s in p.sends.iter().rev() {
                        work.push(PpWork::Node(PpNode::Send(s), indent));
                    }
                    for b in p.bundles.iter().rev() {
                        work.push(PpWork::Node(PpNode::Bundle(b), indent));
                    }
                }
            }
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // descend: `_build_string_from_expr`
    // -----------------------------------------------------------------------

    /// The **single source** for "which children does this `Expr` arm have, and
    /// in what order" — used by both [`descend_expr`] and [`combine_expr`] so
    /// the two cannot drift.
    enum ExprPlan<'a> {
        /// No worklisted children: [`inline_expr`] renders it in place. The
        /// grounds, `EVarBody`, the `None` instance, and the three re-entrant
        /// arms (`ESetBody`, `EMapBody`, `EZipperBody`).
        Inline,
        /// `{prefix}{wrap_with_braces(p)}`
        Unary {
            prefix: &'static str,
            p: &'a Option<Par>,
            msg: &'static str,
        },
        /// `{p1} {op} {wrap_with_braces(p2)}`
        Binary {
            op: &'static str,
            p1: &'a Option<Par>,
            m1: &'static str,
            p2: &'a Option<Par>,
            m2: &'static str,
        },
        /// `wrap_with_braces("{target} matches {pattern}")`
        Matches {
            target: &'a Option<Par>,
            mt: &'static str,
            pattern: &'a Option<Par>,
            mp: &'static str,
        },
        /// `{open}{elements}{remainder}{close}`, with the three-way
        /// remainder/elements case analysis. `EListBody` and `EPathmapBody`.
        Bracketed {
            ps: &'a [Par],
            remainder: &'a Option<Var>,
            open: &'static str,
            close: &'static str,
        },
        /// `{|entries remainder|}` over a `PathMap<()>`. Descent decodes
        /// entries into the traversal-local PDA arena rather than forcing a
        /// persistent flat projection on the EPathMap.
        PathMap { pathmap: &'a EPathMap },
        /// `ReadZipper` / `WriteZipper`. Both the underlying set entries and
        /// decodable cursor segments are worklisted in the same PDA.
        Zipper { zipper: &'a EZipper },
        /// `({elements})`
        Tuple { ps: &'a [Par] },
        /// `({target}).{name}({args})` — arguments first, THEN the target.
        Method { m: &'a EMethod },
    }

    fn expr_plan(e: &Expr) -> ExprPlan<'_> {
        let Some(instance) = &e.expr_instance else {
            return ExprPlan::Inline;
        };
        match instance {
            ExprInstance::ENegBody(ENeg { p }) => ExprPlan::Unary {
                prefix: "-",
                p,
                msg: "ENeg par field was None, should be Some",
            },
            ExprInstance::ENotBody(ENot { p }) => ExprPlan::Unary {
                prefix: "~",
                p,
                msg: "ENot par field was None, should be Some",
            },
            ExprInstance::EMultBody(EMult { p1, p2 }) => ExprPlan::Binary {
                op: "*",
                p1,
                m1: "EMult p1 field was None, should be Some",
                p2,
                m2: "EMult p2 field was None, should be Some",
            },
            ExprInstance::EDivBody(EDiv { p1, p2 }) => ExprPlan::Binary {
                op: "/",
                p1,
                m1: "EDiv p1 field was None, should be Some",
                p2,
                m2: "EDiv p2 field was None, should be Some",
            },
            ExprInstance::EModBody(EMod { p1, p2 }) => ExprPlan::Binary {
                op: "%",
                p1,
                m1: "EMod p1 field was None, should be Some",
                p2,
                m2: "EMod p2 field was None, should be Some",
            },
            ExprInstance::EPercentPercentBody(EPercentPercent { p1, p2 }) => ExprPlan::Binary {
                op: "%%",
                p1,
                m1: "EPercentPercent p1 field was None, should be Some",
                p2,
                m2: "EPercentPercent p2 field was None, should be Some",
            },
            ExprInstance::EPlusBody(EPlus { p1, p2 }) => ExprPlan::Binary {
                op: "+",
                p1,
                m1: "EPlus p1 field was None, should be Some",
                p2,
                m2: "EPlus p2 field was None, should be Some",
            },
            ExprInstance::EPlusPlusBody(EPlusPlus { p1, p2 }) => ExprPlan::Binary {
                op: "++",
                p1,
                m1: "EPlusPlus p1 field was None, should be Some",
                p2,
                m2: "EPlusPlus p2 field was None, should be Some",
            },
            ExprInstance::EMinusBody(EMinus { p1, p2 }) => ExprPlan::Binary {
                op: "-",
                p1,
                m1: "EMinus p1 field was None, should be Some",
                p2,
                m2: "EMinus p2 field was None, should be Some",
            },
            // ⚠ Renders with the SAME `-` operator as `EMinus`. Verbatim.
            ExprInstance::EMinusMinusBody(EMinusMinus { p1, p2 }) => ExprPlan::Binary {
                op: "-",
                p1,
                m1: "EMinusMinus p1 field was None, should be Some",
                p2,
                m2: "EMinusMinus p2 field was None, should be Some",
            },
            ExprInstance::EAndBody(EAnd { p1, p2 }) => ExprPlan::Binary {
                op: "&&",
                p1,
                m1: "EAnd p1 field was None, should be Some",
                p2,
                m2: "EAnd p2 field was None, should be Some",
            },
            ExprInstance::EOrBody(EOr { p1, p2 }) => ExprPlan::Binary {
                op: "||",
                p1,
                m1: "EOr p1 field was None, should be Some",
                p2,
                m2: "EOr p2 field was None, should be Some",
            },
            ExprInstance::EEqBody(EEq { p1, p2 }) => ExprPlan::Binary {
                op: "==",
                p1,
                m1: "EEq p1 field was None, should be Some",
                p2,
                m2: "EEq p2 field was None, should be Some",
            },
            ExprInstance::ENeqBody(ENeq { p1, p2 }) => ExprPlan::Binary {
                op: "!=",
                p1,
                m1: "ENeq p1 field was None, should be Some",
                p2,
                m2: "ENeq p2 field was None, should be Some",
            },
            ExprInstance::EGtBody(EGt { p1, p2 }) => ExprPlan::Binary {
                op: ">",
                p1,
                m1: "EGt p1 field was None, should be Some",
                p2,
                m2: "EGt p2 field was None, should be Some",
            },
            ExprInstance::EGteBody(EGte { p1, p2 }) => ExprPlan::Binary {
                op: ">=",
                p1,
                m1: "EGte p1 field was None, should be Some",
                p2,
                m2: "EGte p2 field was None, should be Some",
            },
            ExprInstance::ELtBody(ELt { p1, p2 }) => ExprPlan::Binary {
                op: "<",
                p1,
                m1: "ELt p1 field was None, should be Some",
                p2,
                m2: "ELt p2 field was None, should be Some",
            },
            ExprInstance::ELteBody(ELte { p1, p2 }) => ExprPlan::Binary {
                op: "<=",
                p1,
                m1: "ELte p1 field was None, should be Some",
                p2,
                m2: "ELte p2 field was None, should be Some",
            },
            ExprInstance::EMatchesBody(EMatches { target, pattern }) => ExprPlan::Matches {
                target,
                mt: "EMatches target field was None, should be Some",
                pattern,
                mp: "EMatches pattern field was None, should be Some",
            },
            ExprInstance::EListBody(EList { ps, remainder, .. }) => ExprPlan::Bracketed {
                ps: ps.as_slice(),
                remainder,
                open: "[",
                close: "]",
            },
            ExprInstance::ETupleBody(ETuple { ps, .. }) => ExprPlan::Tuple { ps: ps.as_slice() },
            ExprInstance::EPathmapBody(pathmap) => ExprPlan::PathMap { pathmap },
            ExprInstance::EZipperBody(zipper) => ExprPlan::Zipper { zipper },
            ExprInstance::EMethodBody(method) => ExprPlan::Method { m: method },
            // The remaining re-entrant collections, plus every leaf. See [`inline_expr`].
            ExprInstance::ESetBody(_)
            | ExprInstance::EMapBody(_)
            | ExprInstance::EVarBody(_)
            | ExprInstance::GBool(_)
            | ExprInstance::GInt(_)
            | ExprInstance::GString(_)
            | ExprInstance::GUri(_)
            | ExprInstance::GByteArray(_)
            | ExprInstance::GDouble(_)
            | ExprInstance::GBigInt(_)
            | ExprInstance::GBigRat(_)
            | ExprInstance::GFixedPoint(_) => ExprPlan::Inline,
        }
    }

    fn descend_expr<'a>(
        pp: &mut PrettyPrinter,
        e: &'a Expr,
        decoded_entries: &'a Arena<Par>,
        work: &mut Vec<PpWork<'a>>,
        vals: &mut Vec<String>,
    ) -> Result<(), InterpreterError> {
        // Every worklisted child of an `Expr` goes through the CATCHING
        // `build_string_from_message`, i.e. indent 0 and capped.
        fn child<'a>(field: &'a Option<Par>, message: &'static str) -> PpWork<'a> {
            PpWork::OptPar {
                field,
                message,
                render: OptRender::Message,
                indent: 0,
            }
        }

        fn push_pathmap_children<'a>(
            pathmap: &'a EPathMap,
            decoded_entries: &'a Arena<Par>,
            work: &mut Vec<PpWork<'a>>,
        ) {
            match pathmap.mode() {
                EPathMapMode::Empty => {}
                EPathMapMode::Set => {
                    pathmap
                        .entry_trie()
                        .for_each_raw_set_entry_reverse(|key| {
                            let entry = models::rust::canonical_path::decode_trie_path(key)
                                .expect("set-mode EPathMap keys are canonical Par paths");
                            push_catch(
                                work,
                                CatchKind::Message,
                                PpWork::Node(PpNode::Par(decoded_entries.alloc(entry)), 0),
                            );
                        })
                        .expect("set-mode dispatch checked before traversal");
                }
                EPathMapMode::Map => {
                    pathmap
                        .entry_trie()
                        .for_each_raw_map_entry_reverse(|key, value| {
                            // The forward rendering of one binding is key then
                            // value. Push the value first so this LIFO PDA
                            // evaluates the key first.
                            push_catch(
                                work,
                                CatchKind::Message,
                                PpWork::Node(PpNode::Par(value), 0),
                            );
                            let key = models::rust::canonical_path::decode_trie_path(key)
                                .expect("map-mode EPathMap keys are canonical Par paths");
                            push_catch(
                                work,
                                CatchKind::Message,
                                PpWork::Node(PpNode::Par(decoded_entries.alloc(key)), 0),
                            );
                        })
                        .expect("map-mode dispatch checked before traversal");
                }
            }
        }

        match expr_plan(e) {
            ExprPlan::Inline => vals.push(inline_expr(pp, e)?),

            ExprPlan::Unary { p, msg, .. } => {
                work.push(PpWork::Combine(PpKont::ExprK { expr: e }));
                push_catch(work, CatchKind::Message, child(p, msg));
            }

            ExprPlan::Binary { p1, m1, p2, m2, .. } => {
                work.push(PpWork::Combine(PpKont::ExprK { expr: e }));
                push_catch(work, CatchKind::Message, child(p2, m2));
                push_catch(work, CatchKind::Message, child(p1, m1));
            }

            ExprPlan::Matches {
                target,
                mt,
                pattern,
                mp,
            } => {
                work.push(PpWork::Combine(PpKont::ExprK { expr: e }));
                push_catch(work, CatchKind::Message, child(pattern, mp));
                push_catch(work, CatchKind::Message, child(target, mt));
            }

            ExprPlan::Bracketed { ps, .. } | ExprPlan::Tuple { ps } => {
                work.push(PpWork::Combine(PpKont::ExprK { expr: e }));
                // `build_vec`
                for p in ps.iter().rev() {
                    push_catch(work, CatchKind::Message, PpWork::Node(PpNode::Par(p), 0));
                }
            }

            ExprPlan::PathMap { pathmap } => {
                work.push(PpWork::Combine(PpKont::ExprK { expr: e }));
                push_pathmap_children(pathmap, decoded_entries, work);
            }

            ExprPlan::Zipper { zipper } => {
                let pathmap = zipper.pathmap.as_ref().expect("zipper pathmap was None");
                work.push(PpWork::Combine(PpKont::ExprK { expr: e }));

                // Cursor segments render after the map entries. Push them first
                // (in reverse) so the LIFO machine preserves that order.
                use models::rust::canonical_path::{decode_trie_path, tag};
                for segment in zipper.current_path.iter().rev() {
                    let mut framed = segment.clone();
                    framed.push(tag::TERM);
                    let channel = decode_trie_path(&framed).ok().and_then(|par| {
                        let par = decoded_entries.alloc(par);
                        match par.exprs.first().and_then(|ex| ex.expr_instance.as_ref()) {
                            Some(ExprInstance::EListBody(list)) if !list.ps.is_empty() => {
                                Some(&list.ps[0])
                            }
                            _ => None,
                        }
                    });
                    match channel {
                        Some(channel) => {
                            push_catch(work, CatchKind::Channel, PpWork::Channel(channel, 0));
                        }
                        None => work.push(PpWork::Literal(format!("0x{}", hex::encode(segment)))),
                    }
                }

                push_pathmap_children(pathmap, decoded_entries, work);
            }

            ExprPlan::Method { m } => {
                work.push(PpWork::Combine(PpKont::ExprK { expr: e }));
                // ⚠ The arguments are collected BEFORE the target is rendered.
                push_catch(
                    work,
                    CatchKind::Message,
                    child(&m.target, "target field on Method was None, should be Some"),
                );
                for arg in m.arguments.iter().rev() {
                    push_catch(work, CatchKind::Message, PpWork::Node(PpNode::Par(arg), 0));
                }
            }
        }
        Ok(())
    }

    /// The arms with no worklisted children: the grounds, `EVarBody`, the
    /// absent instance, and the three that re-enter the drive.
    ///
    /// ⚠ The bodies of `ESetBody`, `EMapBody` and `EZipperBody` are the
    /// pre-conversion bodies **unchanged**. See the module documentation for
    /// why they cannot be worklisted and what that costs.
    fn inline_expr(pp: &mut PrettyPrinter, e: &Expr) -> Result<String, InterpreterError> {
        let Some(instance) = &e.expr_instance else {
            // TODO: Figure out if we can prevent prost from generating - OLD
            return Ok(String::from("Nil"));
        };
        match instance {
            ExprInstance::ESetBody(eset) => {
                let par_set = ParSetTypeMapper::eset_to_par_set(eset.clone());
                let pars = par_set.ps;
                let remainder = &par_set.remainder;

                //TODO same problem with comma

                let elements = pp.build_vec(&pars.sorted_pars);
                let remainder_string = pp.build_remainder_string(remainder);
                let full_result = if remainder.is_some() && !elements.is_empty() {
                    format!("Set({}{})", elements, remainder_string)
                } else if remainder.is_some() {
                    format!("Set({})", remainder_string)
                } else {
                    format!("Set({})", elements)
                };

                Ok(full_result)
            }

            ExprInstance::EMapBody(emap) => {
                let par_map = ParMapTypeMapper::emap_to_par_map(emap.clone());
                let sorted_list = par_map.ps.sorted_list;
                let remainder = &par_map.remainder;
                let mut result = String::from("{");

                for (i, (key, value)) in sorted_list.iter().enumerate() {
                    result.push_str(&pp.build_string_from_message(key));
                    result.push_str(" : ");
                    result.push_str(&pp.build_string_from_message(value));

                    if i != sorted_list.len() - 1 {
                        result.push_str(", ");
                    }
                }

                result.push_str(&pp.build_remainder_string(remainder));
                result.push('}');

                Ok(result)
            }

            ExprInstance::EZipperBody(_) => {
                unreachable!("EZipper is worklisted by ExprPlan::Zipper")
            }

            ExprInstance::EVarBody(EVar { v }) => Ok(pp.build_string_from_var(
                v.as_ref()
                    .expect("var field on EVar was None, should be Some"),
            )),

            ExprInstance::GBool(b) => Ok(b.to_string()),
            ExprInstance::GInt(i) => Ok(i.to_string()),
            ExprInstance::GString(s) => Ok(format!("\"{}\"", s)),
            ExprInstance::GUri(u) => Ok(format!("`{}`", u)),
            ExprInstance::GByteArray(bs) => Ok(hex::encode(bs)),
            ExprInstance::GDouble(bits) => {
                let f = f64::from_bits(*bits);
                if f == f.floor() && f.is_finite() {
                    Ok(format!("{:.1}f64", f))
                } else {
                    Ok(format!("{}f64", f))
                }
            }
            ExprInstance::GBigInt(bytes) => Ok(format!("{}n", twos_complement_to_decimal(bytes))),
            ExprInstance::GBigRat(rat) => {
                let num_str = twos_complement_to_decimal(&rat.numerator);
                let den_str = twos_complement_to_decimal(&rat.denominator);
                Ok(format!("{}/{}r", num_str, den_str))
            }
            ExprInstance::GFixedPoint(fp) => {
                let unscaled_str = twos_complement_to_decimal(&fp.unscaled);
                if fp.scale == 0 {
                    Ok(format!("{}p0", unscaled_str))
                } else {
                    let scale = fp.scale as usize;
                    let is_negative = unscaled_str.starts_with('-');
                    let digits = if is_negative {
                        &unscaled_str[1..]
                    } else {
                        &unscaled_str
                    };
                    if digits.len() <= scale {
                        let padded = format!("{:0>width$}", digits, width = scale + 1);
                        let (integer, fraction) = padded.split_at(padded.len() - scale);
                        let prefix = if is_negative { "-" } else { "" };
                        Ok(format!("{}{}.{}p{}", prefix, integer, fraction, scale))
                    } else {
                        let (integer, fraction) = digits.split_at(digits.len() - scale);
                        let prefix = if is_negative { "-" } else { "" };
                        Ok(format!("{}{}.{}p{}", prefix, integer, fraction, scale))
                    }
                }
            }

            // Reached only if [`expr_plan`] and this function disagree about
            // which arms are inline; they are written adjacently for exactly
            // that reason.
            other => unreachable!(
                "pretty-printer drive: `{:?}` was classified inline but has no inline \
                 rendering — `expr_plan` and `inline_expr` have drifted",
                std::mem::discriminant(other)
            ),
        }
    }

    // -----------------------------------------------------------------------
    // combine
    // -----------------------------------------------------------------------

    /// Pop `n` child values, in the order they were produced.
    fn take(vals: &mut Vec<String>, n: usize) -> Vec<String> {
        assert!(
            vals.len() >= n,
            "pretty-printer drive: a continuation wanted {} children but the value stack \
             holds {}",
            n,
            vals.len()
        );
        vals.split_off(vals.len() - n)
    }

    fn one(vals: &mut Vec<String>) -> String {
        vals.pop()
            .expect("pretty-printer drive: a continuation wanted a child that was never pushed")
    }

    fn combine(
        pp: &mut PrettyPrinter,
        kont: PpKont<'_>,
        vals: &mut Vec<String>,
        catches: &mut Vec<CatchFrame>,
    ) -> Result<(), InterpreterError> {
        match kont {
            PpKont::EndCatch => {
                let frame = catches
                    .pop()
                    .expect("pretty-printer drive: `EndCatch` with no open catching scope");
                // ⚠ The fallback is NOT capped. That asymmetry is pre-existing
                // (`build_string_from_node`) and is reproduced exactly.
                if !frame.failed || mutating(DriveMutation::EndCatchCapsFallback) {
                    let value = one(vals);
                    vals.push(pp.cap(&value));
                }
            }

            PpKont::SendK { send } => {
                let chan = one(vals);
                let data = take(vals, send.data.len());
                let str = if send.persistent {
                    String::from("!!(")
                } else {
                    String::from("!(")
                };
                vals.push(format!("{}{}{})", chan, str, data.join(", ")));
            }

            PpKont::RecvBindJoin { recv, index } => {
                // ⚠ The source was pushed LAST, so it comes off FIRST.
                let (source, patterns) = match mutating(DriveMutation::RecvBindJoinPatternsFirst) {
                    true => {
                        let patterns = take(vals, recv.binds[index].patterns.len());
                        (one(vals), patterns)
                    }
                    false => {
                        let source = one(vals);
                        (source, take(vals, recv.binds[index].patterns.len()))
                    }
                };
                let mut string = patterns.join(", ");
                if recv.persistent {
                    string.push_str(" <= ");
                } else if recv.peek {
                    string.push_str(" <<- ");
                } else {
                    string.push_str(" <- ");
                }
                string.push_str(&source);
                if index != recv.binds.len() - 1 {
                    string.push_str("  & ");
                }
                vals.push(string);
            }

            PpKont::ReceiveK { recv, indent } => {
                // Pop order is the REVERSE of the render order: body, then
                // guard, then binds. `receive_guard` is the same predicate the
                // descent used to decide whether to push a guard value at all,
                // so the two cannot disagree about how many values are here.
                let body_str = one(vals);
                let where_clause = match receive_guard(recv) {
                    // ⚠ The grammar attaches `where` to the RECEIPT, not to a
                    // bind (`receipt: conc1(bind) optional('where' guard)`), so
                    // it goes after `binds_string` — after **every** bind — and
                    // inside the parentheses.
                    Some(_) => format!(" where {}", one(vals)),
                    None => String::new(),
                };
                let binds_string = take(vals, recv.binds.len()).concat();
                if !body_str.is_empty() {
                    vals.push(format!(
                        "for( {}{} ) {{\n{}{}{}\n{}}}",
                        binds_string,
                        where_clause,
                        pp.indent_string().repeat(indent + 1),
                        body_str,
                        pp.indent_string().repeat(indent),
                        ""
                    ));
                } else {
                    vals.push(format!("for( {}{} ) {{}}", binds_string, where_clause));
                }
            }

            PpKont::BundleK { show, indent } => {
                let body = one(vals);
                vals.push(format!(
                    "{}{{\n{}{}\n}}",
                    show,
                    pp.indent_string().repeat(indent + 1),
                    body
                ));
            }

            PpKont::NewK { variables, indent } => {
                let body = one(vals);
                let result = format!(
                    "new {} in {{\n{}{}",
                    variables,
                    pp.indent_string().repeat(indent + 1),
                    body
                );
                vals.push(format!(
                    "{}\n{}{}",
                    result,
                    pp.indent_string().repeat(indent),
                    "}"
                ));
            }

            PpKont::CaseJoin { indent } => {
                let source = one(vals);
                let pattern = one(vals);
                // Both depend only on `indent`, so recomputing them here is the
                // same string the recursive form built before its mutations.
                let open_brace = format!("{{\n{}", pp.indent_string().repeat(indent + 1));
                let close_brace = format!("\n{}}}", pp.indent_string().repeat(indent));
                vals.push(format!(
                    "{} => {}{}{}",
                    pattern, open_brace, source, close_brace
                ));
            }

            PpKont::MatchK { m, indent } => {
                let cases = take(vals, m.cases.len());
                let target = one(vals);
                let mut cases_string = String::new();
                for (i, case) in cases.iter().enumerate() {
                    cases_string.push_str(&pp.indent_string().repeat(indent + 1));
                    cases_string.push_str(case);
                    if i != cases.len() - 1 {
                        cases_string.push('\n');
                    }
                }
                let result = format!(
                    "match {} {{\n{}{}",
                    target,
                    pp.indent_string().repeat(indent + 1),
                    cases_string
                );
                vals.push(format!(
                    "{}\n{}{}",
                    result,
                    pp.indent_string().repeat(indent),
                    "}"
                ));
            }

            PpKont::ConnectiveK { conn } => match &conn.connective_instance {
                Some(ConnectiveInstance::ConnAndBody(value)) => {
                    let ps = take(vals, value.ps.len());
                    vals.push(format!("{{ {} }}", ps.join(" /\\ ")));
                }
                Some(ConnectiveInstance::ConnOrBody(value)) => {
                    let ps = take(vals, value.ps.len());
                    vals.push(format!("{{ {} }}", ps.join(" \\/ ")));
                }
                Some(ConnectiveInstance::ConnNotBody(_)) => {
                    let p = one(vals);
                    vals.push(format!("~{{{}}}", p));
                }
                // A continuation is pushed only for the three arms above.
                _ => unreachable!(
                    "pretty-printer drive: `ConnectiveK` over a leaf connective — the \
                     descend handler pushed a continuation it should not have"
                ),
            },

            PpKont::ParK { par, indent } => {
                // The eight categories, in walk order.
                //
                // ⚠ The category PARTITION is not observable in the output, and
                // that is worth knowing rather than discovering. The separator
                // between two categories and the separator between two siblings
                // WITHIN a category are the same string, so this whole loop is
                // extensionally `children.join(&separator)` for *any* partition
                // of the same total. A mutation permuting this array therefore
                // passes the differential — measured, not assumed. What the
                // differential does police is the DESCEND order (which child
                // lands at which index), which is where a worklist conversion
                // actually goes wrong; a mutation un-reversing any one of the
                // eight pushes below is caught.
                //
                // The grouped form is kept because it mirrors the recursive
                // body line for line, and because it is the form that stays
                // correct if a category ever gets its own separator.
                let lengths = [
                    par.bundles.len(),
                    par.sends.len(),
                    par.receives.len(),
                    par.news.len(),
                    par.exprs.len(),
                    par.matches.len(),
                    par.unforgeables.len(),
                    par.connectives.len(),
                ];
                let children = take(vals, lengths.iter().sum());
                let separator = format!(" |\n{}", pp.indent_string().repeat(indent));
                vals.push(render_par_categories(&children, lengths, &separator));
            }

            PpKont::ChannelK { par } => {
                // ⚠ `is_building_channel` is SET, NEVER RESET. See the module
                // documentation: the recursive form leaves it set on the way
                // out, and every nested render after this one sees it.
                if mutating(DriveMutation::ChannelResetsFlag) {
                    pp.is_building_channel = false;
                }
                let str = one(vals);
                let quoted = if str.len() > 60 {
                    quote_if_not_new(par, str, &pp.news_shift_indices, pp.bound_shift)
                } else {
                    let whitespace = "\n(\\s\\s)*";
                    let replaced = regex::Regex::new(whitespace)
                        .unwrap()
                        .replace_all(&str, " ");
                    quote_if_not_new(
                        par,
                        replaced.to_string(),
                        &pp.news_shift_indices,
                        pp.bound_shift,
                    )
                };
                vals.push(quoted);
            }

            PpKont::ExprK { expr } => combine_expr(pp, expr, vals),

            #[cfg(test)]
            PpKont::TestJoinThree => {
                let three = take(vals, 3);
                vals.push(three.join(" | "));
            }
        }
        Ok(())
    }

    fn take_pathmap_repr(pp: &PrettyPrinter, pathmap: &EPathMap, vals: &mut Vec<String>) -> String {
        let child_count = match pathmap.mode() {
            EPathMapMode::Empty | EPathMapMode::Set => pathmap.len(),
            EPathMapMode::Map => pathmap
                .len()
                .checked_mul(2)
                .expect("an addressable EPathMap cannot overflow its child count"),
        };
        let rendered = take(vals, child_count);
        let elements = match pathmap.mode() {
            EPathMapMode::Empty | EPathMapMode::Set => rendered.join(", "),
            EPathMapMode::Map => {
                let mut result = String::new();
                for (index, pair) in rendered.chunks_exact(2).enumerate() {
                    if index != 0 {
                        result.push_str(", ");
                    }
                    result.push_str(&pair[0]);
                    result.push_str(" : ");
                    result.push_str(&pair[1]);
                }
                result
            }
        };
        let remainder_string = pp.build_remainder_string(&pathmap.remainder);
        if pathmap.remainder.is_some() && !elements.is_empty() {
            format!("{{|{}{}|}}", elements, remainder_string)
        } else if pathmap.remainder.is_some() {
            format!("{{|{}|}}", remainder_string)
        } else {
            format!("{{|{}|}}", elements)
        }
    }

    fn combine_expr(pp: &mut PrettyPrinter, e: &Expr, vals: &mut Vec<String>) {
        match expr_plan(e) {
            ExprPlan::Inline => unreachable!(
                "pretty-printer drive: `ExprK` over an inline arm — [`descend_expr`] pushes \
                 a continuation only for the worklisted plans"
            ),

            ExprPlan::Unary { prefix, .. } => {
                let p = one(vals);
                vals.push(format!("{}{}", prefix, wrap_with_braces(p)));
            }

            ExprPlan::Binary { op, .. } => {
                let p2 = one(vals);
                let p1 = one(vals);
                vals.push(format!("{} {} {}", p1, op, wrap_with_braces(p2)));
            }

            ExprPlan::Matches { .. } => {
                let pattern = one(vals);
                let target = one(vals);
                vals.push(wrap_with_braces(format!("{} matches {}", target, pattern)));
            }

            ExprPlan::Bracketed {
                ps,
                remainder,
                open,
                close,
            } => {
                let elements = take(vals, ps.len()).join(", ");
                // ⚠ Reads `free_shift` / `bound_shift` AFTER the elements have
                // been rendered — which is where the recursive form reads them.
                let remainder_string = pp.build_remainder_string(remainder);
                let full_result = if remainder.is_some() && !elements.is_empty() {
                    format!("{}{}{}{}", open, elements, remainder_string, close)
                } else if remainder.is_some() {
                    format!("{}{}{}", open, remainder_string, close)
                } else {
                    format!("{}{}{}", open, elements, close)
                };
                vals.push(full_result);
            }

            ExprPlan::PathMap { pathmap } => {
                let rendered = take_pathmap_repr(pp, pathmap, vals);
                vals.push(rendered);
            }

            ExprPlan::Zipper { zipper } => {
                let pathmap = zipper.pathmap.as_ref().expect("zipper pathmap was None");
                let path_segments = take(vals, zipper.current_path.len());
                let pathmap_repr = take_pathmap_repr(pp, pathmap, vals);
                let zipper_type = if zipper.is_write_zipper {
                    "WriteZipper"
                } else {
                    "ReadZipper"
                };
                let current_path_repr = render_cursor_position(zipper.cursor_kind, &path_segments);
                vals.push(format!(
                    "{}(at: {}, {})",
                    zipper_type, current_path_repr, pathmap_repr
                ));
            }

            ExprPlan::Tuple { ps } => {
                let elements = take(vals, ps.len()).join(", ");
                vals.push(format!("({})", elements));
            }

            ExprPlan::Method { m } => {
                let target = one(vals);
                let args = take(vals, m.arguments.len());
                vals.push(format!(
                    "({}).{}({})",
                    target,
                    m.method_name,
                    args.join(", ")
                ));
            }
        }
    }

    /// `_build_channel_string`'s closure, lifted out so the continuation can
    /// apply it. `p` is the channel's own `Par`; the two state values are read
    /// AFTER the sub-render, exactly as the closure's call site read them.
    fn quote_if_not_new(
        p: &Par,
        s: String,
        news_shift_indices: &[NewBindRange],
        bound_shift: i32,
    ) -> String {
        let is_bound_new = match p.exprs.as_slice() {
            [x] => match &x.expr_instance {
                Some(instance) => match instance {
                    ExprInstance::EVarBody(EVar { v }) => match v {
                        Some(v) => match &v.var_instance {
                            Some(instance) => match instance {
                                VarInstance::BoundVar(level) => PrettyPrinter::is_new_var(
                                    level,
                                    news_shift_indices,
                                    bound_shift,
                                ),
                                _ => false,
                            },
                            None => false,
                        },
                        None => false,
                    },

                    _ => false,
                },
                None => false,
            },
            _ => false,
        };

        if is_bound_new {
            s
        } else {
            format!("@{{{}}}", s)
        }
    }
}

// ===========================================================================
// the differential: the machine against the recursive twin
// ===========================================================================

#[cfg(test)]
mod differential {
    //! # The machine and the recursive twin must print the same bytes
    //!
    //! `build_channel_string`'s output is **block-resident and
    //! replay-compared** — `SystemDeployPlatformFailure::UnexpectedResult` ->
    //! `Display` -> `error_msg` -> `ProcessedSystemDeploy::Failed`, compared
    //! byte-for-byte at `casper/src/rust/rholang/replay_runtime.rs:745-758` —
    //! and it is reachable from untrusted input through `rho:io:stdout`. A
    //! conversion that changed one character of it would be a consensus fault,
    //! not a cosmetic regression. So the obligation is **string equality**
    //! against the pre-conversion body, not "looks the same".
    //!
    //! ## The corpus, and what each part is for
    //!
    //! | corpus | what it covers |
    //! |---|---|
    //! | [`tame`]d `generate_par(1..=4)` | the *structural* product — `Send` x `Receive` x `New` x `Match` x `Bundle` x `Connective` x `Expr` nested to depth 4, with counts small enough that the printer's `i32` arithmetic is well-defined |
    //! | RAW `generate_par(1..=4)` | the same shapes with `free_count` / `bind_count` / var levels drawn from `any::<i32>()`, compared by *disposition* — the two forms must agree even where the shared arithmetic overflows |
    //! | [`every_node_kind`] | all ten [`PpNode`] variants, each reached through the entry point that actually reaches it |
    //! | [`every_expr_arm`] | all 36 `ExprInstance` arms, including the three re-entrant ones (`ESet`, `EMap`, `EZipper`) and both `wrap_with_braces` shapes |
    //! | [`two_binds_that_bind_different_counts`] | ★ the ONLY shape where a *sequenced* `bound_shift` differs from a precomputed one |
    //! | [`a_receive_with_a_where_guard`] | ★ `Receive.condition` — a field NEITHER printer used to read, and which `generate_par` never generates, so no proptest corpus here reaches it |
    //! | [`a_match_nested_in_a_send`] | the `Match` target rendered MID-traversal, with siblings on both sides and printer state already moved |
    //! | [`a_failing_region_is_spliced_not_propagated`] | a catch frame firing mid-render, with the fallback spliced into the middle of a larger value — driven through `drive::drive_splice_probe`, because no `Par` can produce a failing catch any more |
    //! | [`the_capping_call_sites_are_reproduced`] | `EndCatch` caps on success and does NOT cap the fallback — run in a child process, because the cap is an environment variable |
    //! | [`a_new_costs_one_range_however_many_names_it_binds`] | ★ `New::bind_count` is an attacker-controlled `i32`; the per-`New` cost is `Θ(1)` and does not depend on it, and the *display* cap is checked separately in the same loop |
    //! | [`a_bind_range_answers_membership_exactly_as_a_vector_did`] | ★ the membership obligation of the interval representation, differential against the materialised `Vec<i32>` it replaced — including the wrapping regime |
    //! | [`the_star_prefix_survives_past_the_display_cap`] | ★ NO byte moves past the cap; the converse of the check that stood here while the recorded indices were clamped |
    //! | [`the_star_prefix_is_exact_across_nested_news`] | ★ four further slots the single-`New` fixture cannot see, found by census; also the only fixture where `is_new_var` scans more than one interval |
    //! | [`a_new_with_a_negative_bind_count_binds_nothing`] | a negative `bind_count` is representable; the empty range is a decision |
    //! | [`a_match_with_no_target_panics_like_every_other_absent_required_field`] | the second behaviour change the `Match` fix carries |
    //!
    //! ## ⚠ Anti-vacuity
    //!
    //! `generate_par`'s size ranges were `0..1` — i.e. *exactly zero* elements —
    //! until 2026-07-26, so every property quantified over it was passing over a
    //! two-element set (`models/src/rust/test_utils/test_utils.rs`, module
    //! documentation). Every property test here therefore asserts that its
    //! corpus reached the shape it claims to cover, via
    //! [`assert_generator_not_vacuous`] or an explicit structural check, and the
    //! constructed corpora are checked for the *rendered* feature they exist to
    //! exercise rather than merely for a non-empty string.

    use models::rhoapi::connective::ConnectiveInstance;
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::g_unforgeable::UnfInstance;
    use models::rhoapi::var::{VarInstance, WildcardMsg};
    use models::rhoapi::{
        Bundle, Connective, ConnectiveBody, EAnd, EDiv, EEq, EGt, EGte, EList, ELt, ELte, EMap,
        EMatches, EMethod, EMinus, EMinusMinus, EMod, EMult, ENeg, ENeq, ENot, EOr,
        EPercentPercent, EPlus, EPlusPlus, ESet, ETuple, EVar, EZipper, Expr, GBigRational,
        GFixedPoint, GPrivate, GUnforgeable, KeyValuePair, Match, MatchCase, New, Par, Receive,
        ReceiveBind, Send, Var, VarRef,
    };
    use models::rust::rhoapi_ext::EPathMap;
    use models::rust::test_utils::test_utils::{assert_generator_not_vacuous, generate_par};
    use proptest::prelude::*;

    use super::{NewBindRange, PpNode, PrettyPrinter, UNPRINTABLE_ANY};

    // -----------------------------------------------------------------------
    // the comparison
    // -----------------------------------------------------------------------

    /// Compare BOTH public entry points that traverse: the message printer and
    /// the channel printer. Each gets a *fresh* printer, because the printer
    /// threads state across a whole run and a shared instance would make the
    /// second comparison depend on the first.
    #[track_caller]
    fn agree(what: &str, term: &Par) {
        let driven = PrettyPrinter::new().build_string_from_message(term);
        let recursive = PrettyPrinter::new().oracle_build_string_from_message(term);
        assert_eq!(
            driven, recursive,
            "DIFFERENTIAL FAILED ({what}, build_string_from_message): the explicit-worklist \
             printer and the recursive twin disagree. These bytes are block-resident and \
             replay-compared."
        );

        let driven_channel = PrettyPrinter::new().build_channel_string(term);
        let recursive_channel = PrettyPrinter::new().oracle_build_channel_string(term);
        assert_eq!(
            driven_channel, recursive_channel,
            "DIFFERENTIAL FAILED ({what}, build_channel_string): the explicit-worklist \
             printer and the recursive twin disagree. THIS is the entry point whose output \
             reaches `ProcessedSystemDeploy::Failed` and is compared byte-for-byte in replay."
        );
    }

    // -----------------------------------------------------------------------
    // the proptest corpus
    // -----------------------------------------------------------------------

    /// Clamp the *numeric* fields `generate_par` draws from `any::<i32>()` into
    /// the range a normalizer can actually produce.
    ///
    /// ⚠ This is not a weakening of the corpus: the SHAPES are untouched. It
    /// exists because the printer's own arithmetic — `bound_shift +=
    /// free_count`, `free_shift + level`, `bound_shift - level - 1` — is plain
    /// `i32` addition in *both* forms, so an `any::<i32>()` `free_count`
    /// overflows and panics before either form prints anything. The raw corpus
    /// is still compared, by disposition, in
    /// [`the_two_forms_agree_even_where_the_arithmetic_overflows`].
    fn tame(par: &mut Par) {
        fn var(v: &mut Var) {
            match &mut v.var_instance {
                Some(VarInstance::FreeVar(level)) | Some(VarInstance::BoundVar(level)) => {
                    *level = level.rem_euclid(4)
                }
                Some(VarInstance::Wildcard(_)) | None => {}
            }
        }
        fn opt_var(v: &mut Option<Var>) {
            if let Some(v) = v {
                var(v);
            }
        }
        fn expr(e: &mut Expr) {
            match &mut e.expr_instance {
                Some(ExprInstance::ENotBody(ENot { p: Some(p) })) => tame(p),
                _ => {}
            }
        }
        fn connective(c: &mut Connective) {
            if let Some(ConnectiveInstance::ConnNotBody(p)) = &mut c.connective_instance {
                tame(p);
            }
        }

        for send in &mut par.sends {
            if let Some(chan) = &mut send.chan {
                tame(chan);
            }
            for d in &mut send.data {
                tame(d);
            }
        }
        for receive in &mut par.receives {
            receive.bind_count = receive.bind_count.rem_euclid(4);
            for bind in &mut receive.binds {
                bind.free_count = bind.free_count.rem_euclid(4);
                opt_var(&mut bind.remainder);
                for p in &mut bind.patterns {
                    tame(p);
                }
                if let Some(source) = &mut bind.source {
                    tame(source);
                }
            }
            if let Some(body) = &mut receive.body {
                tame(body);
            }
        }
        for new in &mut par.news {
            new.bind_count = new.bind_count.rem_euclid(4);
            if let Some(p) = &mut new.p {
                tame(p);
            }
            for injected in new.injections.values_mut() {
                tame(injected);
            }
        }
        for e in &mut par.exprs {
            expr(e);
        }
        for m in &mut par.matches {
            if let Some(target) = &mut m.target {
                tame(target);
            }
            for case in &mut m.cases {
                case.free_count = case.free_count.rem_euclid(4);
                if let Some(pattern) = &mut case.pattern {
                    tame(pattern);
                }
                if let Some(source) = &mut case.source {
                    tame(source);
                }
            }
        }
        for bundle in &mut par.bundles {
            if let Some(body) = &mut bundle.body {
                tame(body);
            }
        }
        for c in &mut par.connectives {
            connective(c);
        }
    }

    // ⚠ RETIRED, NOT DELETED — `bound_new_bind_counts`, below, is commented out
    // rather than removed because it is the *measured evidence* for the defect
    // it worked around, and that evidence should stay next to the fix.
    //
    // It clamped `New::bind_count` in the RAW corpus and nothing else. It
    // existed because `PpNode::New` computed
    // `(0..n.bind_count).map(|i| i + bound_shift).collect()` — an UNBOUNDED
    // `Vec<i32>`, in both forms — while `build_variables` was already clamped
    // to `max_var_count = 128`. `generate_par` draws `bind_count` from
    // `any::<i32>()`, so one draw near `i32::MAX` asked for an 8 GiB vector and
    // turned `the_two_forms_agree_even_where_the_arithmetic_overflows` into an
    // out-of-memory test. That is the practical reachability argument for the
    // defect: a property test found it by accident.
    //
    // Both forms now record the introduced indices as an INTERVAL
    // (`PrettyPrinter::new_bind_range`), which allocates nothing per name, so
    // the raw corpus can carry a raw `bind_count` again — which is strictly
    // MORE coverage than the helper allowed, since `bind_count` is now compared
    // at the magnitudes that used to be unreachable. The helper has no
    // remaining caller; keeping it live would re-hide exactly the shapes the
    // fix opened.
    //
    // fn bound_new_bind_counts(par: &mut Par) {
    //     for send in &mut par.sends {
    //         if let Some(chan) = &mut send.chan {
    //             bound_new_bind_counts(chan);
    //         }
    //         for d in &mut send.data {
    //             bound_new_bind_counts(d);
    //         }
    //     }
    //     for receive in &mut par.receives {
    //         for bind in &mut receive.binds {
    //             for p in &mut bind.patterns {
    //                 bound_new_bind_counts(p);
    //             }
    //             if let Some(source) = &mut bind.source {
    //                 bound_new_bind_counts(source);
    //             }
    //         }
    //         if let Some(body) = &mut receive.body {
    //             bound_new_bind_counts(body);
    //         }
    //     }
    //     for new in &mut par.news {
    //         new.bind_count = new.bind_count.rem_euclid(4);
    //         if let Some(p) = &mut new.p {
    //             bound_new_bind_counts(p);
    //         }
    //         for injected in new.injections.values_mut() {
    //             bound_new_bind_counts(injected);
    //         }
    //     }
    //     for e in &mut par.exprs {
    //         if let Some(ExprInstance::ENotBody(ENot { p: Some(p) })) = &mut e.expr_instance {
    //             bound_new_bind_counts(p);
    //         }
    //     }
    //     for m in &mut par.matches {
    //         if let Some(target) = &mut m.target {
    //             bound_new_bind_counts(target);
    //         }
    //         for case in &mut m.cases {
    //             if let Some(pattern) = &mut case.pattern {
    //                 bound_new_bind_counts(pattern);
    //             }
    //             if let Some(source) = &mut case.source {
    //                 bound_new_bind_counts(source);
    //             }
    //         }
    //     }
    //     for bundle in &mut par.bundles {
    //         if let Some(body) = &mut bundle.body {
    //             bound_new_bind_counts(body);
    //         }
    //     }
    //     for c in &mut par.connectives {
    //         if let Some(ConnectiveInstance::ConnNotBody(p)) = &mut c.connective_instance {
    //             bound_new_bind_counts(p);
    //         }
    //     }
    // }

    /// The generator the string-equality property runs over.
    fn tamed_par(depth: usize) -> BoxedStrategy<Par> {
        generate_par(depth)
            .prop_map(|mut p| {
                tame(&mut p);
                p
            })
            .boxed()
    }

    /// Does this term contain a node of every kind the *generator* can produce?
    /// Used as the anti-vacuity predicate, so a future `0..1`-style regression
    /// in `generate_par` fails loudly here instead of silently emptying the
    /// corpus.
    fn reaches_a_receive_with_binds(p: &Par) -> bool {
        p.receives.iter().any(|r| !r.binds.is_empty())
            || p.sends.iter().any(|s| {
                s.data.iter().any(reaches_a_receive_with_binds)
                    || s.chan.as_ref().is_some_and(reaches_a_receive_with_binds)
            })
            || p.news
                .iter()
                .any(|n| n.p.as_ref().is_some_and(reaches_a_receive_with_binds))
            || p.bundles
                .iter()
                .any(|b| b.body.as_ref().is_some_and(reaches_a_receive_with_binds))
    }

    fn reaches_a_match_with_cases(p: &Par) -> bool {
        p.matches.iter().any(|m| !m.cases.is_empty())
            || p.sends.iter().any(|s| {
                s.data.iter().any(reaches_a_match_with_cases)
                    || s.chan.as_ref().is_some_and(reaches_a_match_with_cases)
            })
            || p.news
                .iter()
                .any(|n| n.p.as_ref().is_some_and(reaches_a_match_with_cases))
            || p.bundles
                .iter()
                .any(|b| b.body.as_ref().is_some_and(reaches_a_match_with_cases))
    }

    #[test]
    fn the_corpus_is_not_vacuous() {
        assert_generator_not_vacuous(
            "a `Receive` carrying at least one bind (the fold this conversion sequences)",
            tamed_par(4),
            256,
            reaches_a_receive_with_binds,
        );
        assert_generator_not_vacuous(
            "a `Match` carrying at least one case (the mutation-between-children site)",
            tamed_par(4),
            256,
            reaches_a_match_with_cases,
        );
        assert_generator_not_vacuous(
            "a `New` (the build_variables-then-mutate site)",
            tamed_par(4),
            256,
            |p: &Par| !p.news.is_empty(),
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        /// ★ THE DELIVERABLE: string equality over the structural corpus.
        #[test]
        fn the_two_forms_print_identical_bytes(term in tamed_par(4)) {
            agree("generate_par(4)", &term);
        }

        #[test]
        fn the_two_forms_print_identical_bytes_shallow(term in tamed_par(1)) {
            agree("generate_par(1)", &term);
        }

        #[test]
        fn the_two_forms_print_identical_bytes_mid(term in tamed_par(2)) {
            agree("generate_par(2)", &term);
        }

        /// The RAW corpus — `free_count`, `bind_count` and var levels drawn from
        /// `any::<i32>()`. In the `dev` profile the printer's shared `i32`
        /// arithmetic overflows and panics on most of these; in `release`,
        /// `overflow-checks` are off and it wraps. Either way the two forms must
        /// agree, so what is compared is the *disposition*: the same string, or
        /// the same panic payload.
        ///
        /// ⚠ This is the only place the differential can distinguish "both
        /// panicked" from "both returned", so it is also the only place that
        /// covers the shapes `tame` normalises away.
        ///
        /// ★ `bind_count` is now drawn RAW too. It used to be clamped by
        /// `bound_new_bind_counts` (retired above), because the unbounded
        /// `introduced_news_shift_idx` turned a draw near `i32::MAX` into an
        /// 8 GiB allocation. Both forms record an INTERVAL now
        /// (`PrettyPrinter::new_bind_range`), which allocates nothing per name,
        /// so this property covers `bind_count` at magnitudes — including
        /// negative ones — that the workaround had to exclude.
        ///
        /// ⚠ This is the M4 evidence for the bound, and it is a *live* one:
        /// with either materialised form restored and `bind_count` drawn raw,
        /// this property is SIGKILLed by an 8 GiB cgroup; with the interval it
        /// completes in seconds.
        ///
        /// ⚠⚠ **That sentence is a resource observation, not an assertion.**
        /// `super::representation_mutations` executed the materialising mutant
        /// and found this property's `prop_assert_eq` CANNOT reject it: the
        /// mutant is byte-equivalent, and both sides of the comparison call the
        /// mutated `new_bind_range` (the twin included), so the two strings are
        /// identical. What this test contributes to that row is that it *runs
        /// out of memory*, which is why the row is executed elsewhere as a
        /// slope — `representation_mutations::the_materialised_form_costs_four_\
        /// bytes_per_name` — and why this property is deliberately excluded
        /// from the mutation registry: under that mutant it is the ~8 GiB hang
        /// itself, and a `proptest` failure would additionally persist a seed
        /// into `proptest-regressions/`. Note also that the record disagrees
        /// with itself on the failure MODE — "SIGKILLed" here against ">300 s /
        /// 7.31 GiB, thrashing, `oom_kill 0`" in the table — and neither is
        /// re-run.
        #[test]
        fn the_two_forms_agree_even_where_the_arithmetic_overflows(
            term in generate_par(4)
        ) {
            let driven = quietly(|| PrettyPrinter::new().build_string_from_message(&term));
            let recursive = quietly(|| PrettyPrinter::new().oracle_build_string_from_message(&term));
            prop_assert_eq!(
                driven, recursive,
                "the explicit-worklist printer and the recursive twin took different \
                 dispositions on a term whose counts are outside the normalizer's range"
            );
        }
    }

    // -----------------------------------------------------------------------
    // the corpus, PROMOTED
    // -----------------------------------------------------------------------

    // Each `#[test]` below is one entry of
    // `rholang/proptest-regressions/rust/interpreter/pretty_printer.txt` written out as
    // Rust. They are the terms that once made the two printer forms disagree.
    //
    // # Why they are here as well as in the corpus
    //
    // proptest replays the corpus, but only as an anonymous seed: a failure reports the
    // property's name and a blob of Debug, and nobody can cite it in a bug report or run it
    // alone. A named test gives each counterexample an identity and a place in the ordinary
    // run. It also survives a corpus file being moved, renamed or regenerated — this file
    // is one of the three in the repository whose corpus was untracked until 2026-07-27, so
    // that is not a hypothetical.
    //
    // # ★ The negative `bind_count` is NOT a defect — and the evidence is in this file
    //
    // Entry 2 carries `bind_count: -389915091`. A bind count that counts names bound by a
    // `new` cannot meaningfully be negative, so it deserves an explicit disposition rather
    // than a shrug. It is intended, on three independent grounds:
    //
    // 1. **The wire type admits it.** `New::bind_count` is protobuf `int32` — see
    //    `models/src/lib.rs` and the `self.r.i32()?` reads in
    //    `models/src/rust/rholang/bincode_decoder.rs`. A peer can put any `i32` on the wire, so
    //    the printer's domain is `i32`, not `0..`. Totality over the wire type is a
    //    consensus obligation, not a nicety.
    // 2. **The generator draws it deliberately.** `generate_new` in
    //    `models/src/rust/test_utils/test_utils.rs` uses `any::<i32>()` for `bind_count`,
    //    and this module's own header says so in as many words: the RAW corpus has
    //    "`free_count` / `bind_count` / var levels drawn from `any::<i32>()`, compared by
    //    *disposition* — the two forms must agree even where the shared arithmetic
    //    overflows".
    // 3. **The semantics are already pinned by name.**
    //    [`a_new_with_a_negative_bind_count_binds_nothing`] asserts exactly what a negative
    //    count means: zero names printed, `new  in { .. }` rendered, and one recorded
    //    interval carrying the RAW count and reporting `is_empty()`.
    //
    // The sibling `models/tests/par_corpus/mod.rs` pins `bind_count: i32::MIN` on
    // purpose for the same reason. So this entry is not a bug report; it is the historical
    // record of the input that drove the handling, and promoting it keeps that record
    // executable. **No defect is filed.**
    //
    // # The two assertions
    //
    // 1. ★ the term's `Debug` equals the corpus text, character for character. Without it a
    //    promoted test could quietly assert the differential over a term nobody recorded,
    //    and pass forever. Nothing is normalised here — unlike the MeTTaIL corpora these
    //    terms contain no `UniqueId` and no hash-ordered container, so the comparison is
    //    literal.
    // 2. the differential the corpus exists to protect: both printer entry points, both
    //    forms, byte-identical.

    /// Entry 0 — `cc c8930bdff03532829e702037cb79d414440a203926038d21b137ae60cf253995`.
    ///
    /// A `New` binding three names around an empty `Par`. The minimal shape that reaches the
    /// `build_variables`-then-mutate site at all.
    #[test]
    fn corpus_0_a_new_binding_three_names_around_an_empty_par() {
        let term = models::par_from_default! {
            news: vec![New {
                bind_count: 3,
                p: Some(Par::default()),
                uri: vec![],
                injections: Default::default(),
                locally_free: vec![],
            }],
            ..Default::default()
        };

        assert_eq!(
            format!("{term:?}"),
            "Par { sends: [], receives: [], news: [New { bind_count: 3, p: Some(Par { sends: \
             [], receives: [], news: [], exprs: [], matches: [], unforgeables: [], bundles: \
             [], connectives: [], conditionals: [], locally_free: [], connective_used: false \
             }), uri: [], injections: {}, locally_free: [] }], exprs: [], matches: [], \
             unforgeables: [], bundles: [], connectives: [], conditionals: [], locally_free: \
             [], connective_used: false }",
            "the reconstructed term is not the one the corpus recorded"
        );

        agree("corpus entry 0", &term);
    }

    /// Entry 1 — `cc fbfd3af8dad13c7fa0a1e3b9b89f5558d2251c881da404422fbc05048382708f`.
    ///
    /// The same `New` shape, but reached THROUGH a `Bundle`. The bundle matters: it is a
    /// separate descent path in both printers, and a `New` nested under one exercises the
    /// interaction between bundle framing and the name-shift bookkeeping.
    #[test]
    fn corpus_1_a_new_reached_through_a_bundle() {
        let term = models::par_from_default! {
            bundles: vec![Bundle {
                body: Some(models::par_from_default! {
                    news: vec![New {
                        bind_count: 1,
                        p: Some(Par::default()),
                        uri: vec![],
                        injections: Default::default(),
                        locally_free: vec![],
                    }],
                    ..Default::default()
                }),
                write_flag: false,
                read_flag: false,
            }],
            ..Default::default()
        };

        assert_eq!(
            format!("{term:?}"),
            "Par { sends: [], receives: [], news: [], exprs: [], matches: [], unforgeables: \
             [], bundles: [Bundle { body: Some(Par { sends: [], receives: [], news: [New { \
             bind_count: 1, p: Some(Par { sends: [], receives: [], news: [], exprs: [], \
             matches: [], unforgeables: [], bundles: [], connectives: [], conditionals: [], \
             locally_free: [], connective_used: false }), uri: [], injections: {}, \
             locally_free: [] }], exprs: [], matches: [], unforgeables: [], bundles: [], \
             connectives: [], conditionals: [], locally_free: [], connective_used: false }), \
             write_flag: false, read_flag: false }], connectives: [], conditionals: [], \
             locally_free: [], connective_used: false }",
            "the reconstructed term is not the one the corpus recorded"
        );

        agree("corpus entry 1", &term);
    }

    /// Entry 2 — `cc 6fd0823e04a91afde8ce0daeaab0d97644eb8f3ced4a2d785289954b1a1203e0`.
    ///
    /// ★ The `bind_count: -389915091` entry. See this section's header: the value is
    /// intended, the printer's domain is the whole of `i32`, and
    /// [`a_new_with_a_negative_bind_count_binds_nothing`] pins what it means. What makes
    /// THIS term worth its own test is the NESTING — a negative outer count wrapped around a
    /// well-formed inner `New`, so the outer's empty interval has to not disturb the inner's
    /// shift bookkeeping.
    #[test]
    fn corpus_2_a_negative_bind_count_wrapped_around_a_well_formed_new() {
        let term = models::par_from_default! {
            news: vec![New {
                bind_count: -389_915_091,
                p: Some(models::par_from_default! {
                    news: vec![New {
                        bind_count: 1,
                        p: Some(Par::default()),
                        uri: vec![],
                        injections: Default::default(),
                        locally_free: vec![],
                    }],
                    ..Default::default()
                }),
                uri: vec![],
                injections: Default::default(),
                locally_free: vec![],
            }],
            ..Default::default()
        };

        assert_eq!(
            format!("{term:?}"),
            "Par { sends: [], receives: [], news: [New { bind_count: -389915091, p: Some(Par \
             { sends: [], receives: [], news: [New { bind_count: 1, p: Some(Par { sends: [], \
             receives: [], news: [], exprs: [], matches: [], unforgeables: [], bundles: [], \
             connectives: [], conditionals: [], locally_free: [], connective_used: false }), \
             uri: [], injections: {}, locally_free: [] }], exprs: [], matches: [], \
             unforgeables: [], bundles: [], connectives: [], conditionals: [], locally_free: \
             [], connective_used: false }), uri: [], injections: {}, locally_free: [] }], \
             exprs: [], matches: [], unforgeables: [], bundles: [], connectives: [], \
             conditionals: [], locally_free: [], connective_used: false }",
            "the reconstructed term is not the one the corpus recorded"
        );

        agree("corpus entry 2", &term);

        // And the documented semantics, asserted HERE so this test states what the value
        // means rather than merely surviving it: the outer `New` binds nothing.
        let mut printer = PrettyPrinter::new();
        let printed = printer.build_string_from_message(&term);
        assert!(
            printed.starts_with("new  in {"),
            "a negative outer `bind_count` printed names between `new` and `in`: {printed:?}"
        );
        assert!(
            printer.news_shift_indices[0].is_empty(),
            "the outer `New`'s interval is not empty: {:?}",
            printer.news_shift_indices[0]
        );
    }

    /// Run `body`, returning `Ok(value)` or `Err(panic payload)`, with the
    /// panic hook silenced for the duration so that an expected overflow does
    /// not fill the test log with backtraces.
    ///
    /// ⚠ `set_hook` is process-global. Under `cargo nextest` (the mandated
    /// runner) each test is its own process, so there is nothing to race with.
    /// Under a threaded `cargo test` the only consequence is that a *different*
    /// test panicking inside this window prints no message; its failure is
    /// still reported, and in the `release` profile — which is what CI runs —
    /// overflow checks are off, so this path does not even fire.
    pub(super) fn quietly<T>(body: impl FnOnce() -> T) -> Result<T, String> {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body));
        std::panic::set_hook(previous);
        outcome.map_err(|payload| {
            payload
                .downcast_ref::<&str>()
                .map(|s| (*s).to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| String::from("<non-string panic payload>"))
        })
    }

    // -----------------------------------------------------------------------
    // constructed corpora
    // -----------------------------------------------------------------------

    fn gint(i: i64) -> Par {
        models::par_from_default! {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::GInt(i)),
            }],
            ..Default::default()
        }
    }

    fn gstring(s: &str) -> Par {
        models::par_from_default! {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::GString(s.to_string())),
            }],
            ..Default::default()
        }
    }

    fn bound_var(level: i32) -> Par {
        models::par_from_default! {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::EVarBody(EVar {
                    v: Some(Var {
                        var_instance: Some(VarInstance::BoundVar(level)),
                    }),
                })),
            }],
            ..Default::default()
        }
    }

    fn free_var(level: i32) -> Par {
        models::par_from_default! {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::EVarBody(EVar {
                    v: Some(Var {
                        var_instance: Some(VarInstance::FreeVar(level)),
                    }),
                })),
            }],
            ..Default::default()
        }
    }

    fn expr_par(instance: ExprInstance) -> Par {
        models::par_from_default! {
            exprs: vec![Expr {
                expr_instance: Some(instance),
            }],
            ..Default::default()
        }
    }

    /// A `New` that introduces `bind_count` names over `body`. Used to move
    /// `bound_shift` *mid-traversal*, which is the whole point of the
    /// two-bind corpus below.
    pub(super) fn new_par(bind_count: i32, body: Par) -> Par {
        models::par_from_default! {
            news: vec![New {
                bind_count,
                p: Some(body),
                uri: vec![],
                injections: Default::default(),
                locally_free: vec![],
            }],
            ..Default::default()
        }
    }

    /// ★ Every one of the ten [`PpNode`] variants, each through the entry point
    /// that actually reaches it.
    #[test]
    fn every_node_kind() {
        // Par / Expr / Send / Receive / New / Match / Bundle / Connective /
        // Unforgeable, all inside ONE term so that the state they thread
        // interacts.
        let term = models::par_from_default! {
            bundles: vec![Bundle {
                body: Some(gint(1)),
                write_flag: true,
                read_flag: false,
            }],
            sends: vec![Send {
                chan: Some(gstring("chan")),
                data: vec![gint(2), bound_var(0)],
                persistent: true,
                locally_free: vec![],
                connective_used: false,
            }],
            receives: vec![Receive {
                binds: vec![ReceiveBind {
                    patterns: vec![free_var(0)],
                    source: Some(gstring("src")),
                    remainder: Some(Var {
                        var_instance: Some(VarInstance::Wildcard(WildcardMsg {})),
                    }),
                    free_count: 1,
                }],
                body: Some(gint(3)),
                persistent: false,
                peek: true,
                bind_count: 1,
                locally_free: vec![],
                connective_used: false,
                condition: None,
            }],
            news: vec![New {
                bind_count: 2,
                p: Some(bound_var(0)),
                uri: vec![],
                injections: Default::default(),
                locally_free: vec![],
            }],
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::GBool(true)),
            }],
            matches: vec![Match {
                target: Some(gint(4)),
                cases: vec![MatchCase {
                    pattern: Some(free_var(0)),
                    source: Some(gint(5)),
                    free_count: 1,
                    guard: None,
                }],
                locally_free: vec![],
                connective_used: false,
            }],
            unforgeables: vec![GUnforgeable {
                unf_instance: Some(UnfInstance::GPrivateBody(GPrivate {
                    id: vec![0xde, 0xad],
                })),
            }],
            connectives: vec![
                Connective {
                    connective_instance: Some(ConnectiveInstance::ConnAndBody(ConnectiveBody {
                        ps: vec![gint(6), gint(7)],
                    })),
                },
                Connective {
                    connective_instance: Some(ConnectiveInstance::ConnOrBody(ConnectiveBody {
                        ps: vec![gint(8), gint(9)],
                    })),
                },
                Connective {
                    connective_instance: Some(ConnectiveInstance::ConnNotBody(gint(10))),
                },
                Connective {
                    connective_instance: Some(ConnectiveInstance::VarRefBody(VarRef {
                        index: 0,
                        depth: 0,
                    })),
                },
                Connective {
                    connective_instance: Some(ConnectiveInstance::ConnBool(true)),
                },
                Connective {
                    connective_instance: Some(ConnectiveInstance::ConnInt(true)),
                },
                Connective {
                    connective_instance: Some(ConnectiveInstance::ConnString(true)),
                },
                Connective {
                    connective_instance: Some(ConnectiveInstance::ConnUri(true)),
                },
                Connective {
                    connective_instance: Some(ConnectiveInstance::ConnByteArray(true)),
                },
                Connective {
                    connective_instance: None,
                },
            ],
            ..Default::default()
        };

        // ANTI-VACUITY: the fixture really does carry all eight categories.
        assert!(
            !term.bundles.is_empty()
                && !term.sends.is_empty()
                && !term.receives.is_empty()
                && !term.news.is_empty()
                && !term.exprs.is_empty()
                && !term.matches.is_empty()
                && !term.unforgeables.is_empty()
                && !term.connectives.is_empty(),
            "the every_node_kind fixture lost a category"
        );
        agree("every_node_kind", &term);

        // `PpNode::Var` is reachable only through `build_string_from_var`,
        // which is a shared leaf, and `PpNode::Unprintable` only through
        // `build_string_from_node`. Both are driven directly.
        for level in [0i32, 1, 2] {
            let v = Var {
                var_instance: Some(VarInstance::BoundVar(level)),
            };
            assert_eq!(
                PrettyPrinter::new().build_string_from_message(&v),
                PrettyPrinter::new().oracle_build_string_from_message(&v),
                "DIFFERENTIAL FAILED (PpNode::Var)"
            );
        }
        assert_eq!(
            PrettyPrinter::new().build_string_from_node(PpNode::Unprintable(UNPRINTABLE_ANY)),
            PrettyPrinter::new()
                .oracle_build_string_from_node(PpNode::Unprintable(UNPRINTABLE_ANY)),
            "DIFFERENTIAL FAILED (PpNode::Unprintable)"
        );
        // ⚠ And the ONE path with no enclosing catch frame: the drive itself
        // must return `Err`, which is what the public wrapper turns into the
        // UNCAPPED fallback.
        assert_eq!(
            PrettyPrinter::new().build_string_from_node(PpNode::Unprintable(UNPRINTABLE_ANY)),
            "<unprintable: Bug found: Attempt to print unknown prost::Message type: Any { .. }>",
            "the no-enclosing-frame error path stopped producing the pinned fallback"
        );
    }

    /// ★ Every `ExprInstance` arm, including the three that re-enter the drive.
    #[test]
    fn every_expr_arm() {
        let a = || Some(gint(11));
        let b = || Some(gint(-3));
        let arms: Vec<ExprInstance> = vec![
            ExprInstance::ENegBody(ENeg { p: a() }),
            ExprInstance::ENotBody(ENot { p: a() }),
            ExprInstance::EMultBody(EMult { p1: a(), p2: b() }),
            ExprInstance::EDivBody(EDiv { p1: a(), p2: b() }),
            ExprInstance::EModBody(EMod { p1: a(), p2: b() }),
            ExprInstance::EPercentPercentBody(EPercentPercent { p1: a(), p2: b() }),
            ExprInstance::EPlusBody(EPlus { p1: a(), p2: b() }),
            ExprInstance::EPlusPlusBody(EPlusPlus { p1: a(), p2: b() }),
            ExprInstance::EMinusBody(EMinus { p1: a(), p2: b() }),
            ExprInstance::EMinusMinusBody(EMinusMinus { p1: a(), p2: b() }),
            ExprInstance::EAndBody(EAnd { p1: a(), p2: b() }),
            ExprInstance::EOrBody(EOr { p1: a(), p2: b() }),
            ExprInstance::EEqBody(EEq { p1: a(), p2: b() }),
            ExprInstance::ENeqBody(ENeq { p1: a(), p2: b() }),
            ExprInstance::EGtBody(EGt { p1: a(), p2: b() }),
            ExprInstance::EGteBody(EGte { p1: a(), p2: b() }),
            ExprInstance::ELtBody(ELt { p1: a(), p2: b() }),
            ExprInstance::ELteBody(ELte { p1: a(), p2: b() }),
            ExprInstance::EMatchesBody(EMatches {
                target: a(),
                pattern: b(),
            }),
            // Collections, with and without a remainder — the three-way case
            // analysis in the `Bracketed` combine.
            ExprInstance::EListBody(EList {
                ps: vec![gint(1), gstring("two")],
                locally_free: vec![],
                connective_used: false,
                remainder: None,
            }),
            ExprInstance::EListBody(EList {
                ps: vec![gint(1)],
                locally_free: vec![],
                connective_used: false,
                remainder: Some(Var {
                    var_instance: Some(VarInstance::FreeVar(0)),
                }),
            }),
            ExprInstance::EListBody(EList {
                ps: vec![],
                locally_free: vec![],
                connective_used: false,
                remainder: Some(Var {
                    var_instance: Some(VarInstance::BoundVar(0)),
                }),
            }),
            ExprInstance::ETupleBody(ETuple {
                ps: vec![gint(1), gint(2), gint(3)],
                locally_free: vec![],
                connective_used: false,
            }),
            ExprInstance::ESetBody(ESet {
                ps: vec![gint(3), gint(1), gint(2)],
                locally_free: vec![],
                connective_used: false,
                remainder: None,
            }),
            ExprInstance::ESetBody(ESet {
                ps: vec![gint(3), gint(1)],
                locally_free: vec![],
                connective_used: false,
                remainder: Some(Var {
                    var_instance: Some(VarInstance::Wildcard(WildcardMsg {})),
                }),
            }),
            ExprInstance::EMapBody(EMap {
                kvs: vec![
                    KeyValuePair {
                        key: Some(gstring("c")),
                        value: Some(gint(3)),
                    },
                    KeyValuePair {
                        key: Some(gstring("a")),
                        value: Some(gint(1)),
                    },
                ],
                locally_free: vec![],
                connective_used: false,
                remainder: None,
            }),
            ExprInstance::EPathmapBody(EPathMap::new(vec![gint(1), gint(2)], vec![], false, None)),
            ExprInstance::EPathmapBody(EPathMap::new(
                vec![gint(1)],
                vec![],
                false,
                Some(Var {
                    var_instance: Some(VarInstance::FreeVar(1)),
                }),
            )),
            ExprInstance::EPathmapBody(EPathMap::new_map(
                [(gstring("key-b"), gint(2)), (gstring("key-a"), gint(1))],
                vec![],
                false,
                None,
            )),
            ExprInstance::EZipperBody(EZipper {
                pathmap: Some(EPathMap::new(vec![gint(7)], vec![], false, None)),
                current_path: vec![],
                is_write_zipper: false,
                locally_free: vec![],
                connective_used: false,
                cursor_kind: 0,
            }),
            ExprInstance::EZipperBody(EZipper {
                pathmap: Some(EPathMap::new(vec![gint(7)], vec![], false, None)),
                // Deliberately NOT a valid encoded segment: the arm's `Err`
                // branch renders it as hex, which is the path the fallback in
                // `inline_expr` takes.
                current_path: vec![vec![0xff, 0x01]],
                is_write_zipper: true,
                locally_free: vec![],
                connective_used: false,
                cursor_kind: 1,
            }),
            ExprInstance::EZipperBody(EZipper {
                pathmap: Some(EPathMap::new_map(
                    [(gstring("key"), gint(7))],
                    vec![],
                    false,
                    None,
                )),
                current_path: vec![],
                is_write_zipper: true,
                locally_free: vec![],
                connective_used: false,
                cursor_kind: 0,
            }),
            // ★ The DISCRIMINATOR, driven on all five rendering rows over ONE
            // fixed `current_path`, so the twin has to agree about the arm and
            // not merely about the segments. `03 0a` is `encode_trie_segment(5)`
            // — the segment shared by the bare entry `5` (key `03 0a`) and the
            // singleton list `[5]` (key `03 0a 00`), which is exactly the pair
            // `cursor_kind` exists to tell apart.
            ExprInstance::EZipperBody(EZipper {
                pathmap: Some(EPathMap::new(vec![gint(5)], vec![], false, None)),
                current_path: vec![vec![0x03, 0x0a]],
                is_write_zipper: false,
                locally_free: vec![],
                connective_used: false,
                cursor_kind: 0, // Split  -> [5]
            }),
            ExprInstance::EZipperBody(EZipper {
                pathmap: Some(EPathMap::new(vec![gint(5)], vec![], false, None)),
                current_path: vec![vec![0x03, 0x0a]],
                is_write_zipper: false,
                locally_free: vec![],
                connective_used: false,
                cursor_kind: 1, // Bare   -> 5
            }),
            ExprInstance::EZipperBody(EZipper {
                pathmap: Some(EPathMap::new(vec![gint(5)], vec![], false, None)),
                current_path: vec![vec![0x03, 0x0a]],
                is_write_zipper: false,
                locally_free: vec![],
                connective_used: false,
                cursor_kind: 2, // Prefix -> [5]?
            }),
            // A wire value no `CursorKind` decodes. `cursor_kind` is a peer
            // -controlled field, so the printer is handed these; it must render
            // them rather than assert, because it IS the error path.
            ExprInstance::EZipperBody(EZipper {
                pathmap: Some(EPathMap::new(vec![gint(5)], vec![], false, None)),
                current_path: vec![vec![0x03, 0x0a]],
                is_write_zipper: false,
                locally_free: vec![],
                connective_used: false,
                cursor_kind: 99,
            }),
            // A `Bare` cursor at a depth no bare entry inhabits (a bare key is
            // exactly ONE segment), which is representable on the wire.
            ExprInstance::EZipperBody(EZipper {
                pathmap: Some(EPathMap::new(vec![gint(5)], vec![], false, None)),
                current_path: vec![vec![0x03, 0x0a], vec![0x03, 0x0c]],
                is_write_zipper: false,
                locally_free: vec![],
                connective_used: false,
                cursor_kind: 1,
            }),
            ExprInstance::EMethodBody(EMethod {
                method_name: "nth".to_string(),
                target: Some(expr_par(ExprInstance::EListBody(EList {
                    ps: vec![gint(1), gint(2)],
                    locally_free: vec![],
                    connective_used: false,
                    remainder: None,
                }))),
                arguments: vec![gint(0), gstring("x")],
                locally_free: vec![],
                connective_used: false,
            }),
            ExprInstance::EVarBody(EVar {
                v: Some(Var {
                    var_instance: Some(VarInstance::BoundVar(0)),
                }),
            }),
            ExprInstance::EVarBody(EVar {
                v: Some(Var {
                    var_instance: Some(VarInstance::Wildcard(WildcardMsg {})),
                }),
            }),
            ExprInstance::GBool(false),
            ExprInstance::GInt(-42),
            ExprInstance::GString("with \"quotes\"".to_string()),
            ExprInstance::GUri("rho:io:stdout".to_string()),
            ExprInstance::GByteArray(vec![0x00, 0xff, 0x10]),
            ExprInstance::GDouble(2.5f64.to_bits()),
            ExprInstance::GDouble(3.0f64.to_bits()),
            ExprInstance::GBigInt(vec![0x01, 0x00]),
            ExprInstance::GBigRat(GBigRational {
                numerator: vec![0x07],
                denominator: vec![0x02],
            }),
            ExprInstance::GFixedPoint(GFixedPoint {
                unscaled: vec![0x7b],
                scale: 0,
            }),
            ExprInstance::GFixedPoint(GFixedPoint {
                unscaled: vec![0x7b],
                scale: 2,
            }),
            ExprInstance::GFixedPoint(GFixedPoint {
                unscaled: vec![0xff, 0x85],
                scale: 4,
            }),
        ];

        // ANTI-VACUITY: the printer's own dispatch has 36 arms; a fixture that
        // silently lost half of them would still pass every comparison below.
        assert!(
            arms.len() >= 36,
            "the every_expr_arm fixture covers only {} constructions",
            arms.len()
        );

        // ★ ANTI-VACUITY for the `cursor_kind` family specifically. `agree`
        // proves the two printers say the SAME thing; it says nothing about
        // whether that thing is discriminating. Two printers that both dropped
        // the arm would agree perfectly. So: the fixture must contain zippers
        // that differ ONLY in `cursor_kind`, and the printer must give them
        // DIFFERENT strings.
        let cursor_kind_family: Vec<&ExprInstance> = arms
            .iter()
            .filter(|instance| match instance {
                ExprInstance::EZipperBody(z) => z.current_path == vec![vec![0x03, 0x0au8]],
                _ => false,
            })
            .collect();
        assert_eq!(
            cursor_kind_family.len(),
            4,
            "the fixture must hold four zippers agreeing on `current_path` and \
             differing only on `cursor_kind`; it holds {}",
            cursor_kind_family.len()
        );
        let rendered: Vec<String> = cursor_kind_family
            .iter()
            .map(|instance| {
                PrettyPrinter::new().build_string_from_message(&expr_par((*instance).clone()))
            })
            .collect();
        for (i, left) in rendered.iter().enumerate() {
            for right in rendered.iter().skip(i + 1) {
                assert_ne!(
                    left, right,
                    "★ two zippers that are `!=` and hash differently print \
                     identically. `cursor_kind` is half the cursor: it decides \
                     whether the segments name the bare entry `5` (key `03 0a`) \
                     or the singleton list `[5]` (key `03 0a 00`), which one map \
                     may hold at the same time."
                );
            }
        }

        for (i, instance) in arms.into_iter().enumerate() {
            let bare = expr_par(instance.clone());
            agree(&format!("expr arm #{i} (bare)"), &bare);

            // Again with the printer in a NON-default state, so any arm that
            // reads `free_shift` / `bound_shift` / `base_id` is compared under
            // a shift rather than at zero.
            let shifted = models::par_from_default! {
                news: vec![New {
                    bind_count: 2,
                    p: Some(expr_par(instance)),
                    uri: vec![],
                    injections: Default::default(),
                    locally_free: vec![],
                }],
                ..Default::default()
            };
            agree(&format!("expr arm #{i} (under a New)"), &shifted);
        }

        // The absent instance, which is its own arm.
        agree("expr arm: absent instance", &models::par_from_default! {
            exprs: vec![Expr {
                expr_instance: None,
            }],
            ..Default::default()
        });
    }

    // -----------------------------------------------------------------------
    // ★ the shape that distinguishes a SEQUENCED bound_shift from a
    //   precomputed one
    // -----------------------------------------------------------------------

    /// Two binds whose patterns bind *different* counts, with a `New` inside
    /// the first bind's pattern.
    ///
    /// ## Why this is the only shape that matters
    ///
    /// The `Receive` fold sets, per bind `i`:
    ///
    /// ```text
    ///   free_shift = bound_shift + previous_free_i
    /// ```
    ///
    /// `previous_free_i` is a static prefix sum of `free_count`, but
    /// `bound_shift` at that instant is **whatever bind `i-1`'s sub-renders
    /// left behind** — bind 0 sets it to 0, and a `New` inside bind 0's pattern
    /// raises it by its `bind_count`. A machine that precomputed `free_shift`
    /// would use bind 0's incoming value for every bind and print
    /// `free0` where this prints `free3`.
    ///
    /// So the fixture must have (a) at least two binds, (b) different
    /// `free_count`s, and (c) a `bound_shift` mutation inside the first bind.
    /// All three are asserted below before the comparison runs.
    #[test]
    fn two_binds_that_bind_different_counts() {
        let receive = Receive {
            binds: vec![
                ReceiveBind {
                    // (c): the `New` moves `bound_shift` from 0 to 3 while
                    // bind 0 renders, so bind 1 does NOT see bind 0's value.
                    patterns: vec![new_par(3, bound_var(0)), free_var(0)],
                    source: Some(gstring("first")),
                    remainder: None,
                    free_count: 2,
                },
                ReceiveBind {
                    patterns: vec![
                        free_var(0),
                        expr_par(ExprInstance::EListBody(EList {
                            ps: vec![gint(1)],
                            locally_free: vec![],
                            connective_used: false,
                            remainder: Some(Var {
                                var_instance: Some(VarInstance::FreeVar(0)),
                            }),
                        })),
                    ],
                    source: Some(new_par(1, gstring("second"))),
                    remainder: None,
                    free_count: 1,
                },
                ReceiveBind {
                    patterns: vec![free_var(2)],
                    source: Some(gstring("third")),
                    remainder: None,
                    free_count: 5,
                },
            ],
            body: Some(bound_var(0)),
            persistent: false,
            peek: false,
            bind_count: 8,
            locally_free: vec![],
            connective_used: false,
            condition: None,
        };

        // ANTI-VACUITY (a)+(b): >= 2 binds, and the free counts differ.
        assert!(receive.binds.len() >= 2, "fewer than two binds");
        let counts: Vec<i32> = receive.binds.iter().map(|b| b.free_count).collect();
        assert!(
            counts.windows(2).any(|w| w[0] != w[1]),
            "every bind binds the same number of names — this fixture cannot distinguish a \
             sequenced `bound_shift` from a precomputed one"
        );
        // ANTI-VACUITY (c): a `New` really is inside bind 0's pattern.
        assert!(
            receive.binds[0].patterns.iter().any(|p| !p.news.is_empty()),
            "no `New` inside the first bind's pattern — `bound_shift` would not move \
             mid-fold and the fixture would be vacuous"
        );

        let term = models::par_from_default! {
            receives: vec![receive],
            ..Default::default()
        };
        agree("two_binds_that_bind_different_counts", &term);

        // ★ THE PIN, blessed from the RECURSIVE twin. `agree` above proves the
        // two forms match each other; this literal proves they match the
        // PRE-CONVERSION bytes, so a change that moved BOTH together would
        // still be caught.
        //
        // The derivation, bind by bind — `free_shift = bound_shift +
        // previous_free`, `free_id = rotate(base_id)`, `base_id =
        // increment(base_id)`:
        //
        // | bind | incoming `bound_shift` | `previous_free` | `free_shift` | `free_id` | first free var |
        // |---|---|---|---|---|---|
        // | 0 | 0 (initial) | 0 | 0 | `x` | `x0` |
        // | 1 | **3** (left by bind 0's `new`) | 2 | **5** | `y` | **`y5`** |
        // | 2 | **1** (left by bind 1's source `new`) | 3 | **4** | `z` | `z6` (level 2) |
        //
        // ★ `y5` is the discriminating character. A machine that precomputed
        // `free_shift` would use bind 0's incoming `bound_shift` (0) for every
        // bind and print `y2` there. Likewise the body's `a7` is
        // `bound_shift = 0 + totally_free (2 + 1 + 5) = 8`, minus level 0,
        // minus 1.
        let printed = PrettyPrinter::new().build_string_from_message(&term);
        assert_eq!(
            printed,
            concat!(
                "for( @{new y0, y1, y2 in { y2 }}, @{x0} <- @{\"first\"}",
                "  & @{y5}, @{[1...free5]} <- @{new z0 in { \"second\" }}",
                "  & @{z6} <- @{\"third\"} ) {\n",
                "  a7\n",
                "}"
            ),
            "the SEQUENCED `bound_shift` rendering moved — see the derivation above this \
             assertion. These bytes are block-resident and replay-compared."
        );
        // ANTI-VACUITY: the discriminating character really is in the output.
        assert!(
            printed.contains("@{y5}"),
            "bind 1's first free variable is not `y5`, so this fixture no longer \
             distinguishes a sequenced `bound_shift` from a precomputed one. Rendered: \
             {printed}"
        );
    }

    // -----------------------------------------------------------------------
    // ★ the `where` guard — a field neither printer used to read
    // -----------------------------------------------------------------------

    /// `Receive.condition` reaches the page, in the body's environment, after
    /// every bind.
    ///
    /// ## Why this fixture is constructed rather than generated
    ///
    /// `generate_par` sets `condition: None` unconditionally
    /// (`models/src/rust/test_utils/test_utils.rs`), so **no** proptest corpus
    /// in this module reaches a guard. That is not a reason to trust the
    /// property; it is the reason this test exists. Every structural assertion
    /// below is checked against the rendered string before `agree` runs, so the
    /// fixture cannot pass by failing to reach the feature.
    ///
    /// ## The three properties, and what each would look like if it broke
    ///
    /// | property | how it breaks | what the reader sees |
    /// |---|---|---|
    /// | the guard is rendered at all | `receive_guard` returns `None`, or the clause is dropped from the `format!` | a guarded receive printed as an unguarded one — plausible, and admitting strictly more |
    /// | it renders in the **body's** de Bruijn environment | the render is sequenced before `Mutate(AddBoundShift)` | the same binder named two ways in one term |
    /// | it renders after **every** bind | the clause is emitted per bind, or before the last | `where` attached to a bind, which the grammar has no production for |
    ///
    /// The second is also a [`super::drive::DriveMutation`]
    /// (`ReceiveConditionBeforeBoundShift`) and is separated by the executed
    /// mutation table; asserting it here as well is what names the *rendered
    /// consequence* rather than only the disagreement.
    #[test]
    fn a_receive_with_a_where_guard() {
        // Two binds, so "after every bind" is distinguishable from "after the
        // first". The guard names a variable BOTH binds contribute to, which is
        // what makes the environment observable.
        let term = models::par_from_default! {
            receives: vec![Receive {
                binds: vec![
                    ReceiveBind {
                        patterns: vec![free_var(0)],
                        source: Some(gstring("left")),
                        remainder: None,
                        free_count: 1,
                    },
                    ReceiveBind {
                        patterns: vec![free_var(0)],
                        source: Some(gstring("right")),
                        remainder: None,
                        free_count: 1,
                    },
                ],
                body: Some(bound_var(0)),
                persistent: false,
                peek: false,
                bind_count: 2,
                locally_free: vec![],
                connective_used: false,
                condition: Some(expr_par(ExprInstance::EGtBody(EGt {
                    p1: Some(bound_var(1)),
                    p2: Some(gint(5)),
                }))),
            }],
            ..Default::default()
        };

        agree("a_receive_with_a_where_guard", &term);

        let printed = PrettyPrinter::new().build_string_from_message(&term);

        // ANTI-VACUITY 1: the feature is reached at all.
        assert!(
            printed.contains(" where "),
            "the fixture rendered no `where` clause, so every assertion below is about a \
             term that does not exercise the guard. Rendered: {printed}"
        );

        // ANTI-VACUITY 2 + the ENVIRONMENT property, derived rather than
        // blessed:
        //
        // | slot | shift | rendered |
        // |---|---|---|
        // | bind 0's `FreeVar(0)` | `free_shift = 0 + 0` | `x0` |
        // | bind 1's `FreeVar(0)` | `free_shift = 0 + 1` (bind 0's `free_count`) | `y1` |
        // | the guard's `BoundVar(1)` | `bound_shift = 2`, so `2 - 1 - 1` | `z0` |
        // | the body's `BoundVar(0)` | `bound_shift = 2`, so `2 - 0 - 1` | `z1` |
        //
        // ★ `z0` and `z1` are the discriminating characters, and BOTH are
        // asserted. `z0` alone cannot tell "the guard renders in the body's
        // environment" from "the guard renders in *some* environment"; the pair
        // pins that the two share one `bound_shift` of 2. Rendering the guard
        // before the interposed `AddBoundShift` gives `bound_shift = 0` and
        // prints `z-2` there instead — which is what
        // `DriveMutation::ReceiveConditionBeforeBoundShift` does.
        assert_eq!(
            printed,
            concat!(
                "for( @{x0} <- @{\"left\"}  & @{y1} <- @{\"right\"} where z0 > 5 ) {\n",
                "  z1\n",
                "}"
            ),
            "the guarded rendering moved. These bytes are block-resident and \
             replay-compared."
        );

        // ANTI-VACUITY 3 + the PLACEMENT property: the clause follows the LAST
        // bind, which is where the grammar puts it
        // (`receipt: conc1(bind) optional('where' guard)`).
        let where_at = printed.find(" where ").expect("asserted above");
        let last_bind_at = printed
            .find("@{\"right\"}")
            .expect("the second bind's source must be rendered");
        assert!(
            where_at > last_bind_at,
            "the `where` clause precedes the last bind, which is not a receipt-level \
             placement. Rendered: {printed}"
        );
        assert_eq!(
            printed.matches(" where ").count(),
            1,
            "the clause is emitted more than once — it belongs to the receipt, not to a \
             bind. Rendered: {printed}"
        );

        // ★ The EMPTY guard is not a guard, and that is a decision both
        // decision sites already make (`Matcher::check_commit` and
        // `Reduce::eval_receive` both treat `Some(Par::default())` as "commit").
        // Without this the printer could agree with itself while disagreeing
        // with the reducer about what the term means — and every unguarded
        // receive's bytes would move, which is the opposite of confining them.
        let mut empty_guard = term.clone();
        empty_guard.receives[0].condition = Some(Par::default());
        let printed_empty = PrettyPrinter::new().build_string_from_message(&empty_guard);
        let mut no_guard = term.clone();
        no_guard.receives[0].condition = None;
        assert_eq!(
            printed_empty,
            PrettyPrinter::new().build_string_from_message(&no_guard),
            "`condition: Some(Par::default())` rendered differently from `condition: None`, \
             but both decision sites commit unconditionally on either. Rendered: \
             {printed_empty}"
        );
        assert!(
            !printed_empty.contains("where"),
            "an empty guard grew a `where` clause: {printed_empty}"
        );
        agree("a_receive_with_an_empty_where_guard", &empty_guard);
        agree("a_receive_with_no_where_guard", &no_guard);

        // ★ The OTHER `format!` arm. `ReceiveK` renders `for( … ) {}` when the
        // body renders to the EMPTY STRING — which is not the same thing as an
        // empty `Par`: `Par::default()` renders `"Nil"`, three bytes, and takes
        // the arm above. The only leaf that renders to nothing is a
        // `Connective` with no instance (`descend_connective`'s `None` arm
        // pushes `String::new()`), so that is what reaches it. It is a separate
        // line of code and was separately capable of dropping the clause.
        let mut empty_body = term.clone();
        empty_body.receives[0].body = Some(models::par_from_default! {
            connectives: vec![Connective {
                connective_instance: None,
            }],
            ..Default::default()
        });
        let printed_empty_body = PrettyPrinter::new().build_string_from_message(&empty_body);
        assert!(
            printed_empty_body.ends_with(" ) {}"),
            "this fixture does not reach the empty-body arm, so it is not testing it: \
             {printed_empty_body}"
        );
        assert!(
            printed_empty_body.contains(" where "),
            "the empty-body arm dropped the guard: {printed_empty_body}"
        );
        agree("a_guarded_receive_with_an_empty_body", &empty_body);
    }

    // -----------------------------------------------------------------------
    // ★ `New::bind_count` is attacker-controlled, and it used to be an
    //   unbounded allocation
    // -----------------------------------------------------------------------

    /// `New::bind_count` is a `sint32` an attacker controls in a hand-built
    /// `Par`, and the printer is reachable from untrusted input through
    /// `rho:io:stdout` -> `build_channel_string`. `PpNode::New` used to compute
    /// `(0..n.bind_count).map(|i| i + bound_shift).collect::<Vec<i32>>()`, so a
    /// `bind_count` near `i32::MAX` requested ~8 GiB **before** any cap applied
    /// — while `build_variables`, on the very next line, had been clamped to
    /// `max_var_count = 128` all along.
    ///
    /// ## What is asserted, and why it is not a timing or an OOM test
    ///
    /// The allocation is not directly observable, but its **length** is:
    /// `news_shift_indices` holds exactly what was allocated. So the assertion
    /// is that the length is `1` — one interval per `New` **entered** — at
    /// `bind_count`s three orders of magnitude apart and at `i32::MAX`. That is
    /// strictly stronger than a `<= max_var_count` bound: it says the printer's
    /// per-`New` memory does not depend on `bind_count` **at all**, which is
    /// what "the DoS is closed at the root" means. Reverting to either
    /// materialised form fails this in microseconds, with no memory pressure
    /// and no flakiness — an OOM test would prove less, and would prove it by
    /// destabilising the machine.
    ///
    /// ## ⚠ The display cap is asserted here too, and separately
    ///
    /// Bounding the recorded indices and bounding the printed names are now
    /// *different* obligations discharged by *different* code
    /// ([`super::PrettyPrinter::new_bind_range`] and
    /// [`super::PrettyPrinter::printed_bind_extent`]). Before, one clamp did
    /// both, so a test of either was a test of both. Now a regression that
    /// dropped the display cap — re-opening the DoS through the `Vec<String>`
    /// `build_variables` builds — would leave the interval assertion green, so
    /// the rendered name count is checked in the same loop.
    #[test]
    pub(super) fn a_new_costs_one_range_however_many_names_it_binds() {
        let printer_cap = PrettyPrinter::new().max_var_count;
        assert_eq!(
            printer_cap, 128,
            "the display cap moved; the derivations in this module assume 128"
        );

        for bind_count in [1_000i32, 100_000, i32::MAX] {
            let term = new_par(bind_count, gint(0));

            let mut driven = PrettyPrinter::new();
            let driven_out = driven.build_string_from_message(&term);
            let mut recursive = PrettyPrinter::new();
            let recursive_out = recursive.oracle_build_string_from_message(&term);

            assert_eq!(
                driven_out, recursive_out,
                "DIFFERENTIAL FAILED (bind_count = {bind_count}): the interval form was \
                 applied to one body and not the other"
            );

            // (1) THE BOUND: one interval per `New`, whatever it binds — and it
            // carries the RAW count, so the marking is exact as well as cheap.
            assert_eq!(
                driven.news_shift_indices,
                vec![NewBindRange {
                    start: 0,
                    count: bind_count
                }],
                "the driver recorded {:?} for a `New` binding {bind_count} names. More than \
                 one entry means the per-`New` cost depends on `bind_count` (the DoS is \
                 back); one entry with a truncated `count` means the marking was clamped \
                 (bytes move past the display cap)",
                driven.news_shift_indices
            );
            assert_eq!(
                recursive.news_shift_indices, driven.news_shift_indices,
                "the recursive twin recorded {:?} where the driver recorded {:?}, so the \
                 differential is comparing two different computations",
                recursive.news_shift_indices, driven.news_shift_indices
            );

            // (2) THE DISPLAY CAP, which is now a separate obligation: the
            // header between `new ` and ` in {` names exactly `printer_cap`
            // variables.
            let header = declared_names(&driven_out);
            assert_eq!(
                header.len(),
                printer_cap as usize,
                "a `New` binding {bind_count} names PRINTED {} of them — \
                 `printed_bind_extent` is not bounding the rendered `Vec<String>`",
                header.len()
            );
            assert_eq!(
                (
                    header.first().map(String::as_str),
                    header.last().map(String::as_str)
                ),
                (Some("x0"), Some("x127")),
                "the printed names are not the first `printer_cap` of the interval: {header:?}"
            );

            // ANTI-VACUITY: the fixture really does ask for more names than the
            // cap, so `header.len() == cap` is a cap and not a coincidence.
            assert!(
                bind_count > printer_cap,
                "a fixture at bind_count = {bind_count} cannot exhibit a cap at {printer_cap}"
            );
        }
    }

    /// ★ THE FOOTPRINT the interval representation exists to bound — the one
    /// row of the representation table that no byte-level check can stand in
    /// for.
    ///
    /// # Why this test had to be written
    ///
    /// The recorded row reads *"the interval materialised again, unclamped (the
    /// pre-`bd7cb45f` form) — ✔ `the_two_forms_agree_even_where_the_arithmetic_\
    /// overflows`; 6.92 s / 4 MiB peak becomes >300 s / 7.31 GiB peak under an
    /// 8 GiB cgroup"*. Two things about it are unlike every other row in either
    /// table:
    ///
    /// 1. **Its mutant is byte-equivalent.** Materialising the interval changes
    ///    no rendered byte — it changes what the printer *holds*. Every string
    ///    comparison in this module is blind to it by construction, so its
    ///    evidence has to be a resource measurement, and a resource measurement
    ///    is what the row cites.
    /// 2. **Its recorded evidence must not be re-run.** The cited property test
    ///    draws `bind_count` from `any::<i32>()`, so the mutant asks for up to
    ///    `(2^31 - 1) x 4 B` ~ 8 GiB, and the measurement records it
    ///    *thrashing* rather than being killed (`oom_kill 0`) — i.e. it HANGS.
    ///    A suite that ran it would hang with it, which is a worse defect than
    ///    the un-executed row it would replace.
    ///
    /// # What is executed in its place
    ///
    /// The row's claim is `Θ(bind_count)` versus `Θ(1)` — a SLOPE. A slope is
    /// measurable at any scale, and this test measures it at a bounded one:
    ///
    /// | leg | where | what it shows |
    /// |---|---|---|
    /// | production, bounded | here | the retained footprint is **identical** at `bind_count` = 0, 1, 128, 1'000 and 100'000 — slope `0` bytes/name |
    /// | production, at the attacker's maximum | [`a_new_costs_one_range_however_many_names_it_binds`] | one interval carrying the raw count at `bind_count = i32::MAX`, which is where a materialised form would ask for ~8 GiB |
    /// | the counterfactual | `representation_mutations` | the same site under [`super::BindRangeMutation::IntervalMaterialisedAgain`] retains exactly `4 x bind_count` bytes more — slope `4` bytes/name — measured at `bind_count <= 100'000` and *extrapolated* to `i32::MAX`, never executed there |
    ///
    /// ⚠ The `i32::MAX` fixture is deliberately NOT in this test's loop. Under
    /// the materialising mutant it would be refused by
    /// [`super::MAX_MATERIALISED_BIND_COUNT`]'s guard, and a refusal is not a
    /// rejection: this test has to go RED under that mutation, not abstain. The
    /// `i32::MAX` end of the production claim is asserted by the neighbour
    /// above, whose refusal under the mutant IS the row's resource claim, made
    /// executable.
    #[test]
    pub(super) fn the_bytes_a_new_retains_do_not_depend_on_bind_count() {
        // Two words, no pointer: the type cannot be hiding a heap allocation
        // whose growth this test would then be blind to.
        assert_eq!(
            std::mem::size_of::<NewBindRange>(),
            8,
            "`NewBindRange` is no longer two `i32`s, so `capacity * size_of` is no longer the \
             footprint of the recorded bounds"
        );

        let bind_counts = [0i32, 1, 128, 1_000, super::MAX_MATERIALISED_BIND_COUNT];
        let mut measured: Vec<(i32, usize)> = Vec::with_capacity(bind_counts.len());
        for bind_count in bind_counts {
            let term = new_par(bind_count, gint(0));
            let mut printer = PrettyPrinter::new();
            let rendered = printer.build_string_from_message(&term);

            // N2 — the observation came from a printer that actually entered
            // the `New` site, rather than from one that never ran it.
            assert!(
                rendered.starts_with("new "),
                "the fixture did not render as a `New`, so nothing was measured: {rendered:?}"
            );
            assert_eq!(
                printer.news_shift_indices.len(),
                1,
                "one `New` entered must record exactly one bound record; found {:?}",
                printer.news_shift_indices
            );

            measured.push((
                bind_count,
                super::bytes_retained_for_recorded_bounds(&printer),
            ));
        }

        let (_, baseline) = measured[0];
        for (bind_count, bytes) in &measured {
            assert_eq!(
                *bytes, baseline,
                "the bytes retained for a `New`'s bound record MOVED with `bind_count`: \
                 {bytes} B at bind_count = {bind_count} against {baseline} B at bind_count = 0. \
                 The record is supposed to be the INTERVAL, whose size is independent of the \
                 count; a footprint that tracks the count is the materialised form, i.e. an \
                 attacker-chosen ~8 GiB request on a path reachable from `rho:io:stdout`. \
                 Full series: {measured:?}"
            );
        }

        // ANTI-VACUITY: the largest fixture really does ask for enough names
        // that a materialised form would be visible here — 100'000 names is
        // 400'000 B, four orders of magnitude above the baseline — so
        // "identical" is a bound and not a coincidence of small numbers.
        let widest = super::MAX_MATERIALISED_BIND_COUNT as usize * std::mem::size_of::<i32>();
        assert!(
            widest > baseline * 100,
            "the widest fixture would materialise {widest} B against a {baseline} B baseline; \
             that is not a large enough separation for this test to detect the form it excludes"
        );
    }

    /// The names a rendered `new ... in { ... }` declares, in order.
    fn declared_names(rendered: &str) -> Vec<String> {
        let head = rendered
            .strip_prefix("new ")
            .and_then(|s| s.split_once(" in {"))
            .map(|(head, _)| head)
            .unwrap_or_else(|| panic!("not a rendered `New`: {rendered:?}"));
        if head.is_empty() {
            return Vec::new();
        }
        head.split(", ").map(str::to_string).collect()
    }

    /// ★ THE MEMBERSHIP OBLIGATION of the interval representation, discharged
    /// against the thing it replaced.
    ///
    /// `news_shift_indices` was a `Vec<i32>` consulted with `Vec::contains`, on
    /// a byte-for-byte replay-compared path. Replacing it with an interval is
    /// only byte-preserving if [`NewBindRange::contains`] is *the same
    /// predicate* as `Vec::contains` over the materialised run. This test is
    /// that comparison: it materialises `S(start, count) = { start (+) i : i in
    /// [0, count) }` with the same wrapping `i32` addition the printer
    /// performed, and checks the two answers agree on every index in a window
    /// around the run plus the extremes.
    ///
    /// ⚠ The wrapping cases are the point. `(i32::MAX - 2, 5)` and
    /// `(i32::MIN, 3)` are exactly where a "widen to `i64` and compare" reading
    /// of "contiguous interval" would disagree with the bytes the node ships
    /// (`release`, `overflow-checks` off, so the materialised form wrapped).
    #[test]
    pub(super) fn a_bind_range_answers_membership_exactly_as_a_vector_did() {
        // (start, count) — materialisable counts only; the huge ones are
        // covered by `a_new_costs_one_range_however_many_names_it_binds`.
        let cases: [(i32, i32); 14] = [
            (0, 0),
            (0, 1),
            (0, 129),
            (5, 3),
            (-4, 7),
            // The wrapping regime: `start + count - 1` past `i32::MAX`, so the
            // run runs off the top of the type and resumes at `i32::MIN`.
            (i32::MAX - 2, 5),
            (i32::MAX, 3),
            (i32::MAX - 100, 300),
            (2_147_483_000, 1_000),
            // The bottom of the type, which does not wrap but is where an
            // off-by-one in the `wrapping_sub` would show.
            (i32::MAX, 1),
            (i32::MIN, 3),
            (i32::MIN + 1, 400),
            // Empty runs.
            (0, -1),
            (i32::MIN, -5),
        ];

        let mut wrapping_cases_seen = 0usize;
        for (start, count) in cases {
            // The materialised form, with the printer's own arithmetic.
            let materialised: Vec<i32> = (0..count).map(|i| start.wrapping_add(i)).collect();
            let range = NewBindRange { start, count };
            if count > 0 && (start as i64) + (count as i64 - 1) > i32::MAX as i64 {
                wrapping_cases_seen += 1;
            }

            let mut probes: Vec<i32> = vec![i32::MIN, -1, 0, 1, i32::MAX];
            // A window around the run, wrapping at the ends of the type just as
            // the materialised form did.
            for delta in -3i64..=(count.max(0) as i64 + 3) {
                probes.push(start.wrapping_add(delta as i32));
            }
            // And the wrapped image of the far end, which is exactly where an
            // `i64`-widened `contains` would answer differently.
            for delta in -3i32..=3 {
                probes.push(start.wrapping_add(count).wrapping_add(delta));
            }

            for idx in probes {
                assert_eq!(
                    range.contains(idx),
                    materialised.contains(&idx),
                    "membership diverged at start = {start}, count = {count}, idx = {idx}: \
                     the interval says {}, the vector it replaced says {}",
                    range.contains(idx),
                    materialised.contains(&idx)
                );
            }
            assert_eq!(
                range.is_empty(),
                materialised.is_empty(),
                "`is_empty` disagrees with the materialised run at start = {start}, \
                 count = {count}"
            );
        }

        // ANTI-VACUITY: the corpus really did exercise the wrapping regime, so
        // the agreement above is not an agreement about non-wrapping cases only.
        assert!(
            wrapping_cases_seen >= 4,
            "only {wrapping_cases_seen} case(s) wrapped past `i32::MAX`; this test cannot \
             distinguish the wrapping `contains` from an `i64`-widened one"
        );
    }

    /// ★ NO BYTE MOVES past the display cap — the guard that stands where
    /// `the_clamp_changes_the_star_prefix_past_the_display_cap` used to.
    ///
    /// ## What this test used to be, and why it is now its own opposite
    ///
    /// `news_shift_indices` is read back by `is_new_var`, which decides the `*`
    /// prefix on a bound variable, and the printer's output is replay-compared
    /// (`casper/src/rust/rholang/replay_runtime.rs`). `bd7cb45f` bounded the
    /// allocation by clamping the recorded indices to `max_var_count`, which
    /// meant a variable in slot `>= start + 128` stopped being marked as
    /// new-bound: at `bind_count = 129`, slot 0 kept `*x0` and slot 128 went
    /// `*x128` -> `x128`. The test at this position **pinned that delta**, on
    /// the reasoning that the printer should mark exactly the names it prints.
    ///
    /// Holding the interval instead of materialising it removes the delta's
    /// cause rather than its symptom — `NewBindRange::contains` is exact, so
    /// every slot is marked exactly as it was before `bd7cb45f` — and bounds
    /// the allocation *harder*, `Θ(1)` instead of `Θ(max_var_count)`. So the
    /// subject of the old test no longer exists, and the check that stood on it
    /// is replaced by its converse rather than deleted: past the display cap,
    /// the `*` **survives**.
    ///
    /// ⚠ Reachable: `new x1, ..., x129 in { ... }` is legal Rholang, so neither
    /// the delta nor its absence is confined to hand-built terms.
    ///
    /// ## The two obligations, which now come apart
    ///
    /// | obligation | asserted by |
    /// |---|---|
    /// | slot `>= cap` is still MARKED (`*x128`) | this test |
    /// | name `>= cap` is still NOT PRINTED (`x128` absent from the header) | this test, and [`a_new_costs_one_range_however_many_names_it_binds`] |
    ///
    /// The second is what makes the first non-trivial: the printer marks a name
    /// the same render declines to print. That asymmetry is *deliberate* and
    /// pre-dates `bd7cb45f` — it is the cost of a display cap — and it is now
    /// harmless, because marking no longer allocates.
    #[test]
    pub(super) fn the_star_prefix_survives_past_the_display_cap() {
        let cap = PrettyPrinter::new().max_var_count; // 128

        // Slot `cap` — i.e. de Bruijn level `bind_count - cap - 1` — is the
        // FIRST slot `bd7cb45f`'s clamp stopped marking.
        let bind_count = cap + 1; // 129
        let inside_cap = new_par(bind_count, bound_var(bind_count - 1)); // slot 0
        let past_cap = new_par(bind_count, bound_var(0)); // slot 128

        let inside = PrettyPrinter::new().build_string_from_message(&inside_cap);
        let past = PrettyPrinter::new().build_string_from_message(&past_cap);

        assert!(
            inside.contains("*x0"),
            "a variable INSIDE the display cap lost its `*` prefix. Rendered: {inside}"
        );
        assert!(
            past.contains("*x128"),
            "a variable PAST the display cap lost its `*` prefix — the recorded interval is \
             being truncated at `max_var_count`, which moves bytes on a replay-compared \
             path. Rendered: {past}"
        );
        // The display cap is still in force: `x128` is marked but NOT declared.
        let declared = declared_names(&past);
        assert_eq!(
            declared.len(),
            cap as usize,
            "the header declared {} names at bind_count = {bind_count}; the display cap is \
             not in force, so the `*` above proves nothing about marking past a cap",
            declared.len()
        );
        assert!(
            !declared.iter().any(|n| n == "x128"),
            "`x128` WAS declared, so slot 128 is not past the display cap in this fixture"
        );
        // ANTI-VACUITY: the two fixtures differ ONLY in the referenced level,
        // so the rendered `*` really is attributable to the slot.
        assert_ne!(
            inside, past,
            "the two fixtures render identically, so this test cannot locate the slot"
        );
        // And the twin agrees, at both slots.
        assert_eq!(
            inside,
            PrettyPrinter::new().oracle_build_string_from_message(&inside_cap),
            "DIFFERENTIAL FAILED (star prefix inside the cap)"
        );
        assert_eq!(
            past,
            PrettyPrinter::new().oracle_build_string_from_message(&past_cap),
            "DIFFERENTIAL FAILED (star prefix past the cap)"
        );
    }

    /// ★ The delta the single-`New` fixture above CANNOT see: a `New` inside a
    /// `New`, where the recorded intervals must both be exact *and* start at
    /// different offsets.
    ///
    /// Found by census rather than by reasoning. Diffing the rendered output of
    /// the three code states (pre-`bd7cb45f`, `bd7cb45f`, the interval form)
    /// over a fixture sweep turned up **seven** byte deltas, not the one the
    /// single-`New` test pinned: `new 3 in { new 200 in { <var> } }` loses the
    /// `*` on slots 199, 200, 201 **and** 202 under the clamp. That is what
    /// makes this fixture worth its own test — the outer `New` moves
    /// `bound_shift` to 3, so the inner interval is `[3, 203)` and the clamped
    /// form recorded `[3, 131)`, cutting four *distinct* slots whose levels are
    /// nowhere near the cap.
    ///
    /// It also exercises the one thing a single interval cannot: `is_new_var`
    /// scanning MORE THAN ONE recorded interval, where the outer `[0, 3)` and
    /// the inner `[3, 203)` are adjacent and must not be conflated.
    #[test]
    pub(super) fn the_star_prefix_is_exact_across_nested_news() {
        let cap = PrettyPrinter::new().max_var_count; // 128
        let (outer, inner) = (3i32, 200i32);
        assert!(
            inner > cap,
            "the inner `New` must bind past the display cap for this fixture to bite"
        );

        // `bound_shift` is `outer + inner` = 203 inside the body, so de Bruijn
        // `level` maps to slot `203 - level - 1`.
        let total = outer + inner;
        for level in 0..total {
            let slot = total - level - 1;
            let term = new_par(outer, new_par(inner, bound_var(level)));
            let rendered = PrettyPrinter::new().build_string_from_message(&term);
            assert!(
                rendered.contains(&format!("*x{slot}\n")),
                "slot {slot} (level {level}) lost its `*` prefix — every slot in \
                 [0, {total}) is bound by one of the two nested `New`s. Rendered tail: {:?}",
                &rendered[rendered.len().saturating_sub(20)..]
            );
            assert_eq!(
                rendered,
                PrettyPrinter::new().oracle_build_string_from_message(&term),
                "DIFFERENTIAL FAILED (nested `New`, level {level})"
            );
        }

        // Two intervals recorded, not one merged run and not 203 indices.
        let mut driven = PrettyPrinter::new();
        let _ = driven.build_string_from_message(&new_par(outer, new_par(inner, gint(0))));
        assert_eq!(
            driven.news_shift_indices,
            vec![
                NewBindRange {
                    start: 0,
                    count: outer
                },
                NewBindRange {
                    start: outer,
                    count: inner
                },
            ],
            "the two nested `New`s did not record two intervals at the expected offsets"
        );
    }

    /// ⚠ A NEGATIVE `bind_count` is representable — the proto declares
    /// `sint32 bindCount`, which prost generates as `i32` — and it is now
    /// explicit rather than incidental.
    ///
    /// `0..negative` is the EMPTY range, so a negative `bind_count` prints no
    /// names and marks no slot. That is **the same value both earlier forms
    /// produced** — the unclamped `(0..bind_count)` and `bd7cb45f`'s
    /// `(0..min(128, bind_count))` — so neither the clamp nor the interval
    /// changes this case; the point of pinning it is that "empty range" is a
    /// decision, and a future `as usize` or `.max(0)` on that expression would
    /// silently turn it into a 4 GiB allocation or a panic instead.
    ///
    /// ⚠ **The interval representation has to reproduce it, and the assertion
    /// had to change shape to keep saying so.** The `New` handler now records
    /// one interval unconditionally — that uniformity is what makes
    /// `news_shift_indices.len()` equal the number of `New`s entered — so
    /// "binds nothing" can no longer be spelled `news_shift_indices.is_empty()`.
    /// It is spelled here as what it actually means: exactly one interval was
    /// recorded, that interval is empty, and no shift index whatsoever is a
    /// member of it.
    ///
    /// `bound_shift += bind_count` still runs with the raw value and still
    /// moves `bound_shift` DOWNWARD — that is shared arithmetic, untouched by
    /// either fix, and it is asserted here so the two behaviours are not
    /// conflated.
    #[test]
    pub(super) fn a_new_with_a_negative_bind_count_binds_nothing() {
        for bind_count in [-1i32, -128, -1_000_000] {
            let term = new_par(bind_count, gint(0));

            let mut driven = PrettyPrinter::new();
            let driven_out = driven.build_string_from_message(&term);
            let mut recursive = PrettyPrinter::new();
            let recursive_out = recursive.oracle_build_string_from_message(&term);

            assert_eq!(
                driven_out, recursive_out,
                "DIFFERENTIAL FAILED (bind_count = {bind_count})"
            );
            // No names printed between `new` and `in`.
            assert_eq!(
                driven_out, "new  in {\n  0\n}",
                "a negative `bind_count` did not render as binding zero names"
            );
            assert_eq!(
                driven.news_shift_indices,
                vec![NewBindRange {
                    start: 0,
                    count: bind_count
                }],
                "the `New` did not record exactly one interval carrying the RAW count"
            );
            let introduced = driven.news_shift_indices[0];
            assert!(
                introduced.is_empty(),
                "a negative `bind_count` produced a non-empty interval: {introduced:?}"
            );
            // ...and "empty" is membership, not just a flag: nothing is in it.
            for probe in [i32::MIN, bind_count, -1, 0, 1, 127, 128, i32::MAX] {
                assert!(
                    !introduced.contains(probe),
                    "shift index {probe} is a member of the empty interval {introduced:?} — a \
                     negative `count` is being read as unsigned somewhere"
                );
            }
            // The arithmetic is NOT clamped, and that is deliberate.
            assert_eq!(
                driven.bound_shift, bind_count,
                "`bound_shift` did not take the raw (unclamped) `bind_count`; the bound is \
                 supposed to apply to the ALLOCATION, not the arithmetic"
            );
        }
    }

    // -----------------------------------------------------------------------
    // the error paths
    // -----------------------------------------------------------------------

    /// A `Match` nested inside a `Send`'s data: the target is rendered
    /// **mid-traversal**, with printer state already moved by the siblings
    /// before it and with siblings still to come after it.
    ///
    /// ## ⚠ What this test used to be, and why it changed
    ///
    /// Until the `Match` target defect was fixed, this fixture's purpose was
    /// the opposite: the `Match` target *always* failed, so this was the
    /// witness that a catch frame fires mid-traversal and that its fallback is
    /// spliced into the middle of a larger render rather than replacing it.
    /// No `Par` can produce a failing catch any more, so that half moved to
    /// [`a_failing_region_is_spliced_not_propagated`], which drives the shape
    /// directly through `drive::drive_splice_probe`. What is left here is the
    /// other half, now the positive claim: the target renders **correctly**
    /// in that same mid-traversal position.
    #[test]
    fn a_match_nested_in_a_send() {
        let term = models::par_from_default! {
            sends: vec![Send {
                chan: Some(gstring("out")),
                data: vec![
                    gint(1),
                    models::par_from_default! {
                        matches: vec![Match {
                            target: Some(gint(2)),
                            cases: vec![
                                MatchCase {
                                    pattern: Some(free_var(0)),
                                    source: Some(gint(3)),
                                    free_count: 1,
                                    guard: None,
                                },
                                MatchCase {
                                    pattern: Some(gint(4)),
                                    source: Some(new_par(2, bound_var(0))),
                                    free_count: 0,
                                    guard: None,
                                },
                            ],
                            locally_free: vec![],
                            connective_used: false,
                        }],
                        ..Default::default()
                    },
                    gint(5),
                ],
                persistent: false,
                locally_free: vec![],
                connective_used: false,
            }],
            ..Default::default()
        };

        agree("a_match_nested_in_a_send", &term);

        let printed = PrettyPrinter::new().build_string_from_message(&term);
        // ★ THE TARGET IS RENDERED, in the middle, and the siblings survive.
        assert!(
            printed.contains("match 2 {"),
            "the `Match` target did not render as its target (`2`) mid-traversal; \
             rendered: {printed}"
        );
        assert!(
            !printed.contains("<unprintable"),
            "a sub-render still fails inside an ordinary term — after the `Match` target \
             fix no `Par` should be able to make this printer produce a fallback. \
             Rendered: {printed}"
        );
        assert!(
            printed.contains("1, match") && printed.contains(", 5"),
            "the siblings around the `match` sub-render were lost; rendered: {printed}"
        );
        // ANTI-VACUITY: the target's rendering is decided by the TARGET, not by
        // some constant. The same fixture with a different target must print a
        // different `match` head — otherwise `contains("match 2 {")` above would
        // pass for a printer that hard-coded it.
        let mut other = term.clone();
        other.sends[0].data[1].matches[0].target = Some(gstring("a different target"));
        let other_printed = PrettyPrinter::new().build_string_from_message(&other);
        assert!(
            other_printed.contains("match \"a different target\" {"),
            "the `match` head does not track its target; rendered: {other_printed}"
        );
    }

    /// ⚠ A SECOND behaviour change the `Match` target fix carries, recorded so
    /// it is a decision and not a surprise: a `Match` whose `target` is `None`
    /// used to render an error string, and now **panics**.
    ///
    /// ## Why `.expect` and not a fallback
    ///
    /// `RhoTypes.proto` declares `Par target = 1 [(scalapb.field).no_box =
    /// true]`, exactly as it declares `New::p`, `Send::chan`, `Bundle::body`
    /// and `Receive::body` — all of which prost generates as `Option<Par>` and
    /// all of which this printer has always projected with `.expect`. A
    /// `Match::target` that took `unwrap_or_default()` would be the only
    /// required field in the file that silently renders `Nil` for a malformed
    /// term, which is the same class of silent fallback the closed `PpNode`
    /// dispatch was introduced to eliminate.
    ///
    /// So the panic surface is not new — it is one arm wider on a printer that
    /// already panics for every other absent required field. The message is the
    /// file's standard `"<field> field on <message> was None, should be Some"`.
    ///
    /// ⚠ Reachability is worth stating plainly: a normalizer never emits
    /// `target: None`, so no deploy source reaches this. A hand-built or
    /// protobuf-decoded `Par` can. See the report accompanying this change.
    #[test]
    fn a_match_with_no_target_panics_like_every_other_absent_required_field() {
        let no_target = models::par_from_default! {
            matches: vec![Match {
                target: None,
                cases: vec![],
                locally_free: vec![],
                connective_used: false,
            }],
            ..Default::default()
        };

        let driven = quietly(|| PrettyPrinter::new().build_string_from_message(&no_target));
        let recursive =
            quietly(|| PrettyPrinter::new().oracle_build_string_from_message(&no_target));
        assert_eq!(
            driven, recursive,
            "the driver and the recursive twin take different dispositions on an absent \
             `Match::target`"
        );
        assert_eq!(
            driven,
            Err(String::from(
                "target field on Match was None, should be Some"
            )),
            "an absent `Match::target` no longer panics with the file's standard message"
        );

        // ANTI-VACUITY: the SAME fixture with a target present must NOT panic,
        // so the `Err` above is attributable to the absent field and not to
        // something else in the fixture.
        let with_target = models::par_from_default! {
            matches: vec![Match {
                target: Some(gint(1)),
                cases: vec![],
                locally_free: vec![],
                connective_used: false,
            }],
            ..Default::default()
        };
        assert!(
            quietly(|| PrettyPrinter::new().build_string_from_message(&with_target)).is_ok(),
            "the fixture panics even with a target present, so the absent-target assertion \
             above proves nothing"
        );

        // And this is the same discipline as its siblings — a `New` with no
        // `p` panics too, which is what "like every other absent required
        // field" means.
        let no_p = models::par_from_default! {
            news: vec![New {
                bind_count: 1,
                p: None,
                uri: vec![],
                injections: Default::default(),
                locally_free: vec![],
            }],
            ..Default::default()
        };
        assert_eq!(
            quietly(|| PrettyPrinter::new().build_string_from_message(&no_p)),
            Err(String::from("p field on New was None, should be Some")),
            "the printer's pre-existing `.expect` discipline for absent required fields \
             moved, so `Match::target` is no longer consistent with it"
        );
    }

    /// The unwind must drop the failed sub-render's **spawned work and pending
    /// values**, not merely the item that failed.
    ///
    /// ⚠ No production term can reach this: the only `Err` the printer can
    /// produce is `PpNode::Unprintable`, which is a leaf and errors before it
    /// spawns anything, so every catch in a real render truncates zero items.
    /// The frame semantics are nevertheless what makes the conversion faithful,
    /// so they are exercised directly through a `cfg(test)` seed
    /// (`drive::drive_truncation_probe`) that spawns work and pushes a value
    /// before failing.
    #[test]
    fn a_failing_region_takes_its_spawned_work_with_it() {
        let sibling = gstring("MUST NOT APPEAR");
        let mut printer = PrettyPrinter::new();
        let rendered = super::drive::drive_truncation_probe(&mut printer, &sibling)
            .expect("the probe's catch frame should have absorbed the error");
        assert_eq!(
            rendered,
            "<unprintable: Bug found: Attempt to print unknown prost::Message type: Any { .. }>",
            "the catch frame did not truncate cleanly back to its mark"
        );
        assert!(
            !rendered.contains("MUST NOT APPEAR"),
            "work spawned by the failed region still ran"
        );
        assert!(
            !rendered.contains("must be truncated"),
            "a value pushed by the failed region survived the unwind"
        );
    }

    /// The fallback of a failed region is **spliced in place**: the values
    /// rendered before and after it survive, and the unwind stops at the
    /// innermost frame instead of running to the top.
    ///
    /// ## ⚠ Why this is a `cfg(test)` seed and not a term
    ///
    /// `a_match_nested_in_a_send` used to prove this with an ordinary `Send`,
    /// because the `Match` target defect made every `match` fail. Fixing the
    /// defect removed the last `Par`-expressible `Err`, so the property would
    /// otherwise have become a check that cannot fail — it would have had no
    /// subject at all. `drive::drive_splice_probe` reconstructs the shape:
    /// `before`, a failing catching region, `after`, joined into one value.
    ///
    /// The claim is an exact string, not a `contains`: a fallback that
    /// propagated one frame too far would still *contain* the fallback text.
    #[test]
    fn a_failing_region_is_spliced_not_propagated() {
        let before = gstring("BEFORE");
        let after = gstring("AFTER");
        let mut printer = PrettyPrinter::new();
        let rendered = super::drive::drive_splice_probe(&mut printer, &before, &after)
            .expect("the probe's catch frames should have absorbed the error");

        assert_eq!(
            rendered,
            concat!(
                "\"BEFORE\" | ",
                "<unprintable: Bug found: Attempt to print unknown prost::Message type: ",
                "Any { .. }>",
                " | \"AFTER\""
            ),
            "the fallback was not spliced in place — either a sibling was lost (the unwind \
             ran past its own frame) or the fallback's own bytes moved"
        );
        // ANTI-VACUITY: all three positions are distinguishable, so an
        // assertion about the MIDDLE one is really about the middle one.
        assert!(
            rendered.starts_with("\"BEFORE\"") && rendered.ends_with("\"AFTER\""),
            "the probe's siblings are not at the ends, so 'spliced in the middle' is not \
             what this fixture measures. Rendered: {rendered}"
        );
        assert!(
            !rendered.contains("MUST NOT APPEAR") && !rendered.contains("must be truncated"),
            "the failed region's spawned work or pending value survived: {rendered}"
        );
    }

    /// The unwind must stop at the **innermost** open catching scope — the
    /// recursive form's `?` returns to its nearest enclosing catching entry
    /// point, not to the outermost one.
    ///
    /// ## ★ This test exists because the claim was previously unfalsifiable
    ///
    /// `run` unwinds to `catches.last_mut()`. Mutating that to
    /// `catches.first_mut()` passed all 29 tests in this file, because every
    /// other probe opens its catching scopes in sequence — `push_catch` puts
    /// `EndCatch`, the guarded item and `BeginCatch` on the work stack together,
    /// so a frame is always closed before the next is opened, and `catches`
    /// never held more than one element. `first` and `last` of a one-element
    /// list are the same element, so the assertion could not fail.
    ///
    /// `drive::drive_nested_catch_probe` opens two frames at once. The
    /// distinguishing observations, both asserted below:
    ///
    /// * the siblings inside the outer region **survive** — unwinding to the
    ///   outer frame truncates them away;
    /// * the drive completes at all — unwinding to the outer frame leaves the
    ///   inner frame open, and `run`'s "catching scope(s) never closed"
    ///   assertion fires.
    #[test]
    fn the_unwind_stops_at_the_innermost_open_frame() {
        const FALLBACK: &str =
            "<unprintable: Bug found: Attempt to print unknown prost::Message type: Any { .. }>";

        let before = gstring("BEFORE");
        let after = gstring("AFTER");

        let rendered = quietly(|| {
            let mut printer = PrettyPrinter::new();
            super::drive::drive_nested_catch_probe(&mut printer, &before, &after)
                .expect("the probe's catch frames should have absorbed the error")
        });
        let rendered = rendered.unwrap_or_else(|payload| {
            panic!(
                "the nested-catch probe PANICKED ({payload}) instead of completing. The \
                 unwind is not stopping at the innermost open frame: absorbing at an OUTER \
                 frame truncates the inner frame's `EndCatch` off the work stack, so the \
                 inner scope is never closed."
            )
        });

        assert_eq!(
            rendered,
            format!("\"BEFORE\" | {FALLBACK} | \"AFTER\""),
            "the unwind did not stop at the innermost open frame — the siblings inside the \
             OUTER catching scope were truncated away with the inner failure"
        );

        // ANTI-VACUITY: two frames really are open when the failure fires.
        // Without that, `first_mut()` and `last_mut()` are the same element and
        // everything above passes for either. The single-layer probe is the
        // control: it renders the same string with only ONE frame open, so a
        // difference in disposition between the two is attributable to nesting.
        let single = quietly(|| {
            let mut printer = PrettyPrinter::new();
            super::drive::drive_splice_probe(&mut printer, &before, &after)
                .expect("the single-layer probe's catch frame absorbs the error")
        });
        assert_eq!(
            single.as_deref(),
            Ok(format!("\"BEFORE\" | {FALLBACK} | \"AFTER\"").as_str()),
            "the single-layer control disagrees with the nested probe, so the nested probe's \
             result is not attributable to the nesting"
        );
    }

    // -----------------------------------------------------------------------
    // the capping call sites
    // -----------------------------------------------------------------------

    /// The driver must call `cap` in exactly the places the recursive form
    /// called it — including the asymmetry that `EndCatch` caps a **successful**
    /// sub-render and does **not** cap a fallback.
    ///
    /// ## ★ Why this is a PANIC-disposition test and not a string comparison
    ///
    /// `PrettyPrinter::cap` is `format!("{}...", &str[..n])` — a *prefix*
    /// truncation. Prefix truncations compose: if an inner value `v` is spliced
    /// into an outer render at offset `p` and the outer render is then capped
    /// to the same `n`, then `cap(v)` and `v` agree on `v`'s first `n` bytes,
    /// so they can only differ from byte `p + n >= n` onward — which the outer
    /// cap has already discarded. **No internal `cap` call site is observable
    /// in the returned string.** A comparison of outputs therefore cannot
    /// distinguish "caps the fallback" from "does not", and a test that tried
    /// would pass for a printer that capped everywhere or nowhere. (That is not
    /// hypothetical: an earlier version of this test compared strings, and a
    /// deliberate mutation removing the `!frame.failed` guard passed it.)
    ///
    /// What *is* observable is that `&str[..n]` **panics when `str.len() < n`**.
    /// So "was this sub-render capped?" is decidable from whether the render
    /// panics, for an `n` between the sub-render's length and the whole render's
    /// length. The fallback is 81 bytes, so `n = 100` and `n = 200` decide the
    /// fallback's capping, and the shorter `n` decide the successful sub-renders'.
    ///
    /// ## ⚠ The fallback decider had to move, or it would have stopped deciding
    ///
    /// It used to be an ordinary term — a `match` beside a long sibling, whose
    /// target *always* failed because of the `Match` target defect. Fixing that
    /// defect removed the last `Par` that can make a catch frame fail, so the
    /// probe kept running and stopped proving anything: with no failed frame,
    /// `EndCatch`'s `!frame.failed` guard is unreachable and a mutation
    /// deleting it is an equivalent mutant. That is exactly the failure mode
    /// this file has already hit twice (a prefix-truncation comparison that a
    /// real mutation passed, and a child process that ran zero tests).
    ///
    /// So the fallback side is now decided by `drive::drive_splice_probe`,
    /// which constructs a failing catching region between two rendered
    /// siblings, and is swept over the same `TRIM` values. The claim is
    /// twofold: it must not panic (a capped 81-byte fallback panics for every
    /// `TRIM > 81`), and the fallback must survive **in full** inside the
    /// spliced value (a capped fallback is truncated for every `TRIM < 81`).
    /// Between them the two cover the whole sweep.
    ///
    /// ## Why a child process
    ///
    /// `Printer::output_capped()` reads `PRETTY_PRINTER_OUTPUT_TRIM_AFTER` from
    /// the process environment, which under a threaded `cargo test` would race
    /// every other printer test in this binary. So the test re-execs **itself**
    /// once per `n` — the same discipline `rholang/tests/stack_depth_gate.rs`
    /// uses for its probes — and when it *is* the child (the variable is
    /// already set) it runs the comparison and reports a machine-readable
    /// summary line.
    ///
    /// ⚠ The parent asserts the child's libtest output says `1 passed`. Without
    /// that, a mistyped filter makes the child run **zero** tests, exit 0, and
    /// the whole test pass while proving nothing.
    #[test]
    fn the_capping_call_sites_are_reproduced() {
        const TRIM: &str = "PRETTY_PRINTER_OUTPUT_TRIM_AFTER";
        // Spans the fallback's 81 bytes on both sides, so both the
        // successful-sub-render and the fallback call sites are decided.
        const TRIMS: [i32; 5] = [4, 8, 40, 100, 200];
        /// The bytes `EndCatch` must NOT cap. Spelled out rather than
        /// `format!`-ed from an error so that a change to the error's `Display`
        /// shows up here as a failure instead of silently re-deriving.
        const FALLBACK: &str =
            "<unprintable: Bug found: Attempt to print unknown prost::Message type: Any { .. }>";

        let module = module_path!()
            .split_once("::")
            .map(|(_, rest)| rest)
            .expect("module_path! is crate-qualified");
        let test_name = format!("{module}::the_capping_call_sites_are_reproduced");

        if std::env::var(TRIM).is_err() {
            let exe = std::env::current_exe().expect("current_exe");
            let mut total_ok = 0usize;
            let mut total_panics = 0usize;
            let mut total_fallback_intact = 0usize;
            let mut mutant_differed_at = 0usize;
            let mut mutant_agreed_at = 0usize;
            for trim in TRIMS {
                let output = std::process::Command::new(&exe)
                    .args(["--exact", &test_name, "--nocapture"])
                    .env(TRIM, trim.to_string())
                    .output()
                    .expect("failed to re-exec the capping child");
                let text = String::from_utf8_lossy(&output.stdout).into_owned()
                    + &String::from_utf8_lossy(&output.stderr);
                assert!(
                    output.status.success(),
                    "the capping child failed at {TRIM}={trim}:\n{text}"
                );
                assert!(
                    text.contains("1 passed"),
                    "the capping child ran the wrong number of tests at {TRIM}={trim} — the \
                     filter `{test_name}` matched nothing, so this test proved NOTHING:\n{text}"
                );
                let summary = text
                    .lines()
                    .find(|l| l.starts_with("CAP-PROBE"))
                    .unwrap_or_else(|| panic!("the child printed no CAP-PROBE line:\n{text}"))
                    .to_string();
                let ok: usize = field(&summary, "ok=");
                let panics: usize = field(&summary, "panics=");
                total_ok += ok;
                total_panics += panics;
                total_fallback_intact += field(&summary, "fallback_intact=");
                match field(&summary, "mutant_differs=") {
                    0 => mutant_agreed_at += 1,
                    _ => mutant_differed_at += 1,
                }
            }

            // ★ THE EXECUTED REDDENING LEG for `drive`'s `EndCatch` row.
            //
            // Everything else in this test asserts the guard is GREEN. That
            // says nothing about whether it CAN go red: the mutation that makes
            // it — `EndCatch` capping the fallback as well as the success value
            // — had only ever been performed by hand, observed, and reverted,
            // and the row "`EndCatch` caps the fallback too | ✔" was a record of
            // that afternoon rather than a property of the code.
            //
            // `DriveMutation::EndCatchCapsFallback` is the same defect as a
            // call. It is decided at the SWEEP level because a single trim
            // cannot speak for the sweep, and the child RECORDS rather than
            // predicts — predicting which side of the fallback's length a trim
            // lands on would be re-deriving `Printer::cap`'s policy here instead
            // of observing it.
            //
            // ⚠ MEASURED, and not what a first reading of the boundary suggests.
            // The mutant is caught at EVERY trim in the sweep, longer ones
            // included, because `Printer::cap`'s operator arm is
            // `format!("{}...", &rendered[..floor_boundary_in_range(rendered, n)])`
            // — not a no-op above the string's length but a PANIC (`&str[..n]`
            // out of range), and at exactly its length still an appended `...`.
            // A capped fallback is damaged at any budget. The assertion below
            // records that measurement rather than the prediction that preceded
            // it.
            assert_eq!(
                mutant_differed_at,
                TRIMS.len(),
                "the `EndCatchCapsFallback` mutant survived {mutant_agreed_at} of the {} \
                 trims in {TRIMS:?} intact. Measured 2026-07-27 it is caught at every one: \
                 `Printer::cap`'s operator arm damages a fallback at any budget — truncating \
                 below its {} bytes, panicking above them, appending `...` at exactly them. \
                 If a trim now agrees, `cap` changed, and the `!frame.failed` guard is no \
                 longer what keeps the fallback intact at that budget.",
                TRIMS.len(),
                FALLBACK.len()
            );
            // ANTI-VACUITY: the probe set must actually straddle the cap, i.e.
            // some renders must survive and some must be truncated past their
            // own length. A probe set that only ever panicked (or only ever
            // succeeded) would agree trivially.
            assert!(
                total_ok > 0 && total_panics > 0,
                "the capping probe never straddled the cap: {total_ok} value(s) and \
                 {total_panics} panic(s) across {TRIMS:?}. It cannot decide where `cap` is \
                 called."
            );
            // ANTI-VACUITY for the FALLBACK side: the child reports one intact
            // fallback per `TRIM`, so a child that skipped the splice probe is
            // caught here rather than passing silently.
            assert_eq!(
                total_fallback_intact,
                TRIMS.len(),
                "the splice probe reported {total_fallback_intact} intact fallback(s) across \
                 {} trims — the fallback side of the capping asymmetry was not decided",
                TRIMS.len()
            );
            // ANTI-VACUITY for the SWEEP: at least one `TRIM` must exceed the
            // fallback's own length, or "capping the fallback would panic" is
            // vacuously true and the sweep cannot detect the mutation.
            assert!(
                TRIMS.iter().any(|t| *t as usize > FALLBACK.len()),
                "no probed TRIM exceeds the fallback's {} bytes, so capping it would never \
                 panic and this sweep cannot decide the `!frame.failed` guard: {TRIMS:?}",
                FALLBACK.len()
            );
            return;
        }

        // --- we are the child: the cap is live for this process. ---
        let trim: i32 = std::env::var(TRIM)
            .expect("the child branch is entered only when TRIM is set")
            .parse()
            .expect("TRIM must be an integer");

        // A long value: every capped sub-render of it is longer than every
        // `trim` probed, so it decides the SUCCESS-side call sites.
        let long = gstring(&"a".repeat(400));
        let probes: Vec<(&str, Par)> = vec![
            // Shorter than every probed `trim` -> capping it panics.
            ("a one-byte ground", gint(1)),
            ("a long ground", long.clone()),
            (
                "a send whose data and channel are both capped sub-renders",
                models::par_from_default! {
                    sends: vec![Send {
                        chan: Some(long.clone()),
                        data: vec![long.clone(), gint(7)],
                        persistent: false,
                        locally_free: vec![],
                        connective_used: false,
                    }],
                    ..Default::default()
                },
            ),
            // A `match` beside a long sibling. This was the FALLBACK decider
            // while the `Match` target defect stood; now that the target
            // renders, its short target render ("1") is a SUCCESS-side probe
            // that panics for every `trim > 1` — which is why it still earns
            // its place in the straddle. The fallback side moved to the splice
            // probe below.
            ("a match beside a long sibling", models::par_from_default! {
                exprs: long.exprs.clone(),
                matches: vec![Match {
                    target: Some(gint(1)),
                    cases: vec![],
                    locally_free: vec![],
                    connective_used: false,
                }],
                ..Default::default()
            }),
        ];

        let mut ok = 0usize;
        let mut panics = 0usize;
        for (what, term) in &probes {
            let driven = quietly(|| PrettyPrinter::new().build_string_from_message(term));
            let recursive = quietly(|| PrettyPrinter::new().oracle_build_string_from_message(term));
            match &driven {
                Ok(_) => ok += 1,
                Err(_) => panics += 1,
            }
            assert_eq!(
                driven, recursive,
                "DIFFERENTIAL FAILED (cap call sites, {TRIM}={trim}, {what}): the driver \
                 called `cap` in a different place than the recursive twin. See this test's \
                 documentation for why the disposition, not the string, is the observable."
            );
        }

        // ★ THE FALLBACK DECIDER. `EndCatch` caps a SUCCESSFUL sub-render and
        // must NOT cap a fallback. No `Par` can make a catch frame fail any
        // more, so the shape is driven directly. Both siblings are long, so the
        // success-side caps cannot panic and the ONLY thing this probe's
        // disposition can be reporting is the fallback's.
        let long_after = gstring(&"z".repeat(400));
        let spliced = quietly(|| {
            let mut pp = PrettyPrinter::new();
            super::drive::drive_splice_probe(&mut pp, &long, &long_after)
                .expect("the splice probe's catch frames absorb the error")
        });
        let spliced = spliced.unwrap_or_else(|payload| {
            panic!(
                "at {TRIM}={trim} the spliced render PANICKED ({payload}) — `EndCatch` capped \
                 the {}-byte fallback, which `&str[..{trim}]` cannot do. The `!frame.failed` \
                 guard is gone.",
                FALLBACK.len()
            )
        });
        assert!(
            spliced.contains(FALLBACK),
            "at {TRIM}={trim} the fallback did not survive INTACT inside the spliced render — \
             `EndCatch` truncated it. Rendered: {spliced}"
        );

        // ★ THE EXECUTED REDDENING LEG for the `EndCatch` row of `drive`'s
        // mutation table. Everything above asserts the guard is GREEN; nothing
        // asserted it could go red, because the mutation that makes it go red —
        // capping the fallback too — had only ever been performed by hand.
        //
        // `DriveMutation::EndCatchCapsFallback` is that mutation as a call. It
        // is decided here rather than in `drive_mutations` because it is the one
        // row whose witness needs a capping ENVIRONMENT, which is exactly why
        // this test runs in a child process at all.
        //
        // Under the mutation, `EndCatch` caps the fallback. For every `TRIM`
        // shorter than the fallback that is a panic (`&str[..trim]` cannot split
        // a multi-byte boundary and `Printer::cap` panics on an operator budget
        // shorter than the string); for a `TRIM` longer than it, the fallback
        // survives and the two agree. So the leg asserts the disposition
        // DIFFERS at some trim in the sweep, and the sweep's own trims straddle
        // the fallback length.
        let mutated = quietly(|| {
            let mut pp = PrettyPrinter::new();
            super::drive::with_mutation(super::drive::capping_mutation(), || {
                super::drive::drive_splice_probe(&mut pp, &long, &long_after)
                    .expect("the splice probe's catch frames absorb the error")
            })
        });
        //
        // The child RECORDS the disposition; the sweep DECIDES. A single trim
        // cannot decide a claim about a guard that only bites below the
        // fallback's length, and predicting which side of the boundary a given
        // trim lands on would be re-deriving `Printer::cap`'s policy here
        // instead of observing it.
        let mutant_differs = match &mutated {
            Err(_) => 1usize,
            Ok(rendered) => usize::from(!rendered.contains(FALLBACK)),
        };

        println!(
            "CAP-PROBE trim={trim} ok={ok} panics={panics} fallback_intact=1 \
             mutant_differs={mutant_differs}"
        );
    }

    /// Read `name=<usize>` out of the child's summary line.
    fn field(line: &str, name: &str) -> usize {
        let rest = line
            .split_once(name)
            .unwrap_or_else(|| panic!("no `{name}` in `{line}`"))
            .1;
        rest.split_whitespace()
            .next()
            .unwrap_or_else(|| panic!("no value after `{name}` in `{line}`"))
            .parse()
            .unwrap_or_else(|_| panic!("`{name}` in `{line}` is not a number"))
    }

    // -----------------------------------------------------------------------
    // `is_building_channel` — set once, never reset
    // -----------------------------------------------------------------------

    /// ⚠ Pre-existing and load-bearing: the first channel render in a printer's
    /// life permanently suppresses the `*` prefix on bound-`new` variables. The
    /// driver must reproduce it, which means comparing a printer that has
    /// ALREADY rendered a channel.
    #[test]
    fn the_sticky_is_building_channel_flag_is_reproduced() {
        let bound_new = models::par_from_default! {
            news: vec![New {
                bind_count: 1,
                p: Some(bound_var(0)),
                uri: vec![],
                injections: Default::default(),
                locally_free: vec![],
            }],
            ..Default::default()
        };

        let mut driven = PrettyPrinter::new();
        let mut recursive = PrettyPrinter::new();

        let before_driven = driven.build_string_from_message(&bound_new);
        let before_recursive = recursive.oracle_build_string_from_message(&bound_new);
        assert_eq!(before_driven, before_recursive, "before any channel render");
        // ANTI-VACUITY: the `*` prefix is present BEFORE the flag is set —
        // otherwise the comparison after it is set proves nothing.
        assert!(
            before_driven.contains('*'),
            "the bound-`new` variable did not print with a `*` prefix, so this test cannot \
             observe the flag. Rendered: {before_driven}"
        );

        driven.build_channel_string(&gstring("anything"));
        recursive.oracle_build_channel_string(&gstring("anything"));
        assert!(
            driven.is_building_channel && recursive.is_building_channel,
            "the flag was not set by a channel render"
        );

        let after_driven = driven.build_string_from_message(&bound_new);
        let after_recursive = recursive.oracle_build_string_from_message(&bound_new);
        assert_eq!(
            after_driven, after_recursive,
            "the sticky `is_building_channel` flag is not reproduced"
        );
        assert!(
            !after_driven.contains('*'),
            "ANTI-VACUITY: the flag had no effect, so this test proved nothing. Rendered: \
             {after_driven}"
        );
    }

    // -----------------------------------------------------------------------
    // depth: the property the conversion exists for
    // -----------------------------------------------------------------------

    /// The driver renders a term far deeper than the recursive form's own
    /// ceiling, on the default test stack.
    ///
    /// ⚠ The ORACLE is deliberately NOT run here — at 41,984 B/level it would
    /// `SIGSEGV`, which is not catchable and would take the whole test binary
    /// with it. The equality obligation is discharged by the corpora above, at
    /// depths the recursive form survives; this test discharges the *depth*
    /// obligation on its own. `rholang/tests/stack_depth_gate.rs` is the real
    /// bar (bisected minimum stack over 4 -> 4,096, both profiles).
    #[test]
    fn the_driver_renders_far_past_the_recursive_ceiling() {
        const DEPTH: usize = 4_096;
        let mut term = gint(0);
        for _ in 0..DEPTH {
            term = expr_par(ExprInstance::EListBody(EList {
                ps: vec![term],
                locally_free: vec![],
                connective_used: false,
                remainder: None,
            }));
        }
        let printed = PrettyPrinter::new().build_string_from_message(&term);
        assert_eq!(
            printed.chars().take_while(|c| *c == '[').count(),
            DEPTH,
            "the driver did not render the full nesting"
        );

        // Iterative teardown: `<Par as Drop>` is still recursive.
        let mut peel = term;
        loop {
            let next = match peel
                .exprs
                .first_mut()
                .and_then(|e| e.expr_instance.as_mut())
            {
                Some(ExprInstance::EListBody(list)) if !list.ps.is_empty() => list.ps.remove(0),
                _ => break,
            };
            peel = next;
        }
    }
}

// rholang/src/test/scala/coop/rchain/rholang/interpreter/PrettyPrinterTest.scala
#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use rholang_parser::ast::Proc;

    use crate::rust::interpreter::compiler::normalize::{normalize_ann_proc, ProcVisitOutputs};
    use crate::rust::interpreter::compiler::normalizer::ground_normalize_matcher::normalize_ground;
    use crate::rust::interpreter::errors::InterpreterError;
    use crate::rust::interpreter::pretty_printer::PrettyPrinter;
    use crate::rust::interpreter::test_utils::utils::collection_proc_visit_inputs_and_env;

    #[test]
    fn identifier_increment_is_stack_safe_for_a_twenty_thousand_character_carry() {
        std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(|| {
                let printer = PrettyPrinter::new();
                assert_eq!(printer.increment("a".into()), "b");
                assert_eq!(printer.increment("z".into()), "aa");
                assert_eq!(printer.increment("az".into()), "ba");
                assert_eq!(printer.increment("zz".into()), "aaa");
                let incremented = printer.increment("z".repeat(20_000));
                assert_eq!(incremented.len(), 20_001);
                assert!(incremented.bytes().all(|byte| byte == b'a'));
            })
            .expect("spawn identifier increment stack gate")
            .join()
            .expect("identifier increment overflowed or panicked");
    }

    /// ★ THE REPLACEMENT PIN for a fixed defect.
    ///
    /// `_build_string_from_message`'s `Match` arm used to call
    /// `self.build_string_from_message(&m.target)`, and `m.target` is an
    /// `Option<Par>`, **not** a `Par` (`RhoTypes.proto` declares
    /// `Par target = 1 [(scalapb.field).no_box = true]`, which prost generates
    /// as `Option<Par>`). `Option<Par>` matched no `downcast_ref` arm, so the
    /// dispatch fell to its final `else` and **every** `match` term this
    /// printer rendered showed its target as
    ///
    /// ```text
    /// match <unprintable: Bug found: Attempt to print unknown prost::Message type: Any { .. }> {
    /// ```
    ///
    /// Its predecessor,
    /// `a_match_target_renders_as_an_error_string_and_that_is_pinned`, held
    /// those bytes so the closed-`PpNode` conversion could be proven
    /// byte-neutral against them. That job is done; this test replaces it
    /// rather than deleting it, so the behaviour is never unpinned.
    ///
    /// ⚠ These bytes are consensus-observable: `build_channel_string` reaches
    /// `SystemDeployPlatformFailure::UnexpectedResult` -> `Display` ->
    /// `error_msg` -> `ProcessedSystemDeploy::Failed`, which is serialized into
    /// the block and compared **byte-for-byte in replay validation**
    /// (`casper/src/rust/rholang/replay_runtime.rs:745-758`), and the printer
    /// is reachable from untrusted input through `rho:io:stdout`.
    ///
    /// ## The claim, in two parts
    ///
    /// 1. an ABSOLUTE pin, derived by hand: `match {target} {\n  \n}` with the
    ///    target rendered in full;
    /// 2. a COMPOSITIONAL identity — the target renders exactly as the same
    ///    `Par` renders on its own, which is the precise sense in which it is
    ///    now "a `Par` position like any other". A stub that special-cased the
    ///    target would satisfy (1) for one fixture and fail (2).
    #[test]
    fn a_match_target_renders_as_its_target() {
        use models::rhoapi::expr::ExprInstance;
        use models::rhoapi::{Expr, Match, Par, Send};

        fn gint(i: i64) -> Par {
            models::par_from_default! {
                exprs: vec![Expr {
                    expr_instance: Some(ExprInstance::GInt(i)),
                }],
                ..Default::default()
            }
        }

        fn no_cases(target: Par) -> Match {
            // No cases: the pin is about the TARGET, and a case body would
            // make it brittle against unrelated changes to case rendering.
            Match {
                target: Some(target),
                cases: vec![],
                locally_free: vec![],
                connective_used: false,
            }
        }

        // (1) THE ABSOLUTE PIN. Derived, not transcribed: the target is a
        // one-expression `Par` holding `GInt(42)`, which renders "42"; `MatchK`
        // emits `"match " + target + " {\n" + indent_string().repeat(0 + 1)`
        // and then `"\n" + indent_string().repeat(0) + "}"`, and the empty case
        // list contributes nothing between them.
        assert_eq!(
            PrettyPrinter::new().build_string_from_message(&no_cases(gint(42))),
            "match 42 {\n  \n}",
            "the `Match` target rendering moved. These bytes reach block-resident, \
             replay-compared error messages; see this test's documentation."
        );

        // ANTI-VACUITY: the defect's bytes are gone, and the target is not
        // being swallowed into the empty-`Par` rendering either.
        let rendered = PrettyPrinter::new().build_string_from_message(&no_cases(gint(42)));
        assert!(
            !rendered.contains("<unprintable"),
            "the `Match` target is still rendering as an error string: {rendered}"
        );
        assert!(
            rendered.contains("42"),
            "the `Match` target was not rendered at all: {rendered}"
        );

        // (2) THE COMPOSITIONAL IDENTITY, over targets of increasing shape —
        // a ground, a structured `Send` (so the target is genuinely traversed
        // rather than matched by some ground-only shortcut), and the empty
        // `Par`, whose "Nil" is the one rendering that could be mistaken for a
        // stub.
        let targets = vec![
            gint(42),
            gint(-7),
            models::par_from_default! {
                sends: vec![Send {
                    chan: Some(gint(1)),
                    data: vec![gint(2), gint(3)],
                    persistent: false,
                    locally_free: vec![],
                    connective_used: false,
                }],
                ..Default::default()
            },
            Par::default(),
        ];
        // ANTI-VACUITY: at least one target must render to something OTHER
        // than a bare literal, or (2) is only re-testing (1).
        assert!(
            targets
                .iter()
                .any(|t| PrettyPrinter::new().build_string_from_message(t).len() > 4),
            "every target in the compositional corpus is a short literal"
        );
        for target in targets {
            let standalone = PrettyPrinter::new().build_string_from_message(&target);
            assert_eq!(
                PrettyPrinter::new().build_string_from_message(&no_cases(target)),
                format!("match {standalone} {{\n  \n}}"),
                "the `Match` target does not render the way the same `Par` renders on \
                 its own — it is no longer 'a `Par` position like any other'"
            );
        }
    }

    //ground tests
    #[test]
    fn bool_true_should_print_as_true() {
        let proc = Proc::BoolLiteral(true);
        let expr = normalize_ground(&proc).unwrap();
        let mut printer = PrettyPrinter::new();

        assert_eq!(printer.build_string_from_expr(&expr), "true");
    }

    #[test]
    fn bool_false_should_print_as_false() {
        let proc = Proc::BoolLiteral(false);
        let expr = normalize_ground(&proc).unwrap();
        let mut printer = PrettyPrinter::new();

        assert_eq!(printer.build_string_from_expr(&expr), "false");
    }

    #[test]
    fn ground_int_should_print_as_string_int() {
        let proc = Proc::LongLiteral(7);
        let expr = normalize_ground(&proc).unwrap();
        let mut printer = PrettyPrinter::new();

        assert_eq!(printer.build_string_from_expr(&expr), "7".to_string());
    }

    #[test]
    fn ground_string_should_print_as_string() {
        let proc = Proc::StringLiteral("String");
        let expr = normalize_ground(&proc).unwrap();
        let target: String = "\"String\"".to_string();
        let mut printer = PrettyPrinter::new();

        assert_eq!(printer.build_string_from_expr(&expr), target);
    }

    #[test]
    fn prime_check_strings_should_print_correctly() {
        let mut printer = PrettyPrinter::new();

        let nil_proc = Proc::StringLiteral("Nil");
        let nil_expr = normalize_ground(&nil_proc).unwrap();
        assert_eq!(printer.build_string_from_expr(&nil_expr), "\"Nil\"");

        let pr_proc = Proc::StringLiteral("Pr");
        let pr_expr = normalize_ground(&pr_proc).unwrap();
        assert_eq!(printer.build_string_from_expr(&pr_expr), "\"Pr\"");

        let co_proc = Proc::StringLiteral("Co");
        let co_expr = normalize_ground(&co_proc).unwrap();
        assert_eq!(printer.build_string_from_expr(&co_expr), "\"Co\"");
    }

    #[test]
    fn ground_uri_should_print_with_back_ticks() {
        let proc = Proc::UriLiteral("Uri".into());
        let expr = normalize_ground(&proc).unwrap();
        let target: String = "`Uri`".to_string();
        let mut printer = PrettyPrinter::new();

        assert_eq!(printer.build_string_from_expr(&expr), target);
    }

    //collections tests
    #[test]
    fn list_should_print() {
        use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

        let (inputs, env) = collection_proc_visit_inputs_and_env();
        let parser = rholang_parser::RholangParser::new();

        // Create list: [P, *x, 7...ignored]
        let proc = ParBuilderUtil::create_ast_list(
            vec![
                ParBuilderUtil::create_ast_proc_var("P", &parser),
                ParBuilderUtil::create_ast_eval_name_var("x", &parser),
                ParBuilderUtil::create_ast_long_literal(7, &parser),
            ],
            Some(ParBuilderUtil::create_ast_var("ignored")),
            &parser,
        );

        let mut printer = PrettyPrinter::create(0, 2);
        let normalizer_result: Result<ProcVisitOutputs, InterpreterError> =
            normalize_ann_proc(&proc, inputs.clone(), &env, &parser);
        let normalizer_result_as_par = &normalizer_result.unwrap().par;
        let result = printer.build_string_from_message(normalizer_result_as_par);

        assert_eq!(result, "[x0, x1, 7...free0]");
    }

    #[test]
    fn set_should_print() {
        use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

        let (inputs, env) = collection_proc_visit_inputs_and_env();
        let parser = rholang_parser::RholangParser::new();

        // Create set: Set(P, *x, 7...ignored)
        let proc = ParBuilderUtil::create_ast_set(
            vec![
                ParBuilderUtil::create_ast_proc_var("P", &parser),
                ParBuilderUtil::create_ast_eval_name_var("x", &parser),
                ParBuilderUtil::create_ast_long_literal(7, &parser),
            ],
            Some(ParBuilderUtil::create_ast_var("ignored")),
            &parser,
        );

        let mut printer = PrettyPrinter::create(0, 2);
        let normalizer_result: Result<ProcVisitOutputs, InterpreterError> =
            normalize_ann_proc(&proc, inputs.clone(), &env, &parser);
        let normalizer_result_as_par = &normalizer_result.unwrap().par;
        let result = printer.build_string_from_message(normalizer_result_as_par);

        assert_eq!(result, "Set(7, x1, x0...free0)");
    }

    #[test]
    fn map_should_print() {
        use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

        let (inputs, env) = collection_proc_visit_inputs_and_env();
        let parser = rholang_parser::RholangParser::new();

        // Create map: {7 : "Seven", P : *x...ignored}
        let proc = ParBuilderUtil::create_ast_map(
            vec![
                ParBuilderUtil::create_ast_key_value_pair(
                    ParBuilderUtil::create_ast_long_literal(7, &parser),
                    ParBuilderUtil::create_ast_string_literal("Seven", &parser),
                ),
                ParBuilderUtil::create_ast_key_value_pair(
                    ParBuilderUtil::create_ast_proc_var("P", &parser),
                    ParBuilderUtil::create_ast_eval_name_var("x", &parser),
                ),
            ],
            Some(ParBuilderUtil::create_ast_var("ignored")),
            &parser,
        );

        let mut printer = PrettyPrinter::create(0, 2);
        let normalizer_result: Result<ProcVisitOutputs, InterpreterError> =
            normalize_ann_proc(&proc, inputs.clone(), &env, &parser);
        let normalizer_result_as_par = &normalizer_result.unwrap().par;
        let result = printer.build_string_from_message(normalizer_result_as_par);

        assert_eq!(result, "{7 : \"Seven\", x0 : x1...free0}");
    }

    #[test]
    fn map_should_print_commas_correctly() {
        use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

        let (inputs, env) = collection_proc_visit_inputs_and_env();
        let parser = rholang_parser::RholangParser::new();

        // Create map: {"c" : 3, "b" : 2, "a" : 1}
        let proc = ParBuilderUtil::create_ast_map(
            vec![
                ParBuilderUtil::create_ast_key_value_pair(
                    ParBuilderUtil::create_ast_string_literal("c", &parser),
                    ParBuilderUtil::create_ast_long_literal(3, &parser),
                ),
                ParBuilderUtil::create_ast_key_value_pair(
                    ParBuilderUtil::create_ast_string_literal("b", &parser),
                    ParBuilderUtil::create_ast_long_literal(2, &parser),
                ),
                ParBuilderUtil::create_ast_key_value_pair(
                    ParBuilderUtil::create_ast_string_literal("a", &parser),
                    ParBuilderUtil::create_ast_long_literal(1, &parser),
                ),
            ],
            None,
            &parser,
        );

        let mut printer = PrettyPrinter::new();
        let normalizer_result: Result<ProcVisitOutputs, InterpreterError> =
            normalize_ann_proc(&proc, inputs.clone(), &env, &parser);
        let normalizer_result_as_par = &normalizer_result.unwrap().par;
        let result = printer.build_string_from_message(normalizer_result_as_par);

        let target = r#"{"a" : 1, "b" : 2, "c" : 3}"#;
        assert_eq!(result, target);
    }
}

// ===========================================================================
// ★ R3 — the recorded mutation tables, EXECUTED
// ===========================================================================

#[cfg(test)]
mod drive_mutations {
    //! # The differential's teeth, run rather than remembered
    //!
    //! [`super::drive`]'s module documentation carries a nine-row table of
    //! deliberate defects with a "caught / not caught" column. Every row was
    //! produced by editing this file by hand, running the suite, writing down
    //! the outcome, and reverting the edit. Nothing re-ran any of them.
    //!
    //! That is the shape of an un-failable guard. The claim "the differential
    //! has teeth" was a claim about an afternoon: a later refactor that stopped
    //! the differential from separating one of these mutants would leave the
    //! table asserting that it still does, and the table is what a reader
    //! consults before deciding a change is safe.
    //!
    //! The class of behaviour the differential excludes is *this drive,
    //! defective*, and nothing but the drive can supply a member of it — so the
    //! drive supplies them, under [`super::drive::DriveMutation`]. Each row
    //! below is now a call, not a memory.
    //!
    //! ## What each row asserts
    //!
    //! For a mutation `m` and a witness term `t`, two things together, because
    //! either alone is satisfiable by an accident:
    //!
    //! * **the control** — the UNMUTATED drive agrees with the recursive twin
    //!   on `t`, so `t` is a term the differential passes today, and
    //! * **the leg** — the mutated drive DISAGREES with the recursive twin on
    //!   `t`, so the comparison the differential performs is one that separates
    //!   this defect.
    //!
    //! A comparator that rejected everything would fail the control; one that
    //! accepted everything would fail the leg. Both directions, in the shape of
    //! `rholang/tests/normalize_oracle_provenance.rs::the_provenance_check_can_go_red`.
    //!
    //! ## The ninth row is a theorem, not a test
    //!
    //! `ParK`'s permuted category-length array is recorded as "not caught — an
    //! EQUIVALENT mutant". Equivalence is a claim about the mutant's semantics,
    //! and it was itself unchecked — which is the more dangerous of the two
    //! readings, because "the suite cannot see this defect" and "this is not a
    //! defect" are indistinguishable from the outside and have opposite
    //! consequences. It is proven at [`super::drive::render_par_categories`] and
    //! executed at [`the_category_partition_is_not_observable`], over every one
    //! of the 8! = 40,320 permutations.

    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::var::VarInstance;
    use models::rhoapi::{EVar, Expr, Match, MatchCase, New, Par, Receive, ReceiveBind, Send, Var};

    use super::drive::{
        every_mutation, mutations_that_are_equivalent, mutations_the_differential_must_separate,
        render_par_categories, with_mutation, DriveMutation,
    };
    use super::PrettyPrinter;

    // -----------------------------------------------------------------------
    // row 9 — the equivalent mutant, as a checked theorem
    // -----------------------------------------------------------------------

    /// Every permutation of every length vector renders byte-identically, and
    /// identically to `children.join(separator)`.
    ///
    /// This is the executed form of the proof at
    /// [`super::drive::render_par_categories`]. It is exhaustive in the
    /// permutation (all 8! of them) and representative in the shape: the length
    /// vectors below cover the empty partition, a single non-empty category,
    /// several non-empty categories with zeroes interleaved (the case where
    /// `prev_non_empty` matters), and every category non-empty.
    #[test]
    fn the_category_partition_is_not_observable() {
        /// Heap's algorithm — every permutation of an 8-element array.
        fn permutations(mut a: [usize; 8], k: usize, out: &mut Vec<[usize; 8]>) {
            match k {
                1 => out.push(a),
                _ => {
                    for i in 0..k {
                        permutations(a, k - 1, out);
                        match k % 2 {
                            0 => a.swap(i, k - 1),
                            _ => a.swap(0, k - 1),
                        }
                    }
                }
            }
        }

        let vectors: [[usize; 8]; 6] = [
            [0, 0, 0, 0, 0, 0, 0, 0],
            [3, 0, 0, 0, 0, 0, 0, 0],
            [1, 0, 2, 0, 0, 1, 0, 0],
            [0, 0, 0, 0, 0, 0, 0, 4],
            [1, 1, 1, 1, 1, 1, 1, 1],
            [2, 0, 1, 3, 0, 0, 2, 1],
        ];
        let separator = " |\n  ";

        let mut checked = 0usize;
        for lengths in vectors {
            let total: usize = lengths.iter().sum();
            let children: Vec<String> = (0..total).map(|i| format!("c{i}")).collect();
            let expected = children.join(separator);

            // The join identity itself — the proof's conclusion, not just its
            // invariance. Without this the theorem could hold vacuously by the
            // function returning a constant.
            assert_eq!(
                render_par_categories(&children, lengths, separator),
                expected,
                "the grouped render must equal `children.join(separator)` for {lengths:?}"
            );

            let mut perms = Vec::with_capacity(40_320);
            permutations(lengths, 8, &mut perms);
            assert_eq!(
                perms.len(),
                40_320,
                "Heap's algorithm must emit 8! permutations"
            );
            for perm in perms {
                assert_eq!(
                    render_par_categories(&children, perm, separator),
                    expected,
                    "permuting the category lengths from {lengths:?} to {perm:?} CHANGED the \
                     render. The mutation table calls this an equivalent mutant; if this \
                     fires, it is not one, and it is an uncaught defect rather than a \
                     harmless permutation."
                );
                checked += 1;
            }
        }

        // N2: the exhaustion actually ran, and on non-degenerate data. A vector
        // of all zeroes permutes to itself, so without a populated vector this
        // test would be 40,320 comparisons of "" against "".
        assert_eq!(checked, 6 * 40_320, "every vector must be exhausted");
        let populated = vectors
            .iter()
            .filter(|v| v.iter().sum::<usize>() > 0)
            .count();
        assert!(populated >= 5, "the corpus must be mostly non-degenerate");
    }

    // -----------------------------------------------------------------------
    // rows 1-8 — the mutants the differential must separate
    // -----------------------------------------------------------------------

    fn gint(n: i64) -> Par {
        models::par_from_default! {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::GInt(n)),
            }],
            ..Default::default()
        }
    }

    fn gstring(s: &str) -> Par {
        models::par_from_default! {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::GString(s.to_string())),
            }],
            ..Default::default()
        }
    }

    fn bound_var(level: i32) -> Par {
        models::par_from_default! {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::EVarBody(EVar {
                    v: Some(Var {
                        var_instance: Some(VarInstance::BoundVar(level)),
                    }),
                })),
            }],
            ..Default::default()
        }
    }

    fn free_var(level: i32) -> Par {
        models::par_from_default! {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::EVarBody(EVar {
                    v: Some(Var {
                        var_instance: Some(VarInstance::FreeVar(level)),
                    }),
                })),
            }],
            ..Default::default()
        }
    }

    fn bind(source: Par, patterns: Vec<Par>, free_count: i32) -> ReceiveBind {
        ReceiveBind {
            patterns,
            source: Some(source),
            remainder: None,
            free_count,
        }
    }

    /// The witness corpus. Every mutation must be separated by at least one of
    /// these, and which one is reported, so a term that stops covering its site
    /// shows up as a named gap rather than as silence.
    fn witnesses() -> Vec<(&'static str, Par)> {
        vec![
            // Two binds, the first introducing free variables: the ONLY shape
            // where a sequenced `bound_shift`/`free_shift` differs from a
            // precomputed one.
            (
                "two_binds_that_bind_different_counts",
                models::par_from_default! {
                    receives: vec![Receive {
                        binds: vec![
                            bind(gint(1), vec![free_var(0), free_var(1)], 2),
                            bind(gint(2), vec![free_var(0)], 1),
                        ],
                        body: Some(gstring("body")),
                        persistent: false,
                        peek: false,
                        bind_count: 3,
                        locally_free: vec![],
                        connective_used: false,
                        condition: None,
                    }],
                    ..Default::default()
                },
            ),
            // A case whose pattern binds, so the interposed `AddBoundShift`
            // separates the pattern's level from the source's.
            (
                "match_case_with_a_binding_pattern",
                models::par_from_default! {
                    matches: vec![Match {
                        target: Some(gint(7)),
                        cases: vec![MatchCase {
                            pattern: Some(free_var(0)),
                            source: Some(bound_var(0)),
                            free_count: 2,
                            guard: None,
                        }],
                        locally_free: vec![],
                        connective_used: false,
                    }],
                    ..Default::default()
                },
            ),
            // A send whose data move printer state before the channel is read.
            ("send_whose_data_bind", models::par_from_default! {
                sends: vec![Send {
                    chan: Some(bound_var(0)),
                    data: vec![
                        models::par_from_default! {
                            news: vec![New {
                                bind_count: 2,
                                p: Some(bound_var(0)),
                                uri: vec![],
                                injections: std::collections::BTreeMap::new(),
                                locally_free: vec![],
                            }],
                            ..Default::default()
                        },
                        gint(9),
                    ],
                    persistent: false,
                    locally_free: vec![],
                    connective_used: false,
                }],
                ..Default::default()
            }),
            // A `New` whose body reads the variables it introduced.
            ("new_binding_two_names", models::par_from_default! {
                news: vec![New {
                    bind_count: 2,
                    p: Some(models::par_from_default! {
                        exprs: vec![bound_var(0).exprs[0].clone(), bound_var(1).exprs[0].clone()],
                        ..Default::default()
                    }),
                    uri: vec![],
                    injections: std::collections::BTreeMap::new(),
                    locally_free: vec![],
                }],
                ..Default::default()
            }),
            // Two distinguishable exprs in one category.
            ("par_with_two_distinct_exprs", models::par_from_default! {
                exprs: vec![
                    Expr {
                        expr_instance: Some(ExprInstance::GInt(1)),
                    },
                    Expr {
                        expr_instance: Some(ExprInstance::GString("z".into())),
                    },
                ],
                ..Default::default()
            }),
            // A `New`-bound name used INSIDE a receive whose source is rendered
            // as a channel first: the `*` prefix is suppressed only while
            // `is_building_channel` is still set.
            (
                "new_name_read_after_a_channel_render",
                models::par_from_default! {
                    news: vec![New {
                        bind_count: 1,
                        p: Some(models::par_from_default! {
                            receives: vec![Receive {
                                binds: vec![bind(bound_var(0), vec![free_var(0)], 1)],
                                body: Some(bound_var(0)),
                                persistent: false,
                                peek: false,
                                bind_count: 1,
                                locally_free: vec![],
                                connective_used: false,
                                condition: None,
                            }],
                            ..Default::default()
                        }),
                        uri: vec![],
                        injections: std::collections::BTreeMap::new(),
                        locally_free: vec![],
                    }],
                    ..Default::default()
                },
            ),
            // ★ A receive whose `where` guard NAMES the variable its bind
            // introduces. The guard is normalized in the body's environment
            // (`p_input_normalizer`'s `InputPhase::Guard`), so it must render
            // after the interposed `AddBoundShift`; rendering it before prints
            // the same binder at a different level. The guard has to mention a
            // bound variable or the two orderings are indistinguishable.
            (
                "receive_with_a_where_guard_over_its_binder",
                models::par_from_default! {
                    receives: vec![Receive {
                        binds: vec![bind(gstring("chan"), vec![free_var(0)], 1)],
                        body: Some(bound_var(0)),
                        persistent: false,
                        peek: false,
                        bind_count: 1,
                        locally_free: vec![],
                        connective_used: false,
                        condition: Some(bound_var(0)),
                    }],
                    ..Default::default()
                },
            ),
            // A bind with two patterns and a source that is distinguishable
            // from them, so swapping the pop order is observable.
            (
                "bind_with_two_patterns_and_a_distinct_source",
                models::par_from_default! {
                    receives: vec![Receive {
                        binds: vec![bind(
                            gstring("SOURCE"),
                            vec![gstring("P0"), gstring("P1")],
                            0,
                        )],
                        body: Some(gstring("body")),
                        persistent: false,
                        peek: false,
                        bind_count: 0,
                        locally_free: vec![],
                        connective_used: false,
                        condition: None,
                    }],
                    ..Default::default()
                },
            ),
        ]
    }

    /// Render `term` through both public traversing entry points, so a mutation
    /// visible through only one of them still counts.
    fn render(term: &Par) -> (String, String) {
        (
            PrettyPrinter::new().build_string_from_message(term),
            PrettyPrinter::new().build_channel_string(term),
        )
    }

    fn render_recursive(term: &Par) -> (String, String) {
        (
            PrettyPrinter::new().oracle_build_string_from_message(term),
            PrettyPrinter::new().oracle_build_channel_string(term),
        )
    }

    /// ★ The executed mutation table.
    #[test]
    fn the_recorded_mutation_table_is_executable() {
        let corpus = witnesses();

        // The control, first and separately: every witness is a term the
        // differential passes TODAY. Without this, a "separation" below could be
        // the drive and the twin having always disagreed on that term.
        for (what, term) in &corpus {
            assert_eq!(
                render(term),
                render_recursive(term),
                "the witness `{what}` must be a term the UNMUTATED differential passes, or \
                 the separations below are not attributable to any mutation"
            );
        }

        // …and the mutations must be off by default, so `with_mutation` is what
        // turns them on rather than the harness merely observing a broken drive.
        let mut separated_by: Vec<(DriveMutation, Vec<&str>)> = Vec::new();
        for mutation in mutations_the_differential_must_separate() {
            let mut witnesses_that_separate: Vec<&str> = Vec::new();
            for (what, term) in &corpus {
                let mutated = with_mutation(mutation, || render(term));
                if mutated != render_recursive(term) {
                    witnesses_that_separate.push(what);
                }
            }
            separated_by.push((mutation, witnesses_that_separate));
        }

        let unseparated: Vec<&DriveMutation> = separated_by
            .iter()
            .filter(|(_, ws)| ws.is_empty())
            .map(|(m, _)| m)
            .collect();
        assert!(
            unseparated.is_empty(),
            "the differential does NOT separate {unseparated:?}. The mutation table in \
             `drive`'s documentation records every one of these as caught; if a row is no \
             longer caught, either the drive changed so the mutation is now equivalent — in \
             which case it needs a proof like `render_par_categories`'s — or the witness \
             corpus stopped reaching its site. Full report: {separated_by:?}"
        );

        for (mutation, witnesses) in &separated_by {
            println!(
                "  {mutation:?}: separated by {} witness(es) {witnesses:?}",
                witnesses.len()
            );
        }

        // Every mutation is classified exactly once. Without this the two lists
        // could drift apart and a mutation could quietly belong to neither.
        let mut classified = every_mutation();
        let total = classified.len();
        classified.sort_by_key(|m| format!("{m:?}"));
        classified.dedup();
        assert_eq!(
            classified.len(),
            total,
            "a mutation is classified more than once: {classified:?}"
        );
    }

    /// ★ **The equivalent mutants, asserted EQUAL rather than merely omitted.**
    ///
    /// # `NewMutatesBeforeVariables` — and how a recorded result went stale
    ///
    /// The table records this row as **"✔ 11 tests"**, and when it was written
    /// that was true. `build_variables` then read
    /// `self.bound_shift + i` (`a4c23a58^`), so moving the `bound_shift`
    /// mutation ahead of it shifted every printed name by `bind_count`.
    ///
    /// `a4c23a58` changed the body to `introduced.start + i` — deliberately, so
    /// that "the names printed" and "the indices marked" have a single
    /// authority. That change is right, and it also **silently retired the
    /// mutation**: `build_variables` now reads `introduced` (captured before
    /// either ordering diverges) and `bound_id()` (a function of `base_id` and
    /// `rotation`, not of `bound_shift`), so neither ordering is observable.
    ///
    /// Nothing noticed, because nothing re-ran the table. A reader consulting it
    /// today would believe eleven tests stand between this ordering and a
    /// regression; none do. That is the entire argument for executing a
    /// mutation table rather than recording one, and it was found by executing
    /// this one.
    ///
    /// **Proof of equivalence.** Between the two orderings the only difference
    /// is the value of `pp.bound_shift` during the call to `build_variables`.
    /// `build_variables(introduced)` evaluates
    /// `(0..printed_bind_extent(introduced.count)).map(|i| bound_id() ++ (introduced.start + i))`;
    /// `printed_bind_extent` reads `max_var_count` and its argument, `bound_id`
    /// reads `base_id` and `rotation`, and `introduced` was computed before the
    /// mutation in both orderings. None of the three reads `bound_shift`. Every
    /// other reader of `bound_shift` — the body render, `news_shift_indices` —
    /// runs after the point at which the two orderings have both applied the
    /// same `+= bind_count`. So the two drives are observationally identical. ∎
    ///
    /// **What replaced it.** The property the row was *meant* to protect — that
    /// the printed names come from the PRE-mutation interval — is still real,
    /// and is now policed by [`DriveMutation::NewIntervalReadsPostMutationShift`],
    /// which moves the mutation ahead of `new_bind_range` itself. That one is in
    /// the must-separate list above and is caught.
    #[test]
    fn the_equivalent_mutants_really_are_equivalent() {
        let corpus = witnesses();
        // N2: the corpus reaches a `New` at all, or "identical output" would be
        // the identity of two drives that never visited the site.
        let news = corpus
            .iter()
            .filter(|(_, term)| reaches_a_new(term))
            .count();
        assert!(
            news >= 3,
            "the corpus must contain several `New`s for an equivalence claim about the `New` \
             handler to mean anything; found {news}"
        );

        for mutation in mutations_that_are_equivalent() {
            for (what, term) in &corpus {
                assert_eq!(
                    with_mutation(mutation, || render(term)),
                    render(term),
                    "{mutation:?} is classified EQUIVALENT but changed the render of \
                     `{what}`. Either the proof in this test's documentation is wrong, or \
                     the drive changed and this mutation now has teeth — in which case it \
                     belongs in `mutations_the_differential_must_separate` and the \
                     differential must be shown to catch it."
                );
            }
            println!(
                "  {mutation:?}: byte-identical over {} witnesses",
                corpus.len()
            );
        }
    }

    /// Does this term contain a `New` anywhere the drive will visit?
    fn reaches_a_new(term: &Par) -> bool {
        use models::rust::rholang::par_children::reachable_pars;
        reachable_pars(term).iter().any(|p| !p.news.is_empty())
    }
}

// ===========================================================================
// ★ R3, second table — the `New` REPRESENTATION mutations, EXECUTED
// ===========================================================================

#[cfg(test)]
mod representation_mutations {
    //! # The second recorded table, run rather than remembered
    //!
    //! [`super::drive`]'s module documentation carries **two** mutation tables.
    //! `a8ec319f` made the first one — the traversal's — executable, and in
    //! doing so falsified one of its rows: `New` mutating `bound_shift` before
    //! `build_variables` was recorded as caught by 11 tests, and `a4c23a58` had
    //! since made the two orderings observationally identical. Nothing noticed,
    //! because nothing re-ran the table.
    //!
    //! The second table — five rows about how a `New`'s bound names are
    //! REPRESENTED — was still a record, written the same way and carrying the
    //! same risk. This module executes it.
    //!
    //! # Why this table has a different shape from the first
    //!
    //! A [`super::drive::DriveMutation`] perturbs the worklist driver, so the
    //! recursive twin still computes the unperturbed answer and the
    //! **differential** separates the two: each row names a witness *term*.
    //!
    //! The mutations here perturb pure functions the driver and the twin
    //! **share** — `pretty_printer_oracle.rs` calls
    //! [`super::PrettyPrinter::new_bind_range`] and
    //! [`super::PrettyPrinter::build_variables`] directly — so both forms move
    //! together and *no output differential can see them at all*. What separates
    //! them is a property assertion, so each row names the **assertion that
    //! rejects**, and the harness checks the panic payload for it. A row that
    //! merely said "some test failed" would not distinguish the assertion that
    //! is supposed to reject from an unrelated one that happens to.
    //!
    //! ## Rule N, discharged
    //!
    //! * **N1, the judge.** Every row states, per test, `Red(fragment)`,
    //!   `Green` or `Refused`, and the harness asserts the *whole* verdict
    //!   vector — so a test that stops rejecting AND a test that starts
    //!   rejecting for the wrong reason are both failures. `Red` additionally
    //!   requires the panic payload to contain the fragment, which is the
    //!   assertion's own words.
    //! * **N2, the subject.** The mutations are performed inside the production
    //!   functions, and every registry test reaches them through the public
    //!   entry points (`build_string_from_message` / a `NewBindRange` built by
    //!   hand). The control leg runs the whole registry with no mutation in
    //!   force and requires it green, so a "separation" cannot be a test that
    //!   was already failing.
    //!
    //! # ⚠ The second row is a RESOURCE claim and is not executed as written
    //!
    //! `the interval materialised again, unclamped` is recorded as costing
    //! ">300 s wall, 7.31 GiB peak RSS, thrashing against an 8 GiB cgroup with
    //! `oom_kill 0`" — it is not killed, it HANGS, and a suite that ran it would
    //! hang with it. It is executed as a **slope** instead of a magnitude:
    //! [`super::MAX_MATERIALISED_BIND_COUNT`] bounds the mutant at 100'000
    //! names (400 KB), [`the_materialised_form_costs_four_bytes_per_name`]
    //! measures `4 B/name` against the interval's `0 B/name` and extrapolates
    //! arithmetically to `i32::MAX`, and any fixture past the bound is REFUSED
    //! with an explanatory panic — a refusal the table records as a verdict in
    //! its own right, because "this fixture is where the 8 GiB request lives" is
    //! exactly what the row says.

    use models::rhoapi::Par;

    use super::differential::{
        a_bind_range_answers_membership_exactly_as_a_vector_did,
        a_new_costs_one_range_however_many_names_it_binds,
        a_new_with_a_negative_bind_count_binds_nothing, new_par, quietly,
        the_bytes_a_new_retains_do_not_depend_on_bind_count,
        the_star_prefix_is_exact_across_nested_news, the_star_prefix_survives_past_the_display_cap,
    };
    use super::{
        bytes_retained_for_recorded_bounds, with_bind_range_mutation, BindRangeMutation,
        PrettyPrinter, MATERIALISATION_REFUSED, MAX_MATERIALISED_BIND_COUNT,
    };

    // -----------------------------------------------------------------------
    // the registry and the verdicts
    // -----------------------------------------------------------------------

    /// Pairs each test with its own name, so the two cannot drift.
    macro_rules! registry {
        ($($test:path),* $(,)?) => { vec![$((stringify!($test), $test as fn())),*] };
    }

    /// Every deterministic test that observes the `New` bound's representation.
    ///
    /// ⚠ `the_two_forms_agree_even_where_the_arithmetic_overflows` — the test
    /// the second row of the recorded table names — is deliberately ABSENT, for
    /// two independent reasons. It draws `bind_count` from `any::<i32>()`, so
    /// under the materialising mutant it is the ~8 GiB hang the row measured;
    /// and it is a `proptest`, so a failure would persist a seed into
    /// `proptest-regressions/`, making a deliberate mutation leave a permanent
    /// artefact in the tree. Its property is covered here by
    /// [`the_bytes_a_new_retains_do_not_depend_on_bind_count`] and
    /// [`the_materialised_form_costs_four_bytes_per_name`].
    fn registry() -> Vec<(&'static str, fn())> {
        registry![
            a_new_costs_one_range_however_many_names_it_binds,
            the_bytes_a_new_retains_do_not_depend_on_bind_count,
            a_bind_range_answers_membership_exactly_as_a_vector_did,
            the_star_prefix_survives_past_the_display_cap,
            the_star_prefix_is_exact_across_nested_news,
            a_new_with_a_negative_bind_count_binds_nothing,
        ]
    }

    /// What a registry test must do under one mutation.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Verdict {
        /// The test REJECTS, and its panic payload contains this fragment — the
        /// assertion's own words, so the row records WHICH clause rejected
        /// rather than merely that something did.
        Red(&'static str),
        /// The test passes: this mutation is invisible to it. Recorded rather
        /// than omitted, because "the suite cannot see this defect" and "this
        /// test is not the one that sees it" look identical from outside.
        Green,
        /// The mutation was DECLINED on this test's fixtures by
        /// [`super::MAX_MATERIALISED_BIND_COUNT`]'s guard. Only reachable for
        /// [`BindRangeMutation::IntervalMaterialisedAgain`], and it is evidence
        /// rather than an abstention: it says this fixture is one where the
        /// historical form issues the request the row measured.
        Refused,
    }

    /// What a registry test actually did.
    #[derive(Clone, Debug)]
    enum Observed {
        Green,
        Red(String),
        Refused,
    }

    impl Observed {
        fn agrees_with(&self, expected: &Verdict) -> bool {
            match (self, expected) {
                (Observed::Green, Verdict::Green) => true,
                (Observed::Refused, Verdict::Refused) => true,
                (Observed::Red(payload), Verdict::Red(fragment)) => payload.contains(fragment),
                _ => false,
            }
        }

        fn summary(&self) -> String {
            match self {
                Observed::Green => String::from("GREEN"),
                Observed::Refused => String::from("REFUSED"),
                Observed::Red(payload) => {
                    format!("RED({})", payload.lines().next().unwrap_or("").trim())
                }
            }
        }
    }

    /// One row of the recorded table.
    struct Row {
        mutation: BindRangeMutation,
        /// The row as `drive`'s documentation states it, so a reader can line
        /// the two up without a second lookup.
        recorded: &'static str,
        /// Every registry test, with the verdict this row asserts. Checked for
        /// completeness against [`registry`], so a new test cannot be added
        /// without every row deciding what it does.
        expected: &'static [(&'static str, Verdict)],
    }

    // The assertion fragments below are substrings of the failure messages the
    // named tests already carry. They are what makes a row say WHICH clause
    // rejected; if an assertion is reworded, the row goes red until it is
    // re-stated, which is the intended coupling.
    const RECORDED_INTERVAL_TRUNCATED: &str =
        "one entry with a truncated `count` means the marking was clamped";
    const EXTENT_NOT_BOUNDING: &str = "`printed_bind_extent` is not bounding the rendered";
    const MEMBERSHIP_DIVERGED: &str = "membership diverged at start = ";
    const STAR_LOST_PAST_CAP: &str = "the recorded interval is being truncated at `max_var_count`";
    const CAP_NOT_IN_FORCE: &str = "the display cap is not in force";
    const STAR_LOST_IN_NEST: &str = "lost its `*` prefix — every slot in";
    const NEGATIVE_READ_UNSIGNED: &str = "is being read as unsigned somewhere";
    const FOOTPRINT_MOVED: &str = "MOVED with `bind_count`";

    /// ★ THE EXECUTED TABLE. Every row below was measured by running it; the
    /// `recorded` column is what the documentation said before it was.
    fn table() -> Vec<Row> {
        vec![
            Row {
                mutation: BindRangeMutation::RecordedIntervalReClamped,
                recorded: "the recorded interval re-clamped to `max_var_count` (`bd7cb45f`'s \
                           form) — 3 tests",
                expected: &[
                    (
                        "a_new_costs_one_range_however_many_names_it_binds",
                        Verdict::Red(RECORDED_INTERVAL_TRUNCATED),
                    ),
                    (
                        "the_bytes_a_new_retains_do_not_depend_on_bind_count",
                        Verdict::Green,
                    ),
                    (
                        "a_bind_range_answers_membership_exactly_as_a_vector_did",
                        Verdict::Green,
                    ),
                    (
                        "the_star_prefix_survives_past_the_display_cap",
                        Verdict::Red(STAR_LOST_PAST_CAP),
                    ),
                    (
                        "the_star_prefix_is_exact_across_nested_news",
                        Verdict::Red(STAR_LOST_IN_NEST),
                    ),
                    (
                        "a_new_with_a_negative_bind_count_binds_nothing",
                        Verdict::Green,
                    ),
                ],
            },
            Row {
                mutation: BindRangeMutation::IntervalMaterialisedAgain,
                recorded: "the interval materialised again, unclamped (the pre-`bd7cb45f` form) \
                           — the raw property test; 6.92 s / 4 MiB peak becomes >300 s / 7.31 \
                           GiB peak under an 8 GiB cgroup",
                expected: &[
                    (
                        "a_new_costs_one_range_however_many_names_it_binds",
                        Verdict::Refused,
                    ),
                    (
                        "the_bytes_a_new_retains_do_not_depend_on_bind_count",
                        Verdict::Red(FOOTPRINT_MOVED),
                    ),
                    (
                        "a_bind_range_answers_membership_exactly_as_a_vector_did",
                        Verdict::Green,
                    ),
                    (
                        "the_star_prefix_survives_past_the_display_cap",
                        Verdict::Green,
                    ),
                    (
                        "the_star_prefix_is_exact_across_nested_news",
                        Verdict::Green,
                    ),
                    (
                        "a_new_with_a_negative_bind_count_binds_nothing",
                        Verdict::Green,
                    ),
                ],
            },
            Row {
                mutation: BindRangeMutation::ContainsWidenedToI64,
                recorded: "`contains` widened to `i64` (the mathematical interval, not the \
                           shipped wrap) — `a_bind_range_answers_membership_exactly_as_a_\
                           vector_did`",
                expected: &[
                    (
                        "a_new_costs_one_range_however_many_names_it_binds",
                        Verdict::Green,
                    ),
                    (
                        "the_bytes_a_new_retains_do_not_depend_on_bind_count",
                        Verdict::Green,
                    ),
                    (
                        "a_bind_range_answers_membership_exactly_as_a_vector_did",
                        Verdict::Red(MEMBERSHIP_DIVERGED),
                    ),
                    (
                        "the_star_prefix_survives_past_the_display_cap",
                        Verdict::Green,
                    ),
                    (
                        "the_star_prefix_is_exact_across_nested_news",
                        Verdict::Green,
                    ),
                    (
                        "a_new_with_a_negative_bind_count_binds_nothing",
                        Verdict::Green,
                    ),
                ],
            },
            Row {
                mutation: BindRangeMutation::ContainsDropsEmptyGuard,
                recorded: "the `is_empty` guard dropped from `contains` (negative `count` read \
                           as unsigned) — `a_new_with_a_negative_bind_count_binds_nothing`",
                expected: &[
                    (
                        "a_new_costs_one_range_however_many_names_it_binds",
                        Verdict::Green,
                    ),
                    (
                        "the_bytes_a_new_retains_do_not_depend_on_bind_count",
                        Verdict::Green,
                    ),
                    (
                        "a_bind_range_answers_membership_exactly_as_a_vector_did",
                        Verdict::Red(MEMBERSHIP_DIVERGED),
                    ),
                    (
                        "the_star_prefix_survives_past_the_display_cap",
                        Verdict::Green,
                    ),
                    (
                        "the_star_prefix_is_exact_across_nested_news",
                        Verdict::Green,
                    ),
                    (
                        "a_new_with_a_negative_bind_count_binds_nothing",
                        Verdict::Red(NEGATIVE_READ_UNSIGNED),
                    ),
                ],
            },
            Row {
                mutation: BindRangeMutation::PrintedExtentOneTooMany,
                recorded: "`printed_bind_extent` off by one (the display cap alone) — \
                           `a_new_costs_one_range_however_many_names_it_binds`",
                expected: &[
                    (
                        "a_new_costs_one_range_however_many_names_it_binds",
                        Verdict::Red(EXTENT_NOT_BOUNDING),
                    ),
                    (
                        "the_bytes_a_new_retains_do_not_depend_on_bind_count",
                        Verdict::Green,
                    ),
                    (
                        "a_bind_range_answers_membership_exactly_as_a_vector_did",
                        Verdict::Green,
                    ),
                    (
                        "the_star_prefix_survives_past_the_display_cap",
                        Verdict::Red(CAP_NOT_IN_FORCE),
                    ),
                    (
                        "the_star_prefix_is_exact_across_nested_news",
                        Verdict::Green,
                    ),
                    (
                        "a_new_with_a_negative_bind_count_binds_nothing",
                        Verdict::Green,
                    ),
                ],
            },
            Row {
                mutation: BindRangeMutation::PrintedExtentOneTooFew,
                recorded: "the other direction of the same off-by-one, which the recorded row \
                           did not enumerate",
                expected: &[
                    (
                        "a_new_costs_one_range_however_many_names_it_binds",
                        Verdict::Red(EXTENT_NOT_BOUNDING),
                    ),
                    (
                        "the_bytes_a_new_retains_do_not_depend_on_bind_count",
                        Verdict::Green,
                    ),
                    (
                        "a_bind_range_answers_membership_exactly_as_a_vector_did",
                        Verdict::Green,
                    ),
                    (
                        "the_star_prefix_survives_past_the_display_cap",
                        Verdict::Red(CAP_NOT_IN_FORCE),
                    ),
                    (
                        "the_star_prefix_is_exact_across_nested_news",
                        Verdict::Green,
                    ),
                    (
                        "a_new_with_a_negative_bind_count_binds_nothing",
                        Verdict::Green,
                    ),
                ],
            },
        ]
    }

    /// Every mutation exactly once. A new variant that is not given a row is a
    /// compile error at the exhaustive `match`, and a duplicated one fails the
    /// assertion.
    fn every_mutation_has_exactly_one_row(rows: &[Row]) {
        for row in rows {
            match row.mutation {
                BindRangeMutation::RecordedIntervalReClamped
                | BindRangeMutation::IntervalMaterialisedAgain
                | BindRangeMutation::ContainsWidenedToI64
                | BindRangeMutation::ContainsDropsEmptyGuard
                | BindRangeMutation::PrintedExtentOneTooMany
                | BindRangeMutation::PrintedExtentOneTooFew => {}
            }
        }
        let mut seen: Vec<String> = rows.iter().map(|r| format!("{:?}", r.mutation)).collect();
        let total = seen.len();
        seen.sort();
        seen.dedup();
        assert_eq!(
            seen.len(),
            total,
            "a mutation has more than one row: {seen:?}"
        );
        assert_eq!(
            total, 6,
            "the table has {total} rows; the enum has 6 variants, and a variant without a row is \
             a mutation nothing measures"
        );
    }

    // -----------------------------------------------------------------------
    // the runner
    // -----------------------------------------------------------------------

    /// ★ The executed representation table.
    #[test]
    fn the_recorded_representation_table_is_executable() {
        let rows = table();
        every_mutation_has_exactly_one_row(&rows);

        // N2, the control: with NO mutation in force the whole registry is
        // green. Without this, a "rejection" below could be a test that was
        // already failing for an unrelated reason.
        for (name, test) in registry() {
            assert!(
                quietly(test).is_ok(),
                "the registry test `{name}` fails with NO mutation in force, so nothing it does \
                 under one is attributable to the mutation"
            );
        }

        let mut report = String::new();
        for row in &rows {
            // The row decides every registry test, and only registry tests.
            let registered: Vec<&str> = registry().into_iter().map(|(name, _)| name).collect();
            let decided: Vec<&str> = row.expected.iter().map(|(name, _)| *name).collect();
            assert_eq!(
                decided, registered,
                "row {:?} decides {decided:?} but the registry is {registered:?}. Every row must \
                 state a verdict for every test, or a row could quietly stop covering one",
                row.mutation
            );

            let mut reds = 0usize;
            report.push_str(&format!(
                "\n  {:?}\n    recorded: {}\n",
                row.mutation, row.recorded
            ));
            for (name, test) in registry() {
                let expected = row
                    .expected
                    .iter()
                    .find(|(n, _)| *n == name)
                    .map(|(_, verdict)| *verdict)
                    .expect("checked exhaustive above");

                let observed = match with_bind_range_mutation(row.mutation, || quietly(test)) {
                    Ok(()) => Observed::Green,
                    Err(payload) if payload.contains(MATERIALISATION_REFUSED) => Observed::Refused,
                    Err(payload) => Observed::Red(payload),
                };
                if matches!(observed, Observed::Red(_)) {
                    reds += 1;
                }
                report.push_str(&format!("    {name}: {}\n", observed.summary()));

                assert!(
                    observed.agrees_with(&expected),
                    "under {:?}, `{name}` was expected to be {expected:?} but was {}.\n\nA test \
                     that stopped rejecting is either a stale row — the code changed and this \
                     mutant is now EQUIVALENT, which needs a proof and a replacement property, \
                     the way `NewMutatesBeforeVariables` did — or a fixture that stopped \
                     reaching the site. A test that started rejecting, or rejected with a \
                     different assertion than the row names, means the row no longer describes \
                     what the suite does.\n\nFull payload: {:?}",
                    row.mutation,
                    observed.summary(),
                    observed
                );
            }

            assert!(
                reds >= 1,
                "NO test rejects under {:?}. Either the mutation is now equivalent — in which \
                 case it needs a proof, and the property it used to protect needs a check that \
                 still has teeth — or every fixture that reached its site is gone. A row with no \
                 red is a row that measures nothing.",
                row.mutation
            );
        }
        println!("{report}");
    }

    /// ⚠ **The table's judge is itself a checker, and a checker that has not
    /// been shown to reject is the defect it is supposed to catch.**
    ///
    /// [`Observed::agrees_with`] is what turns a run into a verdict. If it
    /// accepted everything, [`the_recorded_representation_table_is_executable`]
    /// would pass with every row mis-stated and would still read as evidence.
    /// So it is exercised on a REAL observation — one row of the table, run for
    /// real — against the four ways a row can be wrong.
    #[test]
    fn the_tables_own_judge_can_reject() {
        fn observe(mutation: Option<BindRangeMutation>, test: fn()) -> Observed {
            let run = || match quietly(test) {
                Ok(()) => Observed::Green,
                Err(payload) if payload.contains(MATERIALISATION_REFUSED) => Observed::Refused,
                Err(payload) => Observed::Red(payload),
            };
            match mutation {
                Some(m) => with_bind_range_mutation(m, run),
                None => run(),
            }
        }

        let rejected = observe(
            Some(BindRangeMutation::ContainsDropsEmptyGuard),
            a_new_with_a_negative_bind_count_binds_nothing,
        );
        assert!(
            rejected.agrees_with(&Verdict::Red(NEGATIVE_READ_UNSIGNED)),
            "the row this test stands on no longer holds: {}",
            rejected.summary()
        );
        assert!(
            !rejected.agrees_with(&Verdict::Green),
            "a REJECTION satisfied a `Green` row, so a row that claims a mutation is invisible \
             would pass while the mutation is caught"
        );
        assert!(
            !rejected.agrees_with(&Verdict::Refused),
            "a REJECTION satisfied a `Refused` row, so a declined fixture and a rejected \
             assertion are indistinguishable to the table"
        );
        assert!(
            !rejected.agrees_with(&Verdict::Red(MEMBERSHIP_DIVERGED)),
            "a rejection satisfied a row naming a DIFFERENT assertion. The fragments are what \
             make a row say WHICH clause rejected; if any rejection matches any fragment, the \
             table records only that something failed"
        );

        let passed = observe(None, a_new_with_a_negative_bind_count_binds_nothing);
        assert!(
            passed.agrees_with(&Verdict::Green),
            "the control leg is not green, so nothing below it is attributable: {}",
            passed.summary()
        );
        assert!(
            !passed.agrees_with(&Verdict::Red(NEGATIVE_READ_UNSIGNED)),
            "a PASS satisfied a `Red` row, so a mutation that stopped being caught would still \
             read as caught — which is exactly how the `bound_shift` row survived `a4c23a58`"
        );
    }

    // -----------------------------------------------------------------------
    // row 2, quantified — the slope, never the magnitude
    // -----------------------------------------------------------------------

    /// ★ The recorded row's `Θ(bind_count)` versus `Θ(1)`, executed as a SLOPE
    /// at bounded scale and extrapolated arithmetically to the magnitude the
    /// row cites.
    ///
    /// # What is measured, and what is only computed
    ///
    /// | quantity | how it is obtained |
    /// |---|---|
    /// | interval footprint at `bind_count` <= 100'000 | MEASURED, from `news_shift_indices`'s own capacity |
    /// | materialised footprint at `bind_count` <= 100'000 | MEASURED, at the production site under the mutant |
    /// | `4 B` per name | MEASURED as the exact difference, at five counts spanning five orders of magnitude |
    /// | ~8 GiB at `bind_count = i32::MAX` | COMPUTED from that slope; never allocated |
    /// | ">300 s, 7.31 GiB RSS, `oom_kill 0`" | the row's own prior measurement, retained as documentation and deliberately not reproduced |
    ///
    /// # The mutant is byte-equivalent, and that is asserted here
    ///
    /// Materialising the interval changes what the printer HOLDS and not what
    /// it prints, so every string comparison in the suite is blind to it. That
    /// is not an assumption: this test renders the same corpus with and without
    /// the mutant and requires the bytes to be identical. It is also the reason
    /// the table's row for this mutation is `Green` almost everywhere — an
    /// output differential cannot see a pure-cost defect, and a table that
    /// claimed otherwise would be wrong.
    #[test]
    fn the_materialised_form_costs_four_bytes_per_name() {
        let counts = [0i32, 1, 128, 1_000, 10_000, MAX_MATERIALISED_BIND_COUNT];
        let mut series: Vec<(i32, usize, usize)> = Vec::with_capacity(counts.len());

        for bind_count in counts {
            let term = new_par(bind_count, Par::default());

            let mut interval_printer = PrettyPrinter::new();
            let interval_bytes_render = interval_printer.build_string_from_message(&term);
            let interval_bytes = bytes_retained_for_recorded_bounds(&interval_printer);

            let (materialised_render, materialised_bytes) =
                with_bind_range_mutation(BindRangeMutation::IntervalMaterialisedAgain, || {
                    let mut printer = PrettyPrinter::new();
                    let render = printer.build_string_from_message(&term);
                    let bytes = bytes_retained_for_recorded_bounds(&printer);
                    (render, bytes)
                });

            // The mutant is a pure-COST defect: not one byte moves.
            assert_eq!(
                interval_bytes_render, materialised_render,
                "the materialising mutant changed the RENDER at bind_count = {bind_count}. It is \
                 supposed to be byte-equivalent — it allocates and retains, and returns the same \
                 interval — so if this fires, the table's `Green` verdicts for it are describing \
                 a different mutation than the one being run"
            );

            // ★ …and the assertion the recorded row NAMES cannot reject either.
            // `the_two_forms_agree_even_where_the_arithmetic_overflows` compares
            // the driver against the recursive twin, and the twin CALLS
            // `new_bind_range`, so both forms materialise together and their
            // comparison is between two identical strings. Whatever that row's
            // ✔ recorded, it was not an assertion rejecting.
            let twin =
                with_bind_range_mutation(BindRangeMutation::IntervalMaterialisedAgain, || {
                    PrettyPrinter::new().oracle_build_string_from_message(&term)
                });
            assert_eq!(
                materialised_render, twin,
                "the driver and the recursive twin DISAGREED under the materialising mutant at \
                 bind_count = {bind_count}. The row that records this mutation credits \
                 `the_two_forms_agree_even_where_the_arithmetic_overflows` with catching it, and \
                 the reason that credit is wrong is that both forms call the mutated function. \
                 If this fires they no longer share the site, and the row's disposition has to be \
                 re-derived rather than corrected"
            );

            series.push((bind_count, interval_bytes, materialised_bytes));
        }

        let (_, interval_at_zero, _) = series[0];
        for (bind_count, interval_bytes, materialised_bytes) in &series {
            // Θ(1): the interval's footprint does not move at all.
            assert_eq!(
                *interval_bytes, interval_at_zero,
                "the interval form's footprint moved from {interval_at_zero} B to \
                 {interval_bytes} B at bind_count = {bind_count}"
            );
            // Θ(bind_count): the materialised form's does, by exactly 4 B/name.
            assert_eq!(
                *materialised_bytes,
                interval_at_zero + (*bind_count as usize) * std::mem::size_of::<i32>(),
                "the materialised form retained {materialised_bytes} B at bind_count = \
                 {bind_count}; the pre-`bd7cb45f` form held one `i32` per name, so the slope \
                 must be exactly {} B/name over a {interval_at_zero} B baseline. Full series \
                 (bind_count, interval, materialised): {series:?}",
                std::mem::size_of::<i32>()
            );
        }

        // ANTI-VACUITY: the series really does span the range it claims a slope
        // over, and the two forms really are separated at the wide end.
        let (widest, interval_wide, materialised_wide) = *series
            .last()
            .expect("the series is non-empty by construction");
        assert_eq!(widest, MAX_MATERIALISED_BIND_COUNT);
        assert!(
            materialised_wide > interval_wide * 1_000,
            "at bind_count = {widest} the materialised form holds {materialised_wide} B against \
             the interval's {interval_wide} B; that is not a separation this test can call a \
             slope"
        );

        // THE EXTRAPOLATION — computed, never allocated. `bind_count` is an
        // attacker-controlled `i32`, so the reachable maximum is `i32::MAX`.
        let at_max = (i32::MAX as i64) * (std::mem::size_of::<i32>() as i64);
        assert_eq!(
            at_max, 8_589_934_588,
            "the fitted line evaluated at `i32::MAX` must be (2^31 - 1) x 4 B"
        );
        let gib = at_max as f64 / (1024.0 * 1024.0 * 1024.0);
        assert!(
            (7.999..8.0).contains(&gib),
            "the extrapolated request is {gib} GiB, which is not the ~8 GiB the row records"
        );
        println!(
            "  materialised form: {} B/name over a {interval_at_zero} B baseline; extrapolated \
             to bind_count = i32::MAX that is {at_max} B ({gib:.9} GiB), measured only up to \
             bind_count = {widest}",
            std::mem::size_of::<i32>()
        );
    }

    /// ⚠ The guard that keeps the row above from being executed as written.
    ///
    /// A fixture that asks the materialising mutant for more than
    /// [`MAX_MATERIALISED_BIND_COUNT`] names must be REFUSED in milliseconds
    /// rather than served. This is the property that makes the whole table safe
    /// to run: without it, a maintainer who adds an `i32::MAX` fixture to any
    /// registry test would not get a failure, they would get a suite that never
    /// finishes.
    #[test]
    fn the_materialising_mutant_refuses_a_fixture_it_cannot_afford() {
        let over = new_par(MAX_MATERIALISED_BIND_COUNT + 1, Par::default());
        let refusal =
            with_bind_range_mutation(BindRangeMutation::IntervalMaterialisedAgain, || {
                quietly(|| {
                    let _ = PrettyPrinter::new().build_string_from_message(&over);
                })
            })
            .expect_err("a fixture past the bound must be refused, not served");
        assert!(
            refusal.contains(MATERIALISATION_REFUSED),
            "the mutant panicked without the refusal marker, so a harness cannot tell a declined \
             fixture from a rejected assertion: {refusal}"
        );
        assert!(
            refusal.contains("8 GiB"),
            "the refusal must carry the magnitude it is declining, or it reads as an arbitrary \
             limit: {refusal}"
        );

        // ...and exactly at the bound it is served, so the guard is a bound and
        // not a blanket refusal.
        let at = new_par(MAX_MATERIALISED_BIND_COUNT, Par::default());
        let bytes = with_bind_range_mutation(BindRangeMutation::IntervalMaterialisedAgain, || {
            let mut printer = PrettyPrinter::new();
            let _ = printer.build_string_from_message(&at);
            bytes_retained_for_recorded_bounds(&printer)
        });
        assert!(
            bytes >= MAX_MATERIALISED_BIND_COUNT as usize * std::mem::size_of::<i32>(),
            "at exactly the bound the mutant retained {bytes} B, which is less than the \
             materialisation it is supposed to have performed"
        );

        // ...and nothing survives the scope: the retention is released with the
        // mutation, so one row of the table cannot leak into the next.
        let after = bytes_retained_for_recorded_bounds(&PrettyPrinter::new());
        assert_eq!(
            after,
            PrettyPrinter::new().news_shift_indices.capacity()
                * std::mem::size_of::<super::NewBindRange>(),
            "the materialised intervals outlived their `with_bind_range_mutation` scope"
        );
    }
}
