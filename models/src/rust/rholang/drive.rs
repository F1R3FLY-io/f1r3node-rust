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
//! | `sorter::sort_drive::SortTraversal` (crate-private) | `SortNode<'t>` | `ValItem` | `()` | F-2 |
//! | `term_ops::CloneTraversal` (**generated**) | `CloneNode<'t>` | `CloneVal` | `()` | F-4 |
//!
//! ★ The F-4 instance is the first **generated** one, and it is the reason
//! [`drive_with`] exists — see §D.
//!
//! ## ★ Bring-your-own-stacks: [`drive`] versus [`drive_with`]
//!
//! [`drive`] allocates a work stack and a value stack per call. For a traversal
//! whose *input* is deep that is one malloc/free pair amortised over thousands of
//! nodes and it does not show up. For a traversal whose input is **shallow and
//! frequent** it is the dominant cost, and the number is measured rather than
//! feared: the production produce-depth distribution
//! (`models/benches/wire_encode_bench.rs`, 1,773 instrumented datums) puts
//! **95.43% of terms at depth 2**, where a `Par` clone touches a handful of
//! nodes and two extra allocations are a double-digit regression.
//!
//! So [`drive_with`] takes both stacks by `&mut` and [`drive`] is the owning
//! convenience wrapper over it. A caller that clones on a hot path parks the two
//! allocations in a thread-local and hands them in — the shape
//! [`crate::rust::rholang::wire_encode`]'s `take_ops` / `give_ops` already uses,
//! whose measured steady state is 0 allocations per encode.
//!
//! ⚠ **[`drive_with`] leaves both stacks EMPTY on every return path**, including
//! the [`Outcome::Done`] early exit and the `?` abort, because a pooled buffer
//! must be parked empty (`take_ops`'s reuse predicate is `is_empty()`). Clearing
//! `vals` runs `Val`'s destructor, which for a `Val` that owns a deep term is
//! itself Θ(depth) — see the warning under [`Outcome::Done`]. An instance in that
//! position must hand its stack to
//! [`crate::rust::rholang::par_children::dismantle_all`] instead of letting the
//! clear run; the generated clone traversal is not in that position, because its
//! `Err` is uninhabited and it never reports [`Outcome::Done`], so its only exit
//! is the normal one that drains `vals` to empty anyway.
//!
//! ## ★★ SER/DE: what this trait cannot host yet, and exactly what it needs
//!
//! [`crate::rust::rholang::wire_encode`] (the bincode encoder) and
//! [`crate::rust::rholang::par_codec`] (the decoder) are the remaining members,
//! and they are **not** hosted here. This section exists so that the next
//! attempt starts from the measurements rather than rediscovering them; every
//! number below was taken from the code or from a gate, not estimated.
//!
//! ### The structural difference: interleaved I/O
//!
//! A post-order fold's parent knows its children *before* any of them runs, so
//! `descend` can push the whole region at once. A codec's parent does **not**:
//! bytes sit *between* the children, so the next obligation is discovered only
//! after the previous child's subtree is complete. Both machines are therefore
//! **resumable coroutines**, and the phenomenon is visible as a count —
//! `par_codec`'s `Machine::step` has **50** `self.ops.push` sites and **14**
//! `self.repeat(` sites; `wire_encode`'s loop re-pushes whenever
//! `resume != NO_RESUME`.
//!
//! ⚠ [`Traversal::combine`] is deliberately **not** handed the work stack —
//! that is the type-level guarantee stage F-2 gained, replacing the sorter's
//! `debug_assert_eq!(before, work.len(), "a Combine must not push work")`. So
//! as specified, `combine` cannot express a resume at all.
//!
//! ### The extension — [`Outcome::Tail`], LANDED
//!
//! [`Outcome`]'s third arm, [`Outcome::Tail`]`(Node<'t>)`: *"I consumed my
//! children; this descent produces my value in my place."* A resumable node is
//!
//! ```text
//!   Descend(node @ cursor c):  run bounded fields from c
//!       program spent      ⇒  push one value                  (a leaf)
//!       child at cursor c' ⇒  push [Combine(Resume{node, c'}), Descend(child)]
//!   Combine(Resume{node, c'}): pop the child's value
//!       program spent      ⇒  Outcome::Value(v)
//!       more to come       ⇒  Outcome::Tail(node @ c')        (re-enter descend)
//! ```
//!
//! Both invariants survive **unweakened**, which is the whole point — Invariant
//! 2 is what makes the consensus-critical sorter's conversion trustworthy, and
//! it may not be relaxed to admit a new member:
//!
//! * the suspension region `[Combine(Resume), Descend(child)]` scans (pop
//!   order) `Descend` ⇒ `avail = 1`, `Resume` of arity 1 ⇒ `avail = 1`.
//!   **Invariant 1 satisfied**, and `Tail` pushes nothing during `descend`, so
//!   Invariant 1 is not even reached on the tail step.
//! * **Invariant 2 preserved, FOR EVERY ARITY.** ⚠ This bullet used to read
//!   `Δ(V + D + C − A) = (−1) + (+1) + (−1) − (−1) = 0`, which is the
//!   **arity-1 special case stated as if it were general** — the four terms
//!   happen to be ±1 only when the continuation consumes exactly one value, and
//!   the sorter and `rho-pure-eval` both have higher-arity continuations. The
//!   general statement, over a `Combine(k)` with `a = arity(k)`:
//!
//! ```text
//!     popping the Combine        C − 1        and        A − a
//!     combine pops its values    V − a
//!     the Tail pushes a Descend  D + 1
//!
//!     Δ(V + D + C − A)  =  (−a) + (+1) + (−1) − (−a)
//!                        =  −a + 1 − 1 + a
//!                        =  0        for every a ∈ ℕ.    ∎
//! ```
//!
//!   The `−a` from the value pops and the `+a` from removing the promise cancel
//!   *identically*, so the arity never appears in the result. That is why the
//!   arithmetic is sound for a `Kont` of any width, and why it is worth writing
//!   with the symbol rather than with the instance.
//!
//! * ★★ **And a THIRD obligation, which neither invariant can express:
//!   TERMINATION.** A `Tail` is the only step in the machine that does not
//!   strictly reduce the pending obligations — it may re-enter a node already
//!   visited. A `Combine` that tails to a `Descend` that suspends and tails
//!   again with an *unadvanced* cursor runs forever, and `Δ = 0` holds at every
//!   single loop head throughout. Invariant 2 is a conservation law; it is
//!   blind to progress by construction. The disposition is a contract plus
//!   [`Traversal::MAX_TAILS_PER_DESCENT`], a `debug_assertions`-only bound in
//!   the [`Traversal::arity`] idiom — see [`Outcome::Tail`] for both halves and
//!   `models/tests/drive_configuration_gate.rs` for the executed RED.
//!
//! The counted repeat is the same shape: `Rep{kind, n}` descends as
//! `[Combine(RepResume{kind, n−1}), Descend(kind.start())]` and the resume
//! tails back into `Rep{kind, n−1}` — which is why `remaining` never has to be
//! materialised as `n` separate ops. ★ Its termination witness is the *decreasing
//! `n`*, which is exactly the "strictly advanced" contract instantiated for a
//! counted repeat.
//!
//! ### The four blockers, measured
//!
//! **1. The encoder's op is at its pinned ceiling with ZERO headroom.**
//! `models/tests/wire_encode_space.rs` asserts `size_of::<Op>() <= 4 *
//! size_of::<usize>()` and the measured value is **exactly 32 B**. The op stack
//! grows at a measured **2.000 entries per level** (8,193 entries = 262,176 B at
//! depth 4,096), so one extra word is +65 KB there. Splitting one four-arm enum
//! into `Node` + `Kont` behind an outer [`Step`] discriminant cannot be *assumed*
//! to re-pack into 32 B. ⇒ the conversion must be measured against that gate,
//! and the gate must be watched RED on a deliberately widened op first.
//!
//! **2. `drive` owns its stacks, so the encoder's zero-allocation steady state
//! is unreachable through it.** `wire_encode`'s `take_ops`/`give_ops` carry a
//! **thread-local pooled allocation** across calls; the measured steady state is
//! `0 allocations / 0 B` per encode for all five shapes in
//! `the_steady_state_allocation_table` (the derived path costs 1 allocation
//! each), and it holds even after an 18,800,104 B encode. Routing through
//! [`drive`] as written re-introduces one malloc/free pair per `hash_produce`,
//! turning a **stated acceptance criterion** into a regression. ⇒ `drive` needs
//! a bring-your-own-stacks entry point (`drive_with(&mut work, &mut vals, …)`),
//! with `drive` as the owning convenience wrapper over it. That is a genuine
//! API addition and not a workaround.
//!
//! **3. The decoder has 18 per-type value stacks, not one.** `Machine` carries
//! 23 `Vec` fields: one op stack, **18 typed value stacks**, and four
//! side-frame stacks. Sibling order is preserved by `take_n` = `Vec::split_off`
//! on the *typed* stack at **22** sites. Those stacks belong in
//! [`Traversal::State`] and `Val` becomes `()`.
//!
//! **4. …and that makes Invariant 2 weaker for the decoder, which the docs must
//! not hide.** With `Val = ()` the value stack degenerates to a ledger of child
//! completions, so Invariant 2 asserts only that the suspension structure is a
//! *chain* — a true statement, but it no longer cross-checks any arity against
//! any pop, because a `*Build`'s child counts are read from the stream mid-node
//! and parked in `ParFrame` / `ReceiveTail` / `NewFrame` rather than being a
//! function of the `Kont`. The decoder's arity guard is and remains
//! `take_n`'s length check returning `MachineInvariant`, plus
//! `models/tests/par_codec_differential.rs` and the malformed-input corpus.
//! ⇒ a decoder instance must not be described as getting the sorter's
//! cross-check. It does not.
//!
//! ### Status
//!
//! Blockers 3 and 4 are *design consequences*, already resolved above. Blocker 2
//! is solved: [`drive_with`] exists and takes both stacks by `&mut`. Blocker 1 is
//! **answered by measurement, and the answer is that it never applied to
//! [`Outcome::Tail`] at all** — see the next section.
//!
//! ### ⚠⚠ CORRECTED 2026-07-29 — what `Tail` is, and what it is NOT
//!
//! `Tail` is **resumption for interleaved-I/O traversals**, and that is the whole
//! of it. It has been described elsewhere in this repository as the fix for the
//! generated `Clone`'s throughput gap against its own derived oracle. **It is
//! not**, and the two mechanisms are worth separating precisely:
//!
//! | | `Outcome::Tail` | what the clone gap needs |
//! |---|---|---|
//! | kind | **control flow** — "produce my value by re-entering `descend`" | **storage** — "construct the child in its final slot" |
//! | moves data | **no** | that is its entire purpose |
//! | changes | `Outcome` only; [`Step`] is untouched | widens `Step::Descend` with a destination |
//! | name | resumption | **destination-passing descent** (out-parameter / `sret`-threading) |
//!
//! ★ Worse than merely not helping: under `Tail` each resumption *pops* the
//! child's value and has to park it until the node completes — either back on
//! `vals` (a **fourth** 248-byte move for the clone instance) or in
//! [`Traversal::State`]. A tail call changes *when* a value is produced; that gap
//! is about *where* it lands. The derive is fast because **Rust's `sret` ABI
//! already is destination-passing**, and the driven form breaks that chain by
//! routing through `Vec<Val>`.
//!
//! ⇒ Anyone reaching for `Tail` to close a byte-movement gap should read
//! `models/build/wire_schema.rs`'s clone-throughput section, which now carries the
//! deterministic instruction- and store-level accounting.
//!
//! ### ★ And blocker 1 does not bind `Tail`, for a structural reason
//!
//! `Outcome` is a **return value**: it lives in a register pair or an `sret` slot
//! for the length of the `match` that consumes it. The 32-byte ceiling
//! `models/tests/wire_encode_space.rs` pins is a ceiling on a **per-level stack
//! cell**, because the op stack grows at a measured 2.000 entries per level and
//! one extra word there is +65 kB at depth 4,096. `Tail` adds an arm to `Outcome`
//! and **does not touch [`Step`]**, so it multiplies by nothing. Measured on the
//! generated clone instance: `size_of::<Step<'_, CloneTraversal>>() == 16` —
//! unchanged by this stage and now pinned by
//! `models/tests/drive_step_width_gate.rs` — against
//! `size_of::<Outcome<CloneVal, CloneNode<'_>>>() == 256`, of which not one byte
//! is per-level.
//!
//! ⚠ What blocker 1 *does* still bind is any design that widens `Step` — which is
//! the encoder's `Op` split, and the destination-passing descent above. Those must
//! be measured against that gate, and the gate must be watched RED on a
//! deliberately widened node first. ★ One datum for whoever does: widening
//! `CloneKont` from 8 B to 16 B **does not widen `Step` at all** (it stays 16 B —
//! rustc packs the discriminant into the non-null `&Par` niche), which was measured
//! on 2026-07-29 and refutes half of an earlier attribution that charged a
//! regression to "`Step` 16 → 24 B". Nothing in F-1 or F-2 changed the ser/de lanes — confirmed byte-for-byte
//! by `par_codec_differential` (13/13), `wire_encode_differential` (13/13),
//! `serializer_par_byte_goldens` (7/7), `wire_encode_space` (8/8) and the
//! `bincode_ser` / `bincode_de` depth subjects in
//! `rholang/tests/stack_depth_gate.rs`.
//!
//! ## ⚠ CORRECTED 2026-07-29 — the `mettail-rust` analogue is NOT this trait
//!
//! This paragraph used to claim that *"there is a structurally identical driver
//! in the `mettail-rust` workspace (`rholang-runtime`)"* and that the two were
//! deliberate duplicates, "each documented as an instance of the other". **The
//! second driver does not exist in that form.** A search of that workspace for
//! `trait Traversal` and for `fn drive<T: Traversal>` returns nothing; there is
//! no generic trampoline there to be an instance of.
//!
//! What *does* exist is a **hand-written, monomorphic** machine in
//! `mettail-rust/rholang-runtime/src/rholang_ast.rs`: its own `enum Job<'a>`,
//! `struct Stacks<'a>` and `fn drive(seed: Seed<'_>, root_env: &BoundEnv) ->
//! Result<Par, RholangAstLowerError>`, specialised to that crate's AST and
//! carrying its own copies of the two invariants inline. It is the same *idea* —
//! defunctionalized continuations on an explicit LIFO — and it is emphatically
//! not the same *code*, so a change here neither propagates to it nor is
//! constrained by it.
//!
//! The repository-independence argument the old paragraph gave is still the
//! reason there are two machines rather than one: `mettail-rust` depends one-way
//! on this repo, and a single shared generic trampoline would make every driver
//! change a cross-repo rebuild. That argument is unaffected by the correction.
//! What is affected is anybody who read "structurally identical" and expected to
//! be able to port a fix across by copying a file.

use std::fmt::Write as _;

// ===========================================================================
// §A  The alphabet
// ===========================================================================

/// One pending obligation: descend into a node, or run a continuation.
///
/// `Step` is the *only* thing that ever occupies the work stack, which is what
/// makes the two invariants checkable: the driver can read back exactly what a
/// `descend` pushed and score it.
/// ⚠ `T: 't` — the visitor must outlive the term its work items borrow. The bound
/// is not new policy; it is [`Traversal::Node`]'s `where Self: 't` propagated to
/// every use, which became required once [`Outcome::Tail`] put that GAT in
/// [`Traversal::combine`]'s return type. Every instance is zero-sized and
/// satisfies it trivially.
pub enum Step<'t, T: Traversal + 't> {
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
/// ★ Spelled as a named enum rather than `ControlFlow<Val, Val>` because the
/// arms do not all carry a value, so `Break` / `Continue` would say nothing
/// about which is which. [`Outcome::Value`] *is* the `Continue` arm and
/// [`Outcome::Done`] *is* the `Break` arm of that formulation;
/// [`Outcome::Tail`] has no counterpart there at all.
///
/// ## ★ Width: this is a RETURN VALUE, not a stack cell
///
/// `Outcome` is returned from [`Traversal::combine`] into a register pair or an
/// `sret` slot and dies at the end of the `match` that consumes it. It is **not**
/// on the Θ(depth) work stack — only [`Step`] is, and [`Step`] does not mention
/// `Outcome`. So the `size_of::<Op>() <= 32` style ceiling that governs a
/// per-level stack cell does not govern this type, and adding [`Outcome::Tail`]
/// costs **zero** bytes per level. For the generated clone instance the measured
/// widths are `size_of::<Step<'_, CloneTraversal>>() == 16` (pinned by
/// `models/tests/drive_step_width_gate.rs`) against
/// `size_of::<Outcome<CloneVal, CloneNode<'_>>>() == 256`, and the second number
/// multiplies by nothing.
pub enum Outcome<V, N> {
    /// An ordinary child value. The driver pushes it and carries on.
    Value(V),
    /// **This is the answer.** The driver abandons every pending obligation and
    /// returns it. See the module docs on what abandoning the value stack costs
    /// when `Val` owns a term.
    Done(V),
    /// ★★ **RESUMPTION.** *"I consumed my children and produced no value; this
    /// descent produces my value in my place."* The driver pushes
    /// `Step::Descend(node)` and carries on.
    ///
    /// This is the arm an **interleaved-I/O** traversal needs. A post-order
    /// fold's parent knows all its children before any of them runs, so its
    /// `descend` can push the whole region at once. A codec's parent does not:
    /// bytes sit *between* the children, so the next obligation is discovered
    /// only after the previous child's subtree is complete. Both such machines
    /// are resumable coroutines, and [`Traversal::combine`] is deliberately
    /// **not** handed the work stack — so without this arm `combine` cannot
    /// express a resume at all.
    ///
    /// ## ★ Why it carries a `Node` and not the work stack
    ///
    /// Handing `combine` a `&mut Vec<Step>` would have expressed resumption too,
    /// and would have thrown away the type-level property stage F-2 gained when
    /// it replaced the sorter's `debug_assert_eq!(before, work.len(), "a Combine
    /// must not push work")`. A `Tail` can schedule **exactly one descent**, not
    /// an arbitrary region: the guarantee survives verbatim, and it is the
    /// property that makes a `combine` reviewable in isolation.
    ///
    /// ## ⚠⚠ THE OBLIGATION THIS ARM INTRODUCES: TERMINATION
    ///
    /// Without `Tail`, the machine terminates for a structural reason: every
    /// step strictly reduces the pending obligations, because `descend` may only
    /// push work for children of the node it was given and the input is finite.
    /// **A `Tail` can push an obligation for a node the machine has already
    /// visited**, and a `Combine` that tails to a `Descend` that suspends and
    /// tails again *with an unadvanced cursor* loops forever — while **both
    /// invariants hold at every single loop head.** Invariant 2 is a
    /// conservation law, not a progress measure; it cannot see this.
    ///
    /// So the obligation is stated as a **contract** and backed by a **measured
    /// bound**:
    ///
    /// > **Contract.** The node a `Tail` carries must be strictly *advanced*
    /// > with respect to the `Kont` that produced it — its cursor must name a
    /// > position later in that node's own finite field program. A traversal
    /// > whose `Kont` records a cursor `c` and whose `Tail` carries `c' > c`
    /// > satisfies this by construction, and the field program's length is the
    /// > number of resumptions the node can possibly need.
    ///
    /// > **Backstop.** [`Traversal::MAX_TAILS_PER_DESCENT`], checked by the
    /// > `debug_assertions`-only [`Ledger`]. Its default is **0** — a traversal
    /// > that does not tail declares nothing and any `Tail` from it is rejected
    /// > on the spot — so the constant is opt-in and fails closed.
    ///
    /// ★ The backstop is a *second, independent statement* of a bound the
    /// instance already knows (for a codec: its field count), in exactly the
    /// discipline [`Traversal::arity`] already establishes for the pop counts —
    /// and like `arity` it costs nothing in release.
    Tail(N),
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
    ///
    /// ⚠ **`where Self: 't` is REQUIRED, and it is a real statement.** Once
    /// [`Outcome::Tail`] made this GAT appear in [`Traversal::combine`]'s RETURN
    /// type, rustc requires the bound (E0195, and rustc names it: *"missing
    /// required bound on `Node`"*, rust-lang/rust#87479). It reads: **the visitor
    /// must outlive the term it borrows.** Every instance satisfies it trivially —
    /// all of them are zero-sized — and it is deliberately the precise bound
    /// rather than `Self: 'static`, which would forbid a visitor that borrows its
    /// own configuration (an environment, an oracle) even when that borrow
    /// outlives the traversal.
    type Node<'t>: Copy
    where
        Self: 't;

    /// The value one `Step` produces.
    type Val;

    /// A defunctionalized continuation: the constructor to apply, the borrowed
    /// *shell* of the node being rebuilt (flags, cached bitsets, remainders —
    /// everything that is not a child), and the child counts needed to slice
    /// the value stack.
    ///
    /// ⚠ `where Self: 't` for the same reason as [`Traversal::Node`] — see there.
    type Kont<'t>
    where
        Self: 't;

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

    /// ★★ **The termination bound for [`Outcome::Tail`]** — how many times ONE
    /// descent may be resumed.
    ///
    /// The driver keeps a `debug_assertions`-only tally and requires
    ///
    /// ```text
    ///   tails  <=  MAX_TAILS_PER_DESCENT  ×  original_descents
    /// ```
    ///
    /// where `original_descents` is the root plus every `Descend` pushed by a
    /// `descend` that was **not itself tail-generated**. An unadvancing tail
    /// chain therefore grows `tails` against a base that stands still, and trips
    /// the bound instead of running forever.
    ///
    /// ## ⚠ It is a BUDGET, not an exact progress measure — and why it cannot be
    ///
    /// An exact measure would need to say *"this node has been resumed `k`
    /// times"*, which requires **node identity**, and the driver has none: a
    /// [`Traversal::Node`] is an opaque `Copy` value with no equality, no hash
    /// and no address it is willing to expose. (Requiring `Eq + Hash` on `Node`
    /// and keeping a visited map would cost a hash per node on the hot path, to
    /// catch a defect that is a programming error rather than a data-dependent
    /// one. That trade is not worth making, and stating the alternative is part
    /// of stating the choice.)
    ///
    /// So the bound is deliberately loose in the legitimate direction and finite
    /// in the pathological one. For a sequence of eight items with
    /// `MAX_TAILS_PER_DESCENT = 8`, a correct traversal takes 7 tails against a
    /// budget of 16 — 2.3× of slack — while an unadvancing one is stopped at 17.
    /// ★ What it guarantees is exactly what is needed: **the machine cannot spin
    /// forever without saying so.**
    ///
    /// ⚠ **The default is 0, and that is deliberate: this constant fails
    /// closed.** A traversal that does not resume declares nothing, and the first
    /// `Tail` it ever emits — a refactor's accident, say — is rejected
    /// immediately rather than accepted because the bound was permissive. A
    /// resumable instance must state its own bound, and it always knows one: for
    /// a codec it is the length of the longest field program, i.e. the most
    /// resumptions any single node can need.
    ///
    /// ★ Like [`Traversal::arity`], this is a **second, independent statement**
    /// of a number the instance already knows, so the driver can cross-check the
    /// instance against itself. Also like `arity`, it is read only under
    /// `debug_assertions` and costs nothing in release.
    const MAX_TAILS_PER_DESCENT: usize = 0;

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
    ///
    /// Then report one of three things: [`Outcome::Value`] (the ordinary case),
    /// [`Outcome::Done`] (early exit), or [`Outcome::Tail`] (this node is not
    /// finished; re-enter `descend` on the given node in my place). ⚠ `Tail`
    /// carries an obligation — see its documentation — and it does **not** hand
    /// `combine` the work stack, so a `combine` still cannot push an arbitrary
    /// region.
    fn combine<'t>(
        &mut self,
        state: &mut Self::State,
        kont: Self::Kont<'t>,
        vals: &mut Vec<Self::Val>,
    ) -> Result<Outcome<Self::Val, Self::Node<'t>>, Self::Err>
    where
        Self: 't;

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
    /// How many [`Outcome::Tail`]s have been taken.
    ///
    /// ★ Not part of either invariant — a `Tail` is `Δ = 0` and both invariants
    /// are blind to it. This counter and [`Ledger::original_descents`] exist for
    /// the one property `Tail` can break that the invariants cannot see:
    /// **termination**.
    tails: usize,
    /// The **budget base**: descents the machine was given along a path that has
    /// not yet tailed — the root, plus every `Descend` in the region of a
    /// `descend` that was itself *not* tail-generated.
    ///
    /// ⚠ Both exclusions are load-bearing, and the second one is subtle enough
    /// that getting it wrong makes the whole check vacuous. A `Tail`'s own
    /// `Descend` obviously must not be credited. But the node it re-enters then
    /// runs `descend` and pushes *its next child*, and crediting THAT would make
    /// the budget grow once per tail — so `tails <= MAX × original_descents`
    /// would hold forever and the backstop would catch nothing. Excluding the
    /// regions pushed *under* a tail-generated descend is what keeps the base
    /// constant while a tail chain spins.
    original_descents: usize,
    /// `true` between [`Ledger::pushed_tail`] and the [`Ledger::popped_descend`]
    /// that takes the `Descend` it pushed.
    ///
    /// ★ Exact, not approximate: [`Ledger::pushed_tail`] pushes onto a LIFO and
    /// the driver's very next action is `work.pop()`, so the flag is consumed by
    /// the descend it was set for and by no other.
    next_descend_is_tail: bool,
    /// `true` while running the `descend` of a tail-generated `Descend`, so
    /// [`Ledger::pushed_region`] can decline to credit its region.
    in_tail_descend: bool,
}

#[cfg(not(debug_assertions))]
struct Ledger;

#[cfg(debug_assertions)]
impl Ledger {
    fn new<'t, T: Traversal + 't>(root: &Step<'t, T>) -> Self {
        let led = match root {
            Step::Descend(_) => Ledger {
                descends: 1,
                combines: 0,
                sum_arity: 0,
                tails: 0,
                original_descents: 1,
                next_descend_is_tail: false,
                in_tail_descend: false,
            },
            Step::Combine(k) => Ledger {
                descends: 0,
                combines: 1,
                sum_arity: T::arity(k),
                tails: 0,
                original_descents: 0,
                next_descend_is_tail: false,
                in_tail_descend: false,
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
        // Consume the flag `pushed_tail` set, so the region this descend is about
        // to push is attributed to a tail rather than to the original term.
        self.in_tail_descend = std::mem::take(&mut self.next_descend_is_tail);
    }

    fn popped_combine<'t, T: Traversal + 't>(&mut self, kont: &T::Kont<'t>) {
        self.combines -= 1;
        self.sum_arity -= T::arity(kont);
    }

    /// ★★ [`Outcome::Tail`] — the `Δ = 0` bookkeeping, and the **termination**
    /// backstop that neither invariant can supply.
    ///
    /// The conservation side is a one-liner: the `Combine` that produced this
    /// `Tail` was already removed by [`Ledger::popped_combine`] (`C − 1`,
    /// `A − a`) and `combine` popped its `a` values (`V − a`), so pushing one
    /// `Descend` (`D + 1`) closes the books — see the module docs' generalized
    /// arithmetic. **Invariant 2 is preserved for every arity, not just 1.**
    ///
    /// The termination side is the real work. `original_descents` does not move
    /// here, deliberately: an unadvancing tail chain therefore drives `tails`
    /// up against a constant and is caught, where a counter that credited each
    /// tail's own `Descend` would make the bound vacuously true forever.
    fn pushed_tail<T: Traversal>(&mut self) {
        self.descends += 1;
        self.tails += 1;
        self.next_descend_is_tail = true;
        assert!(
            self.tails <= T::MAX_TAILS_PER_DESCENT * self.original_descents,
            "{}",
            tail_bound_message(
                self.tails,
                T::MAX_TAILS_PER_DESCENT,
                self.original_descents
            )
        );
    }

    /// Invariant 1, over everything one `descend` call pushed.
    fn pushed_region<'t, T: Traversal + 't>(
        &mut self,
        region: &[Step<'t, T>],
        values_pushed: usize,
    ) {
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
                    // ★ Credited to the tail budget's base only when this region
                    // came from a descend the machine was GIVEN. A region pushed
                    // under a tail-generated descend is the tail's own doing, and
                    // crediting it would make the budget grow once per tail —
                    // i.e. vacuous. See `Ledger::original_descents`.
                    if !self.in_tail_descend {
                        self.original_descents += 1;
                    }
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
    fn new<'t, T: Traversal + 't>(_root: &Step<'t, T>) -> Self {
        Ledger
    }
    #[inline(always)]
    fn check(&self, _vals_len: usize) {}
    #[inline(always)]
    fn popped_descend(&mut self) {}
    #[inline(always)]
    fn popped_combine<'t, T: Traversal + 't>(&mut self, _kont: &T::Kont<'t>) {}
    #[inline(always)]
    fn pushed_tail<T: Traversal>(&mut self) {}
    #[inline(always)]
    fn pushed_region<'t, T: Traversal + 't>(
        &mut self,
        _region: &[Step<'t, T>],
        _values_pushed: usize,
    ) {
    }
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
pub fn drive<'t, T: Traversal + 't>(
    visitor: &mut T,
    state: &mut T::State,
    root: Step<'t, T>,
) -> Result<T::Val, T::Err> {
    let mut work: Vec<Step<'t, T>> = Vec::with_capacity(T::WORK_CAPACITY);
    let mut vals: Vec<T::Val> = Vec::with_capacity(T::VAL_CAPACITY);
    drive_with(visitor, state, root, &mut work, &mut vals)
}

/// [`drive`], on stacks the caller owns.
///
/// ★ The entry point a **hot, shallow** traversal needs: [`drive`]'s two
/// `Vec::with_capacity` calls are one malloc/free pair each, which disappears
/// against a deep input and dominates a shallow one. 95.43% of production terms
/// are at depth 2 (`models/benches/wire_encode_bench.rs`), so the generated
/// `Clone` traversal parks both allocations in a thread-local and hands them in
/// — the shape [`crate::rust::rholang::wire_encode`]'s `take_ops` / `give_ops`
/// already uses, whose measured steady state is 0 allocations per call.
///
/// # Preconditions
///
/// Both stacks must be **empty**. They are asserted empty on entry, because a
/// stale value on `vals` would satisfy the final-configuration check with the
/// *previous* call's answer, and a stale obligation on `work` would run it.
///
/// # Postcondition
///
/// Both stacks are left **empty** on every return path — the normal exit, the
/// [`Outcome::Done`] early exit, and the `?` abort — so the caller may park them
/// in a pool whose reuse predicate is `is_empty()`. Capacity is retained; that is
/// the whole point.
///
/// ⚠ Clearing `vals` runs `Val`'s destructor. For `Val = bool` / `Ordering` /
/// `()` that is free; for a `Val` that owns a deep `Par` it is
/// `drop_in_place::<Par>`, itself Θ(depth). An instance in that position must
/// route through [`crate::rust::rholang::par_children::dismantle_all`] rather
/// than rely on this clear — see the module docs under [`Outcome::Done`].
///
/// A panic unwinding out of `descend` / `combine` leaves the stacks populated;
/// that is safe (they are the caller's `Vec`s and drop normally) and a pool that
/// checks `is_empty()` before reusing simply declines the buffer.
///
/// # Errors
///
/// Whatever `descend` or `combine` returns.
///
/// # Panics
///
/// On a malformed machine configuration: see the module docs. The **final**
/// configuration is checked unconditionally; the running invariants are checked
/// under `debug_assertions`.
pub fn drive_with<'t, T: Traversal + 't>(
    visitor: &mut T,
    state: &mut T::State,
    root: Step<'t, T>,
    work: &mut Vec<Step<'t, T>>,
    vals: &mut Vec<T::Val>,
) -> Result<T::Val, T::Err> {
    assert!(
        work.is_empty() && vals.is_empty(),
        "{}",
        nonempty_entry_message(work.len(), vals.len())
    );

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
                if let Err(e) = visitor.descend(state, node, work, vals) {
                    // Same discard as the `Combine` arm below; see its comment.
                    work.clear();
                    vals.clear();
                    return Err(e);
                }
                ledger.pushed_region::<T>(&work[before_work..], vals.len() - before_vals);
            }
            Step::Combine(kont) => {
                ledger.popped_combine::<T>(&kont);
                match visitor.combine(state, kont, vals) {
                    Ok(Outcome::Value(v)) => vals.push(v),
                    // ★★ RESUMPTION. Exactly one descent, scheduled in the
                    // place of the value this `Combine` did not produce. The
                    // ledger's `pushed_tail` carries both the `Δ = 0`
                    // bookkeeping and the TERMINATION backstop that neither
                    // invariant can express — see `Outcome::Tail`.
                    Ok(Outcome::Tail(node)) => {
                        work.push(Step::Descend(node));
                        ledger.pushed_tail::<T>();
                    }
                    Ok(Outcome::Done(v)) => {
                        // ★ Abandon the pending obligations. The stacks belong
                        // to the CALLER now, so this clear is observable and
                        // load-bearing rather than cosmetic: a pooled work
                        // stack — the shape `wire_encode`'s `give_ops` already
                        // uses — must be parked EMPTY.
                        work.clear();
                        vals.clear();
                        return Ok(v);
                    }
                    // ⚠ The `?` abort discards both stacks, exactly as a
                    // recursive form's `?` discards its pending frames — but
                    // with caller-owned stacks the discard has to be WRITTEN,
                    // because the buffers outlive the call.
                    Err(e) => {
                        work.clear();
                        vals.clear();
                        return Err(e);
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

/// The [`drive_with`] entry precondition's diagnostic.
///
/// Cold and non-generic for the same reason as [`final_configuration_message`]:
/// the check on the hot entry path is two comparisons and a call that never
/// happens.
#[cold]
#[inline(never)]
fn nonempty_entry_message(work_len: usize, vals_len: usize) -> String {
    let mut m = String::with_capacity(640);
    let _ = write!(
        m,
        "drive_with: NON-EMPTY STACKS ON ENTRY — {work_len} pending obligation(s) and \
         {vals_len} value(s) were already present. Both stacks must be empty: a stale value on \
         `vals` would satisfy the final-configuration check with the PREVIOUS call's answer \
         (returning the wrong term, silently), and a stale obligation on `work` would be run as \
         if this call had asked for it. `drive_with` leaves both empty on every return path, so \
         a caller that pools them and never mutates them in between cannot reach this; a caller \
         that reached it is sharing one buffer across two live traversals."
    );
    m
}

/// The [`Outcome::Tail`] termination backstop's diagnostic.
///
/// Cold and non-generic for the same reason as [`final_configuration_message`]:
/// the check itself is a multiply and a comparison, and it is compiled only under
/// `debug_assertions` in the first place.
#[cold]
#[inline(never)]
#[cfg(debug_assertions)]
fn tail_bound_message(tails: usize, max_per_descent: usize, original_descents: usize) -> String {
    let mut m = String::with_capacity(1024);
    let _ = write!(
        m,
        "drive: MALFORMED CONFIGURATION — the TAIL BOUND is exceeded. This traversal has taken \
         {tails} `Outcome::Tail`(s) against {original_descents} descent(s) it was actually given, \
         and its `MAX_TAILS_PER_DESCENT` is {max_per_descent}, which permits at most {}. \
         \n\nA `Tail` re-enters `descend` on a node the machine has ALREADY visited, so it is \
         the one step that does not reduce the pending obligations. The contract is that a \
         `Tail`'s node must be strictly ADVANCED with respect to the `Kont` that produced it — \
         its cursor must name a later position in that node's own finite field program. A \
         `combine` that tails with an UNADVANCED cursor loops forever, and it does so while both \
         Invariant 1 and Invariant 2 hold at every loop head: they are conservation laws, not \
         progress measures, and they cannot see this. This assertion is the progress measure. \
         \n\nIf the traversal is correct and simply resumes more often than declared, raise \
         `MAX_TAILS_PER_DESCENT` to the length of the longest field program — the most \
         resumptions any single node can need. If it is 0, this traversal never declared itself \
         resumable at all (0 is the deliberate fail-closed default) and the `Tail` is the defect. \
         See `drive.rs`'s `Outcome::Tail`.",
        max_per_descent.saturating_mul(original_descents)
    );
    m
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
