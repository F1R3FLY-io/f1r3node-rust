//! # `drive` — the one trampoline the `Par` family's traversals share
//!
//! Every traversal over the `Par` / `Expr` / `ExprInstance` family that this
//! workspace has had to convert away from native recursion has converged on the
//! *same* machine: a value stack, a work stack of defunctionalized
//! continuations, and one LIFO loop. `rho-pure-eval`'s `eval_drive`, the
//! sorter's `sort_drive` and the decoder's `par_codec` each wrote that loop out
//! by hand, with its own copy of the same two invariants. This module is that
//! loop, written once.
//!
//! ## What the machine is
//!
//! A **pushdown machine over a finite input alphabet**, in the CEK sense: the
//! control string is a stack of [`Step`]s, the environment is the visitor plus
//! its threaded [`Traversal::State`], and the continuation is *defunctionalized*
//! — a [`Traversal::Kont`] value on the same stack rather than a native frame.
//! Native stack becomes `O(1)` in both nesting depth and sibling width; the
//! recursion lives in `work`.
//!
//! ```text
//!    work  (LIFO, grows down)              vals  (LIFO)
//!   ┌──────────────────────────┐         ┌──────────────────────────┐
//!   │ Combine(k)               │◀─ pushed│                          │
//!   │   … the parent's shell   │   FIRST │  v₀ v₁ … v_{arity−1}     │
//!   ├──────────────────────────┤         │  ▲                       │
//!   │ Descend(child_{n−1})     │         │  └ produced in push order│
//!   │ …                        │◀─ pushed│    consumed by `combine` │
//!   │ Descend(child_0)         │   LAST, │    in one `split_off`    │
//!   └──────────────────────────┘   pops  └──────────────────────────┘
//!                                  FIRST
//! ```
//!
//! One `Step` produces exactly one [`Traversal::Val`]:
//!
//! * a **leaf** `Descend` pushes one value and no work;
//! * a **branching** `Descend` pushes no value and a *region* of work whose net
//!   effect is one value (Invariant 1, below);
//! * a `Combine` pops `arity` values and produces one.
//!
//! ## ★★ Why `descend` takes a whole node and not a field index
//!
//! This is the binding design constraint of the whole program, and it is
//! **measured, not theoretical**. [`crate::rust::rholang::wire`] §A2 records the
//! obvious factoring — a generated table exposing `fn wire_field(i) -> FieldVal`
//! interpreted by a hand-written driver — and what it cost:
//!
//! > `Par` has eleven fields, so each node cost eleven indirect `wire_field`
//! > calls returning a 32-byte enum by value, plus nine more indirect
//! > `wire_len` calls — **21 indirect calls per node where the derived path has
//! > none**. `perf` put **31.8%** of the profile in the driver loop. The result
//! > was **1.7× slower than the derived `Serialize`**.
//!
//! The resolution, in that module's words: **"bounded-field EMISSION is
//! generated and monomorphic; the TRAMPOLINE is hand-written and generic."**
//! [`Traversal`] is shaped to make that the *only* thing it is comfortable to
//! write:
//!
//! | the shape | what it forbids |
//! |---|---|
//! | [`Traversal::descend`] receives the **whole node** by value and returns only after pushing its region | there is no "give me field `i`" hook to interpret, so a per-field indirection has to be invented before it can be paid for |
//! | [`Traversal::Node`] is a **GAT** (`type Node<'t>`) | the trait is **not object-safe**; `&dyn Traversal` does not compile, so `descend` is a static, monomorphized, inlinable call *by construction* rather than by discipline |
//! | [`Traversal::Val`] / [`Traversal::Kont`] are associated types, not a boxed enum | a value never round-trips through an allocation to satisfy the driver |
//!
//! So a conforming `descend` runs every **bounded** field of its node
//! straight-line — the same codegen the derive gets — and returns at the first
//! **descent**. That is *one* call per node **per suspension**, never one per
//! field. A node with no descents costs exactly one call; a node with `k`
//! descents costs `k + 1`.
//!
//! ⚠ The same profile is why [`crate::rust::rholang::wire::NO_RESUME`] exists:
//! asking `node.wire_program().len()` on every descent — to find out whether
//! there was anything to come back *for* — is a virtual call on the one event a
//! deep term is made of, and it measured **2.75%** of the production-weighted
//! mix. A driver that must interrogate its visitor per node has already lost.
//! Nothing in this module interrogates the visitor except [`Traversal::arity`],
//! which is called only under `debug_assertions`.
//!
//! ## ★ Early exit — [`Outcome::Done`]
//!
//! `combine` may report that the value it just built **is the answer**, at which
//! point the driver drops every pending obligation and returns it. Without this,
//! a comparison traversal would be a *performance-correctness regression*
//! against the code it replaces: `models/src/lib.rs`'s `impl PartialEq for Par`
//! is a `&&` chain over ten fields and stops at the first unequal one, so two
//! large `Par`s that differ in `sends` never touch `exprs`. A post-order fold
//! with no early exit walks both terms to completion. The cost of carrying it is
//! one perfectly-predicted branch per `combine`.
//!
//! ⚠ **What early exit drops.** The pending `vals` are released with
//! `Val`'s own destructor. For `Val = bool` / `Ordering` / `()` that is free;
//! for a `Val` that owns a deep `Par` it is `drop_in_place::<Par>`, which is
//! itself Θ(depth) (`rholang/tests/stack_depth_gate.rs`, subject `par_drop`).
//! An instance whose `Val` owns terms must therefore either not use
//! [`Outcome::Done`] or hand its stack to
//! [`crate::rust::rholang::par_children::dismantle_all`] before returning. The
//! same statement applies verbatim to the `?` abort path, and it is the
//! disposition the hand-written machines already documented.
//!
//! ## ⚠ Scope: this driver serves **borrowing** traversals only
//!
//! [`Traversal::Node`] is `Copy` and lifetime-parametric, which is a deliberate
//! restriction with a name: work items **borrow** the input term. Pushing a
//! child is a reference move and never a `<Par as Clone>::clone` (itself
//! Θ(depth) at 15,914 B/level, debug), which is what makes the conversion a
//! strict improvement rather than a trade.
//!
//! The **by-move** sibling — tearing a term *down* rather than reading it — is
//! [`crate::rust::rholang::par_children::dismantle_all`], and it is a separate
//! mechanism on purpose. `Drop` cannot join this program, and the blocker was
//! **measured rather than argued**
//! (`docs/design/audits/four-quadrant-s0-baseline-2026-07-28.md` §4): adding
//! `impl Drop for Par` and compiling produced
//!
//! ```text
//!   353 diagnostics   ← one per moved-out FIELD, so one `..Default::default()`
//!                        on `Par` emits ten
//!    61 unique (file:line)      ← the number that matters
//!     0 downstream crates       ← the build ABORTED at `models`
//! ```
//!
//! | shape | count |
//! |---|---|
//! | struct-literal / functional record update (`Par { exprs: …, ..Default::default() }`) | **38** |
//! | partial move of a field (`union(acc, p.locally_free)`) | **23** |
//! | full destructure (`let Par { … }`) | **0** |
//!
//! and **that is a floor**: cargo stopped at `models`, so `rholang`, `rspace++`,
//! `casper` and `node` were never reached. `E0509` fires on functional record
//! update and on partial moves, not only on the destructuring the original
//! design counted. A future reader must not rediscover this by trying it.
//!
//! ## The two invariants
//!
//! Both are stated over a *loop-head configuration*: `V` = `vals.len()`, `D` =
//! the number of `Descend` steps pending in `work`, `C` = the number of
//! `Combine` steps pending, and `A` = the sum of [`Traversal::arity`] over those
//! pending `Combine`s.
//!
//! ### Invariant 1 — local suspension well-formedness
//!
//! Everything one `descend` call pushes is a **region**. Scanned in *pop* order
//! (last-pushed first) with a running count `avail` of values the region will
//! have produced so far:
//!
//! ```text
//!   avail := 0
//!   for step in region.iter().rev():
//!       Descend  ⇒  avail += 1
//!       Combine k ⇒  require avail ≥ arity(k);  avail := avail − arity(k) + 1
//!   require avail == 1
//! ```
//!
//! An empty region instead requires that the call pushed exactly one value (it
//! was a leaf). This is the generalization of the hand-written machines'
//! "`descend` must push its `Combine` first, then exactly `arity` children": it
//! accepts *nested* continuations, which that rule rejected —
//! `rho-pure-eval`'s `Eval` node pushes `[Extract, ParK{n}, child × n]`, whose
//! `Extract` consumes what `ParK` produces — while still refusing a `Combine`
//! that was pushed with fewer children than its `arity()` claims.
//!
//! ### Invariant 2 — the deficit invariant
//!
//! ```text
//!   V + D + C − A  ==  1
//! ```
//!
//! Read it as *"the number of values the machine still holds or will produce,
//! net of what it has promised to consume, is exactly one — the answer."*
//!
//! * **Initially** the root is one `Descend`: `0 + 1 + 0 − 0 = 1`. (A root
//!   `Combine` is admissible iff its arity is 0; the driver asserts the
//!   condition rather than the shape.)
//! * **Popping a `Descend`** removes 1 from `D`, and Invariant 1 guarantees the
//!   region it pushes contributes either `(+1 to V)` or `(d + c − a = +1)`.
//!   Net 0.
//! * **Popping a `Combine` `k`** removes 1 from `C` and `arity(k)` from `A`,
//!   then `combine` removes `arity(k)` from `V` and adds 1. Net
//!   `−1 + arity − arity + 1 = 0` **iff `combine` really popped `arity(k)`
//!   values.**
//! * **Termination**: `work` empty ⇒ `D = C = A = 0` ⇒ `V = 1`. ∎
//!
//! ★ The third bullet is the whole point. Invariant 1 makes the descend side
//! balance by construction, so what Invariant 2 actually cross-checks is the
//! **deliberate duplication** between [`Traversal::arity`] and the pops that
//! `combine` performs — two independently written statements of one number. A
//! differential test finds a mismatch only if the corpus happens to contain the
//! witness; this finds it on the first malformed configuration of *any* term.
//!
//! ### Why the final configuration is checked unconditionally
//!
//! Invariant 2's terminal instance (`V == 1` with `work` drained) is asserted
//! *outside* `debug_assertions`, because it is the one configuration a release
//! build can still reject and it costs a single comparison per `drive` call.
//! `models/tests/drive_configuration_gate.rs` watches both messages RED — the
//! running invariant in a debug build, the final one in a release build — by
//! running a deliberately miscounting visitor **in a child process** and
//! observing the child's exit status and stderr. (Never `#[should_panic]`: this
//! workspace's standing rule, because an abort is not catchable and a panic
//! that fails to unwind prints nothing.)
//!
//! ## Instances
//!
//! | instance | `Node` | `Val` | `State` | stage |
//! |---|---|---|---|---|
//! | `rho_pure_eval::eval::EvalTraversal` | `EvNode<'t>` | `EvVal` | `()` | F-1 |
//!
//! ★ There is a structurally identical driver in the `mettail-rust` workspace
//! (`rholang-runtime`). Two drivers, deliberately: `mettail-rust` depends
//! one-way on this repo, so a single shared driver would make every driver
//! change a cross-repo rebuild and couple two independently versioned
//! repositories through a hot generic. The duplicated loop buys repository
//! independence; each is documented as an instance of the other.

use std::fmt::Write as _;

// ===========================================================================
// §A  The alphabet
// ===========================================================================

/// One pending obligation: descend into a node, or run a continuation.
///
/// `Step` is the *only* thing that ever occupies the work stack, which is what
/// makes the two invariants checkable: the driver can read back exactly what a
/// `descend` pushed and score it.
pub enum Step<'t, T: Traversal> {
    /// Visit `node`. Produces exactly one value, possibly by pushing a region
    /// of further work (Invariant 1).
    Descend(T::Node<'t>),
    /// Run `kont`, consuming [`Traversal::arity`] values from the value stack
    /// and producing one.
    Combine(T::Kont<'t>),
}

/// What one [`Traversal::combine`] produced, and what the driver should do with
/// it.
///
/// ★ Spelled as a named enum rather than `ControlFlow<Val, Val>` because both
/// arms carry a value, so `Break` / `Continue` would say nothing about which is
/// which. [`Outcome::Value`] *is* the `Continue` arm and [`Outcome::Done`] *is*
/// the `Break` arm of that formulation.
pub enum Outcome<V> {
    /// An ordinary child value. The driver pushes it and carries on.
    Value(V),
    /// **This is the answer.** The driver abandons every pending obligation and
    /// returns it. See the module docs on what abandoning the value stack costs
    /// when `Val` owns a term.
    Done(V),
}

// ===========================================================================
// §B  The trait
// ===========================================================================

/// A traversal expressed as descend / combine over borrowed input.
///
/// ⚠ Deliberately **not object-safe** — see the module docs. The GAT alone
/// makes `dyn Traversal` ill-formed, which is what guarantees that every
/// `descend` is a direct, monomorphized call and not the 1.7×-slower table
/// interpretation `wire.rs` §A2 measured.
pub trait Traversal: Sized {
    /// A **borrowed** input node. `Copy` because pushing a child must be a
    /// reference move, never a deep clone.
    type Node<'t>: Copy;

    /// The value one `Step` produces.
    type Val;

    /// A defunctionalized continuation: the constructor to apply, the borrowed
    /// *shell* of the node being rebuilt (flags, cached bitsets, remainders —
    /// everything that is not a child), and the child counts needed to slice
    /// the value stack.
    type Kont<'t>;

    /// Mutable state threaded through the whole traversal and owned by the
    /// caller — `()` when there is none, `&mut Vec<u8>` for an emitter, a
    /// `&mut PrettyPrinter` for the printer.
    ///
    /// Kept separate from `&mut self` so the *visitor* can stay the immutable
    /// configuration (the environment, an oracle) while the *state* is what the
    /// traversal writes into.
    type State;

    /// The error a traversal aborts with. `?` out of `descend` or `combine`
    /// discards both stacks, exactly as a recursive form's `?` discards its
    /// pending frames.
    type Err;

    /// Work-stack preallocation. Preallocation is a best practice here, not a
    /// premature optimization: the stack's depth is the term's, and geometric
    /// growth on the hot path is measurable.
    const WORK_CAPACITY: usize = 32;

    /// Value-stack preallocation.
    const VAL_CAPACITY: usize = 32;

    /// Visit one node.
    ///
    /// ★ **Run every bounded field straight-line and return at the first
    /// descent.** This is the contract the measured design rests on; see the
    /// module docs. A conforming implementation touches the driver exactly once
    /// per node per suspension.
    ///
    /// Push either
    ///
    /// * exactly one value onto `vals` and nothing onto `work` (a leaf), or
    /// * nothing onto `vals` and a well-formed region onto `work` (Invariant 1):
    ///   the continuation(s) first, then the children in **reverse**, so they
    ///   pop in source order.
    fn descend<'t>(
        &mut self,
        state: &mut Self::State,
        node: Self::Node<'t>,
        work: &mut Vec<Step<'t, Self>>,
        vals: &mut Vec<Self::Val>,
    ) -> Result<(), Self::Err>;

    /// Run one continuation.
    ///
    /// Pop **exactly** [`Traversal::arity`] values — the children pop in
    /// reverse of push order, so the last child comes off first;
    /// `Vec::split_off(len − n)` takes them all at once and preserves sibling
    /// order without a reversal.
    fn combine<'t>(
        &mut self,
        state: &mut Self::State,
        kont: Self::Kont<'t>,
        vals: &mut Vec<Self::Val>,
    ) -> Result<Outcome<Self::Val>, Self::Err>;

    /// How many values this continuation pops.
    ///
    /// ★ Deliberately a **second, independent statement** of the pop counts in
    /// [`Traversal::combine`], so Invariant 2 can cross-check them. Write it as
    /// an exhaustive `match` with no `_` arm: a new `Kont` variant should stop
    /// the build here.
    ///
    /// Called only under `debug_assertions`.
    fn arity(kont: &Self::Kont<'_>) -> usize;
}

// ===========================================================================
// §C  The bookkeeping — `debug_assertions` only
// ===========================================================================

/// The running tally Invariant 2 is checked against.
///
/// Compiled to nothing in release: the release [`Ledger`] is a unit struct whose
/// methods have empty bodies, so the counters do not exist and the driver's hot
/// loop is `pop` + `match` + one call.
#[cfg(debug_assertions)]
struct Ledger {
    /// `D` — pending `Descend`s.
    descends: usize,
    /// `C` — pending `Combine`s.
    combines: usize,
    /// `A` — Σ `arity` over the pending `Combine`s.
    sum_arity: usize,
}

#[cfg(not(debug_assertions))]
struct Ledger;

#[cfg(debug_assertions)]
impl Ledger {
    fn new<T: Traversal>(root: &Step<'_, T>) -> Self {
        let led = match root {
            Step::Descend(_) => Ledger {
                descends: 1,
                combines: 0,
                sum_arity: 0,
            },
            Step::Combine(k) => Ledger {
                descends: 0,
                combines: 1,
                sum_arity: T::arity(k),
            },
        };
        assert_eq!(
            led.descends + led.combines,
            1 + led.sum_arity,
            "drive: MALFORMED ROOT — a root `Step` must satisfy `D + C - A == 1`, i.e. it must \
             be a `Descend` or a `Combine` of arity 0, so that the machine owes exactly one \
             value. This root owes {} (descends={}, combines={}, sum_arity={}).",
            led.descends as isize + led.combines as isize - led.sum_arity as isize,
            led.descends,
            led.combines,
            led.sum_arity
        );
        led
    }

    /// Invariant 2, at a loop head.
    fn check(&self, vals_len: usize) {
        assert_eq!(
            vals_len + self.descends + self.combines,
            1 + self.sum_arity,
            "drive: MALFORMED CONFIGURATION — the DEFICIT INVARIANT `V + D + C - A == 1` is \
             violated (V={}, D={}, C={}, A={}). Either a `Kont`'s `arity()` disagrees with the \
             number of values its `combine` popped, or a `descend` pushed a region that does \
             not produce exactly one value. See `drive.rs`'s module docs, Invariant 2.",
            vals_len,
            self.descends,
            self.combines,
            self.sum_arity
        );
    }

    fn popped_descend(&mut self) {
        self.descends -= 1;
    }

    fn popped_combine<T: Traversal>(&mut self, kont: &T::Kont<'_>) {
        self.combines -= 1;
        self.sum_arity -= T::arity(kont);
    }

    /// Invariant 1, over everything one `descend` call pushed.
    fn pushed_region<T: Traversal>(&mut self, region: &[Step<'_, T>], values_pushed: usize) {
        if region.is_empty() {
            assert_eq!(
                values_pushed, 1,
                "drive: MALFORMED CONFIGURATION — a `descend` that pushed no work is a LEAF and \
                 must push exactly one value; this one pushed {values_pushed}. See `drive.rs`'s \
                 module docs, Invariant 1."
            );
            return;
        }
        assert_eq!(
            values_pushed, 0,
            "drive: MALFORMED CONFIGURATION — a `descend` that pushed {} work item(s) must push \
             NO value (its region produces the value); this one pushed {values_pushed}. A step \
             produces exactly one value. See `drive.rs`'s module docs, Invariant 1.",
            region.len()
        );

        // Scanned in POP order: the last-pushed item comes off the stack first.
        let mut avail: usize = 0;
        for (back, step) in region.iter().rev().enumerate() {
            match step {
                Step::Descend(_) => {
                    self.descends += 1;
                    avail += 1;
                }
                Step::Combine(k) => {
                    let arity = T::arity(k);
                    assert!(
                        avail >= arity,
                        "drive: MALFORMED CONFIGURATION — the `Combine` at region position {} \
                         (of {}, counted from the top of the stack) declares arity {} but only \
                         {} value(s) will have been produced by the time it runs. A `descend` \
                         must push its continuation BEFORE the children that feed it, and must \
                         push as many children as the continuation's `arity()` claims. See \
                         `drive.rs`'s module docs, Invariant 1.",
                        back,
                        region.len(),
                        arity,
                        avail
                    );
                    avail = avail - arity + 1;
                    self.combines += 1;
                    self.sum_arity += arity;
                }
            }
        }
        assert_eq!(
            avail, 1,
            "drive: MALFORMED CONFIGURATION — the region pushed by one `descend` must produce \
             exactly ONE net value; this one produces {avail}. See `drive.rs`'s module docs, \
             Invariant 1."
        );
    }
}

#[cfg(not(debug_assertions))]
impl Ledger {
    #[inline(always)]
    fn new<T: Traversal>(_root: &Step<'_, T>) -> Self {
        Ledger
    }
    #[inline(always)]
    fn check(&self, _vals_len: usize) {}
    #[inline(always)]
    fn popped_descend(&mut self) {}
    #[inline(always)]
    fn popped_combine<T: Traversal>(&mut self, _kont: &T::Kont<'_>) {}
    #[inline(always)]
    fn pushed_region<T: Traversal>(&mut self, _region: &[Step<'_, T>], _values_pushed: usize) {}
}

// ===========================================================================
// §D  The loop
// ===========================================================================

/// Run `visitor` over `root` to completion.
///
/// Native stack is `O(1)` in both nesting depth and sibling width; the
/// recursion lives in the heap-allocated work stack.
///
/// # Errors
///
/// Whatever `descend` or `combine` returns. An abort discards both stacks —
/// identical to a recursive form's `?`, which discards its pending frames.
///
/// # Panics
///
/// On a malformed machine configuration: see the module docs. The **final**
/// configuration is checked unconditionally; the running invariants are checked
/// under `debug_assertions`.
pub fn drive<'t, T: Traversal>(
    visitor: &mut T,
    state: &mut T::State,
    root: Step<'t, T>,
) -> Result<T::Val, T::Err> {
    let mut work: Vec<Step<'t, T>> = Vec::with_capacity(T::WORK_CAPACITY);
    let mut vals: Vec<T::Val> = Vec::with_capacity(T::VAL_CAPACITY);

    let mut ledger = Ledger::new::<T>(&root);
    work.push(root);

    loop {
        ledger.check(vals.len());

        let Some(step) = work.pop() else { break };

        match step {
            Step::Descend(node) => {
                ledger.popped_descend();
                let before_work = work.len();
                let before_vals = vals.len();
                visitor.descend(state, node, &mut work, &mut vals)?;
                ledger.pushed_region::<T>(&work[before_work..], vals.len() - before_vals);
            }
            Step::Combine(kont) => {
                ledger.popped_combine::<T>(&kont);
                match visitor.combine(state, kont, &mut vals)? {
                    Outcome::Value(v) => vals.push(v),
                    Outcome::Done(v) => {
                        // ★ Abandon the pending obligations. `work` is dropped
                        // immediately below, so today the clear is not
                        // observable; it is written because ABANDONMENT is the
                        // machine's disposition on this path and because a
                        // pooled work stack — the shape `wire_encode`'s
                        // `give_ops` already uses — must be parked EMPTY.
                        work.clear();
                        return Ok(v);
                    }
                }
            }
        }
    }

    assert_eq!(
        vals.len(),
        1,
        "{}",
        final_configuration_message(vals.len(), work.len())
    );

    Ok(vals
        .pop()
        .expect("drive: the final-configuration assertion guarantees exactly one value"))
}

/// The unconditional final-configuration diagnostic.
///
/// Built in a cold, non-generic function so the assertion on `drive`'s hot exit
/// path is a comparison and a call that never happens, rather than a format
/// expression monomorphized into every instance.
#[cold]
#[inline(never)]
fn final_configuration_message(vals_len: usize, work_len: usize) -> String {
    let mut m = String::with_capacity(768);
    let _ = write!(
        m,
        "drive: MALFORMED FINAL CONFIGURATION — the work stack drained with {vals_len} value(s) \
         on the value stack and {work_len} obligation(s) still pending; exactly 1 value and 0 \
         obligations are required. Each value beyond the first is one obligation that was never \
         popped: some `Kont` reported an `arity()` LARGER than the number of values its \
         `combine` actually popped, or a `descend` produced a value for a region that was \
         already going to produce one. This is the terminal instance of the deficit invariant \
         (`drive.rs` module docs, Invariant 2) and it is checked UNCONDITIONALLY, because the \
         running invariant is `debug_assertions`-only and this is the one malformed \
         configuration a release build can still reject."
    );
    m
}
