# Θ(depth) Traversals over the `Par` Family — Audit, Fix, and Proof Standard (2026-07-26)

**Status.** Measurement-derived audit. Every quantitative claim in this document
was **measured on this tree, on this machine, on 2026-07-26**, by the harnesses
committed alongside it (`rholang/tests/stack_depth_probe.rs`,
`scripts/stack_depth_probe.sh`, `rholang/tests/stack_depth_gate.rs`). Where a
number is reproduced from an earlier report rather than re-measured, it is
labelled *relayed* and its independent confirmation is given.
[§9](#9-evidence-ledger) carries the per-claim provenance.

**Why it exists.** A 30-character Rholang program with no guest language, no
λ-calculus and no user-defined process aborts the reducer:

```rholang
@"OUT"!([[[[[[[[[0]]]]]]]]])     // depth  9 — ok
@"OUT"!([[[[[[[[[[0]]]]]]]]]])   // depth 10 — thread 'tokio-rt-worker' has overflowed its stack
```

A stack overflow is a `SIGSEGV` handled by Rust's guard-page handler, which
prints and calls `abort()`. It is **not** a catchable error and **not** a failed
deploy: it terminates the whole node process. Program-controlled nesting depth
therefore controls node liveness, which makes this a consensus-liveness defect
rather than a robustness nit.

**What this document claims, and does not claim.** It claims a *complete
enumeration by construction* of the Θ(depth) traversals over the recursive
`Par` type family, a measured per-traversal and per-profile constant for each,
a disposition for each, and a proof standard for the conversions. It does **not**
claim the family is fixed: at the time of writing, **one leg of one traversal**
has landed. [§7](#7-disposition--what-is-done-and-what-is-not) states precisely
what remains and why, and [§8](#8-the-proof-standard) states the limits of the
method used to justify it.

**Diagram convention.** `docs/` in this repo has no PlantUML figure pipeline;
diagrams here are inline unicode box-drawing, matching the surrounding
documents. Mathematical expressions use GitHub-flavored math delimiters.

---

## Table of contents

1. [Executive summary](#1-executive-summary)
2. [The falsification experiment](#2-the-falsification-experiment)
3. [The recursive type family](#3-the-recursive-type-family)
4. [Enumerating the traversals — the method](#4-enumerating-the-traversals--the-method)
5. [Measured constants, per traversal and per profile](#5-measured-constants-per-traversal-and-per-profile)
6. [Why one level costs 195 KB — the attribution](#6-why-one-level-costs-195-kb--the-attribution)
7. [Disposition — what is done and what is not](#7-disposition--what-is-done-and-what-is-not)
8. [★ The proof standard](#8-the-proof-standard)
9. [Evidence ledger](#9-evidence-ledger)
10. [Reproducing every number here](#10-reproducing-every-number-here)

---

## 1. Executive summary

### 1.1 The direct answer

Native stack consumption on the reduce path is **linear in term nesting depth**,
with a per-level constant that is large and that is *not* confined to one
function. Every hand-written traversal over the `Par` family, and every
compiler-derived one, is Θ(depth). The reported depth-10 abort is the
lowest-hanging instance, not the defect.

For `Substitute::substitute` measured in isolation — no parser, no reducer, no
tuplespace:

```math
S(N) \;=\; 225{,}280 \;+\; 194{,}970\,N \quad\text{bytes (debug)}
```

where `$N$` is bracket-nesting depth. Rust's default spawned-thread stack is
2 MiB, so `$S(9) = 1.88\ \text{MiB}$` fits and `$S(10) = 2.07\ \text{MiB}$` does
not — which is exactly the reported threshold, recovered from an independent
measurement that never observed it.

### 1.2 Three corrections to the incoming analysis

**(a) The enumeration was larger than the starting list, and the discovery
method matters more than the list.** The brief named nine candidate members. A
constructive enumeration ([§4](#4-enumerating-the-traversals--the-method)) finds
that the recursive type SCC has **39 message types**, and that **153 functions
across 53 files** lie on a recursion cycle over them. Most are false positives
of an intentionally over-approximating search; the genuine set is given in
[§5](#5-measured-constants-per-traversal-and-per-profile). The point is that the
list is now *derived*, so its completeness is checkable, rather than recalled.

**(b) `prost` decode is already capped — it is the one member that fails
safe.** The brief flagged decode as "the most security-relevant member" because
it operates on untrusted input. Measured: decode is Θ(depth) at 26.00 KiB/level,
**but** `prost`'s `DecodeContext` enforces a recursion limit of 100 nested
messages. The `Par → Expr → EList → Par` spine costs three message levels per
bracket, so decoding **succeeds at term depth 33 and returns `Err` at 34** —
confirmed by bisection. Untrusted input cannot drive the decoder off the stack.
It can, however, drive it into a *rejection*: `encode` has no such limit, so a
term of depth ≥ 34 can be built and serialised but not read back. That asymmetry
is a real constraint on any fix and is recorded in
[§7.3](#73-the-prost-decode-ceiling-is-a-constraint-on-the-fix-not-a-defect-to-fix).

**(c) The debug↔release ratio is not a uniform 10×; it is 2.2×–12.1× and it is
per-traversal.** Recording one ratio would make any regression gate
backend-fragile. The gate therefore never hardcodes a constant for the real
assertion ([§8.5](#85-the-depth-independence-gate)).

### 1.3 What landed

* **Leg-1 (de-cloning) for the substitution SCC and the `prepend_*` family** —
  landed. Measured effect: **debug unchanged** (+0.4%, within bisection
  resolution), **release −25.4%** (36,416 → 27,179 B/level). This faithfully
  reproduces the verdict the eval-SCC's own Leg-1 reached about itself
  (`bb7fcd20`): removing deep copies eliminates `$O(D^2)$` heap churn and a real
  slice of the constant, but **cannot change the class**.
* **The measurement harness** (`stack_depth_probe.rs` + driver script) — the
  instrument that produced every number here.
* **The regression gate** (`stack_depth_gate.rs`) — see
  [§8.5](#85-the-depth-independence-gate).

### 1.4 What did not land

**Leg-2 — the explicit-worklist conversion — for any traversal.** The bug is
therefore still present. [§7](#7-disposition--what-is-done-and-what-is-not) names
each remaining traversal, its measured cost, and the specific reason it is not
yet converted. No ladder is proposed: the enumeration is complete up front, so
the remaining work is a known finite set rather than a sequence of discoveries.

---

## 2. The falsification experiment

The incoming analysis sampled **one** crash instant. A single sample cannot
distinguish "substitution is Θ(depth)" from "something else on the reduce path
is Θ(depth) and substitution merely appeared in the trace". So the first action
was an experiment designed to *falsify* the diagnosis.

**Design.** Build `Par` values of depth `$N$` with an iterative bottom-up loop
(so construction contributes `$O(1)$` stack), call **only**
`Substitute::substitute(term, 0, &Env::new())` on a thread created with an
explicit `stack_size(S)`, and bisect `$S$` for
`$N \in \{10, 20, 40, 80\}$`. No parser, no reducer, no tuplespace, no tokio.

**Predicate.** ≈190 KB/level ⇒ the diagnosis holds. Materially flatter ⇒ the
depth is coming from elsewhere and the aim is wrong.

**Result.**

| depth `$N$` | minimum surviving stack |
|-------------|-------------------------|
| 10          | 2,174,976 B (2,124 KiB) |
| 20          | 4,124,672 B (4,028 KiB) |
| 40          | 8,024,064 B (7,836 KiB) |
| 80          | 15,822,848 B (15,452 KiB) |

Least-squares fit: `$S(N) = 225{,}280 + 194{,}970\,N$` — **190.40 KiB/level**,
against a *relayed* 194,694 B/level. Agreement to **0.14%**.

**Independent confirmation of the constant.** Under `gdb`, breaking at
`substitute.rs:350` and reading `$rsp` at ten successive recursion levels gives
a **dead-constant 194,992 B per level** (ten deltas, zero variance) — matching
the bisected slope to within the 4,096 B bisection resolution.

**Independent confirmation of the threshold.** The fit predicts that a 2 MiB
thread carries depth 9 (1.888 MiB) and not depth 10 (2.074 MiB). Rust's default
stack size for a spawned thread is exactly 2 MiB when `RUST_MIN_STACK` is unset.
The reported reproducer fails at 10 and succeeds at 9. The measurement therefore
recovers the reported threshold without having been shown it.

**Verdict: the diagnosis holds.** Proceed.

> **Reading the intercept.** The relayed law had intercept 474,688 B; this one
> has 225,280 B. The difference (~249 KiB) is the reducer's own frames above
> `substitute`, which the relayed end-to-end measurement includes and this
> isolated one excludes. The *slope* — the only depth-dependent term, and the
> only one that matters for the class — agrees to 0.14%.

---

## 3. The recursive type family

Unbounded nesting is a property of the **types**, so the family is derived from
`models/src/main/protobuf/RhoTypes.proto` rather than from memory: build the
message-reference digraph and take the strongly connected component containing
`Par` (Tarjan; `scripts/`-adjacent helper reproduced in
[§10](#10-reproducing-every-number-here)).

**Result: 39 message types.**

```
Bundle · Connective · ConnectiveBody · EAnd · EDiv · EEq · EGt · EGte · EList
ELt · ELte · EMap · EMatches · EMethod · EMinus · EMinusMinus · EMod · EMult
ENeg · ENeq · ENot · EOr · EPathMap · EPercentPercent · EPlus · EPlusPlus
ESet · ETuple · EZipper · Expr · If · KeyValuePair · Match · MatchCase · New
Par · Receive · ReceiveBind · Send
```

There are **no other non-trivial SCCs** in the file: every cycle in the schema
passes through `Par`.

Two structural facts follow, and both are load-bearing.

**F1 — the family is single-sorted at the recursion points.** Every recursive
field in the SCC bottoms out in `Par` (`Option<Par>`, `Vec<Par>`, or a wrapper
struct holding those). The intermediate types are *shape*, not recursion. A
single `Par`-keyed worklist is therefore sufficient to drive any traversal over
the family — the conversion does not need one continuation type per member.

**F2 — the deep spine is three message levels per bracket.** `[…]` costs
`Par → Expr → EList → Par`. Any per-*message* limit (such as `prost`'s) converts
to a term-depth limit by dividing by three.

```
            one bracket level of  [ [ … ] ]
      ┌───────────────────────────────────────────┐
      │   Par ──exprs──▶ Expr ──e_list_body──▶    │
      │                              EList        │
      │                                │ ps       │
      └────────────────────────────────┼──────────┘
                                       ▼
                                      Par   (next level)
```

---

## 4. Enumerating the traversals — the method

> **This section is the claim under review.** The USER rejected a staged ladder
> precisely because staging implies the enumeration is incomplete. *How the list
> was derived* is therefore the load-bearing part, not the list.

A traversal over this family is Θ(depth) in native stack iff its call depth
grows with term nesting. Three sources produce such traversals, and each is
enumerated by construction rather than by recall.

### 4.1 M1 — derived and generated traversals (closed by inspection)

Any trait implementation *generated* over a recursive type recurses with it.
For the 39 types in [§3](#3-the-recursive-type-family) these are, exhaustively:

| generated impl | source |
|----------------|--------|
| `Clone` | `#[derive(Clone)]` on every `prost` message |
| `PartialEq` | `#[derive(PartialEq)]` |
| `Debug` | `#[derive(Debug)]` |
| `Drop` glue | compiler-synthesised `drop_in_place` |
| `Message::encoded_len`, `Message::encode_raw` | `prost` codegen |
| `Message::merge_field` (decode) | `prost` codegen |

This list is closed because it is exactly the set of derives `prost-build` emits
plus the one impl the compiler always synthesises. Nothing here can be
overridden without editing generated code, which is why their disposition
([§7.2](#72-derived-traversals--leg-1-only-by-construction)) differs from the
hand-written ones.

### 4.2 M2 — hand-written traversals (over-approximate, then verify)

Static search, biased hard against false negatives:

1. Parse every `fn` with a brace-balanced body under `models/src`, `rholang/src`,
   `rspace++/src`, `rho-pure-eval/src`, `casper/src`, `node/src` (**4,503**
   functions).
2. Keep those whose *signature* mentions a family type.
3. Build a call graph keyed on `(file, name)` — **not** on name alone, which
   fuses every `new`/`get`/`run` in the workspace into one 240-node blob and was
   the first attempt's failure mode.
4. Resolve edges in two passes: **intra-file** (a call to `f(` in file `F`
   resolves to `F::f` when `F` defines `f`) and **cross-file trait dispatch**
   (for every method *declared* in a `trait` with a family-typed signature, add
   an edge from any caller to *every* impl — this is what captures
   `Sortable::sort_match`, `SubstituteTrait::substitute*`,
   `SpatialMatcher::spatial_match`, `HasLocallyFree::*`).
5. Report every function on a cycle.

**Result: 153 functions in 53 files.** The over-approximation is deliberate:
false positives are cheap to dismiss by reading (`casper::engine::running::new`
is on the list only because a same-named constructor sits in a file that also
mentions `Par`), whereas a false negative is precisely the rung the USER
predicted would be found later.

Every one of the 153 was read and dispositioned. The genuine traversals are the
subjects in [§5](#5-measured-constants-per-traversal-and-per-profile); the
remainder are name-resolution artefacts.

### 4.3 M3 — measurement as the discriminator of last resort

Static analysis proposes; measurement disposes. Every subject in
[§5](#5-measured-constants-per-traversal-and-per-profile) has a *measured*
bytes-per-level, which is the only evidence that actually settles whether a
candidate is Θ(depth). Two candidates changed status under measurement:

* **`HasLocallyFree::locally_free` / `connective_used` for `Par` and `Expr`** —
  suspected Θ(depth); measured **identical, to the byte**, to `Par::clone`
  (36,508 + 15,875·N in both cases). Reading the impls confirms why: every arm
  returns a *cached* field and never recurses. The entire cost is the by-value
  `self` argument — a deep clone forced by the trait's signature. These are
  Leg-1 defects, not Leg-2 defects.
* **`prost` decode** — first measured at 0.00 KiB/level. That reading was an
  artefact of a wrong proto field number in the probe (`Par.exprs` is field 5,
  not 3; `Expr.e_list_body` is field 20, a two-byte tag) which made `prost` skip
  the payload as an unknown field and measure nothing. The probe now asserts the
  decoded term has the expected depth before reporting, and the corrected value
  is 26.00 KiB/level. **A measurement harness that cannot fail loudly will
  eventually report a comfortable number for the wrong reason.**

---

## 5. Measured constants, per traversal and per profile

All figures are bytes of native stack per unit of bracket nesting, from
`scripts/stack_depth_probe.sh` (bisection to 4,096 B debug / 1,024 B release),
`x86_64-unknown-linux-gnu`, `nightly-2026-02-09`, `-C target-cpu=native`.

⚠ **Read the profile columns, not one of them.** The ratio is 2.2×–12.1× and
varies per traversal, because `rustc` does not overlap the stack slots of
mutually exclusive `match` arms at `-O0` and this family's hot functions are
40-arm matches over `ExprInstance` ([§6](#6-why-one-level-costs-195-kb--the-attribution)).

| # | traversal | debug B/level | release B/level | ratio | kind | reachable from untrusted input |
|---|-----------|--------------:|----------------:|------:|------|---|
| 1 | `Substitute::substitute` / `substitute_no_sort` | **195,728** | **27,179** | 7.2× | hand-written | yes — every COMM |
| 2 | `ParSortMatcher::sort_match` (+ Expr/Send/Receive/New/Match/If/Connective/Bundle sorters) | 78,579 | 6,495 | 12.1× | hand-written | yes — called *by* `substitute` |
| 3 | `PrettyPrinter::_build_string_from_expr` | 41,840 | 4,242 | 9.9× | hand-written | yes — error formatting, `stdout` |
| 4 | `prost` `Message::merge_field` (decode) | 26,624 | — | — | generated | yes — **but capped**, see §7.3 |
| 5 | `<Par as Clone>::clone` (= `HasLocallyFree` by-value readers) | 15,875 | 2,852 | 5.6× | derived | yes |
| 6 | `<ExprInstance as Debug>::fmt` | 3,626 | 1,244 | 2.9× | derived | yes — error messages |
| 7 | `Message::encoded_len` / `encode_raw` | 1,948 | 422 | 4.6× | generated | yes — charged in `substitute_and_charge` |
| 8 | `<Par as PartialEq>::eq` | 1,353 | 310 | 4.4× | derived | yes |
| 9 | `match_pars` (concrete-match fast path) | 1,329 | 377 | 3.5× | hand-written | yes |
| 10 | `drop_in_place::<Par>` | 470 | 219 | 2.2× | synthesised | yes — unavoidable per term |

**Post-Leg-1 values for row 1** (the only row Leg-1 touched):
debug 194,970 → 195,728 (+0.4%, within bisection resolution — i.e. unchanged);
release 36,416 → 27,179 (**−25.4%**).

**Not separately tabulated, same class, lower priority:** `par_to_sexpr` family,
`ParCount::min_max_par`/`min_max_con`, `FoldMatch::free_check`,
`SortedParHashSet`/`SortedParMap` insert/remove/contains, `ParSet`/`ParMap`
`update_locally_free`, `rhoapi_ext::make_mut`, `pathmap_zipper::descend_to`,
`rho-pure-eval::eval_with`, and the normalizer (`normalize_ann_proc` — Θ(*source*
nesting), on the deploy path). Each was found by
[§4.2](#42-m2--hand-written-traversals-over-approximate-then-verify) and read;
none is on the nested-collection reduce path measured here, and each is named so
that "not measured" is not confused with "not found".

### 5.1 What the composite means operationally

The traversals are *sequential*, not nested, so the binding constraint is the
maximum, not the sum. On a default 2 MiB spawned thread:

```math
D_{\max} \;=\; \left\lfloor \frac{2{,}097{,}152 - a}{b} \right\rfloor
```

| profile | binding traversal | `$b$` | `$D_{\max}$` |
|---------|-------------------|------:|-------------:|
| debug   | `substitute` | 195,728 | **9** |
| release | `substitute` | 27,179 | **~76** |

After `substitute` is converted, the constraint moves to `sort_match`
(`$D_{\max}$` ≈ 26 debug / ≈ 320 release), then `PrettyPrinter`, then `Clone`.
**This is why the traversals must be converted as a set rather than in
sequence**: converting only the largest moves the cliff, it does not remove it.

---

## 6. Why one level costs 195 KB — the attribution

A recursion whose frames were a few hundred bytes each would need depth ~10,000
to overflow 2 MiB. 195 KB per level demands an explanation, and the explanation
determines whether constant-factor work can substitute for structural work.

Under `gdb`, the per-level frame chain of `substitute_no_sort` is **20 frames**:

| frame | bytes | function |
|------:|------:|----------|
| 0 | 7,600 | `SubstituteTrait<Par>::substitute_no_sort` |
| 1–15 | 4,800 | iterator/`collect`/`try_fold`/`GenericShunt` adapters |
| **16** | **169,728** | **`SubstituteTrait<Expr>::substitute_no_sort`** |
| 17 | 10,048 | `sub_exp::{closure#0}` |
| 18 | 2,800 | `IntoIter<Expr>::try_fold` |
| 19 | 416 | `Substitute::sub_exp` |
| | **194,992** | **one nesting level** |

**One function is 87% of the cost.** `SubstituteTrait<Expr>::substitute_no_sort`
is a `match` over ~40 `ExprInstance` variants, each arm materialising `Expr`
(504 B), `Par` (248 B) and `Result<Expr, InterpreterError>` (504 B) temporaries.
At `-O0` `rustc` gives every binding its own slot and does **not** overlay slots
across mutually exclusive arms, so the frame is sized for *all forty arms at
once* even though exactly one executes.

This yields two independent axes, and it is important that they are not
confused:

```
   ┌─ Axis A: frame SIZE ────────────────┐   ┌─ Axis B: recursion DEPTH ───────┐
   │ outline match arms; stop cloning    │   │ replace the call stack with an  │
   │ ⇒ constant-factor win (large)       │   │ explicit heap worklist          │
   │ ⇒ class UNCHANGED: still Θ(depth)   │   │ ⇒ class CHANGED: O(1) native    │
   └─────────────────────────────────────┘   └─────────────────────────────────┘
```

Axis A is tempting because it is cheap and the win is large. It is **not a
fix**: it multiplies `$D_{\max}$` by a constant and leaves a program-controlled
abort in place. Only Axis B satisfies the requirement, and only Axis B can pass
the depth-independence gate. Leg-1 (landed) is Axis A; Leg-2 (not landed) is
Axis B. The measured Leg-1 result in
[§1.3](#13-what-landed) is the empirical demonstration that Axis A alone is
insufficient — and it reproduces, on a different SCC, the identical verdict the
eval SCC's own Leg-1 recorded in `bb7fcd20`.

---

## 7. Disposition — what is done and what is not

### 7.1 Landed

| change | file | measured effect |
|---|---|---|
| `prepend_expr` / `prepend_connective` / `prepend_new` / `prepend_bundle`: 4 deep clones per call → 0 | `rholang/src/rust/interpreter/util/mod.rs` | see §1.3 |
| `Par::prepend_send` / `_receive` / `_match` / `_if`: argument moved, not cloned | `models/src/rust/utils.rs` | " |
| By-reference `HasLocallyFree` readers, single shared implementation | `rholang/src/rust/interpreter/matcher/has_locally_free.rs` | " |
| `sub_exp`: discriminant read without deep-cloning the subtree | `rholang/src/rust/interpreter/substitute.rs` | " |
| `SubstituteTrait<Expr>`: entry no longer clones `expr_instance` | " | " |
| `SubstituteTrait<Send>`: destructured instead of `term.clone().chan` | " | " |
| `SubstituteTrait<Bundle>`: 2 deep clones → 0 | " | " |
| `.iter().map(\|p\| …p.clone())` → `.into_iter()` at 12 sites | " | " |

**A correctness note that the shared-reader refactor surfaced.** The private
`expr_locally_free_ref` in `reduce.rs` hardcoded `depth = 0` in its `EVar` arm,
whereas the by-value trait impl threads `depth` through. That was sound at its
only call site (a depth-0 reader) but would have been **wrong** at
`prepend_expr`, which `sub_exp` calls at pattern depth > 0. The shared
implementation is depth-parameterised and `reduce.rs` now delegates to it with
an explicit `0`. This is the kind of divergence a second copy produces, and it
is why the readers were unified rather than duplicated.

### 7.2 Derived traversals — Leg-1 only, by construction

Rows 5–8 and 10 of [§5](#5-measured-constants-per-traversal-and-per-profile) are
`#[derive]`d or compiler-synthesised over `prost`-generated types. They cannot
be trampolined without hand-writing ~39 impls per trait (≈195 impls) or editing
generated code.

**Their fix is Leg-1: remove the call sites, not the impls.** `Par::clone` is
only a hazard when something clones a deep term; if every traversal borrows, no
deep clone occurs. That is what [§7.1](#71-landed) does for the substitution
path. `drop_in_place` is the irreducible member — every term must eventually be
torn down — and at 470 B/level debug / 219 B/level release it supports depth
≈4,400 / ≈9,500 on a 2 MiB thread. **It is the only member for which "Θ(depth)"
is a property of the data rather than of a choice**, and the honest statement is
that it bounds the achievable result until `Par` grows a manual iterative `Drop`
— which is not free, because adding `Drop` to a type forbids the destructuring
moves that [§7.1](#71-landed) just introduced.

### 7.3 The `prost` decode ceiling is a constraint on the fix, not a defect to fix

Measured: decode is Θ(depth) at 26,624 B/level, and `prost` enforces
`RECURSION_LIMIT = 100` nested messages. At three message levels per bracket
([§3](#3-the-recursive-type-family) F2), decode **succeeds at term depth 33 and
returns `Err` at depth 34** — bisected, not inferred.

Consequences, both of which matter for anyone converting the rest of the family:

* Untrusted input **cannot** drive the decoder off the stack. It fails safe, with
  an error, at 858 KiB of stack. This is the opposite of the incoming
  assessment and it should not be "fixed".
* `encode` has **no** matching limit. A term of depth ≥ 34 can therefore be
  constructed, reduced and serialised, but not deserialised. Any conversion that
  raises `$D_{\max}$` past 33 pushes terms into that asymmetry. **This is a
  protocol-visible ceiling that already exists**; the USER's decision was "no
  *new* protocol-level nesting cap", and this is not one, but it must be
  surfaced before it is discovered by a validator.

### 7.4 Not landed — and precisely why

| traversal | why not |
|---|---|
| `Substitute::substitute` **Leg-2** | The conversion is an explicit-worklist rewrite across 8 `SubstituteTrait` impls (~1,344 lines), and — per [§8](#8-the-proof-standard) — is only acceptable in consensus code accompanied by a recursive oracle twin and a differential harness asserting byte-identical results and an identical ordered charge trace. That is a single, well-specified unit of work; it was not completed in this pass. It is the **only** thing standing between the current state and the reported bug being fixed. |
| `ParSortMatcher` family **Leg-2** | Same mechanism, 9 files. Becomes the binding constraint the moment substitution is converted, which is why it must land *with* it, not after it. |
| `PrettyPrinter` **Leg-2** | Same mechanism. Off the hot path but on the *error* path, which untrusted input reaches. |
| rows 5–10 | See [§7.2](#72-derived-traversals--leg-1-only-by-construction) — not convertible by the same mechanism; disposition is Leg-1 plus a documented residual. |

**Why this is not a ladder.** A ladder discovers the next constraint after
removing the previous one and calls each removal a milestone. Here the full set
is enumerated ([§4](#4-enumerating-the-traversals--the-method)), measured
([§5](#5-measured-constants-per-traversal-and-per-profile)), and each member has
a stated disposition *before* any of them is converted. What remains is a known
finite set, with the order determined by the measured constants rather than by
what happens to break next.

---

## 8. ★ The proof standard

> This work lands in the consensus implementation. A reviewer must be able to
> see what was proven, by what method, and what that method does **not** cover,
> without reconstructing the author's reasoning.

### 8.1 The claim

For each converted traversal, the following observables are **unchanged** for
every input:

1. **Result bytes** — the protobuf `encode_to_vec()` of the returned term, or an
   identical `Err` (compared by `{:?}` payload).
2. **The ordered charge trace** — the sequence of `(BillableKind, weight)` pairs
   from the budget's canonical event log: *same tokens, same order, same
   amounts*.
3. **Aggregate cost** — `budget.total_cost()`.
4. **Abort behaviour** — a `?` early-return discards pending work and leaves
   already-reserved charges reserved, identically to the recursive form.

"Observationally neutral" is deliberately **not** the claim; the observables are
named because a validator can see exactly these and nothing else.

### 8.2 Why each conversion is neutral *by construction* — per traversal

Neutrality is argued per traversal, because the argument is not the same for
each and one of them does not have it.

**`Substitute::substitute` / `substitute_no_sort`.** The charge is levied by
`substitute_and_charge` / `substitute_no_sort_and_charge`
(`substitute.rs:52`, `:81`) as

```rust
self.metering.reserve_substitution(Cost::create(
    (subst_term.encoded_len() as i64).max(1), "substitution"))?;
```

— a function of the **result** (or, on the error path, of the **input**), levied
**once**, *outside* the recursion. `substitute_no_sort` and every function in its
SCC contain **no** `reserve_*` or `Cost::` call whatsoever. Therefore the number
of traversal steps, the order in which children are visited, and whether the
stack is native or heap are all invisible to the cost model. Removing clones and
replacing the call stack with a heap stack is cost-neutral **by construction**,
not by measurement. ✔

**`ParSortMatcher` and the sorter family.** Pure functions in `models`, with no
access to a budget: `Sortable::sort_match(&T) -> ScoredTerm<T>` takes no metering
handle and `models` does not depend on the accounting crate. Nothing can charge.
Neutrality reduces to result equality alone. ✔

**`PrettyPrinter`.** Produces a `String` for diagnostics; no budget handle; not
consensus-observable except through error text, which is compared as part of
observable (1). ✔

**Derived `Clone` / `PartialEq` / `Debug` / `Drop`.** `Clone` is identity and
nothing charges for it; `Drop` is unobservable; `PartialEq`/`Debug` are pure.
Their Leg-1 treatment (removing call sites) changes *which* values exist, never
*what* they are. ✔

**`prost` `encoded_len` / `encode_raw`.** ⚠ **These are the exception, and they
break the pattern.** `encoded_len` is not merely traversed — its *return value
is the charge* (see the snippet above). Any change to it is therefore
cost-relevant in the strongest possible sense: an off-by-one in
`encoded_len` is a consensus fork, not a performance regression. **This
traversal is NOT cost-neutral by the argument used for the others** and must not
be converted on that basis. If it is ever converted, its proof obligation is
byte-exact equality of the returned integer over an exhaustive generator, and
that obligation should be discharged before any structural change, not
alongside it. It is called out here so that the general argument in this section
is not applied to it by inheritance.

### 8.3 The differential harness — the empirical check

The standard already accepted in this repository for exactly this change is
`reduce.rs:8986`, `mod differential_trampoline`, introduced with the eval-SCC
trampoline (`a929a2d6`). It is cited rather than reinvented so a reviewer can
see this is the house standard, not a standard invented for this change.

For every term it evaluates the **same** term through both the recursive oracle
(`eval_expr_recursive`, a faithful copy of the pre-trampoline evaluator over
shared `combine_*` helpers, retained at `reduce.rs:3761`) and the production
trampoline, each on a **fresh** budget, and asserts observables (1)–(3) of
[§8.1](#81-the-claim) are equal.

**Corpus and generator.** Hand-written coverage of every `ExprInstance` arm
*including* error paths (division by zero, modulo by zero, wrapping add,
multiplication overflow, negation overflow, unbound variable), plus a
proptest-generated corpus of arbitrary bounded expression trees (400 cases
default, stressed to 3,000).

**Obligation for each traversal converted under this document:** extend that
harness to the new traversal — same three assertions, same fresh-budget
discipline, same oracle-twin pattern — rather than adding a bespoke test.

### 8.4 ★ The limits — what this standard does *not* establish

A reviewer who finds a limitation already named here can trust the rest; one who
finds a limitation that is not named cannot.

1. **Differential testing over a corpus is not a proof over all inputs.** It is
   falsification, not verification. It establishes that no divergence was found
   on the corpus, and nothing more. The *by-construction* arguments in
   [§8.2](#82-why-each-conversion-is-neutral-by-construction--per-traversal) are
   what carry the general claim; the harness is what catches the cases where the
   by-construction argument was wrong about the code.
2. **The observable set is finite and could be incomplete.** Results, charge
   trace and total cost are compared. Anything else a validator can observe —
   for instance iteration order leaking into a *hash*, or an error message
   embedded in a block — is **not** compared, and a divergence there would pass.
3. **The generator does not reach every shape — and this repository already has
   a live instance of that failure.** The eval-SCC corpus is *expression* trees.
   It does not generate deep `Receive`/`Match`/`New` nesting with `env.shift`
   interactions, and those are exactly where substitution's environment handling
   is subtlest. Extending the harness to substitution requires extending the
   generator, and a harness that runs green on a corpus that cannot express the
   risky shape is worse than no harness, because it licenses confidence.

   > **Concrete instance, found while writing this document.** The equivalence
   > test in `rholang/tests/by_reference_readers_equivalence.rs` was first
   > written as a `proptest` driven by
   > `models::rust::test_utils::test_utils::generate_par(3)`. It passed. An
   > anti-vacuity guard added afterwards revealed that this generator yields
   > **zero `Expr` nodes across 256 draws** — every field is
   > `vec(…, 0..1)`, so nearly every draw is an empty `Par`. The test had been
   > asserting a property over an empty set. It is now driven by a
   > **constructed corpus with one representative per schema variant** (36
   > `ExprInstance` arms, 9 `ConnectiveInstance` arms, both `None` arms, and
   > every `Var` shape for the depth-consuming `EVar` arm), with a companion
   > test that fails if the schema gains a variant the corpus does not cover.
   >
   > Anyone extending the differential harness to substitution should treat
   > `generate_par` as unfit for coverage purposes until it is fixed, and should
   > carry an anti-vacuity assertion of their own. **A harness that cannot fail
   > loudly will eventually report a comfortable number for the wrong reason** —
   > this is the second time that happened during this work (the first was the
   > `prost` decode probe in [§4.3](#43-m3--measurement-as-the-discriminator-of-last-resort)).
4. **`encoded_len` charges per result and is not covered by the general
   argument** — [§8.2](#82-why-each-conversion-is-neutral-by-construction--per-traversal),
   final entry.
5. **The measured constants are machine- and toolchain-specific.** They pin the
   *class* (a non-zero slope) portably; the numbers themselves are not portable
   and the gate must not assume they are ([§8.5](#85-the-depth-independence-gate)).
6. **`Drop` cannot be fully removed from the class** without a manual `Drop` impl
   on `Par`, which conflicts with the destructuring moves Leg-1 introduces
   ([§7.2](#72-derived-traversals--leg-1-only-by-construction)). Any claim of
   "fully heap-bounded" must exclude teardown or explain how it was solved.

**What would falsify neutrality:** a traversal that charges per *step* rather
than per *result*; an observable outside the compared set; a corpus that does
not reach a shape the traversal treats specially.

### 8.5 The depth-independence gate

`rholang/tests/stack_depth_gate.rs`. This is what makes "done right" checkable
rather than asserted, and what stops the class from being silently reintroduced.

**Design.** Each probe runs in a **child process** (a stack overflow `abort()`s
and is not unwindable, so an in-process probe destroys the whole test binary
along with every assertion that already passed), on a thread with an **explicit
`stack_size`** (so neither `RUST_MIN_STACK` nor `ulimit -s` can mask a
regression). Terms are built *and dismantled* iteratively, so the harness never
measures itself.

Two assertions, with deliberately different strengths:

| assertion | what it establishes |
|---|---|
| `assert_depth_independent(name, stack)` | The traversal survives a **fixed** 1 MiB stack at depths 4, 16, 64, **256**. A 64× depth range means only an `$O(1)$`-in-depth traversal can pass. **Profile-independent by construction** — it asserts a *shape*, never a constant. This is the real bar. |
| `assert_slope_below(name, ceiling, lo, hi)` | Bisects minimum stack at two depths, derives B/level, fails if it exceeds a ceiling. A **tripwire, not a pass**: it detects a traversal getting *worse* while keeping the residual visible in code. |

**Why the gate is not backend-fragile.** The real assertion never mentions a
byte count. Only the tripwire does, and its ceilings are selected per profile via
`cfg!(debug_assertions)` and set ≈1.5× above measured, so codegen drift does not
flake while an order-of-magnitude regression still trips. Per-profile constants
are recorded in [§5](#5-measured-constants-per-traversal-and-per-profile) and in
the gate's own module documentation, because the next person will not otherwise
know that mettail-rust's `codegen-backend = "cranelift"` inflates them again.

**Current state — stated plainly.**

```
converted_traversals_are_depth_independent  … PASS (list is EMPTY)
theta_depth_tripwire                        … PASS  substitute 195,754 B/level
                                                    sort        78,592
                                                    clone       15,872
                                                    encode       1,932
                                                    drop           464
reported_reproducer_depth_survives_…        … #[ignore]d — RED on purpose
```

The reproducer test asserts the bug is **fixed**. It is not, so it is
`#[ignore]`d with the reason inline, rather than deleted or weakened. Removing
that `#[ignore]` is the definition of done for substitution. The tripwire's
numbers were produced by the gate independently of the bisection in
[§5](#5-measured-constants-per-traversal-and-per-profile) and agree with it to
<1% — the gate and the audit corroborate each other.

---

## 9. Evidence ledger

| # | claim | provenance |
|---|---|---|
| E1 | `substitute` is Θ(depth) at 194,970 B/level (debug) | **Measured** — bisection, `scripts/stack_depth_probe.sh`, 4 depths |
| E2 | Per-level cost is a dead constant 194,992 B | **Measured** — `gdb`, 10 consecutive `$rsp` deltas, zero variance |
| E3 | Depth 9 survives / depth 10 aborts on 2 MiB | **Derived from E1**, matches the *relayed* reproducer without having been shown it |
| E4 | 87% of a level is `SubstituteTrait<Expr>::substitute_no_sort` | **Measured** — `gdb` per-frame `$sp` deltas over 20 frames |
| E5 | Family = 39 message types | **Derived** — Tarjan SCC over `RhoTypes.proto` |
| E6 | 153 candidate functions on a recursion cycle | **Derived** — static call-graph search, §4.2 |
| E7 | All per-traversal constants, both profiles | **Measured** — bisection, table in §5 |
| E8 | Leg-1: debug +0.4%, release −25.4% | **Measured** — same harness, before/after |
| E9 | `prost` decode succeeds at depth 33, fails at 34 | **Measured** — direct bisection over depth |
| E10 | `HasLocallyFree` readers cost exactly `Par::clone` | **Measured** (byte-identical fits) + **read** (impls return cached fields) |
| E11 | Charge is levied on the result, outside the recursion | **Read** — `substitute.rs:52,81`; zero `reserve_*`/`Cost::` in the SCC |
| E12 | Gate reproduces §5's constants to <1% | **Measured** — `stack_depth_gate::theta_depth_tripwire` |
| E13 | debug↔release ratio is 2.2×–12.1×, not 10× | **Measured** — §5; contradicts the *relayed* uniform estimate |
| E14 | Leg-1 preserves behaviour workspace-wide | **Measured** — `cargo nextest run --workspace`: 3349 tests run, **3349 passed**, 32 skipped, 0 failed |
| E15 | By-reference readers equal the by-value trait methods on every schema arm | **Measured** — `rholang/tests/by_reference_readers_equivalence.rs`: 36 `ExprInstance` arms + 9 `ConnectiveInstance` arms + both `None` arms + 5 `Var` shapes, each at depths 0–3 |
| E16 | `generate_par(3)` yields 0 `Expr` nodes over 256 draws | **Measured** — anti-vacuity guard; see §8.4 limit #3 |

---

## 10. Reproducing every number here

```bash
cd f1r3node-rust-mettail
ulimit -c 0          # ⚠ each faulting probe otherwise writes a ~305 MB core (~30 s)

# --- the family (39 types) ---------------------------------------------------
#   Tarjan SCC over the proto; helper in the session scratchpad, ~90 lines.

# --- per-traversal constants -------------------------------------------------
cargo test -p rholang --test stack_depth_probe --no-run
./scripts/stack_depth_probe.sh target/debug/deps/stack_depth_probe-<hash> \
    "subst sort clone drop eq debug encoded_len encode decode locally_free spatial pretty" \
    "10 20 40 80"

cargo test --release -p rholang --test stack_depth_probe --no-run
RESOLUTION=1024 ./scripts/stack_depth_probe.sh \
    target/release/deps/stack_depth_probe-<hash> \
    "subst sort clone drop eq debug encode pretty" "20 40 80 160"

# --- frame attribution (§6) --------------------------------------------------
#   gdb -batch: break substitute.rs:350; run; set language c;
#   continue x6; then `frame N` + `p/x $sp` for N in 0..21.

# --- type sizes --------------------------------------------------------------
./target/debug/deps/stack_depth_probe-<hash> --exact type_sizes --nocapture

# --- the gate ----------------------------------------------------------------
cargo test -p rholang --test stack_depth_gate -- --test-threads 1 --nocapture
```

---

## See also

* `rholang/tests/stack_depth_probe.rs` — the measurement harness (module docs
  explain the fork-per-probe and `mem::forget` isolation discipline).
* `rholang/tests/stack_depth_gate.rs` — the regression gate.
* `scripts/stack_depth_probe.sh` — the bisection driver.
* `reduce.rs:230–355` (`EvVal`/`EvWork`/`EvKont`/`eval_drive`) — **the pattern to
  copy**; `:3761` the oracle twin; `:8986` the differential harness.
* Commits `bb7fcd20` (Leg-1), `a929a2d6` (Leg-2), `9843e4b6` (removal of
  `stacker`) — the accepted precedent, including the explicit finding that
  Leg-1 alone does not change the class.
