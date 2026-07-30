# Θ(depth) Traversals over the `Par` Family — Audit, Fix, and Proof Standard (2026-07-26)

**Status.** Measurement-derived audit, maintained as a **running scientific
ledger**. Every quantitative claim was measured on this tree, on this machine,
by the harnesses committed alongside it (`rholang/tests/stack_depth_probe.rs`,
`scripts/stack_depth_probe.sh`, `rholang/tests/stack_depth_gate.rs`). Where a
number is reproduced from an earlier report rather than re-measured, it is
labelled *relayed* and its independent confirmation is given.
[§9](#9-evidence-ledger) carries the per-claim provenance.

The document is written in **dated strata**, and a reader must know which
stratum a sentence belongs to before trusting it:

| stratum | dated | what it records | superseded by |
|---|---|---|---|
| [§1](#1-executive-summary)–[§10](#10-reproducing-every-number-here) | 2026-07-26 | the state **before** Leg-2: the enumeration, the method, the proof standard, the pre-conversion constants | §11, §12 |
| [§11](#11-leg-2--execution-record-2026-07-2627) | 2026-07-26/27 | the Leg-2 execution record through Stage C-1 (`6ce7c5b9`) | §12 |
| [§12](#12-reconciliation--the-tree-at-b9aaa3d4-2026-07-27) | 2026-07-27 | **the tree as it now stands** (`b9aaa3d4`): every stage that landed after §11 was written, three corrections to §11's own conclusions, and one severe finding neither earlier stratum contains | — |

**Read [§12](#12-reconciliation--the-tree-at-b9aaa3d4-2026-07-27) first if you
want the current state.** Earlier strata are retained unedited so that a
reviewer can see what was predicted and what was found; each superseded claim
carries an inline ⚠ pointer to its correction rather than being silently
rewritten.

---

## 0. Current subject sets — DERIVED FROM THE GATE, NOT TRANSCRIBED

⚠ **This block is the document's only live statement of which traversals are
converted and which are tripwired.** Every other enumeration in this document is
**anchored to a commit** and must be read as history: those are evidence of what
was true at the commit they name, and rewriting them would falsify the ledger.

**Why it is fenced and machine-checked.** The sets below were previously prose,
transcribed in four places, and every copy drifted from the gate that holds them
as executable fact — [§11.4](#114-the-gate-and-one-thing-it-found-in-itself)
named 7 converted subjects, [§12.6](#126-the-family-at-b9aaa3d4--converted-tripwired-and-open)
and [§12.9](#129-gate-composition-and-the-workspace-bar) named 13, and the gate
carried 17. Both prose copies went stale *within the hour* of being reconciled,
twice. Two copies of one truth do not stay equal, so the second copy no longer
has authority: `rholang/tests/stack_depth_gate.rs` publishes the sets as the
constants `CONVERTED_DEPTH`, `CONVERTED_WIDTH`, `TRIPWIRE_DEPTH` and
`TRIPWIRE_WIDTH`, and its test `the_audit_agrees_with_the_gate` **fails** if this
block disagrees with them. The failure lands on the commit that separates them,
not at the next audit.

The gate in turn cannot lie about its own constants: `theta_depth_tripwire`
records every subject it drives through `assert_slope_below` and refuses to
finish unless that set is exactly `TRIPWIRE_DEPTH`, and
`converted_traversals_are_depth_independent` *iterates* `CONVERTED_DEPTH` /
`CONVERTED_WIDTH` rather than listing subjects beside them. So a name here is a
test that runs, and a test that runs is a name here.

<!-- GATE-SUBJECTS:BEGIN — DERIVED from rholang/tests/stack_depth_gate.rs.
     Checked by `the_audit_agrees_with_the_gate`; edit the gate's constants first. -->
```text
converted-depth: substitute_no_sort, substitute_binders, substitute, sort, score_cmp,
                 tree_drop, tree_clone, eval_with_nots, bincode_de, bincode_ser, pretty,
                 normalize, inj_attempt_clone, clone, clone_send_chain
converted-width: substitute_wide, sort_wide, score_cmp_wide, free_check, pretty_wide,
                 normalize_wide
tripwire-depth:  subst_and_charge, substitute_deep_binding, par_drop,
                 normalize_drop, encode, sort_nested_set, sort_nested_map,
                 clone_nested_set
tripwire-width:
totals:          converted=21, tripwired=8
```
<!-- GATE-SUBJECTS:END -->

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
claim the family is fixed: at the time §1–10 were written, **one leg of one
traversal** had landed. [§7](#7-disposition--what-is-done-and-what-is-not)
states what remained *then*;
[§12.6](#126-the-family-at-b9aaa3d4--converted-tripwired-and-open) states what
remains *now*, and [§8](#8--the-proof-standard) states the limits of the method
used to justify it.

**Diagram convention.** `docs/` in this repo has no PlantUML figure pipeline;
diagrams here are inline unicode box-drawing, matching the surrounding
documents.

**Math convention.** Inline mathematics uses GitHub's code-span-protected form
— dollar sign, backtick, expression, backtick, dollar sign, as in $`b\,N`$ —
and display mathematics uses a fenced block whose info string is `math`. The
**reversed** spelling, in which the backticks are on the outside and the dollar
signs on the inside, is a plain code span on GitHub: MathJax never sees it and
it renders as literal text. §§1–11 were originally written that way, in 23
places, and the delimiters were repaired in place on 2026-07-27. That repair
changed delimiters only: **no claim, number or word of prose in §§1–11 was
altered**, which is why those sections still read as the dated record they are.

---

## Table of contents

**Stratum 1 — the pre-Leg-2 audit (2026-07-26).**

1. [Executive summary](#1-executive-summary)
2. [The falsification experiment](#2-the-falsification-experiment)
3. [The recursive type family](#3-the-recursive-type-family)
4. [Enumerating the traversals — the method](#4-enumerating-the-traversals--the-method)
5. [Measured constants, per traversal and per profile](#5-measured-constants-per-traversal-and-per-profile)
6. [Why one level costs 195 KB — the attribution](#6-why-one-level-costs-195-kb--the-attribution)
7. [Disposition — what is done and what is not](#7-disposition--what-is-done-and-what-is-not)
8. [★ The proof standard](#8--the-proof-standard)
9. [Evidence ledger](#9-evidence-ledger)
10. [Reproducing every number here](#10-reproducing-every-number-here)

**Stratum 2 — the Leg-2 execution record (2026-07-26/27).**

11. [Leg-2 — execution record](#11-leg-2--execution-record-2026-07-2627)

**Stratum 3 — reconciliation with the tree (2026-07-27).**

12. [Reconciliation — the tree at `b9aaa3d4`](#12-reconciliation--the-tree-at-b9aaa3d4-2026-07-27)
    1. [What landed after §11 was written](#121-what-landed-after-11-was-written)
    2. [★ A corrected attribution — `normalize` was never the sorter in release](#122--a-corrected-attribution--normalize-was-never-the-sorter-in-release)
    3. [★★ 577 bytes of source abort a release node, before metering](#123--577-bytes-of-source-abort-a-release-node-before-metering)
    4. [★ What the enumeration method can and cannot see](#124--what-the-enumeration-method-can-and-cannot-see)
    5. [★ The vacuity ledger, and the two rules it produced](#125--the-vacuity-ledger-and-the-two-rules-it-produced)
    6. [The family at `b9aaa3d4` — converted, tripwired, and open](#126-the-family-at-b9aaa3d4--converted-tripwired-and-open)
    7. [★ `sort_nested_set` sits 2.2 % under its own ceiling](#127--sort_nested_set-sits-22--under-its-own-ceiling)
    8. [The codec falsification experiment](#128-the-codec-falsification-experiment--why-the-corpus-is-constructed-and-not-random)
    9. [Gate composition and the workspace bar](#129-gate-composition-and-the-workspace-bar)
    10. [Evidence ledger — second amendment](#1210-evidence-ledger--second-amendment)

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

where $`N`$ is bracket-nesting depth. Rust's default spawned-thread stack is
2 MiB, so $`S(9) = 1.88\ \text{MiB}`$ fits and $`S(10) = 2.07\ \text{MiB}`$ does
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
  (`bb7fcd20`): removing deep copies eliminates $`O(D^2)`$ heap churn and a real
  slice of the constant, but **cannot change the class**.
* **The measurement harness** (`stack_depth_probe.rs` + driver script) — the
  instrument that produced every number here.
* **The regression gate** (`stack_depth_gate.rs`) — see
  [§8.5](#85-the-depth-independence-gate).

### 1.4 What did not land

> ⚠ **SUPERSEDED (2026-07-27).** Every statement in this subsection is now
> false: Leg-2 landed for the substitution SCC, the sorter, the score tree,
> `rho-pure-eval::eval_with`, `FoldMatch::free_check` and the RSpace cold-store
> decoder. Thirteen subjects hold a constant native stack across a 1,024×
> parameter range in **both** profiles. See
> [§12.1](#121-what-landed-after-11-was-written) and
> [§12.6](#126-the-family-at-b9aaa3d4--converted-tripwired-and-open). The
> sentence below is retained because it is the prediction against which that
> work is measured.

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

**Design.** Build `Par` values of depth $`N`$ with an iterative bottom-up loop
(so construction contributes $`O(1)`$ stack), call **only**
`Substitute::substitute(term, 0, &Env::new())` on a thread created with an
explicit `stack_size(S)`, and bisect $`S`$ for
$`N \in \{10, 20, 40, 80\}`$. No parser, no reducer, no tuplespace, no tokio.

**Predicate.** ≈190 KB/level ⇒ the diagnosis holds. Materially flatter ⇒ the
depth is coming from elsewhere and the aim is wrong.

**Result.**

| depth $`N`$ | minimum surviving stack |
|-------------|-------------------------|
| 10          | 2,174,976 B (2,124 KiB) |
| 20          | 4,124,672 B (4,028 KiB) |
| 40          | 8,024,064 B (7,836 KiB) |
| 80          | 15,822,848 B (15,452 KiB) |

Least-squares fit: $`S(N) = 225{,}280 + 194{,}970\,N`$ — **190.40 KiB/level**,
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

> ⚠ **SUPERSEDED (2026-07-27) — "lower priority" was the wrong disposition for
> three of these.** All nine were measured in
> [§11.2](#112-the-not-separately-tabulated-list-now-measured), and three of the
> dispositions above did not survive contact with the instrument:
> `rho-pure-eval::eval_with` is a *separate* Θ(depth) SCC on the `where`-guard
> path (21,584 B/level debug); `FoldMatch::free_check` is a Θ(**width**) member,
> an axis this table does not have a column for; and `normalize_ann_proc` is the
> **most operationally severe member of the whole family**, because it runs
> before any term exists and therefore before metering
> ([§12.3](#123--577-bytes-of-source-abort-a-release-node-before-metering)).
> Being *found* is not the same as being *dispositioned*, and only measurement
> closes that gap — see
> [§12.4](#124--what-the-enumeration-method-can-and-cannot-see).

### 5.1 What the composite means operationally

The traversals are *sequential*, not nested, so the binding constraint is the
maximum, not the sum. On a default 2 MiB spawned thread:

```math
D_{\max} \;=\; \left\lfloor \frac{2{,}097{,}152 - a}{b} \right\rfloor
```

| profile | binding traversal | $`b`$ | $`D_{\max}`$ |
|---------|-------------------|------:|-------------:|
| debug   | `substitute` | 195,728 | **9** |
| release | `substitute` | 27,179 | **~76** |

**Measured directly** (bisecting *depth* at a fixed stack, rather than deriving
it from the fit — post-Leg-1):

| thread stack | where that stack comes from | debug | release |
|---|---|---:|---:|
| 2 MiB | Rust's default for a spawned thread when `RUST_MIN_STACK` is unset — **what a tokio worker gets** | **9** | **75** |
| 8 MiB | `.cargo/config.toml` `[env] RUST_MIN_STACK`, so any cargo-launched run | 41 | 307 |
| 128 MiB | the `RUST_MIN_STACK=134217728` workaround on every line of `demos/flt-church-desk/RUN-SHEET.md` | 684 | >4095 |

The derived and directly-measured figures agree exactly ($`\lfloor(2{,}097{,}152-34{,}905)/27{,}179\rfloor = 75`$).

Two things follow. **The reported bug is unchanged in debug** — depth 9 is still
the ceiling, which is what Leg-1 predicted of itself. **Leg-1 did move release**,
from 56 to 75 (+34%), because the release constant fell 25.4%. Neither is a fix:
both are a constant away from a program-controlled abort.

**On retiring the `RUST_MIN_STACK=134217728` workaround.** It cannot be retired
yet, and the audit trail should say why rather than leave it looking like
over-caution: at 128 MiB the release ceiling is >4095 and the debug ceiling is
684, versus 75 / 9 at the default. The workaround is load-bearing for the demos
today. `rholang-runtime/tests/church_desk_demo.rs::every_run_line_in_the_sheet_carries_the_stack_prefix`
asserts the prefix is present; that assertion inverts when — and only when — the
traversals in [§7.4](#74-not-landed--and-precisely-why) are converted.

After `substitute` is converted, the constraint moves to `sort_match`
($`D_{\max}`$ ≈ 26 debug / ≈ 320 release), then `PrettyPrinter`, then `Clone`.
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
fix**: it multiplies $`D_{\max}`$ by a constant and leaves a program-controlled
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
  raises $`D_{\max}`$ past 33 pushes terms into that asymmetry. **This is a
  protocol-visible ceiling that already exists**; the USER's decision was "no
  *new* protocol-level nesting cap", and this is not one, but it must be
  surfaced before it is discovered by a validator.

> ★★ **EXTENDED 2026-07-28 — the obligation in the paragraph above is now
> discharged.** The ceiling is *surfaced*: it has an executable boundary per
> envelope, a staged RED fixture, and an executable consensus leg. Nothing below
> moves a ceiling, bounds the write side, or changes production code — the
> read-side repair widens the set of byte strings a node accepts and therefore
> needs a coordinated version bump, which is F1r3node's decision and not this
> audit's. §7.3.1–§7.3.6 record what was measured and what remains open.

#### 7.3.1 Where the ceiling now lives as an executable claim

| artefact | what it holds | tests |
|---|---|---|
| `models/tests/par_prost_depth_ceiling.rs` | the staged RED fixture (build / write / read) with its **adjacent** depth-33 control; the per-envelope register with the depths written out; the anchoring to `COLLECTION_DEPTH_LIMIT` | 4 |
| `rholang/tests/replay_output_value_depth_ceiling.rs` | ★★ the consensus leg — **red on replay, green on play**, same binary, same term, one bool apart | 1 |
| `rholang/tests/stack_depth_gate.rs` (new section) | the per-envelope boundary with the depths **derived** rather than transcribed; the acknowledged build-side inventory and its headroom tripwire; the completeness check that fails when a new build path joins | 3 |

The three-stage shape of the fixture is the point of it. Each stage names a
different failure:

| stage | assertion | a failure there means |
|---|---|---|
| **build** | `par_depth(&term) == 34`, asserted first | *the term could not be built* — a fixture bug, not a finding |
| **write** | `encode_to_vec()`, then `bytes.len() >= 34` | *the write failed* — the defect **inverted**; something capped the encoder |
| **read** | the error **kind** is `RecursionLimitReached` | ✅ *the read failed* — the defect, and only this |

⚠ The error-kind match is load-bearing, and it was shown red: truncating the
buffer by three bytes makes the decode fail with `BufferUnderflow`, which a bare
`is_err()` would have accepted as this finding.

#### 7.3.2 ★★ The ceiling is PER ENVELOPE — it is a table, not a number

One bracket costs three nested-message levels
(`Par.exprs → Expr.e_list_body → EList.ps`), and the innermost `Par` spends one
more on the leaf `Expr` that carries the ground value. An envelope that
transports the `Par` inside other messages spends its own levels first. Writing
$`W(E)`$ for the nested-message levels envelope $`E`$ interposes before the
outermost `Par` is entered:

```math
D_{\max}(E) \;=\; \left\lfloor \frac{L - 1 - W(E)}{3} \right\rfloor,
\qquad L = \mathtt{RECURSION\_LIMIT} = 100 .
```

Measured at `ab1908e0`, **identically in debug and release** — the limit is a
protocol constant, not a stack measurement, so it does not carry the
per-profile variation everything else in this audit does:

| envelope | $`W`$ | last depth that decodes | first that does not |
|---|---:|---:|---:|
| ★★ `Par` (bare) — the replay decode of `ProduceEventProto.outputValue` | 0 | **33** | **34** |
| `DataAtNameByBlockQuery.par` — `getDataAtName` ingress | 1 | 32 | 33 |
| `DataWithBlockInfo.postBlockData[0]` | 1 | 32 | 33 |
| `RhoDataPayload.par[0]` | 1 | 32 | 33 |
| `WaitingContinuationInfo.postBlockContinuation` | 1 | 32 | 33 |
| `RhoDataResponse > Payload > par[0]` — `getDataAtName` egress | 2 | 32 | 33 |
| `ContinuationsWithBlockInfo > WCI > postBlockContinuation` | 2 | 32 | 33 |
| `ContinuationAtNamePayload > CWBI > WCI > postBlockContinuation` | 3 | 32 | 33 |
| ★ `ContinuationAtNameResponse > … > postBlockContinuation` — `listenForContinuationAtName` egress | 4 | **31** | **32** |

⚠ **Three distinct ceilings, not one, and not "bare minus one".** The
`listenForContinuationAtName` response envelope accepts **31** — two levels below
the bare `Par`. Stating this limit as a single number over-states that
endpoint's capacity by two nesting levels. Every row above is *executed* at both
$`D_{\max}`$ and $`D_{\max}+1`$, in both files, and the closed form is
asserted against the measurement so that a wrong $`W`$ and a wrong depth
cannot survive together.

#### 7.3.3 ★★ The exposure, enumerated, with verdicts

| round trip | verdict |
|---|---|
| cold store / history / LMDB | **not exposed** — bincode wire; `rspace++/src/` contains no `prost` usage |
| block body / deploy log | **not exposed** — `CasperMessage.proto` references exactly one `rhoapi` type (`PCost`, line 293) and `DeployDataProto.term` is a `string` (line 161) |
| ★★ `ProduceEventProto.outputValue` → **replay** | ★★ **exposed, consensus-class** — §7.3.4 |
| gRPC egress (`getDataAtName`, `listenForContinuationAtName`, …) | **exposed, client-class** — ceilings 32 and 31 per §7.3.2 |
| gRPC ingress | **fails safe** — an over-deep request is an `Err`, never a `SIGSEGV`; correct as is |
| ★ `EPathMap` field-8 trie keys | **exposed — a second ceiling**, `COLLECTION_DEPTH_LIMIT = 32`; §7.3.5 |

#### 7.3.4 ★★ The consensus-class member: the replay decode

**Why these bytes reach a decoder at all.** `ProduceEventProto.outputValue` is
`repeated bytes` (`models/src/main/protobuf/CasperMessage.proto:393`), **not** a
nested message. Decoding a block body therefore does not descend into those
bytes and cannot reject them; and because the eventual `Par::decode` is a fresh
*top-level* decode, it receives the full 100-level budget and sits at the
**bare** ceiling of $`W = 0`$. The block is well-formed. Only replay is not.

**The asymmetry, traced.**

```text
  ┌──────────────────────── PROPOSER (play) ────────────────────────┐
  │  Produce::create ──────────▶ output_value: vec![]               │
  │        │                            │                           │
  │        │  RSpace::locked_produce returns THAT Produce           │
  │        ▼                            ▼                           │
  │  produce_inner ────────▶ continue_produce_process(is_replay=❰false❱)
  │                                     │                           │
  │                          reduce.rs:1065 iterates ∅ ⇒ NO DECODE  │
  │                                     │                           │
  │                          reduce.rs writes the bytes ──▶ BLOCK   │
  └─────────────────────────────────────┼───────────────────────────┘
                                        │  the block travels
  ┌─────────────────────────────────────▼───────────────────────────┐
  │                       VALIDATOR (replay)                         │
  │  ReplayRSpace::locked_produce returns the Produce FROM THE TRACE │
  │        │        (comm.produces.find(hash == produce_ref.hash))   │
  │        ▼                                                         │
  │  produce_inner ────────▶ continue_produce_process(is_replay=❰true❱)
  │                                     │                            │
  │                          reduce.rs:1065 DECODES real bytes       │
  │                                     │                            │
  │                          depth ≥ 34 ⇒ Err(DecodeError)          │
  │                                     ▼                            │
  │  EvaluateResult::errors ≠ ∅ ⇒ eval_successful = false            │
  │      (casper/src/rust/rholang/replay_runtime.rs:427)             │
  │                                     ▼                            │
  │  processed_deploy.is_failed != !eval_successful  ⇒  :443         │
  │                                     ▼                            │
  │              ✗ CasperError::ReplayFailure                        │
  └──────────────────────────────────────────────────────────────────┘
```

**The proposer builds a block no validator can replay.** `Produce`'s
`PartialEq`/`Hash`/`Ord` are hash-only by design (`rspace++/src/rspace/trace/event.rs`
— *"metadata fields like `is_deterministic`, `output_value`, and `failed` … must
NOT affect identity"*), so the payload rides in the trace without perturbing
event identity: the rig matches, the COMM fires, and the decode is reached.

**Executed.** `rholang/tests/replay_output_value_depth_ceiling.rs` plays a
program, splices `nested_list(d).encode_to_vec()` into the recorded log's
`output_value`s exactly as a block from a node with a ninth non-deterministic
operation would carry it, rigs the mutated log, and replays. The `is_replay`
bool is read from the two spaces themselves rather than assumed:

| | depth 33 | depth 34 |
|---|---|---|
| play (`is_replay = false`) | green | **green** |
| replay (`is_replay = true`) | green | **RED — `DecodeError(… recursion limit reached)`** |

The run also asserts, as the executable form of *"the proposer never decodes"*,
that all five `Produce`s the play pass recorded carried `output_value == []`
before the splice. Confirmed identical over 25 consecutive runs.

⚠ **Not reachable today, and the guard is a shape rather than a check.**
`output_value` is written from one site gated on `non_deterministic_ops()`,
which at `ab1908e0` contains exactly **eight** entries — `GPT4`, `DALLE3`,
`TEXT_TO_AUDIO`, `OLLAMA_CHAT`, `OLLAMA_GENERATE`, `OLLAMA_MODELS`, `GRPC_TELL`,
`CHROMA_QUERY` (`rholang/src/rust/interpreter/system_processes.rs:192-203`).
None of their return constructions exceeds depth 3, and the deepest sits behind
the non-default `chromadb` feature. **But nothing checks the depth.** The
property that keeps this dormant is those eight functions' return *shapes*. A
ninth operation that echoes a caller-supplied `Par` back through
`contract_call.rs`'s `NonDeterministicCall` arm makes it live with no other
change anywhere.

#### 7.3.5 ★ `COLLECTION_DEPTH_LIMIT = 32` is anchored to this envelope

`models/src/rust/canonical_path.rs:72-83` states the anchoring in prose: the
EPathMap trie-key decoder's limit is *"today's effective prost envelope (prost
`RECURSION_LIMIT` = 100 message levels ≈ 25-33 collection levels)"*. On the
nested-list shape both readers accept, the two boundaries **coincide**:

| wrappers $`n`$ in `[[…[0]…]]` | `Par::decode` (prost) | `decode_trie_path` (trie) |
|---:|---|---|
| 33 | ✅ accepts | ✅ accepts — 32 counted levels, the outer list being the split form at level 0 |
| 34 | ❌ `RecursionLimitReached` | ❌ `DepthLimitExceeded` |

The off-by-one in the second column is the trie codec's own split-form
convention, stated by its own `depth_limit_decode_rejects_beyond_32`. Net of it
the boundaries are the same boundary — which is the anchoring working, not a
coincidence.

⚠ **The consequence, and the reason the two are named together wherever either
is named.** Raise the prost ceiling alone and the trie decoder becomes the
binding constraint one level lower; raise `COLLECTION_DEPTH_LIMIT` alone and
prost becomes it. Either move buys nothing. **They move together or not at
all.** Held by execution in
`models/tests/par_prost_depth_ceiling.rs::the_two_read_ceilings_are_anchored_together`.

#### 7.3.6 The build side clears the wire — an inventory, not a threshold

Every build-side path already carries terms far deeper than any reader accepts,
so a test asserting *"nothing builds deeper than the wire can carry"* would be
red on arrival for every row, and a gate that is red on arrival gets muted. The
claim actually asserted is the true one — **the wire is the binding constraint,
and every build path clears it with headroom** — recorded as an acknowledged
inventory:

| build path | ceiling | measured in |
|---|---:|---|
| ★ `env_get_deploy` — **the binding one** | 283 | `rholang/tests/deploy_depth_ceiling.rs` |
| `plain_deploy` | 6,831 | `rholang/tests/deploy_depth_ceiling.rs` |
| `inj_attempt` `set-initial-cost` | ≥ 1,048,576 | `stack_depth_gate.rs`, `inj_attempt_clone_body` |
| `normalize` (source-text ingress, pre-metering) | ≥ 39,960 | `stack_depth_probe.rs`; evidence row [E63] |

The tripwire is the *ratio*: $`283 / 33 = 8.57`$, floored at **8×**. It fires
when a build path regresses or a read ceiling rises — the same standing this
audit gives `assert_slope_below`, namely *"not a pass; it detects getting
worse"*. Completeness is mechanical: the gate parses
`deploy_depth_ceiling.rs`'s `subject_source` dispatch and fails if it drives a
path this inventory does not classify, so **a new build path cannot join
silently**.

**What was not done, and whose decision it is.** The read-side repair — raising
or removing the ceiling — changes which byte strings a node accepts and is
therefore consensus-visible: it needs a coordinated version bump and belongs to
F1r3node's protocol surface. Capping the write side would be a *new*
protocol-level nesting cap, which the standing decision recorded at the head of
this section forbids. Neither was taken here. What was taken is the
surfacing.

### 7.4 Not landed — and precisely why

> ⚠ **SUPERSEDED (2026-07-27).** All three rows below have landed.
> `Substitute::substitute` Leg-2 landed in two steps — the SCC itself
> (`f11ffb54`) and the removal of the un-sorted intermediate's recursive
> teardown (`b98fa20a`) — and now measures **0 B/level in both profiles**. The
> `ParSortMatcher` family landed as an explicit pushdown machine (`2c32b173`),
> also **0 B/level in both profiles**. `PrettyPrinter` has its prerequisite
> (`739368a4`, the closed `PpNode` alphabet) but its conversion has **not**
> landed and it is the one remaining hand-written depth-axis member; its current
> constants are in
> [§12.6](#126-the-family-at-b9aaa3d4--converted-tripwired-and-open). The table
> is retained as the prediction; the outcome is
> [§12.1](#121-what-landed-after-11-was-written).

| traversal | why not |
|---|---|
| `Substitute::substitute` **Leg-2** | The conversion is an explicit-worklist rewrite across 8 `SubstituteTrait` impls (~1,344 lines), and — per [§8](#8--the-proof-standard) — is only acceptable in consensus code accompanied by a recursive oracle twin and a differential harness asserting byte-identical results and an identical ordered charge trace. That is a single, well-specified unit of work; it was not completed in this pass. It is the **only** thing standing between the current state and the reported bug being fixed. |
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
(`substitute.rs:149`, `:189` — `:52` / `:81` before the 2026-07-28 by-value
conversion added their documentation; the charging code below is unchanged by
it, see [E95]) as

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
7. **Bounding the *stack* can unbound something else.** A Θ(depth) traversal
   aborts on a deep term; that abort is a fault, but it is also a *ceiling*, and
   anything downstream of the traversal was incidentally bounded by it. Removing
   the ceiling removes the bound. The standard establishes $`O(1)`$ native stack
   and says nothing about what the now-reachable depths cost in heap, output size
   or time.

   > **Concrete instance, measured 2026-07-27 on the converted pretty printer.**
   > `PrettyPrinter` indents by nesting level: `PpKont::BundleK`, `NewK`,
   > `ReceiveK`, `MatchK`, `CaseJoin` and `ParK` each emit
   > `indent_string().repeat(indent)`, two bytes per level. For $`D`$ nested
   > `Bundle`s the rendered string is therefore quadratic in $`D`$ — measured
   > exactly:
   >
   > | $`D`$ | 64 | 128 | 256 | 512 | 1,024 |
   > |---|---:|---:|---:|---:|---:|
   > | output bytes | 4,929 | 18,049 | 68,865 | 268,801 | 1,061,889 |
   >
   > which is $`D^{2} + 13D + 1`$ at every point, i.e. $`\Theta(D^{2})`$. At the
   > pre-conversion 41,984 B/level (debug) the printer aborted at
   > $`D_{\max} \approx 50`$ on a 2 MiB worker — 4,929 bytes of output, which is
   > nothing — so the amplification was capped by the fault, not by any policy.
   > After Stage D it renders whatever it is handed: a term of $`10^{5}`$ nested
   > bundles, a few MiB of `Par`, produces a **~10 GB** string, and
   > `PRETTY_PRINTER_OUTPUT_TRIM_AFTER` cannot help because `PrettyPrinter::cap`
   > truncates the *finished* string.
   >
   > This is not an argument against the conversion; the abort was strictly
   > worse, being a node-killing fault reachable from untrusted input through
   > `rho:io:stdout`. It is an argument that **each conversion should state what
   > the removed ceiling was bounding**, and that a heap or output budget is the
   > right instrument for what a stack limit was doing by accident. No such
   > budget exists here yet, which is why this is a named limit and not a
   > footnote.

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
| `assert_depth_independent(name, stack)` | The traversal survives a **fixed** 1 MiB stack at depths 4, 16, 64, **256**. A 64× depth range means only an $`O(1)`$-in-depth traversal can pass. **Profile-independent by construction** — it asserts a *shape*, never a constant. This is the real bar. |
| `assert_slope_below(name, ceiling, lo, hi)` | Bisects minimum stack at two depths, derives B/level, fails if it exceeds a ceiling. A **tripwire, not a pass**: it detects a traversal getting *worse* while keeping the residual visible in code. |

**Why the gate is not backend-fragile.** The real assertion never mentions a
byte count. Only the tripwire does, and its ceilings are selected per profile via
`cfg!(debug_assertions)` and set ≈1.5× above measured, so codegen drift does not
flake while an order-of-magnitude regression still trips. Per-profile constants
are recorded in [§5](#5-measured-constants-per-traversal-and-per-profile) and in
the gate's own module documentation, because the next person will not otherwise
know that mettail-rust's `codegen-backend = "cranelift"` inflates them again.

**Current state — stated plainly.**

> ⚠ **SUPERSEDED (2026-07-27).** The snapshot below is the 2026-07-26 gate. The
> converted list is no longer empty (13 subjects), the tripwire no longer carries
> `substitute`, `sort` or `bincode_de`, and the reproducer is green in **both**
> profiles and no longer `#[ignore]`d. The current composition and readings are
> in [§12.9](#129-gate-composition-and-the-workspace-bar).

```
DEBUG                                              RELEASE
converted_…_are_depth_independent  PASS (empty)    PASS (empty)
theta_depth_tripwire               PASS            PASS
    substitute   195,754 B/level                       27,136 B/level
    sort          78,592                                 6,485
    clone         15,872                                 2,852
    encode         1,932                                   302
    drop             464                                   144
reported_reproducer_…              RED (depth 10)  PASS (depth 70)
```

Both profiles were run (`--include-ignored`, so the reproducer executes). Note
that the reproducer **passes in release and fails in debug**: the class is
present in both, but the constant decides which profile notices. That is exactly
why the real assertion asserts a shape and not a byte count — and why a gate
validated in only one profile would have reported success here.

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
| E17 | Post-Leg-1 max depth: 9/75 (2 MiB), 41/307 (8 MiB), 684/>4095 (128 MiB), debug/release | **Measured** — depth bisected at fixed stack, §5.1 |
| E18 | Gate passes in BOTH profiles; reproducer is RED in debug and PASS in release | **Measured** — §8.5 |

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

---

## 11. Leg-2 — execution record (2026-07-26/27)

> This section is an **amendment**. Sections 1–10 record the state before Leg-2
> and are left as written, so that a reviewer can see what was predicted and
> what was found. Where a §1–10 claim has been superseded, the superseding
> statement is here and says so explicitly.

### 11.1 Three falsification experiments, run before any conversion code

The conversion was preceded by three experiments, each designed to **refute** a
premise of the plan rather than to confirm it. All three ran on the harness in
[§10](#10-reproducing-every-number-here), extended with the subjects named below.

#### F1 — "the score tree is a fifth traversal family" (premise UPHELD)

`models/src/rust/rholang/sorter/score_tree.rs` defines `Tree<T>`, a recursive
**Rust** type that is not a proto message. The enumeration method in
[§3](#3-the-recursive-type-family) — Tarjan over `RhoTypes.proto` — structurally
cannot see it. The refutation condition was "all three probes are flat".

| subject | what it drives | debug B/level | release B/level |
|---|---|---:|---:|
| `score_cmp` | `compare_score`, **depth** axis | **1,329** | 128 |
| `score_cmp_wide` | `compare_score_nodes`, **width** axis | **201** | 0 (see below) |
| `tree_drop` | `drop_in_place::<Tree<ScoreAtom>>` | **370** | 204 |
| `tree_clone` | derived `<Tree<T> as Clone>::clone` | **1,578** | 485 |
| `tree_eq` | derived `<Tree<T> as PartialEq>::eq` | **719** | — |

**Not flat. The premise stands and Stage C is five traversals, not one.**

Two findings that only measurement produces:

* **The width axis is real and was never enumerated.**
  `compare_score_nodes` recursed on the list *tail* (`&left[1..]`), so its
  native stack grew with **sibling count**. Sibling count is program-controlled.
  A proto-message SCC cannot express that axis at all, and no gate looked for it.
* **The release 0 is a codegen accident, not a property.** At `-O2` LLVM turns
  that tail call into a loop; at `-O0` it does not. A consensus-liveness
  property must not rest on an optimiser's discretion, which is the argument for
  an explicit loop rather than for leaving it alone.

⚠ **The gate could not have caught any of this on its own probe shape.** A
linear chain sorts as a **one-element** vector, and `Vec::sort_by` on one
element performs **zero comparisons**. Converting `sort_match` alone would have
left the comparator Θ(depth) *and the gate would have passed*.

#### F2 — "an owned `Env` per work item is acceptable" (premise REFUTED)

`Env::shift(j)` (`rho-pure-eval/src/env.rs`) is
`Env { shift: self.shift + j, ..(*self).clone() }` — a full
`HashMap<i32, Par>` clone, hence a `<Par as Clone>::clone` of **every bound
value**, at **every** binder level.

| subject | environment | debug B/level |
|---|---|---:|
| `subst_binders` | populated, bound value of depth $`N`$ | **48,878** |
| `subst_binders_ground_env` | populated, ground bound value | **33,242** |
| difference | | **15,636** |

15,636 against `Par::clone`'s measured 15,875 — **agreement to 1.5%**. The
environment clone contributes one full deep clone per level, so a worklist
storing owned `Env`s would have remained Θ(depth) *however the traversal itself
was written*. The design therefore carries `(depth, shift_delta)` per item plus
one **borrowed** root environment.

**Post-conversion the two readings are identical (0 B/level each)** — the
mechanism, removed, confirmed by the same instrument that found it.

#### F3 — "decode fails safe" (premise REFUTED — this is the most serious finding)

[§1.2(b)](#12-three-corrections-to-the-incoming-analysis) established that
`prost` decode is capped at 100 nested messages, so it returns `Err` at term
depth 34. That is true, and it is **not the whole story**: `models/build.rs`
also attaches `serde::Serialize` / `serde::Deserialize` to every `.rhoapi`
message, and RSpace serialises datums and continuations with **bincode 1.3.3**
(`rspace++/src/rspace/serializers/serializers.rs`), which has **no recursion
limit at all**.

| | debug B/level | release B/level | cap |
|---|---:|---:|---|
| `prost` decode | 26,624 | — | **`Err` at term depth 34** (bisected) |
| **bincode decode** | **28,362** | **12,894** | ⚠ **none — decoded successfully at depth 800** |
| bincode encode | 3,052 | 329 | none |

Bisected directly: bincode round-trips at depths 33, 34, 40, 100, 200, 400 and
**800**, where `prost` returns `Err` from 34 onward.

**Why this matters, stated precisely.** Encode is ~9× (debug) to ~39×
(release) cheaper per level than decode, so a term can be *written* on a stack
that cannot *read it back* — and the read-back failure is an `abort()`, not an
`Err`. On a 2 MiB worker in release the bincode decode ceiling is

```math
D_{\max} = \left\lfloor \frac{2{,}097{,}152 - 20{,}569}{12{,}894} \right\rfloor = 161
```

Before Leg-2 that was unreachable: `substitute` capped the reducer at depth 75
in release. **After Leg-2 it is reachable**, and a datum written to LMDB at
depth > 161 aborts the node on read-back — on every restart, because the datum
persists. This is a *consequence of the fix*, it is not fixed by the fix, and it
is recorded here rather than discovered by a validator.

It does not change the disposition of [§7.3](#73-the-prost-decode-ceiling-is-a-constraint-on-the-fix-not-a-defect-to-fix)
— it makes it sharper: the encode/decode asymmetry that section names as a
"protocol-visible ceiling that already exists" has a **second, uncapped, harder
failure mode** on the RSpace codec, and any conversion that raises the reducer's
reachable depth walks toward it.

### 11.2 The "not separately tabulated" list, now measured

[§5](#5-measured-constants-per-traversal-and-per-profile) named nine candidates
without measuring them. All nine were measured, on the same harness. The
threshold for "binding" is `Par::clone`'s 15,875 B/level (debug), because below
that the derived-traversal residual dominates anyway.

| subject | debug B/level | release B/level | binding? | attribution |
|---|---:|---:|---|---|
| `normalize` (`Compiler::source_to_adt`) | **78,579** | 7,247 | **yes** | ⚠ attribution **wrong** — see [§12.2](#122--a-corrected-attribution--normalize-was-never-the-sorter-in-release); on the **deploy path**, before any term exists |
| `sorted_par_hash_set::insert` | **78,543** | 6,495 | **yes** | `sort_match` (via `SortedParHashSet::sort`) |
| `sorted_par_map::insert` | **78,583** | 6,495 | **yes** | `sort_match` |
| `eval_with` (nested `ENot`) | **21,584** | 3,359 | **yes** | its **own** recursion — a separate SCC in `rho-pure-eval` |
| `eval_with` (nested `EList`) | 15,914 | 2,611 | — | `<Par as Clone>::clone`; the `EListBody` arm does not descend |
| `rhoapi_ext::make_mut` | 15,850 | 2,320 | — | `<Par as Clone>::clone` (`Arc::make_mut` copy-on-write) |
| `par_to_sexpr` | 7,188 | 835 | no | its own recursion |
| `ParCount::min_max_par` | 5,734 | 1,587 | no | its own recursion, through `connectives` only |
| `FoldMatch::free_check` | **483 / sibling** | 320 / sibling | no | ⚠ **width** axis — recurses on the slice tail |
| `pathmap_zipper` / `par_to_path` | 410 | 0 | no | — |

Four consequences.

1. **Three of them are `sort_match` wearing a different name.** Converting the
   sorter fixes `normalize`, `SortedParHashSet` and `SortedParMap` at once. All
   three must be **re-measured** after that conversion, because each may have a
   second-order constant of its own underneath.

   > ⚠ **PARTLY REFUTED by exactly the re-measurement this paragraph demanded.**
   > `SortedParHashSet` and `SortedParMap` were `sort_match` and fell to the
   > derived `<Par as Clone>::clone` floor when it was converted. `normalize`
   > was **not**: in release its own recursion always bound, and the resemblance
   > to `sort_match`'s constant was a debug-only coincidence. The correction is
   > [§12.2](#122--a-corrected-attribution--normalize-was-never-the-sorter-in-release).
   > The instruction "re-measure, because each may have a second-order constant
   > of its own underneath" was right, and it is what caught this.
2. **`rho-pure-eval::eval_with` is a genuinely independent Θ(depth) SCC**, at
   21,584 B/level on expression nesting, and it is not any of rows 1–10. It is
   reachable from `where`-clause guard evaluation. It is **not** in Leg-2's scope
   as scoped, and it is named here so that "not measured" is not confused with
   "not present".
3. **`FoldMatch::free_check` is a second width-axis member**, in the matcher
   rather than in the sorter.
4. `eval_with` on an `EList` chain and `make_mut` are `Par::clone` — the M3
   discriminator ([§4.3](#43-m3--measurement-as-the-discriminator-of-last-resort))
   working exactly as described: static analysis proposed, measurement disposed.

### 11.3 What landed

| stage | landed | commit |
|---|---|---|
| **A** — harness, canonical child-slot table, iterative `dismantle`, constructed corpus, broadened probe | ✅ | `550b967a` |
| **B** — the substitution SCC | ✅ | `f11ffb54` |
| **C-1** — the score tree's five traversals | ✅ | `6ce7c5b9` |
| **C-2** — `ParSortMatcher` and the nine sorter files | ❌ not landed | — |
| **D** — `PrettyPrinter` | ❌ not landed | — |

> ⚠ **SUPERSEDED (2026-07-27).** C-2 landed at `2c32b173`. Stages E and F —
> which did not exist when this table was written — landed the substitution
> intermediate's teardown (`b98fa20a`), `FoldMatch::free_check` (`6714a128`),
> `rho-pure-eval::eval_with` (`a3fd6fe4`) and the RSpace cold-store decoder
> (`9a5521a2`, `2bcfaf87`, `af1a426b`). D has its prerequisite (`739368a4`) and
> is still open. The complete, current stage table is
> [§12.1](#121-what-landed-after-11-was-written).

#### Stage A — two harness defects that would have voided a green result

**`generate_par` was totally vacuous.**
`models/src/rust/test_utils/test_utils.rs` sized every collection with an
exclusive `0..1`; proptest's `SizeRange::end_incl()` is `end - 1`, so `0..1`
means *exactly zero elements, always*. `generate_par(d)` produced, for every
`d`, one of only **two** values, and `generate_send` / `_receive` / `_new` /
`_match` / `_bundle` / `_connective` were **never invoked**. This is a stronger
form of the defect recorded as limit #3 in
[§8.4](#84--the-limits--what-this-standard-does-not-establish): the earlier
finding was "zero `Expr` nodes across 256 draws"; the cause is that the
generators below `Par` never ran at all.

Fixed to `0..=2`, with `assert_generator_not_vacuous` and six named guards.

> ⚠ **A consequence for whoever extends `models/tests/scored_term_sort_test.rs`.**
> The sorter is a **normalizer** and is therefore **not injective**. Directly
> measured: `Par{exprs:[GInt 1, GInt 2]}` and `Par{exprs:[GInt 2, GInt 1]}` are
> unequal terms (`Par`'s `PartialEq` compares `exprs` as an ordered `Vec`) that
> sort to **equal terms with equal scores**. Three tests in that file assert an
> iff that is consequently false; they pass only because two independent draws
> are unlikely to be permutations of one another. **They cannot gate the
> sorter's own conversion**, which is why Stage C-1 carries its own total-order
> differential.

**The gate's `dismantle` walked `EListBody` only.** Anything else dropped
recursively, so any broadened probe shape would have measured `Drop` rather
than its subject. Replaced by
`models::rust::rholang::par_children::dismantle`, driven by the canonical
child-slot table.

**The canonical child-slot table.** Four independent enumerations of the
36-variant `ExprInstance` schema already existed. `par_children.rs` makes drift
a **compile error** (every match is exhaustive, no `_` arm) and records
`substitute_descends_into` — the checkable statement that `EPathmapBody` and
`EZipperBody` are **not** descended into by substitution, which must be
reproduced verbatim because descending would change signed bytes.

#### Stage B — the substitution SCC

```
                          debug B/level        release B/level
  substitute_no_sort      195,728  ->  0       27,179  ->  0
  substitute (sorted)     195,754  ->  78,583  27,179  ->  6,495   = the SORTER
  binders, deep env        48,878  ->  0
  binders, ground env      33,242  ->  0
  width (siblings)              0  ->  0
```

`reported_reproducer_depth_survives_a_default_worker_stack` is **green** and no
longer `#[ignore]`d.

⚠ **The trap [§5.1](#51-what-the-composite-means-operationally) predicted, in
the measured numbers.** `SubstituteTrait<Par>::substitute` is
`substitute_no_sort` followed by one `ParSortMatcher::sort_match`. Stage B
converted the first half, so the sorted entry point moved from 195,754 to
78,583 B/level — *the sorter's constant to within 0.01%* — which supports depth
≈ 24 on a 2 MiB worker, comfortably past the depth-10 reproducer. **The
reproducer test therefore went green while the sorter, the printer and (before
C-1) the comparator were all still Θ(depth).** That distinction is written into
the test's own doc comment.

**The named residual.** `Env::get` returns its value cloned, because
substituting a `BoundVar` splices the bound term into the result and a binding
may be used many times. Measured 15,850 B/level — `Par::clone` to within 0.2% —
identical in the recursive form, and bounded by the depth of the **bound value**
rather than of the term traversed. It sits in the tripwire as
`substitute_deep_binding`, never in the converted list.

**Breaking change.** `SubstituteTrait<Expr>` is removed. Its `substitute` was
dead and had diverged from its live twin — the `EMinusBody` arm rebuilt the term
as an `EPlusBody`. `Expr`'s live capability survives as the inherent
`Substitute::substitute_expr_no_sort`.

#### Stage C-1 — the score tree

`compare_score`, the sibling walk, `Clone`, `PartialEq` and `Drop` are explicit
worklists; `Debug` is deliberately left derived (row 6's class; assertion-message
only; hand-writing it would trade format-drift risk for no liveness gain).

The obligation was discharged as [§8.2](#82-why-each-conversion-is-neutral-by-construction--per-traversal)
requires for this traversal — *stricter*, not vacuous, because
`cost_accounting/sig.rs` signs `sort_match(&par).term.encode_to_vec()`:
agreement with the retained recursive oracle over every ordered pair,
reflexivity, antisymmetry, transitivity over every ordered triple, and — the
property `sig.rs` actually depends on — that `sort_vec` produces the **oracle's
permutation**, not merely a same-multiset ordering.

`sort_by` is kept and `sort_unstable_by` is rejected: the comparator returns
`Equal` for distinct terms with equal scores, so an unstable sort would be free
to reorder them and fork the canonical form.

### 11.4 The gate, and one thing it found in itself

> ⚠ **SUPERSEDED AS STATUS — retained as history.** The list immediately below
> was true when §11 was written and names **7** subjects; the gate now carries
> **17**. It is left unedited because §11 is a dated stratum and the ledger
> records what was known when. For the current sets see
> [§0](#0-current-subject-sets--derived-from-the-gate-not-transcribed), which is
> the document's only live enumeration and is checked against the gate by
> `the_audit_agrees_with_the_gate`.

`converted_traversals_are_depth_independent` now carries, **in both profiles**:

```
depth axis   substitute_no_sort   substitute_binders
             score_cmp            tree_drop            tree_clone
width axis   substitute_wide      score_cmp_wide
```

`assert_depth_independent` gained a second half, and it is not decoration.
The fixed-stack ladder alone is **insufficient** for the cheap members:
`compare_score` at 1,329 B/level with a ~40 KiB intercept needs 381 KiB at depth
256, which fits inside the gate's 1 MiB stack — a still-Θ(depth) comparator
would have **passed**. `assert_no_slope` bisects the minimum stack at the two
ends of a 4 → 4,096 ladder (4 → 65,536 on the width axis) and requires no
growth. It is profile-independent because it compares a traversal against
itself.

Two harness defects the gate found in itself, both fixed:

* the score-tree subjects built their inputs on the gated thread and reported
  **78,573 B/level** — the sorter's constant to within 0.01%, not the
  comparator's 1,298;
* `substitute_binders` reported **443 B/step** because it dropped a deep
  *environment* through the recursive `drop_in_place` after the traversal had
  already finished.

Both are the same lesson as the `prost` field-number bug in
[§4.3](#43-m3--measurement-as-the-discriminator-of-last-resort) and the
`generate_par` vacuity in §11.3: **a harness that cannot fail loudly will
eventually report a comfortable number for the wrong reason.** That is now
three occurrences in this work.

### 11.5 What remains, and the revised endpoint

> ⚠ **SUPERSEDED (2026-07-27).** Four of the seven rows below have since been
> converted and now measure 0 B/level in both profiles: the sorter (`2c32b173`),
> `rho-pure-eval::eval_with` (`a3fd6fe4`), bincode decode (`9a5521a2` +
> `af1a426b`) and `FoldMatch::free_check` (`6714a128`). The current list — with
> the one hand-written depth-axis member that is left, the derived class, and
> the open normalizer question — is
> [§12.6](#126-the-family-at-b9aaa3d4--converted-tripwired-and-open).

| traversal | debug B/level | release B/level | why it is still here |
|---|---:|---:|---|
| `ParSortMatcher` + 9 sorter files | 78,579 | 6,495 | **Stage C-2, not landed.** It is now the binding constraint on the reduce path, and it is what `substitute` (sorted), `normalize`, `SortedParHashSet` and `SortedParMap` all reduce to. |
| `PrettyPrinter::_build_string_from_expr` | 41,840 | 4,242 | **Stage D, not landed.** Prerequisite: replacing the `&dyn Any` dispatch with a closed `PpNode` enum — `Any` cannot be made exhaustive, and an unmatched type silently yields `"<unprintable: …>"`. Its state mutates and is never restored, so a driver must reproduce the *interleaving*, not merely the post-order. |
| `rho-pure-eval::eval_with` | 21,584 | 3,359 | a separate SCC, discovered by §11.2, outside Leg-2's scope as scoped |
| bincode decode | 28,362 | 12,894 | derived serde; **uncapped**; see §11.1 F3 |
| `<Par as Clone>::clone` | 15,875 | 2,852 | derived — §7.2 disposition; irreducible at `Env::get` |
| `FoldMatch::free_check` | 483 / sibling | 320 / sibling | width axis, in the matcher |
| `drop_in_place::<Par>` | 470 | 219 | the irreducible member — §7.2, §8.4 limit #6 |

**The endpoint stated in [§7.4](#74-not-landed--and-precisely-why) is
incomplete, and §11.2 is why.** "Convert substitution, the sorter and the
printer" leaves at least two measured Θ(depth) members above `Par::clone`'s
constant — `rho-pure-eval::eval_with` and bincode decode — plus a second
width-axis member in the matcher. Done-for-the-family is
`converted_traversals_are_depth_independent` carrying **every hand-written
member on both axes in both profiles**, with each remaining derived member
named, measured and dispositioned in the tripwire. The set is now enumerated;
what is left is finite and stated.

### 11.6 Evidence ledger — amendment

| # | claim | provenance |
|---|---|---|
| E19 | `Tree<T>` carries five Θ traversals, none flat | **Measured** — F1, §11.1 |
| E20 | `compare_score_nodes` is Θ(**width**) at 201 B/sibling (debug), 0 at `-O2` | **Measured** — F1; the release 0 is tail-call optimisation |
| E21 | `Env::shift`'s clone contributes 15,636 B/level, = `Par::clone` to 1.5% | **Measured** — F2, differential of two probes |
| E22 | bincode decode is Θ(depth) at 28,362 B/level and **uncapped** | **Measured** — F3; round-tripped at depth 800 where `prost` `Err`s at 34 |
| E23 | `generate_par` never invoked its sub-generators at all | **Read** (`SizeRange::end_incl`) + **measured** (six anti-vacuity guards) |
| E24 | The sorter is not injective | **Measured** — a two-element permutation with equal terms AND equal scores |
| E25 | Stage B: `substitute_no_sort` 195,728 → 0 B/level; the reproducer survives | **Measured** — probe + gate, both profiles |
| E26 | Stage B: deep-env and ground-env binder probes become identical | **Measured** — 48,878 / 33,242 → 0 / 0 |
| E27 | Stage B residual is `Par::clone` at 15,850 B/level | **Measured** — `subst_deep_binding` |
| E28 | Stage C-1: five score-tree traversals become O(1) on both axes | **Measured** — gate, depth 4→4,096 and width 4→65,536 |
| E29 | `normalize`, `SortedParHashSet`, `SortedParMap` are `sort_match` | **Measured** — all three within 0.05% of 78,579 · ⚠ **half-refuted for `normalize`**, see [§12.2](#122--a-corrected-attribution--normalize-was-never-the-sorter-in-release) |
| E30 | `rho-pure-eval::eval_with` has its own Θ(depth) recursion at 21,584 B/level | **Measured** — nested-`ENot` probe; the nested-`EList` probe measures `Par::clone` instead |

---

## 12. Reconciliation — the tree at `b9aaa3d4` (2026-07-27)

> **Why this section exists, and what it is for.** Fourteen commits landed
> between the §11 amendment (`96ca51a0`) and `b9aaa3d4`; **thirteen** of them are
> this campaign's. (The fourteenth, `7dcff96f`, is the `EZipper` cursor fix from
> the EPathMap wire work; it appears here only as the measurement baseline
> `2c32b173` reports against.) None of the thirteen updated this document. The
> consequence was not a stale paragraph but a **displaced
> ledger**: the only artefact that still described the family truthfully was the
> converted list inside `rholang/tests/stack_depth_gate.rs`, which is a
> *mechanism*, not a record — it says which subjects pass, never which were
> considered, why one was dispositioned differently from its neighbour, or which
> earlier conclusion the passing subject overturned. A reader six months from
> now, opening this file at [§7.4](#74-not-landed--and-precisely-why), would have
> read that substitution was unconverted and re-derived a plan for work that was
> already done — and would not have found the one member of the family that can
> abort a node from 577 bytes of source.
>
> This section restores the ledger. Every correction below is sourced to a commit
> or to a measurement taken on `b9aaa3d4`, and where a §1–11 claim is refuted the
> refutation says which claim, by what evidence, and how far the old number was
> from the new one.

**Measurement conditions for every number dated 2026-07-27 in this section.**
`x86_64-unknown-linux-gnu`, `nightly-2026-02-09`, `-C target-cpu=native`;
bisection of the minimum surviving thread `stack_size` to 4,096 B, one fresh
child process per probe point, `ulimit -c 0`; heavy subprocesses under
`systemd-run --user --scope -p MemoryMax=28G`. Two instruments were used and are
named per reading: `scripts/stack_depth_probe.sh` over
`rholang/tests/stack_depth_probe.rs` (least-squares fit
$`S(N) = a + b\,N`$ over four points), and direct bisection of the gate's
own subjects through `GATE_SUBJECT`/`GATE_DEPTH`/`GATE_STACK`
(`rholang/tests/stack_depth_gate.rs`, two points, quantised exactly as
`assert_slope_below` quantises them). Where both were run they are both quoted,
because their disagreement is the resolution of the instrument and not a
property of the subject.

---

### 12.1 What landed after §11 was written

All thirteen, in the order they were made. "Measured effect" is the reading each
commit's own verification produced, re-checked against `b9aaa3d4` where the
subject still exists.

| # | commit | what it changed | measured effect |
|---|---|---|---|
| 1 | `2c32b173` | **Stage C-2.** `ParSortMatcher` and the nine sorter files become an explicit pushdown machine. The `-O0` frame problem is solved by giving **every** `ExprInstance` arm its own `#[inline(never)]` function — 36 `combine_*` and 11 `run_combine` — because hoisting only the three self-contained arms was measured and refuted (130,458 → 127,4xx B/level, a 2.6 % move). | `sort` 78,438 → **0** debug, 6,589 → **0** release; flat 4 → 4,096. `sorted_par_hash_set` 78,543 → 15,850 and `sorted_par_map` 78,583 → 15,914, i.e. onto the derived `<Par as Clone>::clone` floor. `sort_nested_set` 79,053 → 14,336, `sort_nested_map` 82,534 → 17,818. The flat constant also shrank, 188,416 → 73,728 B. |
| 2 | `18419514` | **Uniform anti-vacuity.** Every gate subject proves that its input — and, where it produces a structure, its output — carries the parameter it claims, through one `assert_carries`, with eight **iterative** walkers (`par_depth`, `par_width`, `eset_depth`, `emap_depth`, `binder_depth`, `tree_depth`, `tree_width`, `printed_bracket_depth`) so no checker measures itself. | **No constant moved** with the assertions live, which is the point: the defense became structural without perturbing what it guards. Two checks it replaced had never been able to fail — see [§12.5](#125--the-vacuity-ledger-and-the-two-rules-it-produced). |
| 3 | `b98fa20a` | **Stage E-1.** `SubstituteTrait::substitute` is `substitute_no_sort` then `sort_match`; the un-sorted intermediate was falling out of scope through the *derived* recursive `drop_in_place::<Par>`. It is now handed to `par_children::dismantle`, at every sorted entry point (`substitute_entry!` gained the `Par` slot each node type belongs to). Also adds `bincode_ser`/`bincode_de` as gate subjects. | `substitute` **437 → 0** B/level debug, **140 → 0** release. Both hold a constant minimum stack across 4 → 4,096 (96 KiB debug, 32 KiB release). This is a **Leg-1** fix — it removes the call site, not the derived impl — applied to close a **Leg-2** residual. |
| 4 | `739368a4` | **Stage D prerequisite.** `PrettyPrinter::_build_string_from_message` took `&dyn std::any::Any`; `Any` cannot be made exhaustive, so an unhandled type produced an error string instead of a compile error. Replaced by the closed `PpNode<'a>` alphabet plus an `AsPpNode` conversion that preserves every call shape. | The printer's input alphabet is finite, which is the precondition for it to be a machine at all. `pretty` 41,813 → 41,984 B/level debug (+0.4 %, two bisection buckets). A **live defect** was found and deliberately *preserved*: the `Match` arm passed `Option<Par>`, so every `match` term the printer renders shows `<unprintable: …>` as its target. Correcting it changes block-resident bytes via `ProcessedSystemDeploy::Failed`, so it is pinned byte-for-byte and left for a separately reviewed change. |
| 5 | `d2591fa1` | **Leg-1 at the task-spawn boundary.** `eval_par` gave each detached branch `term.clone()`. `terms` is not used after the map and the only in-closure use read `terms.len()`, so the width is hoisted, `split` is re-typed to take `term_count: usize`, and `terms.into_iter()` **moves**. | Deletes one `<Par as Clone>::clone` — 15,914 B/level debug / 2,867 release, $`D_{\max} \approx 130`$ on a 2 MiB worker — **per parallel branch**. Guarded by an ordered-trace differential over widths 1, 2, 3, 127, 128, 129, 200, 255, 256, 257, 400 against a verbatim copy of the pre-change body, plus a pairwise-distinctness assertion so a degenerate all-equal trace cannot satisfy it. |
| 6 | `6714a128` | **Stage E, width axis.** `FoldMatch::free_check` recursed on the slice tail; it is now a `for` loop. Deliberately a loop and not a driver: it is a fold with early exit, so there is no post-order reassembly for a `Combine` to do. | 483 → **0** B/sibling debug; O(1) at 44 KiB from width 4 to width 65,536. At the old constant, width 65,536 would have needed ~31 MiB. Gated in **debug** as well as release precisely because `-O2` already turned the tail call into a loop, so a release-only gate would have certified the optimiser's discretion rather than the code. |
| 7 | `a3fd6fe4` | **Stage E, depth axis.** `eval_with` was already a loop over `par.exprs`; the recursion lived in `eval_expr_to_par`, which re-entered `eval_with` at **eleven** sites. Now an explicit worklist with an `Extract` continuation that reproduces the operand-checking **interleaving** (a binop checks `p1` before touching `p2`, so a naive post-order would report the wrong operand's error). | `eval_with_nots` 21,584 → **0** debug, 3,359 → **0** release. 20,000 nested negations evaluate on an ordinary test thread; the recursive form would have needed ~412 MiB. |
| 8 | `be6c90f3` | **Step A.** Par-typed cold-store byte goldens, blessed on the untouched **derived** encoder, covering the four wire shapes where a hand-written codec drifts (`BTreeMap` field, all 12 `serialize_as_empty_bytes` sites, both `EPathMap` arms, all three oneofs at high indices). | The pre-change baseline. Blessed *before* the decoder changed, because a golden captured afterwards pins the new behaviour and is evidence of nothing. |
| 9 | `9a5521a2` | **Step B.** `models/src/rust/rholang/par_codec.rs` — an O(1)-native-stack cold-store **decoder**: an obligation stack of bounded opcodes plus per-type value stacks, 47 types on the machine and 16 retained as bounded leaf calls. The encoder is **not** touched, so byte identity holds by construction. | Removes the family's **shallowest** member ($`D_{\max}`$ 73 debug / 161 release on a 2 MiB worker) and the only one whose failure is *permanent and replicated*: `rspace_importer` writes peer bytes to LMDB without deep-decoding them, so a too-deep datum aborted the node on every read-back, on every restart, on every peer. Depth 4,096 decodes on a 256 KiB stack; a truncated depth-4,096 term is *rejected* on a 256 KiB stack, so the error path is proved too. |
| 10 | `2bcfaf87` | **Steps C+D.** The cold-store read path becomes fallible and heap-bounded: 53 bound sites in 8 files move from `for<'a> Deserialize<'a>` to `ColdStoreDecode`, the four `decode_*` return `Result` instead of `.expect(..)`, and `HistoryError::DecodeError` is added as a distinct condition from `ActionError`. The derived twins are retained as `#[cfg(test)]` oracles. | 26,793 record truncations agree with the oracle on the **production** instantiation `RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>`; a depth-4,096 datum reads back through `decode_datums` on a 256 KiB stack where the derived path needed ~110 MiB. |
| 11 | `000b95d7` | The decoder's teardown early-outs on the success path, where every value stack is already empty. | Eighteen `is_empty` reads replace two allocations per cold-store read. The error path is unchanged and still pinned by `machine_rejects_a_truncated_deep_term_without_overflowing`. |
| 12 | `af1a426b` | **Stage F.** `bincode_de` leaves the tripwire for the converted list, and `stack_depth_probe.rs`'s `bincode_de` arm is flipped to the machine with `bincode_de_derived` retained as the control. | Bisected on this very subject immediately before and after: **28,331 → 0** B/level debug and **12,971 → 0** release, flat 4 → 4,096. The before-figures reproduce §11.1 F3's independently measured 28,362 / 12,894 to **0.11 %** and **0.6 %**. |
| — | `b9aaa3d4` | Prose only: the full bound-site enumeration, correcting `2bcfaf87`'s message from 48 sites to 53 (it omitted the five in `serializers.rs` itself). Records that the four remaining sites in `shared/src/rust/store/key_value_typed_store_impl.rs` are untouched because that store's generic bincode path has no instantiation anywhere in the workspace and nothing it holds contains a `Par`. | The correction was recorded in a *new* commit rather than by amending a published one. |

Two things are worth extracting from that table, because neither is visible from
any single row.

**The last two conversions were not on the reduce path at all.**
`rho-pure-eval::eval_with` is reached from `where`-clause guard evaluation and
`bincode` decode is reached from cold-store reads. Both were outside "Leg-2 as
scoped" ([§11.2](#112-the-not-separately-tabulated-list-now-measured), consequence
2). The scope was set by the *reported* symptom; the family was set by the type.
Where those two disagree, the type wins.

**One conversion was completed by a Leg-1 move.** Stage E-1 (`b98fa20a`) closed
`substitute`'s last 437 B/level not by changing a traversal but by deleting a
*call site* of the derived `drop_in_place::<Par>` — exactly the disposition
[§7.2](#72-derived-traversals--leg-1-only-by-construction) assigns to the derived
class. The two legs are not alternatives applied to different members; they
compose on one member.

---

### 12.2 ★ A corrected attribution — `normalize` was never the sorter in release

[§11.2](#112-the-not-separately-tabulated-list-now-measured) attributed
`normalize`'s 78,579 B/level to `sort_match`, on the strength of the debug
figures agreeing to 0.18 % (78,579 against the sorter's 78,438), and predicted
that converting the sorter would fix the normalizer along with
`SortedParHashSet` and `SortedParMap`. The paragraph carried its own escape
clause — *"all three must be re-measured after that conversion"* — and that
re-measurement refutes it for `normalize`.

**Measured on `b9aaa3d4`, after the sorter was converted**
(`scripts/stack_depth_probe.sh`, subject `normalize`, four depths, least-squares
fit):

| profile | depths | minimum surviving stack | fit | §11.2 reading (pre-conversion) | change |
|---|---|---|---|---:|---|
| debug | 10 / 20 / 40 / 80 | 520 / 944 / 1,796 / 3,496 KiB | $`S(N) = 96{,}701 + 43{,}542\,N`$ | 78,579 | **−44.6 %** |
| release | 20 / 40 / 80 / 160 | 160 / 300 / 584 / 1,152 KiB | $`S(N) = 17{,}631 + 7{,}261\,N`$ | 7,247 | **+0.19 %** — i.e. unchanged |

The release figure moved by 14 B/level across a 140-level span. The bisection
resolution over that span is 4,096 / 140 ≈ 29 B/level, so the two readings are
the *same reading*: **converting the sorter did not change `normalize`'s release
constant at all.**

**Why — and the mechanism is already in this document.**
`Compiler::normalize_term` (`rholang/src/rust/interpreter/compiler/compiler.rs`)
is two Θ(depth) traversals in **sequence**, not one nested in the other:

```
   Compiler::source_to_adt_with_normalizer_env
     ├─ RholangParser::parse                    ── source → AST
     ├─ normalize_ann_proc(…)                   ── Θ(SOURCE nesting)   ┐
     │                                                                 │ sequential:
     └─ ParSortMatcher::sort_match(&result.par) ── Θ(TERM nesting)     ┘ the binding
                                                                         cost is the MAX
```

[§5.1](#51-what-the-composite-means-operationally) states the governing rule for
exactly this shape — *"the traversals are sequential, not nested, so the binding
constraint is the maximum, not the sum"* — and the maximum is
**profile-dependent**:

```math
b_{\text{normalize}} \;=\; \max\bigl(b_{\text{normalize\_ann\_proc}},\; b_{\text{sort\_match}}\bigr)
```

| profile | $`b_{\text{normalize\_ann\_proc}}`$ | $`b_{\text{sort\_match}}`$ | max | what §11.2 saw |
|---|---:|---:|---|---|
| debug | 43,542 | 78,438 | **the sorter** | 78,579 — the sorter, to 0.18 % |
| release | 7,261 | 6,589 | **the normalizer** | 7,247 — read as the sorter, but 10.0 % above it |

**So the original claim is not merely imprecise, it is profile-confined.** In
debug the sorter genuinely masked the normalizer's own recursion, and removing
the sorter revealed it (78,579 → 43,542). In release the normalizer's recursion
was **never** masked; the two constants merely sat within 10 % of one another and
the attribution was read off that proximity. A 10 % gap is far outside the
instrument's resolution and should have been treated as a distinct constant.
The general lesson is the one [§4.3](#43-m3--measurement-as-the-discriminator-of-last-resort)
already states and this instance sharpens: **numerical proximity is not
attribution.** Attribution requires either a differential (remove the suspected
contributor and re-measure — which is what settled it here) or a frame walk.

**The frame walk, for completeness.** Under `gdb`, at the SIGSEGV of a debug
`normalize` probe, the per-level frame chain is exactly four frames and repeats
with **zero variance**:

| frame | bytes | function | source |
|------:|------:|----------|--------|
| 0 | **28,464** | `normalize_ann_proc` | `compiler/normalize.rs:397` (the `Proc::Collection` arm) |
| 1 | 5,904 | `normalize_collection::fold_match` | `normalizer/collection_normalize_matcher.rs:44` |
| 2 | 4,640 | `normalize_collection` | `normalizer/collection_normalize_matcher.rs:169` |
| 3 | 4,512 | `normalize_p_collect` | `normalizer/processes/p_collect_normalizer.rs:19` |
| | **43,520** | **one nesting level** | |

Successive `$sp` deltas at frames 7 → 11 → 15 are 0xAA00 = 43,520 B each, and
43,520 against the bisected 43,542 agrees to **0.05 %** — the same
two-instrument corroboration [§2](#2-the-falsification-experiment) established
for `substitute`. `normalize_ann_proc` alone is **65.4 %** of the level, for the
same reason `SubstituteTrait<Expr>::substitute_no_sort` is 87 % of its level
([§6](#6-why-one-level-costs-195-kb--the-attribution)): it is one function
carrying a ~40-arm `match` over the source AST, and at `-O0` `rustc` sizes the
frame for every arm at once.

---

### 12.3 ★★ 577 bytes of source abort a release node, before metering

This section records a finding that neither [§1–10](#1-executive-summary) nor
[§11](#11-leg-2--execution-record-2026-07-2627) contains. It is the most
operationally severe result of the campaign, and it is **open**: the choice
between converting the normalizer and bounding it first is a decision for the
maintainer, and this section deliberately does not recommend one.

**The reproducer.** No guest language, no λ-calculus, no user-defined process,
no `new`, no send — a single nested list literal:

```text
   ┌──── 288 × '[' ────┐   ┌ 1 ┐   ┌──── 288 × ']' ────┐
   [ [ [ [ … [ [ [ [ [       0     ] ] ] ] ] … ] ] ] ]
   └──────────────────────── 577 bytes ────────────────────────┘

   289 bytes would be a depth-144 term and survive.  The whole input fits in a
   single TCP segment.
```

**Measured** (direct bisection of *depth* at a fixed 2 MiB stack — the stack a
tokio worker gets when `RUST_MIN_STACK` is unset — subject `normalize`, one
fresh child process per point):

| profile | max surviving source depth | first aborting depth | source bytes at the abort |
|---|---:|---:|---:|
| release | **287** | 288 | **577** |
| debug | **45** | 46 | 93 |

At depth 288 in release the process prints
`thread 'probe' has overflowed its stack` / `fatal runtime error: stack
overflow, aborting`; at 287 it returns normally. The figures are consistent with
[§12.2](#122--a-corrected-attribution--normalize-was-never-the-sorter-in-release)'s
fits to within the bisection resolution:
$`\lfloor(2{,}097{,}152 - 17{,}631)/7{,}261\rfloor = 286`$ against a
measured 287, and $`\lfloor(2{,}097{,}152 - 96{,}701)/43{,}542\rfloor = 45`$
against a measured 45.

**Why this is categorically worse than every other member of the family.** Four
independent reasons, each verified against the code rather than argued:

1. **It fires before any term exists, therefore before metering.**
   `InterpreterImpl::inj_attempt`
   (`rholang/src/rust/interpreter/interpreter.rs`) runs
   `Compiler::source_to_adt_with_normalizer_env` as its **first** phase
   (`build-normalized-term`). The budget is not established until the *next*
   phase (`set-initial-cost`, `self.c.reset_from_signed_process(&signed_process)`),
   which consumes the parsed term. There is no term to charge for and no budget
   to charge against, so **cost accounting cannot bound this input** — not
   because the charge is too small, but because the charge does not exist yet.
   Every other member of the family runs after a term has been built and priced.

   ```text
      InterpreterImpl::inj_attempt — the three phases, in order

      ┌ build-normalized-term ─────────────────────────────────────────────┐
      │  Compiler::source_to_adt_with_normalizer_env(term, normalizer_env) │
      │      parse  ──▶  normalize_ann_proc  ──▶  sort_match               │
      │                        ▲                                           │
      │                        └── ✖ SIGSEGV here, at source depth 288     │
      │                            (release, 2 MiB worker)                 │
      └────────────────────────────────────────────────────────────────────┘
                                    │  no budget exists yet
                                    ▼
      ┌ set-initial-cost ──────────────────────────────────────────────────┐
      │  SignedProcess::metered(parsed, …)                                 │
      │  self.c.reset_from_signed_process(&signed_process)  ◀── METERING   │
      │                                                        BEGINS HERE │
      └────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
      ┌ reduce-term ───────────────────────────────────────────────────────┐
      │  every OTHER member of the family lives in here, priced            │
      └────────────────────────────────────────────────────────────────────┘
   ```

2. **The failure is not an `Err`, so the existing error handling is inert.** The
   call site is

   ```rust
   let result = match Compiler::source_to_adt_with_normalizer_env(term, normalizer_env) {
       Ok(p) => { /* … */ Ok(p) }
       Err(e) => Err(self.handle_error(InterpreterError::ParserError(e.to_string()))),
   };
   ```

   A stack overflow is a `SIGSEGV` on the guard page, handled by Rust's runtime,
   which prints and calls `abort()`. It is not unwindable and it is not an
   `Err`, so the `Err` arm — which exists precisely to turn a bad deploy into a
   failed deploy — **never runs**. The node process terminates.

3. **Validators normalize deploys they receive.** `ReplayRuntimeOps::run_user_deploy`
   (`casper/src/rust/rholang/replay_runtime.rs`) calls
   `runtime_ops.evaluate(&processed_deploy.deploy)`, which reaches
   `inj_attempt` and therefore the normalizer, on deploy source that arrived
   from the network. Producing the input requires no privilege and no stake.

4. **The overflow is in the normalizer, not the parser — confirmed, not
   assumed.** The `gdb` backtrace at the fault
   ([§12.2](#122--a-corrected-attribution--normalize-was-never-the-sorter-in-release))
   is a clean repetition of
   `normalize_ann_proc → normalize_p_collect → normalize_collection → fold_match
   → normalize_ann_proc`, with frame 0 inside `models::rust::utils::new_gint_expr`
   at the leaf. The parser has already returned.

**Why it is not a small conversion.** The normalizer is not a single recursive
function with an incidental helper chain. Deriving the recursion by the same
method [§4.2](#42-m2--hand-written-traversals-over-approximate-then-verify) uses
— build the call graph over `rholang/src/rust/interpreter/compiler/`, restricted
to production code (bodies inside a `#[cfg(test)]` item excluded by brace
tracking), then take the strongly connected component containing
`normalize_ann_proc` (Tarjan) — gives:

```text
SCC(normalize_ann_proc) = 26 functions
  canon_quote · normalize_ann_proc · normalize_collection · normalize_name
  normalize_p_bundle · normalize_p_collect · normalize_p_conjunction
  normalize_p_contr · normalize_p_disjunction · normalize_p_eval
  normalize_p_if · normalize_p_input · normalize_p_let · normalize_p_match
  normalize_p_matches · normalize_p_method · normalize_p_negation
  normalize_p_new · normalize_p_par · normalize_p_send · normalize_p_send_sync
  recognize_signed_join · recognize_signed_term · recognize_token_stack
  signature_to_ir · signature_to_native_sig

76 production call sites INSIDE that SCC, across 22 files
  23  compiler/normalize.rs                       (17 dispatch arms + 3 self + 3 recognize)
   7  normalizer/processes/p_input_normalizer.rs
   5  normalizer/cost_accounting/recognize.rs
   5  normalizer/cost_accounting/sig.rs
   4  each of p_if / p_let / p_match
   3  each of collection_normalize_matcher.rs / p_contr
   2  each of p_conjunction / p_disjunction / p_matches / p_method / p_send
   1  each of name_normalize_matcher · p_bundle · p_collect · p_eval
        · p_negation · p_new · p_par · p_send_sync
```

and — this is what distinguishes it from every traversal already converted — the
state is **threaded**, not merely passed down:

| carrier | how it flows | why a naive worklist breaks it |
|---|---|---|
| `ProcVisitInputs.free_map` | **left-to-right across siblings**: `binary_exp` evaluates the left operand, then hands `left_result.free_map` to the right (`normalize.rs`, `binary_exp`) | children are not independent; a driver that visits them in any other order assigns different free-variable levels |
| `ProcVisitInputs.bound_map_chain` | **scoped at binders**: cloned and pushed on entry to a binding form, popped on exit | the driver must reproduce push/pop as an explicit discipline, not as stack unwinding |
| `ProcVisitInputs.par` | **accumulates**: each arm returns `prepend_expr(input_par, expr, depth)` | the result is built on the way *down* as well as up, so a post-order `Combine` alone cannot reassemble it |

That is the same category of obligation
[§11.5](#115-what-remains-and-the-revised-endpoint) records for the printer
("its state mutates and is never restored, so a driver must reproduce the
*interleaving*, not merely the post-order"), and the same one `a3fd6fe4`
discharged for the binop operand order in `eval_with`. It is tractable — three
such conversions have now landed — but it is not a mechanical rewrite, and
`normalize.rs` is additionally the file that decides free-variable numbering,
which is consensus-observable.

**What is open.** Two dispositions are available and they are not equivalent:

* **Convert it** — `normalize_ann_proc` and its 26-function SCC become an
  explicit machine, under the proof standard of
  [§8](#8--the-proof-standard) (a retained recursive oracle twin, a differential
  over a constructed corpus that reaches every `Proc` arm, and — because free
  levels are consensus-visible — equality of the *normalized term bytes*, not
  merely of acceptance).
* **Bound it first** — a depth limit on the source AST, checked before
  normalization, converting the abort into an `Err` that the existing
  `ParserError` arm already handles. This is cheap and it is a **protocol-visible
  ceiling**, in the same class as the `prost` `RECURSION_LIMIT` this document
  already records at [§7.3](#73-the-prost-decode-ceiling-is-a-constraint-on-the-fix-not-a-defect-to-fix)
  and subject to the same standing decision ("no *new* protocol-level nesting
  cap") that section names.

**This is recorded as open, awaiting a maintainer decision, and this document
does not choose between them.** What it does record is that the two are not
mutually exclusive and that the measurement above is what any choice must be
made against.

> ⚠ **SUPERSEDED (2026-07-27) — the decision is made, and it is CONVERT.**
> `normalize_ann_proc`'s 26-function SCC is an explicit pushdown machine at
> commit `07853de0`, and `normalize` / `normalize_wide` are gate subjects in
> `converted_traversals_are_depth_independent` at `88ef41cd`. Measured
> 43,542 → **0** B/level debug and 7,261 → **0** release; the 577-byte
> reproducer, and a 200 kB one, normalize on a 2 MiB worker stack in both
> profiles. **"Bound it first" was rejected on the merits**, and the reason is
> the profile split this very section measures: release aborted at 288 and debug
> at 46, so no single constant is neither inert in release nor newly restrictive
> in debug. Full execution record, including the two derived residuals the
> conversion exposed: [§13](#13-stage-g--the-normalizer-execution-record-2026-07-27).

---

### 12.4 ★ What the enumeration method can and cannot see

[§4](#4-enumerating-the-traversals--the-method) is the section this document
puts most weight on, because the USER rejected a staged ladder precisely on the
grounds that staging implies an incomplete enumeration. It has now been tested
by everything that followed, and it deserves a consolidated verdict rather than
four scattered footnotes.

**The method held where it claimed to hold.** The over-approximating static
search of [§4.2](#42-m2--hand-written-traversals-over-approximate-then-verify)
**did** find `rho-pure-eval::eval_with`, `FoldMatch::free_check` and
`normalize_ann_proc` — all three are named in
[§5](#5-measured-constants-per-traversal-and-per-profile)'s "not separately
tabulated" paragraph. Nothing in the hand-written set has since been discovered
that the search missed. The completeness claim survives.

**Two structural blind spots, both in M1 rather than M2.**
[§4.1](#41-m1--derived-and-generated-traversals-closed-by-inspection) closes the
derived set by Tarjan over `models/src/main/protobuf/RhoTypes.proto`. That
procedure can only see recursion expressed **as proto message references, in
this repository**. Two members are neither:

| missed member | what it is | why the proto SCC cannot see it | found by |
|---|---|---|---|
| `Tree<T>` (`models/src/rust/rholang/sorter/score_tree.rs`) | a recursive **Rust** type built beside the term, carrying five Θ traversals (`compare_score`, the sibling walk, `Clone`, `PartialEq`, `Drop`) | it is not a proto message; it has no entry in `RhoTypes.proto` at all | [§11.1](#f1--the-score-tree-is-a-fifth-traversal-family-premise-upheld) F1, a designed falsification experiment |
| `rhocalc_ast::lower_proc` (`rholang-runtime/src/rhocalc_ast.rs`, the **`mettail-rust`** repository) | a recursive lowering `&Proc → Result<Par, RhocalcAstLowerError>` that both descends a foreign AST and builds a `Par`; recurses directly (`lower_proc(&desugared, env)`) and through `lower_proc_alternatives` | it is in a **different repository**, over a **different** source type, and produces `Par` as an output rather than consuming it as a field | reading, while reconciling this audit |

The generalizable statement, which is worth more than either instance:

> **A type-directed enumeration finds exactly the traversals whose recursion is
> expressed in the schema it reads.** Recursion introduced by a *host-language*
> type over the same data (`Tree<T>`), by a *different* schema that produces the
> data (`lower_proc`), or by an *axis the schema has no notion of* (sibling
> count) is invisible to it — not overlooked, but outside its domain of
> discourse. Every such member found so far was found by **measurement or by
> reading**, never by the schema walk. The schema walk is therefore a lower
> bound on the family and must be documented as one.

**Three dispositions that measurement corrected.** Distinct from the above:
these members *were* enumerated, and what was wrong was the judgement attached
to them.

| member | disposition in §5 | what measurement found | where |
|---|---|---|---|
| `rho-pure-eval::eval_with` | "same class, lower priority… not on the reduce path measured here" | its **own** Θ(depth) SCC at 21,584 B/level debug / 3,359 release, reachable from `where`-clause guard evaluation; converted at `a3fd6fe4` | [§11.2](#112-the-not-separately-tabulated-list-now-measured) |
| `FoldMatch::free_check` | listed among the depth-class members | a Θ(**width**) member at 483 B/sibling debug — an axis with no column in [§5](#5-measured-constants-per-traversal-and-per-profile)'s table, and 0 in release only because `-O2` chose to make it so; converted at `6714a128` | §11.2, `6714a128` |
| `normalize` | "exactly `sort_match`'s constant" | its own recursion, binding in release and merely masked in debug | [§12.2](#122--a-corrected-attribution--normalize-was-never-the-sorter-in-release) |

So: **one blind spot in the method (two instances), and three dispositions
corrected by the instrument.** The distinction matters, because the two have
different remedies. The blind spot is fixed by widening the enumeration — walk
host-language recursive types and producer schemas, not only the consumed one.
The dispositions are fixed by the discipline
[§4.3](#43-m3--measurement-as-the-discriminator-of-last-resort) already states
and which this campaign has now confirmed three more times: **static analysis
proposes, measurement disposes** — and a member named without a measured
constant has not, in fact, been dispositioned.

---

### 12.5 ★ The vacuity ledger, and the two rules it produced

[§11.4](#114-the-gate-and-one-thing-it-found-in-itself) closes with *"that is
now three occurrences in this work"* of a harness reporting a comfortable number
for the wrong reason. The count is out of date and, more importantly, the
**shape** changed: the later instances are not wrong *slopes* but wrong
*passes*, which no ceiling can catch.

Every instance in the tree, with its mechanism:

| # | instance | what it reported | why | source |
|---|---|---|---|---|
| 1 | `prost` decode probe used field 3 for `Par.exprs` (it is 5) and a one-byte tag for `Expr.e_list_body` (it is 20) | **0.00 KiB/level** | `prost` skipped the payload as an unknown field and decoded a shallow term | [§4.3](#43-m3--measurement-as-the-discriminator-of-last-resort) |
| 2 | `generate_par` sized every collection with an exclusive `0..1`; `SizeRange::end_incl()` is `end - 1` | a **PASS** over two values, with `generate_send`/`_receive`/`_new`/`_match`/`_bundle`/`_connective` never invoked | the property was asserted over an effectively empty set | [§8.4](#84--the-limits--what-this-standard-does-not-establish) #3, [§11.3](#stage-a--two-harness-defects-that-would-have-voided-a-green-result) |
| 3 | score-tree subjects built their inputs **on the gated thread** | **78,573 B/level** — the sorter's constant, not the comparator's 1,329 | the setup traversal was inside the measurement | [§11.4](#114-the-gate-and-one-thing-it-found-in-itself) |
| 4 | `substitute_binders` dropped a deep **environment** after the traversal finished | **443 B/step** | derived `drop_in_place::<Par>` ran inside the measurement window | [§11.4](#114-the-gate-and-one-thing-it-found-in-itself) |
| 5 | `assert_slope_below("substitute", …, 16, 64)` while `substitute` still had a 437 B/level slope | **0 B/level** | both probe points sat inside the subject's ~136 KiB **intercept**, where 4 KiB bisection cannot resolve 48 × 437 B | `b98fa20a` |
| 6 | `pretty` asserted only `!s.is_empty()` | a **PASS** for a printer that traverses nothing | `"Nil"` satisfies it — and so does the `<unprintable: …>` fallback, which `739368a4` proved was a **live** output of this very printer for every `match` target | `18419514` |
| 7 | `encode` / `bincode_ser` asserted only `!bytes.is_empty()` | a **PASS** for any non-empty encoding | a collapsed fixture encodes to a few bytes and still passes | `18419514` |
| 8 | the pretty printer's drive unwinds to `catches.last_mut()` — the **innermost** open catching scope; mutating it to `catches.first_mut()` passed the whole suite | a **PASS** for an unwind that returns to the wrong frame | `push_catch` emits `EndCatch`, the guarded item and `BeginCatch` together, so each frame is closed before the next opens and `catches` never held more than **one** element. `first` and `last` of a one-element list are the same element: the assertion was quantified over a set on which the two spellings are equal | `bd7cb45f` |

Eight instances, three mechanisms:

```text
  ┌─ M-a: the fixture does not carry the parameter ───────────────────┐
  │  #1 wrong field number · #2 vacuous generator                     │
  │  #6 empty-string check  · #7 empty-bytes check                    │
  │  #8 catch stack never reached depth 2                             │
  │  ⇒ the subject is exercised at effective parameter 0              │
  └───────────────────────────────────────────────────────────────────┘
  ┌─ M-b: the window contains a traversal that is not the subject ────┐
  │  #3 setup on the gated thread · #4 teardown after the traversal   │
  │  ⇒ the reading is max(subject, contaminant), attributed to the    │
  │    subject                                                         │
  └───────────────────────────────────────────────────────────────────┘
  ┌─ M-c: the ladder cannot resolve the slope ────────────────────────┐
  │  #5 both probe points inside the intercept                        │
  │  ⇒ a large intercept reads as a zero slope on a short ladder      │
  └───────────────────────────────────────────────────────────────────┘
```

Instance 6 is the sharpest of the eight and deserves the extra sentence, because
it is the only one where the vacuous branch was **demonstrably reachable in
production**: `739368a4` established that `_build_string_from_message`'s `Match`
arm passed an `Option<Par>`, which matched no `downcast_ref` arm, so *every*
`match` term this printer rendered emitted
`<unprintable: Bug found: Attempt to print unknown prost::Message type: Any { .. }>`.
A `!s.is_empty()` check does not merely *permit* a printer that traverses
nothing; it was, for that arm, actively certifying one.

That defect is **fixed** as of `bd7cb45f`: the arm now renders `m.target`
through the entry point the original line named, with the `Option` projected by
the same `.expect` discipline the printer's six other required `Option<Par>`
fields already used. The fix is recorded here rather than only in the commit
because it produced a **second-order** instance of this very failure mode, and
that generalization is the reusable part:

> **Deleting a defect can vacate the checks that stood on it.**
> `PpNode::Unprintable` was the drive's only `Err` source and the `Match` arm
> was its only production construction site, so after the fix **no `Par` can
> make the printer return `Err`**. Two live properties — "a fallback is spliced
> mid-render rather than propagated" and "`EndCatch` caps a successful
> sub-render but not a fallback" — were witnessed by an ordinary `Send`
> containing a `match`. Both would have kept running, kept passing, and stopped
> having a subject. They are rebuilt on `cfg(test)` drive seeds
> (`drive_splice_probe`, `drive_nested_catch_probe`) in the same change.

Instance #8 is what the second of those rebuilds found: with the frames
reconstructed *deliberately*, the nesting that no in-sequence probe had ever
produced became expressible, and the `last_mut()` / `first_mut()` claim became
falsifiable for the first time. Extending mechanism **M-a** past measurement
parameters to *state* parameters is the generalization: a check that
discriminates on the shape of a runtime data structure is vacuous unless some
fixture drives that structure into the shape where the discrimination is
observable.

**The two rules, stated once, in the method rather than in eight footnotes.**
Both are now enforced in `rholang/tests/stack_depth_gate.rs`; both are stated
here because they generalize past this gate to any parameterised measurement.

> **Rule V (validity).** Every subject must assert that its **input** carries the
> parameter it claims and — where it produces a structure — that its **output**
> does too, through a checker that is itself iterative so it cannot measure
> itself. Setup and teardown must run off the measured thread. `18419514`
> applies this uniformly through one `assert_carries` and eight iterative
> walkers; the strongest case is `substitute_deep_binding`, whose output must
> contain the *spliced* bound value at full depth, so a substitution that did not
> substitute would read 0 rather than pass.
>
> *Extension, from instance #8 (`bd7cb45f`).* The same obligation applies to a
> subject's **internal state**, not only to its input and output. Where an
> assertion discriminates on the shape of a runtime data structure — the depth of
> a stack, the arity of a frame, the presence of a second element — some fixture
> must drive that structure into the shape where the discrimination is
> observable, or the assertion is quantified over a set on which the alternatives
> coincide and it cannot fail. The check is mechanical: mutate the discriminating
> expression to its opposite and confirm the suite goes red.
>
> **Rule L (ladder).** Both probe points must clear the subject's **own
> intercept**, and the primary assertion must be a **zero-slope** claim over a
> range wide enough that no intercept can absorb it (4 → 4,096 on the depth axis,
> 4 → 65,536 on the width axis) — not a two-point slope against a ceiling. A
> two-point slope is a tripwire: it can only ever certify that a traversal did
> not get *worse*.

A corollary this campaign paid for eight times: **a green number is evidence only
in proportion to the harness's demonstrated ability to go red.** The routine
that makes it evidence is to inject the defect deliberately and confirm the
harness fails — which is what
[§12.8](#128-the-codec-falsification-experiment--why-the-corpus-is-constructed-and-not-random)
did for the codec differential, `2c32b173` did for the sorter golden (a
deliberate score-chain transposition, caught on 2 of 114 entries), `d2591fa1`
did for the split trace (a pairwise-distinctness assertion), and `a3fd6fe4` did
for operand order (`swapping_operands_actually_changes_the_answer`).

---

### 12.6 The family at `b9aaa3d4` — converted, tripwired, and open

> ⚠ **ANCHORED TO `b9aaa3d4` — a snapshot, not a status.** The counts and tables
> in this section describe the tree at that commit (13 converted, 9 tripwired,
> `pretty` still awaiting conversion). They are evidence and are deliberately not
> updated. The live sets are in
> [§0](#0-current-subject-sets--derived-from-the-gate-not-transcribed).
>
> Changes since `b9aaa3d4`: `pretty` / `pretty_wide` (Stage D) and `normalize` /
> `normalize_wide` (Stage G) moved into the converted set, and the tripwire's
> `drop` was **renamed `par_drop`** — the same subject, under the name
> mettail-rust's twin gate already used for it — while `normalize_drop` was added
> alongside it ([§14](#14-the-deploy-path-teardown--par-as-drop-2026-07-27)).

Three dispositions, and every member of the family is in exactly one of them.

```
                       THE Θ(depth) FAMILY over `Par`, at b9aaa3d4
   ═══════════════════════════════════════════════════════════════════════════

   (A) CONVERTED — explicit machine, O(1) native stack           13 subjects
   ┌───────────────────────────────────────────────────────────────────────┐
   │ depth  substitute · substitute_no_sort · substitute_binders           │
   │        sort · score_cmp · tree_drop · tree_clone                      │
   │        eval_with_nots · bincode_de                                    │
   │ width  substitute_wide · sort_wide · score_cmp_wide · free_check      │
   │                                                                       │
   │ assertion  a CONSTANT minimum stack over 4 → 4,096 (depth) and        │
   │            4 → 65,536 (width), in BOTH profiles                       │
   └───────────────────────────────────────────────────────────────────────┘
                                    ▲
              a traversal enters ONLY by being converted ─┘
              and LEAVES the tripwire only this way, never
              by having its ceiling raised
                                    │
   (B) STILL Θ(depth) — TRIPWIRED, "not worse" only            9 subjects
   ┌───────────────────────────────────────────────────────────────────────┐
   │ HAND-WRITTEN, convertible                                             │
   │   pretty ······················ the NEXT conversion (prereq landed)   │
   │ DERIVED / GENERATED — Leg-1 disposition: delete call sites, not impls │
   │   clone · drop · encode · clone_nested_set                            │
   │ NAMED RESIDUALS — bounded by something other than the term's depth    │
   │   substitute_deep_binding (the BOUND value)                           │
   │   sort_nested_set · sort_nested_map (3ⁿ sorts bound them in TIME)     │
   └───────────────────────────────────────────────────────────────────────┘

   ⚠ `bincode_ser` LEFT box (B) in Stage H. See §12.9.

   (C) OPEN — enumerated, measured, no disposition yet           1 member
   ┌───────────────────────────────────────────────────────────────────────┐
   │ normalize_ann_proc ····· Θ(SOURCE nesting), on the DEPLOY path,       │
   │   43,542 B/level debug · 7,261 release                                │
   │   ⚠ aborts a release node at source depth 288 — BEFORE metering       │
   │     exists, so cost accounting cannot bound it.  See §12.3.           │
   └───────────────────────────────────────────────────────────────────────┘
```

**(A) Converted — $`O(1)`$ native stack, asserted in both profiles.**
`converted_traversals_are_depth_independent`, run on `b9aaa3d4`, `--test-threads 1`.
Each entry is the *constant* minimum surviving stack, identical at both ends of
the ladder; a Θ(depth) member cannot produce two equal readings 1,024 parameter
steps apart.

| subject | axis | debug (4 → 4,096) | release (4 → 4,096) | landed |
|---|---|---|---|---|
| `substitute_no_sort` | depth | 96 KiB | 28 KiB | `f11ffb54` |
| `substitute_binders` | depth | 96 KiB | 28 KiB | `f11ffb54` |
| `substitute` | depth | 96 KiB | 32 KiB | `2c32b173` + `b98fa20a` |
| `sort` | depth | 44 KiB | 12 KiB | `2c32b173` |
| `score_cmp` | depth | 12 KiB | 12 KiB | `6ce7c5b9` |
| `tree_drop` | depth | 12 KiB | 12 KiB | `6ce7c5b9` |
| `tree_clone` | depth | 12 KiB | 12 KiB | `6ce7c5b9` |
| `eval_with_nots` | depth | 60 KiB | 12 KiB | `a3fd6fe4` |
| `bincode_de` | depth | 108 KiB | 12 KiB | `9a5521a2` + `af1a426b` |
| `substitute_wide` | width (4 → 65,536) | 96 KiB | 32 KiB | `f11ffb54` |
| `sort_wide` | width (4 → 65,536) | 48 KiB | 12 KiB | `2c32b173` |
| `score_cmp_wide` | width (4 → 65,536) | 12 KiB | 12 KiB | `6ce7c5b9` |
| `free_check` | width (4 → 65,536) | 44 KiB | 12 KiB | `6714a128` |

**(B) Still Θ(depth), tripwired.** `theta_depth_tripwire`, same run. `B/level`
is the tripwire's own two-point figure,
$`(S_{hi} - S_{lo}) / (\text{hi} - \text{lo})`$, integer-divided exactly as
`assert_slope_below` computes it.

| subject | ladder | debug B/level | ceiling | release B/level | ceiling | disposition |
|---|---|---:|---:|---:|---:|---|
| `pretty` | 16 → 64 | **41,984** | 65,000 | **4,266** | 7,000 | **the next conversion.** Hand-written; the one remaining depth-axis member of that kind. Prerequisite landed (`739368a4`). |
| `substitute_deep_binding` | 16 → 128 | 15,872 | 25,000 | 7,241 | 12,000 | named residual — `Env::get` returns its value cloned, because a binding may be spliced many times. Bounded by the depth of the **bound value**, never of the term traversed. |
| `clone` | 16 → 128 | 15,872 | 25,000 | 2,852 | 5,000 | derived — [§7.2](#72-derived-traversals--leg-1-only-by-construction) |
| `drop` | 256 → 4,096 | 464 | 1,500 | 144 | 800 | synthesised — the irreducible member |
| `encode` | 64 → 1,024 | 1,932 | 4,000 | 302 | 1,500 | generated; and [§8.2](#82-why-each-conversion-is-neutral-by-construction--per-traversal) forbids converting it on the general argument, because its **return value is the charge** |
| `sort_nested_set` | 2 → 8 | 14,336 | 79,053 | **7,509** | **7,680** | Stage C-2 residual — see [§12.7](#127--sort_nested_set-sits-22--under-its-own-ceiling) |
| `sort_nested_map` | 2 → 8 | 17,749 | 82,534 | 7,509 | 10,394 | " |
| `clone_nested_set` | 2 → 8 | 16,384 | 25,000 | 3,413 | 9,000 | the derived control the two above are measured against |

`pretty`'s constants are corroborated by three independent readings within
0.4 %: 41,840 / 4,242 ([§5](#5-measured-constants-per-traversal-and-per-profile),
2026-07-26, four-point probe fit), 41,813 debug (`18419514`'s own gate run,
before the `PpNode` refactor), and 41,984 / 4,266 (this reconciliation, gate
ladder; the four-point fits over 16/32/64/128 are 42,022 and 4,252). The
disposition is unchanged from [§11.5](#115-what-remains-and-the-revised-endpoint):
its state mutates and is never restored, so a driver must reproduce the
*interleaving* and not merely the post-order, and `739368a4` documents the
asymmetry a driver must preserve — `build_string_from_message` resets `indent`
to 0 **and** caps its result, its `_`-prefixed twin does neither, and the error
fallback is not capped.

The derived class as a whole — `<Par as Clone>::clone`, `drop_in_place::<Par>`,
`<Par as PartialEq>::eq`, `<ExprInstance as Debug>::fmt` — keeps the
[§7.2](#72-derived-traversals--leg-1-only-by-construction) disposition
**unchanged**: Leg-1 only, remove the call sites and not the impls. `eq` and
`Debug` were not re-measured in this reconciliation because nothing that landed
touches them; their figures remain [§5](#5-measured-constants-per-traversal-and-per-profile)'s
1,353 / 310 and 3,626 / 1,244, dated 2026-07-26. The two Leg-1 call-site
deletions that landed since — `d2591fa1` at the task-spawn boundary and
`b98fa20a` at the sorted-substitution boundary — are the disposition being
executed, not revised.

**(C) Open.** `normalize_ann_proc` — 43,542 B/level debug, 7,261 release, and a
2 MiB worker aborts at source depth 288 in release. Neither converted nor
tripwired: it is not a gate subject at all, only a `stack_depth_probe.rs`
subject. Awaiting the decision in
[§12.3](#123--577-bytes-of-source-abort-a-release-node-before-metering).

> ⚠ **SUPERSEDED (2026-07-27).** Group **(C) is now EMPTY**: `normalize` and
> `normalize_wide` are in group (A) at `88ef41cd`, both at 0 B/level in both
> profiles. `normalize` is the only member ever to enter the converted list
> *from* group (C) rather than from the tripwire — every other conversion was
> promoted from a measured ceiling, this one from a measured constant with no
> disposition at all. See [§13](#13-stage-g--the-normalizer-execution-record-2026-07-27).

**What "done for the family" now means.** [§11.5](#115-what-remains-and-the-revised-endpoint)'s
definition stands and is now within reach of being stated concretely: the
converted list carries **every hand-written member on both axes in both
profiles**. At `b9aaa3d4` that leaves exactly two hand-written members outside
it — `pretty` (a known conversion with its prerequisite landed) and
`normalize_ann_proc` (open) — with every remaining member derived, named,
measured and tripwired.

---

### 12.7 ★ `sort_nested_set` sits 2.2 % under its own ceiling

```text
   HEADROOM UNDER THE TRIPWIRE CEILING, release profile, ladder 2 → 8
   (bar length ∝ measured / ceiling;  ceiling = the MEASURED pre-conversion baseline)

                       0%                                        100%  ceiling
                       ├────────────────────────────────────────────┤
   sort_nested_set     ████████████████████████████████████████████╎▏   7,680
                        7,509 B/level ─────────────────────── 97.8% ┘   ← 2.2% left
   sort_nested_map     ████████████████████████████▏                   10,394
                        7,509 B/level ─── 72.2%
   clone_nested_set    ███████████████▏                                 9,000
                        3,413 B/level ─ 37.9%   (the DERIVED control)
```

Measured on `b9aaa3d4`: minimum stack 12,288 B at parameter 2 and 57,344 B at
parameter 8, so $`(57{,}344 - 12{,}288)/6 = 7{,}509`$ B/level against a
ceiling of 7,680. `sort_nested_map` reads the same 7,509 against a roomier
10,394. In debug the same subjects sit 5.5× and 4.6× under their ceilings.

**This is not a regression, and the tension is deliberate.** The ceiling is not a
tolerance chosen for comfort — it is the **measured pre-conversion baseline**,
pinned by `2c32b173` so that the tripwire can only ever certify that the
self-contained `ESetBody`/`EMapBody`/`EPathmapBody` arms did not get *worse*
than the recursive sorter they replaced. Setting it there is what makes the
assertion meaningful; it is also what leaves 2.2 % of headroom in release.

**The consequence, stated rather than resolved.** 2.2 % is inside the range that
ordinary code motion moves a `-O2` frame. This tripwire will eventually go red
for a reason that is not a regression in the traversal's *class*. When it does,
the standing rule from `b98fa20a` applies and has been held through every stage
of this campaign:

> **A traversal leaves the tripwire only by being converted, never by having its
> ceiling raised.**

So the correct response to that failure is one of: convert the three
self-contained arms (which, note, is bounded in value — those arms sort each
element three times, so a chain of $`n`$ nested sets costs $`3^n`$
sorts and depth 20 was directly observed *not to terminate* in either profile,
making deep set nesting infeasible in **time** long before the stack residual
bites); or re-baseline the ceiling **against a freshly measured pre-conversion
control on the new toolchain**, which is a measurement and not an adjustment;
or accept the red and record why. What is *not* available is nudging 7,680
upward because 7,509 drifted. Recording the tension now means the next person
meets a documented decision instead of an inconvenient assertion.

---

### 12.8 The codec falsification experiment — why the corpus is constructed and not random

This is the strongest methodological result of the campaign, and it generalizes
well past the codec it was performed on.

**The experiment.** After the cold-store decoder (`9a5521a2`) was differential-
tested green against the retained derived oracle, a **single-field drift** was
injected into one arm: the `ETuple` decoder was made to read a `remainder` field
that `ETuple` does not have. This is the smallest realistic hand-written-codec
defect — an arm whose field list has drifted by one from the type it decodes.

**The result.** `models/tests/par_codec_differential.rs` carries 11 tests. Six
went red; five stayed green; **all three `generate_par` proptests were among the
green**.

The split is not luck, and it is re-derivable from the corpus without re-running
the experiment. `corpus::all_par_fields()` builds its `exprs` field from
`corpus::every_expr_instance()`, which carries an `ETupleBody` representative;
`corpus::par_corpus()` additionally emits one `Par` per `ExprInstance` arm,
including `expr::ETupleBody`. Every test that reaches either of those reaches an
`ETuple`:

| test | reaches an `ETuple` via | outcome |
|---|---|---|
| `par_corpus_agrees_with_the_derived_oracle` | `par_corpus()` — both `all_par_fields()` and `expr::ETupleBody` | **RED** |
| `par_corpus_accepts_trailing_bytes_exactly_as_the_oracle_does` | the same corpus, with trailing bytes | **RED** |
| `list_par_with_random_corpus_agrees` | `loaded.pars[0] = all_par_fields()` | **RED** |
| `bind_pattern_corpus_agrees` | `patterns[0] = all_par_fields()` | **RED** |
| `tagged_continuation_corpus_agrees` | `par_body.guard = all_par_fields()` | **RED** |
| `non_root_machine_types_agree` | `par_with_random_corpus().loaded.body = all_par_fields()`; `list_bind_patterns_corpus().loaded` is the bind-pattern corpus | **RED** |
| `epathmap_shapes_agree` | — `EPathMap` shapes carry only `gint` payloads | green |
| `decoded_epathmaps_carry_no_intern_handle` | — `nonground_pathmap()` is two `gint`s | green |
| `generated_pars_agree_with_the_derived_oracle` | — see below | green |
| `round_trip_over_generate_par` | — | green |
| `round_trip_through_every_root` | — | green |

**Why the random corpus could not see it — verified by reading, not inferred.**
`models/src/rust/test_utils/test_utils.rs`'s `generate_expr` offers exactly four
alternatives:

```rust
// generate_expr(depth) — the complete alternative set
ExprInstance::GBool(_) | ExprInstance::GInt(_) | ExprInstance::GString(_)
                       | ExprInstance::ENotBody(ENot { p: Some(p) })
```

`ETupleBody` is not among them. **No draw of `generate_par`, at any depth, with
any seed, at any case count, can produce an `ETuple`.** The three proptests were
not unlucky; they were structurally incapable of reaching the defect. Raising the
case count from 256 to 256,000 would not have changed the outcome by one test.

**What this justifies, and how far.** For a traversal whose specification is a
**closed schema** — a fixed set of variants, each with a fixed field list — a
constructed corpus with one representative per variant provides coverage that no
random generator provides *unless that generator is itself derived from the
schema and proven exhaustive*. The property being asserted is per-arm agreement;
the risk is a missing or drifted arm; a generator that omits an arm cannot
falsify a claim about it. This is precisely
[§8.4](#84--the-limits--what-this-standard-does-not-establish) limit #3 —
*"a harness that runs green on a corpus that cannot express the risky shape is
worse than no harness, because it licenses confidence"* — with the abstract
warning replaced by a measured instance and a mechanism.

The corpus is **not** a replacement for the random one, and `9a5521a2` keeps
both: `generate_par` (non-vacuous since Stage A's `0..=2` fix) explores
*combinations* the constructed corpus does not, while the constructed corpus
guarantees the *alphabet*. Where they were made to disagree, only the second
found the defect. The complementary defenses that do not depend on a corpus at
all are the ones that carry the general claim: 1.88 M malformed inputs
(117,601 truncations at every byte offset, 472,568 byte substitutions, 586,455
out-of-range variant indices, 701,898 hostile `u64` lengths), complete struct
literals in every `*Build` so a new field is a compile error, and the oneof
numbering living in exactly one place (`par_children`) with exhaustive matches
and no `_` arm.

---

### 12.9 Gate composition and the workspace bar

> ⚠ **ANCHORED TO `b9aaa3d4`.** The composition below is that commit's — four
> tests, 13 converted subjects, `pretty` "present but commented out". None of
> those three facts holds at HEAD. Retained as evidence; the live sets are in
> [§0](#0-current-subject-sets--derived-from-the-gate-not-transcribed) and the
> gate now carries seven tests.

**`rholang/tests/stack_depth_gate.rs` at `b9aaa3d4`** — four tests, and what each
one is for:

```text
converted_traversals_are_depth_independent   THE BAR
    depth  substitute_no_sort · substitute_binders · substitute · sort
           score_cmp · tree_drop · tree_clone · eval_with_nots · bincode_de
    width  substitute_wide · sort_wide · score_cmp_wide · free_check
    ⇒ 13 subjects, O(1) over 4 → 4,096 (depth) and 4 → 65,536 (width),
      asserted in BOTH profiles.  `pretty` / `pretty_wide` are present but
      commented out, carrying the Stage-D marker — the list records intent
      as well as achievement.

theta_depth_tripwire                          NOT A PASS — A TRIPWIRE
    pretty · substitute_deep_binding · clone · drop · encode
    sort_nested_set · sort_nested_map · clone_nested_set
    ⇒ 8 subjects under per-profile ceilings; certifies only "not worse".

theta_width_tripwire                          EMPTY, AND WIRED
    ⇒ every width-axis member found so far is converted.  Retained, named and
      wired so a newly discovered Θ(width) traversal is one line rather than
      new infrastructure.

reported_reproducer_depth_survives_a_default_worker_stack
    ⇒ `@"OUT"!([[…[0]…]])` at depth 10 (debug) / 70 (release) on the 2 MiB
      stack a tokio worker actually gets.  GREEN in both profiles since Stage
      B; no longer `#[ignore]`d.  Its own doc comment records that going green
      was NOT the end of the work.
```

**The workspace bar.** Measured for this reconciliation rather than relayed:

```text
$ RUST_MIN_STACK=8388608 \
  systemd-run --user --scope -p MemoryMax=28G \
  cargo nextest run --workspace --no-fail-fast

  Summary [663.688s]  3506 tests run: 3506 passed (26 slow), 33 skipped
```

**Reconciling that count with the tree.** The working tree at measurement time
was `b9aaa3d4` **plus one unrelated uncommitted file** —
`rholang/src/rust/interpreter/pretty_printer.rs`, carrying an in-flight Stage-D
conversion by a concurrent author — which contributes 13 `#[test]` items.
$`3{,}506 - 13 = 3{,}493`$, which is exactly the count `b9aaa3d4` itself carries
and exactly the last figure recorded for this suite. The bar is therefore green
both for HEAD and for HEAD-plus-that-file, and the arithmetic is stated so the
next reader can reproduce the reconciliation instead of wondering why the total
moved.

⚠ **One caveat about this suite that is worth writing down, because it will
mislead somebody otherwise.** An earlier run — same tree, no `--no-fail-fast`,
taken while the machine carried a load average of ~125 from concurrent build
jobs — stopped at `3246/3506 tests run: 3244 passed, 2 failed, 33 skipped`, with
260 tests not run because of the fail-fast. The two failures were
`deep_recursion_longslow_should_not_stackoverflow` and
`deep_recursion_shortslow_should_not_stackoverflow`
(`casper/tests/genesis/contracts/deep_recursion_spec.rs`). Both assert against an
**internal wall-clock budget** — `eval_rholang_code(&code, Duration::from_secs(180))`,
tightened from 300 s to 180 s by `ac2eae51` — and both exceeded it at 180.3 s and
180.9 s under that contention. Re-run in isolation on the same tree they complete
in **93.4 s** and **95.0 s**, i.e. at 52 % of budget, and they passed again in the
quieter full run above.

Two things follow. These two tests are **contention-sensitive rather than
regressions**, and a bar measured on a busy machine must say so rather than
record a red. And a wall-clock assertion is, structurally, the same kind of
claim as a byte-count ceiling: it certifies "not worse than a number chosen on
one machine on one day", so it belongs in the same category as the tripwire
ceilings of [§12.7](#127--sort_nested_set-sits-22--under-its-own-ceiling) and
carries the same eventual obligation to be re-derived rather than relaxed.

**A note on what a workspace-wide green does and does not establish here.** It
establishes that the conversions listed in
[§12.1](#121-what-landed-after-11-was-written) — across the substitution SCC,
the sorter, the score tree, `eval_with`, `free_check` and the cold-store
codec — did not disturb any observable the suite checks. It does
**not** establish depth-independence, which only
`converted_traversals_are_depth_independent` can, because a stack overflow
`abort()`s the test binary rather than failing a test; and it does not establish
consensus neutrality, which is carried by the per-conversion differentials named
in [§12.1](#121-what-landed-after-11-was-written) and by the by-construction
arguments in [§8.2](#82-why-each-conversion-is-neutral-by-construction--per-traversal).

---

### 12.10 Evidence ledger — second amendment

| # | claim | provenance |
|---|---|---|
| E31 | `substitute` is 0 B/level in **both** profiles; the sorted entry point's last 437 / 140 B/level was the derived recursive teardown of the un-sorted intermediate | **Measured** — gate, constant 96 KiB (debug) / 32 KiB (release) from parameter 4 to 4,096; **commit** `b98fa20a` |
| E32 | The sorter is 0 B/level in both profiles, and `SortedParHashSet` / `SortedParMap` fall to the derived `<Par as Clone>::clone` floor | **Measured** — gate `sort` / `sort_wide`; **commit** `2c32b173` (78,438 → 0 debug, 6,589 → 0 release; 78,543 → 15,850 and 78,583 → 15,914) |
| E33 | bincode decode is 0 B/level in both profiles; its pre-conversion slope reproduces §11.1 F3 to 0.11 % / 0.6 % | **Measured** — direct bisection of the subject immediately before and after: 28,331 / 12,971 → 0 / 0; **commits** `9a5521a2`, `af1a426b` |
| E34 | `rho-pure-eval::eval_with` is 0 B/level in both profiles | **Measured** — gate `eval_with_nots`, 60 KiB (debug) / 12 KiB (release) flat 4 → 4,096; **commit** `a3fd6fe4` |
| E35 | `FoldMatch::free_check` is 0 B/sibling in both profiles, gated at `-O0` where the defect was visible | **Measured** — gate `free_check`, 44 KiB at width 4 and at width 65,536; **commit** `6714a128` |
| E36 | ★ `normalize`'s release constant is **unchanged** by the sorter conversion: 7,247 (§11.2) → 7,261, a 0.19 % move against a 29 B/level instrument resolution | **Measured** — `scripts/stack_depth_probe.sh`, subject `normalize`, depths 20/40/80/160, fit $`17{,}631 + 7{,}261\,N`$ |
| E37 | ★ `normalize`'s debug constant **did** move, 78,579 → 43,542, revealing its own recursion under the sorter's | **Measured** — same harness, depths 10/20/40/80, fit $`96{,}701 + 43{,}542\,N`$ |
| E38 | The per-level normalizer frame is a dead constant 43,520 B over a 4-frame chain, of which `normalize_ann_proc` is 65.4 % | **Measured** — `gdb`, successive `$sp` deltas at frames 7 → 11 → 15, zero variance; agrees with E37 to 0.05 % |
| E39 | ★★ A 577-byte source (`[`×288 `0` `]`×288) aborts a **release** node on the 2 MiB stack a tokio worker gets; max surviving source depth 287 release / 45 debug | **Measured** — direct bisection of depth at fixed stack, one child process per point; abort message `fatal runtime error: stack overflow` |
| E40 | The abort is inside the normalizer, not the parser | **Measured** — `gdb` backtrace: a clean repetition of `normalize_ann_proc → normalize_p_collect → normalize_collection → fold_match`, leaf in `models::rust::utils::new_gint_expr` |
| E41 | It fires before metering: `source_to_adt_with_normalizer_env` is `inj_attempt`'s first phase and precedes `reset_from_signed_process` | **Read** — `rholang/src/rust/interpreter/interpreter.rs`, phases `build-normalized-term` then `set-initial-cost` |
| E42 | Validators normalize deploy source they receive | **Read** — `casper/src/rust/rholang/replay_runtime.rs::run_user_deploy` → `runtime_ops.evaluate(&processed_deploy.deploy)` → `inj_attempt` |
| E43 | The normalizer SCC is 26 functions with 76 production call sites across 22 files, carrying threaded state (`free_map` left-to-right, `bound_map_chain` scoped, `ProcVisitInputs.par` accumulating) | **Derived** — Tarjan over the production call graph of `rholang/src/rust/interpreter/compiler/`, `#[cfg(test)]` bodies excluded by brace tracking; **read** — `normalize.rs`'s `binary_exp` / `unary_exp` |
| E44 | `rhocalc_ast::lower_proc` is a Θ(depth) `Par`-producing traversal in a sibling repository that §4.1's proto SCC structurally cannot see | **Read** — `mettail-rust`, `rholang-runtime/src/rhocalc_ast.rs` |
| E45 | ★ `generate_par` cannot produce an `ETuple` at any depth or seed, which is why 6 of 11 differential tests went red under an injected `ETuple` field drift while all 3 `generate_par` proptests stayed green | **Read** — `generate_expr`'s four alternatives are `GBool`/`GInt`/`GString`/`ENotBody`; **measured** — the falsification experiment recorded in `9a5521a2` |
| E46 | `sort_nested_set` is at 7,509 B/level against a 7,680 ceiling — 2.2 % headroom — where the ceiling is the measured pre-conversion baseline | **Measured** — gate `theta_depth_tripwire`, release; independently bisected at 12,288 B (parameter 2) and 57,344 B (parameter 8) |
| E47 | `pretty` is unconverted at 41,984 B/level debug / 4,266 release, corroborated within 0.4 % by three independent readings | **Measured** — gate ladder 16 → 64 on `b9aaa3d4`; against §5's 41,840 / 4,242 and `18419514`'s 41,813 |
| E48 | Two gate assertions could not fail before `18419514`: `pretty`'s `!s.is_empty()` and `encode`/`bincode_ser`'s `!bytes.is_empty()` | **Read** — the `18419514` diff replaces `assert!(!s.is_empty())` with `assert_carries("the PRINTED nesting", printed_bracket_depth(&s), depth)` and adds depth-proportional byte floors |
| E49 | The gate carries 13 converted subjects and 9 tripwire subjects in both profiles at `b9aaa3d4` | **Measured** — `converted_traversals_are_depth_independent` and `theta_depth_tripwire`, `--test-threads 1`, debug and release |
| E50 | The workspace bar is green: 3,506 run, 3,506 passed, 33 skipped, 663.7 s | **Measured** — `cargo nextest run --workspace --no-fail-fast`; 13 of those tests belong to an unrelated in-flight file, so `b9aaa3d4`'s own count is 3,493 |
| E51 | The two `deep_recursion_*_should_not_stackoverflow` failures seen under load are wall-clock-budget timeouts, not regressions | **Measured** — the same two tests complete in 93.4 s and 95.0 s in isolation against their internal 180 s budget (`ac2eae51`) |

---

## 13. Stage G — the normalizer, execution record (2026-07-27)

[§12.3](#123--577-bytes-of-source-abort-a-release-node-before-metering) left one
member of the family with no disposition, and named the choice: convert it, or
bound it first. This section records that the choice was made, what it cost, and
the two things the conversion found that no plan had predicted.

**Commits.** `07853de0` (the machine, the oracle twin, the differential) and
`88ef41cd` (the gate subjects and the named regression). Measurement conditions
are [§12](#12-reconciliation--the-tree-at-b9aaa3d4-2026-07-27)'s, unchanged.

---

### 13.1 The decision, and why "bound it first" was rejected

A pre-normalization depth limit is cheap, is one `if`, and was rejected on the
merits rather than on taste. Two reasons, and the second is the one that
settles it:

1. **It is protocol-visible.** It changes which deploys a node accepts, which
   is the standing decision [§7.3](#73-the-prost-decode-ceiling-is-a-constraint-on-the-fix-not-a-defect-to-fix)
   already names — *no new protocol-level nesting cap*.
2. **★ It cannot be given a profile-independent constant.** This is not a
   preference; it is arithmetic on the numbers
   [§12.3](#123--577-bytes-of-source-abort-a-release-node-before-metering)
   measured. The guard would have to satisfy

   ```math
   D_{\text{guard}} \;\le\; 287 \quad\text{(release, or the guard is inert)}
   \qquad\text{and}\qquad
   D_{\text{guard}} \;\le\; 45 \quad\text{(debug, or the node still aborts)}
   ```

   so any single constant is either **inert in release** — a `debug`-derived 45
   rejects nothing release could not already survive, while release still
   accepts nothing it could not — or **newly restrictive**, refusing at depth 46
   programs that release handled to 287. A guard whose safe value depends on how
   the binary was compiled is not a protocol rule; it is a compilation artefact
   wearing one.

Converting removes the ceiling rather than choosing where to put it. The same
asymmetry is why the named regression test carries **one** depth list for both
profiles ([§13.5](#135-the-gate-and-the-named-regression)).

---

### 13.2 What was converted, and the shape that made it hard

The five conversions that preceded this one are **post-order folds**: children
are independent, so a driver pushes them all at once and reassembles on the way
up. This SCC's state is **threaded**, and all three carriers thread differently:

```text
   ┌─ free_map ─────────────────────────────────────────────────────────┐
   │   sibling₀ ──▶ sibling₁ ──▶ sibling₂        LEFT-TO-RIGHT           │
   │      binds 1     binds 2      binds 0                               │
   │      levels 0    levels 1,2   —             a sibling that binds n  │
   │                                             SHIFTS every later      │
   │                                             sibling's de Bruijn     │
   │                                             levels by n             │
   └─────────────────────────────────────────────────────────────────────┘
   ┌─ bound_map_chain ──────────────────────────────────────────────────┐
   │   push on entry to a binder ─────▶ … ─────▶ pop on exit             │
   │   under the recursion this was STACK UNWINDING; the machine has no  │
   │   unwinding, so each continuation OWNS the chain it resumes with    │
   └─────────────────────────────────────────────────────────────────────┘
   ┌─ ProcVisitInputs.par ──────────────────────────────────────────────┐
   │   built on the way DOWN — `prepend_expr(input_par, …)` — as well as │
   │   up, so a post-order `Combine` alone cannot reassemble the result  │
   └─────────────────────────────────────────────────────────────────────┘
```

Because child $`i`$'s **input** depends on child $`i-1`$'s **output**, the
machine schedules one child at a time and each continuation is a *frame*
holding exactly the locals its recursive counterpart held across its recursive
call. Three step shapes cover all 26 members:

| shape | what it does | which members |
|---|---|---|
| `Step::Done` | publish a value | the leaves — `Nil`, ground literals, `SimpleType`, `ProcVar`, `VarRef`, and the two error arms |
| `Step::Descend` | push one continuation, then one child | every structural arm |
| `Step::Tail` | **replace** the obligation | the six desugarings — `let`, `!?`, multi-receipt `for`, complex-source `for`, `recognize_signed_term`, `recognize_signed_join` |

`Step::Tail` is worth its own row because it is the one place the machine is
*strictly better* than the recursion rather than merely equal to it: those six
end in an unconditional `normalize_ann_proc(&rewritten, input, …)`, i.e. a
proper tail call, which the recursive form paid a native frame for anyway. A
sequential `let` with $`n`$ bindings unrolls through $`n`$ nested `match`
rewrites; under the recursion that was $`n`$ frames stacked on top of the term's
own nesting, and it is now flat.

---

### 13.3 The invariants — and why `sort_drive`'s form had to be re-derived

[`sort_drive`](../../../models/src/rust/rholang/sorter/sort_drive.rs) maintains

```math
|V| \;+\; D \;+\; |C| \;-\; \sum_{k \in C} \mathrm{arity}(k) \;=\; 1
```

where $`V`$ is the value stack, $`D`$ the pending descends, $`C`$ the pending
continuations. **That statement holds in the normalizer's machine too, and it
degenerates.** Every continuation here pops exactly one value — the child that
just finished — so $`\sum \mathrm{arity} = |C|`$ and the law collapses to

```math
|V| \;+\; D \;=\; 1 \qquad\text{(the conservation law)}
```

This is recorded rather than transcribed, for the reason the sorter's own note
gives about the *previous* correction to this invariant: an invariant that fires
spuriously is worse than none, and one that **cannot** fire is worth nothing at
all. The degeneracy is not a weakness of the machine; it is a fact about
threaded traversals. A batch machine spreads a node's $`n`$ obligations across
the work stack where a pop-count can see them; a threaded machine holds $`n-1`$
of them *inside* the continuation, where a pop-count structurally cannot.

The cross-checking strength therefore moves to a **slot invariant**, which is
where the real bug class lives:

```math
\forall k \in C:\quad \mathrm{filled}(k) \;<\; \mathrm{arity}(k)
\qquad\text{and, when }k\text{ runs,}\qquad
\bigl(\mathrm{filled}(k)+1 = \mathrm{arity}(k)\bigr) \iff k \text{ produced a value}
```

* `NormKont::arity` — an **independently spelled** exhaustive `match` giving the
  total child count, read off the *node's* shape: `elements.len()`, `2`, `3`,
  `1 + args.len()`, `1 + |formals| + 1`, $`\sum_{\text{groups}}|g| + |\text{sources}| + [\text{guard}] + 1`$.
* `NormKont::filled` — read off the continuation's **accumulators**: vector
  lengths and `Option` slots, i.e. what the machine has actually delivered.

The two meet exactly once per continuation, and the assertion is two-sided so
neither "finished early" nor "asked for another" can pass.

> **It fired, during development, on the first term it was given.** `ParSeq`
> was written with `idx` meaning *the next operand to schedule* while `filled`
> needs *operands absorbed* — an off-by-one that is invisible in behaviour
> (the operands are still visited in order) and would have sat in the machine
> indefinitely. The assertion reported
> `a continuation absorbed child 50000 of 50000 and then asked for another`
> on the pre-existing 50,000-operand `p_par` test. That is the whole argument
> for spelling `arity` twice.

⚠ Both are computed **only** under `debug_assertions`. `Match::filled` sums over
its completed cases and `Input::filled` over its pattern groups, so evaluating
them unconditionally would put an $`O(n)`$ read on every $`O(1)`$ machine step
and make the drive quadratic **in a release node**. They are diagnostics, not
machinery.

---

### 13.4 ★ The two residuals the conversion exposed, and the ladder down

The conversion did not reach 0 B/level in one step, and the intermediate
readings are the most useful thing this section records: **removing the largest
consumer reveals the next one, and the next two were both derived traversals the
recursion had been masking.** Each was dispositioned exactly as
[§7.2](#72-derived-traversals--leg-1-only-by-construction) prescribes — delete
the *call site*, never the impl.

| # | reading (debug) | what it was | how it was removed |
|---|---:|---|---|
| 0 | **43,542** | `normalize_ann_proc`'s own recursion | the machine (`07853de0`) |
| 1 | **15,850** | `<Par as Clone>::clone` | Leg-1 at 14 call sites |
| 2 | **434** | `drop_in_place::<Par>` | `par_children::dismantle` at 4 sites |
| 3 | **0** | — | — |

**Reading 1 is recognisable, and that is how it was identified.** 15,850 is
`<Par as Clone>::clone`'s constant to within 0.2 % of the 15,875 / 15,914
[§5](#5-measured-constants-per-traversal-and-per-profile) records — the same
floor `SortedParHashSet` and `SortedParMap` fell onto when the sorter was
converted ([§12.1](#121-what-landed-after-11-was-written) row 1). Every arm
cloned its child `Par` into an accumulator and then read `locally_free` /
`connective_used` off the original; those are **shallow** fields, so they are
read first and the deep clone is deleted.

> ★ The sharpest instance is the `HasLocallyFree` readers, and it generalises
> past this file. The impl is
> ```rust
> impl HasLocallyFree<Par> for Par {
>     fn connective_used(&self, p: Par) -> bool { p.connective_used }
>     fn locally_free(&self, p: Par, _depth: i32) -> Vec<u8> { p.locally_free }
> }
> ```
> — it does not recurse, it reads one cached field, and it takes its subject
> **by value**. That signature *forces* every caller to write
> `x.locally_free(x.clone(), d)`: two Θ(depth) deep clones to read one cached
> bitset. `models`' by-reference readers already exist for exactly this reason;
> the normalizer's callers now read the fields directly, which is byte-identical
> by construction because the clone and the original agree on every field.

**Reading 2 is the same residual `b98fa20a` found in `substitute`, in the same
place.** `Compiler::normalize_term` is `normalize_ann_proc` *then*
`ParSortMatcher::sort_match`, and the sorter READS its input and BUILDS a fresh
term — so the un-sorted intermediate falls out of scope, recursively. Both exits
now hand it to `par_children::dismantle`. Three further sites were fixed for the
same reason and they matter more than the arithmetic suggests, because they are
all on the **rejection** path — where a hostile input actually lands:
`norm_drive`'s `unwind`, the three `bundle` rejections, and the duplicate-channel
rejection in `for`. A term that is refused must not overflow on the way *out*.

**Measured, both profiles, by direct bisection of the `normalize` probe:**

| profile | ladder | before | after |
|---|---|---|---|
| debug | 10 / 20 / 40 / 80 | $`S(N) = 96{,}701 + 43{,}542\,N`$ | — |
| debug | 4 / 256 / 1,024 / 4,096 | — | $`S(N) = 241{,}664 + 0\,N`$ (236 KiB, flat) |
| release | 20 / 40 / 80 / 160 | $`S(N) = 17{,}631 + 7{,}261\,N`$ | $`S(N) = 32{,}768 + 0\,N`$ (32 KiB, flat) |
| release | 4 / 256 / 1,024 / 4,096 | — | 32 KiB at every point |

The "before" fits reproduce [§12.2](#122--a-corrected-attribution--normalize-was-never-the-sorter-in-release)'s
E37 and E36 **exactly** — 96,701 + 43,542 N and 17,631 + 7,261 N — which is what
licenses reading the "after" against them.

---

### 13.5 The gate, and the named regression

`normalize` and `normalize_wide` are in
`converted_traversals_are_depth_independent`, O(1) over 4 → 4,096 (depth) and
4 → 65,536 (width), in **both** profiles. Group **(C) of
[§12.6](#126-the-family-at-b9aaa3d4--converted-tripwired-and-open) is now
empty**, and `normalize` is the only member ever promoted into the converted
list from it — every other conversion was promoted from a measured *ceiling*,
this one from a measured *constant with no disposition*.

`the_577_byte_reproducer_is_a_deploy_and_not_a_node_abort` pins the reported
defect at depths **288, 1,152 and 100,000** on a 2 MiB stack.

> ★ **Those depths do not branch on profile, and the reason is
> [§13.1](#131-the-decision-and-why-bound-it-first-was-rejected).** Its
> neighbour `reported_reproducer_depth_survives_a_default_worker_stack` asserts
> 10 in debug and 70 in release, because the traversal *it* guards is still
> profile-sensitive at the margin. Here the profile split **is** the defect. A
> regression test carrying a `cfg!(debug_assertions)` branch would re-import the
> asymmetry into the one artefact whose job is to exclude it.

Both subjects apply **Rule V at both ends**: the input is checked for its
bracket run / sibling count and the *output* for its `EList` nesting / sibling
count, with iterative checkers so nothing measures itself. Without the output
half the gate would pass for a normalizer that **rejected** the input — which
also does not abort, and would be a different regression wearing the same green.

---

### 13.6 The proof, and the eighth thing that could not fail

`normalize` is on the deploy **and replay** path, so [§8](#8--the-proof-standard)
applies in full. There is no charge trace to compare — and that is a property of
this member, not an omission: it runs before a budget exists, and no function in
the SCC contains a `reserve_*` or `Cost::` call. Neutrality reduces to *result*
equality, in full.

* **`compiler::normalize_recursive`** — the 26-function SCC **verbatim** as of
  `6ccf71f2`, `#[cfg(test)]`, with intra-SCC calls renamed so the copy calls
  itself and can never re-enter production. Provenance is recorded inline per
  block as `file:line-line`.
* **`compiler::normalize_differential`** — 8 tests. Every case compares the
  normalized term's `encode_to_vec()` **bytes**, the final `FreeMap`, and the
  final `BoundMapChain` — or, on the error path, the `{:?}` of the error.

**"Both accepted" is deliberately not the claim.** Free-variable numbering is
consensus-visible, and a mis-threaded driver fails *silently*: it emits a
well-formed `Par` with different de Bruijn indices. Nothing crashes; only byte
comparison sees it.

**Anti-vacuity, three ways, because this campaign has been bitten seven times
([§12.5](#125--the-vacuity-ledger-and-the-two-rules-it-produced)).**

1. Every corpus entry declares how many free bindings it carries, and is
   **measured against the oracle**. A corpus that quietly collapsed to closed
   terms — the shape on which a threading defect is invisible — fails.
2. A dedicated `DIFFERING_SIBLING_BIND_COUNTS` family, plus a test that proves
   the witnesses *really do* bind different counts per sibling. If every sibling
   binds the same number, reversing the threading is unobservable and the family
   is decorative.
3. ★ **The differential was proven able to go red.** The exact post-order defect
   — seed every sibling from the *entry* free map instead of its predecessor's —
   was injected into the **production** machine, not into a mock. It turned the
   differential red on `[*a, [*b, *c]]` with the signature `16, 2` against
   `16, 4`: varint de Bruijn index 1 against 2. The injection was then reverted
   and the suite re-run green.

**Two live defects the differential caught while it was being written**, both of
which would have passed a "both accepted" check:

| defect | what it was | how it presented |
|---|---|---|
| `~P` span | the dispatch passed `proc.span` where the recursive form passed `arg.span` — the span the *free map* records for the connective | byte divergence on `new ch in { for (@{~7} <- ch) { Nil } }` |
| the harness itself | `format!("{:?}")` on a `HashMap`-backed `FreeMap` compares **iteration order** | a reported divergence on a case whose encoded bytes were byte-**identical** |

The second belongs in the vacuity ledger as its own mechanism, and it is the
inverse of the other seven: not a check that could not fail, but a check that
could not *succeed* — a false red rather than a false green. It is the same root
cause (comparing a rendering instead of a value) and the same remedy (compare
the value; `HashMap`'s `PartialEq` is order-independent and the derived `Debug`
is for the failure message only).

---

### 13.7 Evidence ledger — third amendment

| # | claim | provenance |
|---|---|---|
| E52 | `normalize` is **0 B/level in both profiles**, flat 4 → 4,096 | **Measured** — `scripts/stack_depth_probe.sh`, $`S(N)=241{,}664+0N`$ debug and $`32{,}768+0N`$ release; gate `converted_traversals_are_depth_independent`, both profiles; **commits** `07853de0`, `88ef41cd` |
| E53 | The pre-conversion baseline reproduces §12.2 exactly | **Measured** — $`96{,}701 + 43{,}542\,N`$ (debug, depths 10/20/40/80) and $`17{,}631 + 7{,}261\,N`$ (release, 20/40/80/160), identical to E37 / E36 |
| E54 | ★★ The 577-byte reproducer is now a processed deploy, not a node abort, in **both** profiles — as is a 200 kB one | **Measured** — source depths 288, 1,152 and 100,000 on a 2 MiB thread; `the_577_byte_reproducer_is_a_deploy_and_not_a_node_abort` |
| E55 | Removing the recursion exposed `<Par as Clone>::clone` at **15,850** B/level — its own constant to 0.2 % — closed by Leg-1 at 14 call sites | **Measured** — intermediate bisection; **read** — `HasLocallyFree<Par>`'s by-value field readers |
| E56 | Removing the clones exposed `drop_in_place::<Par>` at **434** B/level, the same residual and fix as `b98fa20a` | **Measured** — intermediate bisection; closed by `par_children::dismantle` at 4 sites, three of them on rejection paths |
| E57 | The slot invariant fires on a real defect: `ParSeq`'s `idx` meant "next" where `filled` needs "absorbed" | **Measured** — it fired on the pre-existing 50,000-operand `p_par` test during development |
| E58 | ★ The differential can go red, on the shape that fails silently | **Measured** — the post-order defect injected into the production machine diverged on `[*a, [*b, *c]]` (`16,2` vs `16,4`), then reverted |
| E59 | Two live defects were caught by the differential: the `~P` span, and `Debug`-string comparison of a `HashMap`-backed `FreeMap` | **Measured** — both surfaced as byte/rendering divergences on first run |
| E60 | Test parity: all 139 pre-existing `#[test]` functions in the compiler subtree are present, plus 8 new | **Measured** — name-by-name set difference against `HEAD`; `-p rholang --lib` 274 run / 274 passed / 0 skipped |
| E61 | A depth guard cannot be given a profile-independent constant | **Derived** — $`D \le 287`$ (release, else inert) and $`D \le 45`$ (debug, else still aborts) are unsatisfiable together by a useful constant; from E39's measurements |

---

## 14. The deploy path teardown — `<Par as Drop>` (2026-07-27)

**Stratum.** Written after [§13](#13-stage-g--the-normalizer-execution-record-2026-07-27)
landed. It records one finding, its reachability verdict, the gate subject added
to hold it, and a **re-derivation** of the disposition [§7.2] assigned to the
derived-impl class — re-derived because this campaign has repeatedly found
recorded rationales naming mechanisms the code did not have, and an inherited
rationale is not evidence.

### 14.1 The finding, in one sentence

Stage G made the term **builder** heap-bounded and left the term **releaser**
recursive, so a deploy can now construct a `Par` that the process cannot destroy.

### 14.2 The subject existed; its NAME did not

The defect was reported as "*the repo that defines `Par` is the one not measuring
its `Drop`*", on the evidence that a search for `par_drop` finds mettail-rust's
twin gate and nothing here. **The search was right and the conclusion was wrong.**
This gate had measured `drop_in_place::<Par>` since it was written — under the
subject name `drop`, which says which operation and never says on what. One
traversal had two names across two repositories, and the second name was
indistinguishable from an absence.

It is now `par_drop` in both repositories. That is the whole of the naming fix,
and it is worth recording because the cost of the divergence was a defect report
whose central premise was false while its subject matter was real.

### 14.3 Reachability — MEASURED, and the verdict is *yes*

The question that sets the severity is whether a **deploy** can reach the depth
at which the destructor aborts. It can, and the margin is not close.

Method: `rholang/tests/stack_depth_probe.rs`, one child process per
$`(\text{subject}, \text{depth}, \text{stack})`$ point, bisecting depth at a
fixed stack. Debug profile. The stack is fixed at **2 MiB** because that is what
a tokio worker gets — `node/src/main.rs` builds its runtime with
`Builder::new_multi_thread().enable_all()` and never calls `thread_stack_size`,
so workers take Rust's default spawned-thread size, and `RUST_MIN_STACK` is set
only in `.cargo/config.toml`, i.e. for cargo-launched runs and not for a
deployed binary.

| subject | stack | max surviving depth | first failing depth |
|---|---|---:|---:|
| `normalize` (build the term) | 2 MiB | **≥ 39,960** | not reached |
| `par_drop` (release the term) | 2 MiB | **4,414** | 4,453 |
| `par_drop` | 8 MiB | 17,929 | 17,987 |

The two 2 MiB rows are the finding. A source of $`4{,}415`$ bracket levels —
$`2 \times 4{,}415 + 1 = 8{,}831`$ bytes, one TCP segment — normalizes without
difficulty and then aborts the process on teardown. At 8 MiB the same shape
aborts at depth $`17{,}987`$, which reproduces the originally reported
"32,000 aborts, 16,000 does not" exactly: both reported points lie on opposite
sides of $`17{,}987`$.

The abort is not a panic:

```text
thread 'probe' has overflowed its stack
fatal runtime error: stack overflow, aborting
```

`SIGSEGV` on the guard page, handled by the runtime, `abort()`. No
`catch_unwind` sees it, `inj_attempt`'s `Err(e) => handle_error(ParserError(..))`
arm cannot run, and — because `build-normalized-term` precedes
`set-initial-cost` — no budget has been established to charge against. This is
the same shape as the 577-byte reproducer of [§12](#12-reconciliation--the-tree-at-b9aaa3d4-2026-07-27),
one phase later in the same function.

**Bounded by measurement, not by assumption: the wire path is NOT affected.** A
`Par` arriving from a peer is decoded by `prost`, whose `DecodeContext` enforces
`RECURSION_LIMIT = 100` message levels; `models/src/rust/canonical_path.rs`
records the same envelope and its own `COLLECTION_DEPTH_LIMIT = 32`. So the
asymmetry is precise and worth stating plainly: **a `Par` too deep to arrive over
the network can be built from source text.**

### 14.4 Why no existing assertion caught it

```text
   Compiler::source_to_adt(src)          the Par falls out of scope
   ────────────────────────────          ───────────────────────────
   subject `normalize`                   subject `par_drop`
   CONVERTED, 0 B/level                  TRIPWIRED, ~464 B/level
   gated at depth 100,000  ✓             gated for slope only  ✓
                     ╲                   ╱
                      ╲                 ╱
                       ▼               ▼
                  ★ THE JOIN — asserted nowhere
                    a deploy builds what it cannot release
```

Both halves were gated; their composition was not. And the gate's own headline
regression test —
`the_577_byte_reproducer_is_a_deploy_and_not_a_node_abort`, which certifies
source depth **100,000** on a 2 MiB worker — passes only because its fixture
`normalize_body` ends in `par_children::dismantle`. **No production caller does
that.** `Compiler::source_to_adt` returns the sorted `Par` by value and every
caller, `InterpreterImpl::inj_attempt` included, releases it through the derived
destructor. The distance between "depth 100,000 is fine" and "depth 4,415 aborts
the node" is one function call in a test fixture, and the fixture's choice was
correct *as isolation* and misleading *as a claim about deploys*.

The new subject `normalize_drop` is `normalize_body` with `dismantle` replaced by
`drop`. Measured on the gate's own ladder (debug, 256 → 4,096):

| subject | min stack @ 256 | min stack @ 4,096 | B/level |
|---|---:|---:|---:|
| `normalize` | 196 KiB | 196 KiB | **0** |
| `par_drop` | 124 KiB | **1,864 KiB** | 464 |
| `normalize_drop` | 196 KiB | **1,864 KiB** | 444 |

The deep ends are identical to the byte. The two slopes differ only because
`normalize_drop`'s shallow end is clamped at the normalizer's 200,704 B floor
rather than the destructor's 126,976 B one:

```math
\frac{1{,}908{,}736 - 200{,}704}{4{,}096 - 256} = 444.8
\qquad
\frac{1{,}908{,}736 - 126{,}976}{4{,}096 - 256} = 464.0
```

Because the two traversals run **sequentially** rather than nested, the
composition costs the maximum and not the sum, which is asserted as a band whose
ends are structural rather than measured:

```math
S_{\texttt{par\_drop}} \;\le\; S_{\texttt{normalize\_drop}} \;\le\;
  S_{\texttt{par\_drop}} + S_{\texttt{normalize}}
```

`the_deploy_composition_is_bounded_below_by_its_destructor` asserts exactly that.
Neither end inverts when the defect is fixed — $`\max(a,b) \ge b`$ holds however
small $`b`$ becomes — which is why the band is the assertion and the *equality*
(true today) is only recorded. A guard that must be deleted to record success is
a guard that discourages success.

### 14.5 Anti-vacuity — the checkers are shown to refuse a DESTRUCTOR

[§12](#12-reconciliation--the-tree-at-b9aaa3d4-2026-07-27) established the
reddening obligation: a checker that has only ever been compared against subjects
that cleared it has not been shown to be able to fail. The existing control,
`synthetic_sloped`, is a recursive **function**. `par_drop` is recursive **drop
glue** — code no call site names, emitted by the compiler from the type. Those
are different mechanisms, so the destructor gets its own control:

| subject | teardown | measured (debug, 4 → 4,096) |
|---|---|---:|
| `synthetic_drop` | derived recursive `Drop` over a `Box`-linked chain | **95 B/level** |
| `synthetic_drop_flat` | the same chain, detached iteratively | **0 B/level** |

All three depth-axis checkers reject the first and accept the second, and the
sharpest statement needs no checker at all: on the stack that suffices to tear
the chain down iteratively at 4,096 links, tearing the *same* chain down
recursively does not survive.

★ **The control's own N2 check went RED on its first run, and the correction is
a result.** It asserted the destructor's per-level cost was at least the
chain link's 96 B ballast, and measured **95**. The floor was wrong, not the
measurement: `synthetic_recurse` observes its ballast *after* the recursive call,
so the array is live across it and must occupy a frame slot, whereas drop glue
drops fields in declaration order and `[u8; N]` has no destructor — the ballast
is never read and no profile is obliged to keep it. The floor is now derived from
the ABI instead: a recursive call cannot cost less than the return address it
pushes, $`\texttt{size\_of::<usize>()}`$, which holds in every profile and still
catches the only failure the check exists for, since an elided recursion reads
$`\approx 0`$ and not 8.

### 14.6 Do the two repositories' gates agree?

They agree on the **property** and differ on the **constant**, and the difference
is fully explained by the fixtures rather than by either gate being wrong.

| | this repo | mettail-rust |
|---|---|---|
| subject | `par_drop` | `par_drop` |
| assertion | `assert_slope_below("par_drop", ceiling(1500, 800), 256, 4096)` | `assert_slope_below("par_drop", ceiling(600, 200), 512, 4096)` |
| fixture | `nested_list(depth)` — an `EList` chain built directly in `models::rhoapi` | a mettail `Proc` list chain **lowered** by `lower_proc_in_env`, then `drop(par); drop(term);` |
| measured (debug / release) | 464 / 219 | 368 / 95 |

The mettail subject releases **two** structures (the lowered `Par` *and* the
`Proc` it came from, whose teardown is the `language!` macro's own) and its `Par`
is whatever the lowering emits, which is not the single-element `EList` chain
this fixture builds. Two different terms of the same type, so two different
per-level costs; the shared claim — a derived, recursive, non-zero-slope
destructor of order $`10^2`$ B/level — holds in both. Recording it this way is
deliberate: "the gates agree" would be false, "the gates disagree" would imply a
defect in one of them, and neither is what the measurement says.

### 14.7 Disposition — RE-DERIVED, not inherited

The recorded disposition is that `drop_in_place::<Par>` is *"irreducible without
a manual iterative `Drop`, which would forbid the destructuring moves Leg-1
introduced."* Re-derived below, clause by clause.

**The premise holds.** Rust forbids moving a field out of a type that implements
`Drop` (E0509). `par_children::dismantle` destructures `Par` field-by-field, and
`prost`'s derived code moves fields throughout, so a hand-written
`impl Drop for Par` would break the very worklist that is the existing repair. So
"a manual iterative `Drop` forbids the destructuring moves" is **correct**, and
it is correct for a reason the compiler enforces rather than a stylistic one.

**But the conclusion — "therefore irreducible" — does not follow, because it
enumerates one repair.** Three others exist, and each has a different cost:

1. **Call-site interception (Leg-1, the status quo).** Hand every owner to
   `dismantle`. Already applied at 4 sites in the normalizer and the substituter.
   Its weakness is now measured rather than argued: `Compiler::normalize_term`
   dismantles the un-sorted intermediate and **returns the sorted term to a
   caller that does not**, so the repair was applied inside the function and
   missed at its boundary. Completeness here is unbounded and unenforceable —
   there is no compiler support for "this type must never be dropped implicitly",
   short of making it non-`Drop`-able, which is repair 3.
2. **Change the type.** Interpose a wrapper with an iterative destructor on the
   recursive edge (`EList.ps: Vec<Par>` and its siblings). This is the repair
   that removes the class rather than its instances, and it is invasive: the
   `oneof` numbering, the wire codec and every `prost`-derived impl are affected.
3. **Bound the depth at CONSTRUCTION.** Refuse to *build* a `Par` deeper than
   $`D_{\max}`$, so no traversal derived over the type — `Drop`, `Clone`,
   `encode`, `Debug` — can ever be driven past it. **One check bounds the whole
   derived-impl class.**

**On (3), and on the objection already in this ledger.** [E61] records that *"a
depth guard cannot be given a profile-independent constant"*, derived from
$`D \le 287`$ (release) against $`D \le 45`$ (debug): any constant is either
inert in release or newly restrictive in debug. **That derivation was about the
normalizer and does not transfer**, for two independent reasons.

* The spread has narrowed. Those figures came from a traversal costing
  43,542 B/level debug against 7,261 release — a ratio of **6.0**. The
  destructor costs 464 against **144** (measured 2026-07-27, release, subject
  `drop` over 256 → 4,096: 44 KiB → 584 KiB), a ratio of **3.2**, so the
  admissible window $`[D_{\text{debug}}, D_{\text{release}}]`$ is roughly twice
  as wide as the one E61 found empty.
  ⚠ **The recorded release figure did not reproduce.** [§12.6]'s table and the
  gate's own comment carry **219** B/level for this subject; direct bisection of
  the same subject on a release build of this tree gives **144**. The tripwire's
  release ceiling (800) is unaffected and the debug figure reproduces (464
  against the recorded 470, 1.3 %), so this is recorded as a discrepancy to be
  resolved rather than acted on — the ladder used for the original reading is not
  stated, and a two-point slope depends on it.

  ★ **RESOLVED 2026-07-27 in favour of 144, and therefore of the 3.2 above.**
  `par_drop` was re-bisected on a fresh release build over the same
  256 → 4,096 ladder: 45,056 B → 598,016 B, i.e. **144.0 B/level** exactly,
  reproducing the figure in this paragraph to the byte. The same run's debug
  ladder gives **464 B/level** (196,608 B → 1,908,736 B), so the profile ratio is
  $`464/144 = 3.22`$. Two independent corroborations landed with it: the gate's
  own `the_deploy_composition_is_bounded_below_by_its_destructor` printed
  `par_drop 144 B/level` in release and `par_drop 464 B/level` in debug on the
  same tree, and depth bisection at a fixed 2 MiB stack put `par_drop` at
  **14,525** levels — which is $`(2\,\text{MiB} - 45\,056)/144 = 14{,}209`$ to
  within 2%, an arithmetic cross-check the 219 figure fails by a factor of 1.5.
  **The 219 is withdrawn.** [E73]'s "2.1" was $`464/219`$ and is superseded; see
  [E84].
* More decisively, a construction bound **need not be derived from stack at
  all**. `prost`'s `RECURSION_LIMIT = 100` and `canonical_path`'s
  `COLLECTION_DEPTH_LIMIT = 32` are protocol constants, chosen for consensus
  reasons and sitting two orders of magnitude below *either* profile's abort
  point. A bound of that kind is profile-independent by construction, which is
  precisely what E61 says cannot be had — because E61 was looking for a bound
  derived from the measurement, and this one is not.

**Recommendation, and its ownership.** The principled repair is (3): a
construction-time depth bound, chosen to match the envelope the wire path already
enforces, making the source path and the network path agree instead of differ.
⚠ **It is a consensus-visible change** — it changes which deploys are accepted,
so it belongs to F1r3node's protocol surface and is **not** taken here. What is
taken here is the measurement, the gate subject, and this derivation, so that the
decision is made against evidence rather than against a footnote.

Until then the disposition is: **tripwired, reachable, and named.** `par_drop`
and `normalize_drop` are in
[§0](#0-current-subject-sets--derived-from-the-gate-not-transcribed)'s
`tripwire-depth` set, and a traversal leaves that set only by being converted,
never by having its ceiling raised.

### 14.8 ⚠ A second Θ(depth) traversal on the same deploy path, found in passing

Establishing 14.3 required reading `InterpreterImpl::inj_attempt` end to end, and
it performs **three** recursive traversals of the normalized `Par`, not one:

```rust
let parsed = Compiler::source_to_adt_with_normalizer_env(term, normalizer_env)?;   // 0 B/level
let signed_process = SignedProcess::metered(parsed, self.c.signature(), …);        // moves it
self.c.reset_from_signed_process(&signed_process);
signed_process.source_process().cloned()                                           // ★ CLONE
    .expect("metered deploy must retain source process")
```

`SignedProcess::Signed { process: Par, … }` holds the term **by value**, and
`source_process()` returns `Option<&Par>`, so `.cloned()` is
`<Par as Clone>::clone` over the whole deploy term — a tripwired subject at
**15,872 B/level debug** and **2,852 release**. Both figures are re-measured
here rather than relayed: direct bisection of subject `clone` over 16 → 128 on a
release build of this tree gives 56 KiB → 368 KiB, i.e. **2,852 B/level**,
reproducing [§12.6]'s recorded value exactly.

Dividing the 2 MiB a tokio worker gets by those constants gives the deploy
path's real depth ceilings, and the destructor is **not** the binding one:

| traversal on the deploy path | debug B/level | release B/level | $`D_{\max}`$ @ 2 MiB, release | source bytes |
|---|---:|---:|---:|---:|
| `<Par as Clone>::clone` (`inj_attempt`'s `.cloned()`) | 15,872 | 2,852 | **735** | ~1.5 kB |
| `drop_in_place::<Par>` (`par_drop`) | 464 | 144 | 14,563 | ~29 kB |

★ **735 levels is below the 288-deep AST that [§12]'s 577-byte reproducer
already produces — by a factor of only 2.6** — and it is an order of magnitude
tighter than the destructor's own bound.

This is logged rather than fixed: it is a distinct call site with a distinct
repair (the clone is made to satisfy an ownership requirement of the metering
handshake, which is F1r3node's surface), and folding it into this change would
mix a measurement task with a metering-path redesign. It is recorded here with
its number so the next reader does not have to rediscover it, and so that any
future $`D_{\max}`$ chosen under 14.7(3) is chosen against **this** bound and not
against the destructor's.

### 14.9 Evidence ledger — third amendment

| # | claim | provenance |
|---|---|---|
| E62 | `drop_in_place::<Par>` was already a gate subject here, named `drop`; the reported absence was a name mismatch with mettail-rust's `par_drop` | **Read** — `subject()` dispatch and `theta_depth_tripwire`, both pre-existing; renamed 2026-07-27 |
| E63 | ★★ A 2 MiB worker normalizes a source of depth ≥ 39,960 and aborts releasing one of depth 4,453 | **Measured** — `stack_depth_probe.rs`, depth bisection at fixed stack, debug |
| E64 | The reported 16,000-ok / 32,000-abort pair is reproduced: the 8 MiB boundary is 17,929 / 17,987 | **Measured** — same method, `PROBE_STACK=8388608` |
| E65 | The abort is `SIGABRT` (exit 134) via the guard-page handler, not a panic | **Measured** — `fatal runtime error: stack overflow, aborting` + core dump |
| E66 | A deep `Par` cannot arrive over the WIRE: `prost` `RECURSION_LIMIT` = 100 message levels | **Read** — `DecodeContext`; corroborated by `canonical_path.rs`'s `COLLECTION_DEPTH_LIMIT` = 32 |
| E67 | `normalize_drop` and `par_drop` need the identical minimum stack at depth 4,096 (1,908,736 B); the composition's slope is the destructor's | **Measured** — gate ladders, debug |
| E68 | ★ The 577-byte reproducer test's depth-100,000 claim depends on its fixture calling `dismantle`, which no production caller does | **Read** — `normalize_body` against `Compiler::source_to_adt`'s callers; **Measured** — the same fixture with `drop` aborts at 4,453 |
| E69 | The depth checkers reject a recursive DESTRUCTOR and accept an iterative one over the same structure | **Measured** — `synthetic_drop` 95 B/level vs `synthetic_drop_flat` 0 B/level; all three checkers separate them |
| E70 | The destructor control's ballast-derived floor was wrong (95 vs 96) because drop glue never reads the ballast; the ABI-derived floor is correct | **Measured** — the check went RED on its first run; **Derived** — field-drop order and `[u8; N]`'s absent destructor |
| E71 | The two repositories' `par_drop` subjects measure different fixtures (464/219 here, 368/95 there) and agree on the class, not the constant | **Read** — both fixtures; **Measured** — both gates' recorded slopes |
| E72 | ⚠ `inj_attempt` also `.cloned()`s the deploy term through `<Par as Clone>::clone` — 15,872 B/level debug, **2,852 release**, i.e. **735 levels** on a 2 MiB release worker, the deploy path's true depth ceiling | **Read** — `interpreter.rs` `set-initial-cost` phase, `SignedProcess::source_process`; **Measured** — direct bisection of subject `clone`, release, 16 → 128: 56 KiB → 368 KiB |
| E74 | ⚠ The recorded **219** B/level release figure for `par_drop` does not reproduce; direct bisection gives **144** | **Measured** — release build of this tree, subject `drop` over 256 → 4,096: 44 KiB → 584 KiB. The debug figure DOES reproduce (464 vs recorded 470) |
| E73 | E61's "no profile-independent depth constant" does not transfer to a construction bound: the profile ratio fell from 6.0 to 2.1, and a protocol constant is not derived from stack at all | **Derived** — from E61's own figures against this section's; corroborated by two existing protocol depth constants. ⚠ **The "2.1" is SUPERSEDED** — it was $`464/219`$ and E74 already recorded that the 219 does not reproduce; the re-bisected ratio is $`464/144 = 3.22`$ ([E84]). The entry's *conclusion* is unaffected and strengthened: a wider spread makes the admissible window wider still |

---

#### 14.9 Evidence ledger — third amendment (2026-07-27, the metering handshake)

Stratum: this session. Every row is release unless stated, bisected in a child
process at an explicitly-sized stack, and reported with its bracketing evidence.

| # | claim | provenance |
|---|---|---|
| E82 | ★★ E72's `inj_attempt` clone is REMOVED. `SignedProcess::into_source_process` — the by-move twin of `source_process`, in the `par_children::take_par_child_pars` / `par_child_pars` pattern — hands the normalized term to `reducer.inj` instead of copying it | **Read** — `reset_from_signed_process` consumes only `.token()`, and `token()` is `None` on the `Signed` arm, so the metering handshake never observes `process`; **Measured** — new gate subject `inj_attempt_clone`, release: **2,852 → 0 B/level**, max depth on a 2 MiB worker **729 → ≥ 1,048,576**. Flat in **both** profiles (32 KiB release / 196 KiB debug at depths 4 and 4,096 alike), which is what admitted it to `CONVERTED_DEPTH` |
| E83 | ⚠ E72's **735** was arithmetic ($`2\,\text{MiB}/2{,}852`$) and is an UPPER bound; the bisected value is **729** | **Measured** — depth bisection at a fixed 2 MiB stack, release, of both the composition (`inj_attempt_clone` pre-fix) and the standalone `clone` subject: both give 729, 730 aborts. The 6-level gap is the subject's own ~57 KiB intercept |
| E84 | ★ The `par_drop` profile ratio is **3.22** ($`464/144`$), not 2.1. §14.7's figure is confirmed; E73's is superseded and the 219 is withdrawn | **Measured** — release re-bisection over 256 → 4,096 (45,056 B → 598,016 B = 144.0 B/level) plus the debug ladder (196,608 B → 1,908,736 B = 464 B/level) on one tree; corroborated by the gate's own printed ladders in both profiles and by a fixed-stack depth bisection (14,525 levels, within 2% of the 144-derived prediction) |
| E85 | The abort signature for the CLONE path is the same as E65 recorded for the drop path: `thread 'gate' has overflowed its stack` / `fatal runtime error: stack overflow, aborting`, shell status **134** = `128 + SIGABRT` | **Measured** — `inj_attempt_clone` pre-fix at depth 730 on a 2 MiB thread; status read both from `ExitStatus` and from `sh -c … ; echo $?` |
| E86 | ★★ **`Env::get` does NOT outrank the clone, and neither does anything else the cluster had named.** ⚠ **The ORDERING is SUPERSEDED by [E95]** — with the send-path clone removed, `Env::get` became the sole ceiling of its own shape rather than a 3-level surcharge. The measurement below stands; its ranking conclusion was true only of the pre-repair tree. An END-TO-END deploy on a 2 MiB tokio worker stops at source depth **286**; the *same* deploy shape with the deep term routed through a COMM binder stops at **283** — a marginal cost of **3 levels** for `Env::get`, not a new ceiling | **Measured** — `rholang/tests/deploy_depth_ceiling.rs`, real runtime, `thread_stack_size(2 MiB)` set explicitly on the tokio builder, deploy driven from SOURCE through `evaluate_with_term`; both shapes 7,253 B/level (identical growth 1,740,800 B over 16 → 256), differing only in a 20,480 B intercept |
| E87 | ★★★ **A NEW, PREVIOUSLY UNRECORDED Θ(depth) member is the deploy path's binding constraint: `Substitute::substitute_and_charge` opens with `term.clone()`.** ★ **REPAIRED 2026-07-28** — both wrappers take their term by value; see [E92], [E94], [E95]. It takes `term: &A` and calls `self.substitute(term.clone(), …)`, so every substitution copies its input through `<Par as Clone>::clone`. It fires on the ORDINARY SEND path — no binder, no COMM, no environment — and is why E84's control reads 286 | **Read** — `substitute.rs`, `substitute_and_charge` and `substitute_no_sort_and_charge`; **Measured** — `gdb` backtrace at the overflow of a `@"out"!([[…[0]…]])` deploy: a clean 3-frame repetition `<Par as Clone>::clone → <ExprInstance as Clone>::clone → <Expr as ConvertVec>::to_vec` under frame #103 `substitute_and_charge::<Par>`, called from `eval_send`'s data-substitution `map`, on a `spawn_detached` tokio worker |
| E88 | E85's 7,253 B/level and `substitute_deep_binding`'s 7,241 are the SAME traversal at two call sites inside `Substitute`, both ~2.5× the standalone `clone` subject's 2,852 | **Measured** — release ladders: `clone` 2,852.6 B/level (16 → 128), `substitute_deep_binding` 7,241.1 B/level (16 → 128), end-to-end deploy 7,253.3 B/level (16 → 256); the inlining explanation was already recorded on `substitute_deep_binding` and now has a second instance |
| E89 | ⚠ The ingress ceiling depends on the **shape** of the discard, not only on the term: `mk_term(..).map(drop)` measures 135.5 B/level / 14,520 levels where the literal `match … Ok(_parsed_term) => …` measures **84.3 / 21,781**, on one build and one fixture | **Measured** — both forms bisected on a 2 MiB thread, release. The consequence is methodological: a probe for a destructor must reproduce the call shape literally rather than refactor it. ⚠⚠ **BOTH FIGURES AND THE CONCLUSION ARE SUPERSEDED** — the 84.3 is a min-stack-ladder artefact ([E97](#1411-evidence-ledger--fourth-amendment)) and the true slope is **96.0**; the 1.6× spread is ordinary per-build codegen variance for this glue, not a property of the spelling ([E98](#1411-evidence-ledger--fourth-amendment)). See [§14.10](#1410--the-ingress-instance-converted-2026-07-28) |
| E90 | ⚠ `theta_depth_tripwire` FAILS in RELEASE at `291bc217`, before any change in this session, and passes in DEBUG | **Measured** — `synthetic_drop` release ladder 4 → 4,096: 12,288 B → 139,264 B, growth **126,976 B (124 KiB)** at 31.0 B/level, against the leg's own `growth() > 8 * ZERO_SLOPE_TOLERANCE` = 131,072 B — short by 3.1%; **Read** — the leg, both subjects, `drop_chain` and all three constants are byte-identical to `HEAD`. The margin was calibrated on the DEBUG glue (95 B/level ⇒ ~389 KiB, 23.7×); at `-O2` the glue keeps only the tail pointer (31 B/level ⇒ 7.75×) |
| E91 | ★ E90 is a CALIBRATION defect, and the repair is a longer ladder rather than a smaller multiple. `DESTRUCTOR_CONTROL_HI = 16,384` gives the destructor control the same weight of evidence the function control carries, because what a ladder produces is $`\text{slope} \times \text{span}`$ and the two controls differ 3.5× in slope | **Measured** — release `synthetic_drop` growth by span: 4,096 → 126,976 B (0.97× the bar); 8,192 → 258,048 (1.97×); **16,384 → 520,192 (3.97×)**; 32,768 → 1,044,480 (7.97×). The function control over its own 4 → 4,096 ladder grows 454,656 B = 3.47× the same bar, so 16,384 restores parity. Bounded ABOVE by `slope_below_verdict`'s flat ceiling $`\lfloor \texttt{ZERO\_SLOPE\_TOLERANCE} / \text{span} \rfloor`$, which reaches a literal 0 past 16,384 |
| E92 | The metered wrapper's ENTIRE depth slope was `term.clone()` | **Measured** — gate subject `subst_and_charge`, pre-repair: **2,852 B/level release / 15,872 debug**, equal to the standalone `clone` subject's own bisected constants **to the byte** in both profiles |
| E93 | ⚠ **E89's shape-sensitivity does NOT transfer to this subject**, and the difference is structural rather than a contradiction | **Measured** — all four spellings of the pre-repair `subst_and_charge` body (`mem::forget` on both values, `dismantle` on both, `drop` on both, and the fresh-temporary `…(&nested_list(depth), 0, &env).map(drop)`) bisect to **57,344 → 376,832 B** release and **278,528 → 2,056,192 B** debug — identical to the byte. Every teardown here runs AFTER the wrapper returns, so the composition costs $`\max(S_{\text{wrapper}}, S_{\text{teardown}})`$, and at 2,852 against `par_drop`'s 144 the max cannot move. E89's subject is a destructor with nothing else in the frame; this one is dominated by a traversal an order of magnitude larger |
| E94 | ⚠ `metered_wrappers_agree` — the charge-equality proof for this wrapper — could not reject two of the five mutations it exists to reject, and both sat on the lines the repair changes | **Measured** — mutations applied to `substitute.rs` and run: success-arm-charges-`input_len` **rejected**; error-arm-charges-`input_len + 1000` **PASSED**; no-sort-wrapper-error-arm **PASSED**; error-charge-omitted **PASSED**; no-sort-success-arm **PASSED**. Cause: every term in `substitution_corpus` substitutes successfully, so the error arm — the only reader of the hoisted input length — was never exercised, and `substitute_no_sort_and_charge` had no coverage at all despite the plural in the test's name. After hoisting `error_paths_agree`'s 7-case list into a shared `error_corpus()` and driving both wrappers over both arms, all five mutations reject |
| E95 | ★★ The by-value wrappers, measured end to end — **and the improvement is shape-specific** | **Measured** — gate subject `subst_and_charge`: 2,852 → **146 B/level** release (19.5×), 15,872 → **1,462** debug (10.9×). `rholang/tests/deploy_depth_ceiling.rs` on a 2 MiB tokio worker: `plain_deploy` 286 → **6,831** (23.9×, depth 6,832 exits 134); `env_get_deploy` 283 → **283 — UNCHANGED**. E86's marginal-3-levels reading is thereby superseded in its *ordering* consequence: with the send-path clone gone, `Env::get` is no longer a 3-level surcharge on a 286-level ceiling but the SOLE ceiling of its shape, 24× below the other. Any deploy that receives a deep value over a channel is unimproved by this repair |

---

### 14.10 ★★ The ingress instance, CONVERTED (2026-07-28)

**Stratum.** 2026-07-28. [§14.7](#147-disposition--re-derived-not-inherited)
enumerated four repairs for `drop_in_place::<Par>` and recommended (3), a
construction-time depth bound, while noting it is consensus-visible and therefore
not ours to take. This section takes repair **(1)** — call-site interception — at
the two call sites where the traversal is reachable from the network, and reports
the measurement that justifies calling that a *conversion* rather than a
mitigation.

#### 14.10.1 What was reachable, and from where

Every hop was read, not inferred:

```text
  DeployService/doDeploy                node/src/rust/api/deploy_grpc_service_v1.rs:256
       │                                (also HTTP, web_api.rs:421)
       ▼
  block_api::deploy_cosigned            casper/src/rust/api/block_api.rs:477
       │                                ★ SYNCHRONOUS — inline on the tokio
       │                                  worker, no `spawn_blocking`
       ▼
  multi_parent_casper::dispatch         engine/multi_parent_casper/dispatch.rs:66
       ▼
  admit_deploy_cosigned                 block_admission.rs:105
       ▼
  interpreter_util::mk_term(…)          block_admission.rs:112
       │
       │   Ok(_parsed_term) ────────────  block_admission.rs:124
       ▼                                  bound with a leading underscore,
  … the arm ends …                        NEVER READ, released when the arm ends
       ▼
  ★ Θ(depth) drop_in_place::<Par>       96 B of native stack per nesting level
```

The sibling site `admit_deploy` (`:68`/`:80`, the non-cosigned path, reachable via
the trait-default fallback) was identical. A third site, `acceptance.rs:321`, also
builds a `Par` from source but **consumes** it (`canonicalize_for_funding`) and is
not an instance of this defect.

**Measured before the repair:** a depth-21,782 deploy — **43,565 bytes** of
source — aborts the node with `SIGABRT`, shell status **134**, on a 2 MiB worker.
Depth 21,781 exits 0.

#### 14.10.2 ⚠ The severity was POSITION, not depth

This is the correction that matters most for prioritisation. Ingress was the
**highest** of the deploy path's three measured ceilings, not the binding one:

| path | ceiling (2 MiB worker, release) | measured by |
|---|---:|---|
| `env_get_deploy` (reduction) | **283** ← still the binding depth constraint | `rholang/tests/deploy_depth_ceiling.rs` |
| `plain_deploy` (reduction) | 6,831 | same |
| ingress `admit_deploy_cosigned` | **21,781** | `casper/tests/deploy_ingress_depth_ceiling.rs` |

What distinguished it is the four properties [§14.3](#143-reachability--measured-and-the-verdict-is-yes)
and [E53] use to rank this family, all of which it held alone:

1. it fired on **unauthenticated network input** — one gRPC `doDeploy` on port
   40401, with a signature any fresh keypair produces;
2. on the **receiving** node, so no proposer cooperation was needed;
3. **pre-storage and pre-consensus**, so nothing had agreed to spend anything; and
4. **pre-metering** — `admit_deploy_cosigned` never touches a `RuntimeBudget`, so
   cost accounting could not bound it, not because the charge would be too small
   but because no charge exists yet.

The inbound message cap is 16 MiB (`defaults.conf:182`), i.e. **385×** more
headroom than the 43,565-byte attack needed. And the failure is not a rejected
deploy: a guard-page `SIGSEGV` becomes `fatal runtime error: stack overflow` +
`abort()`, which the `Err(..) => …parsing_error(..)` arm three lines away cannot
observe and no `catch_unwind` can contain.

#### 14.10.3 ★ The slope is 96.0 B/level — and 84.3 is withdrawn

Depth bisected at four stack sizes, release, one binary:

| worker stack | max surviving source depth |
|---:|---:|
| 256 KiB | 2,667 |
| 512 KiB | 5,398 |
| 1 MiB | 10,859 |
| 2 MiB | 21,782 |

Pairwise slopes **95.988 / 96.006 / 95.997**. Least squares over
$`S = m\,D + b`$ gives

```math
m = 95.999\ \text{B/level}, \qquad b = 6{,}106\ \text{B}\ (5.96\ \text{KiB}), \qquad r^2 = 1.0000
```

with a largest residual of 21 B — a fifth of one level.

**The disassembly gives the same number from the instruction stream**, which is
what makes this a derivation rather than a fit. In the test binary's release
codegen both halves of the recursive cycle are five callee-saved pushes with no
`sub rsp`:

```text
  drop_in_place::<models::rhoapi::Par>            push r15,r14,r13,r12,rbx     ⇒ 40 B
       │  2 call sites ─────────────┐             + 8 B return address         = 48 B
       ▼                            │
  drop_in_place::<expr::ExprInstance>             push r15,r14,r13,r12,rbx     ⇒ 40 B
       └─ 46 call sites back ───────┘             + 8 B return address         = 48 B

                       one nesting level, across `EList.ps: Vec<Par>` = 96 B
```

⚠ **This corrects the 84.3 B/level [E89] recorded.** That figure came from a
two-point *minimum-stack* ladder at depths 256 and 4,096. Writing the measurement
as $`S(D) = mD + b`$, a min-stack ladder reports

```math
\hat{m} \;=\; \frac{S(D_{\text{hi}}) - S(D_{\text{lo}})}{D_{\text{hi}} - D_{\text{lo}}}
```

which is unbiased only if $`S`$ is affine over the whole span. It is not: below
the composition's parse/normalize floor the bisection returns that floor
(77,824 B here) regardless of depth, so $`S(256)`$ is **clamped**, the numerator
is too small, and $`\hat{m}`$ understates. Four fixed-stack depth bisections have
no such floor in the numerator, and they agree with the instruction bytes.
**84.3 is withdrawn in favour of 96.0.**

#### 14.10.4 ★★ The call-shape question, settled

[E89]'s methodological conclusion — *"a probe for a destructor must reproduce the
call shape literally rather than refactor it"* — rested on a 1.6× spread between
two spellings of one discard (`mk_term(..).map(drop)` at 135.5 B/level against
`match … Ok(_parsed_term) => …` at 84.3).

The mechanism is now identified, and it is not the spelling. The **identical**
function `drop_in_place::<models::rhoapi::Par>` — same crate, same type, same
source, same `-O2` — is emitted with **5 pushes and no `sub rsp`** (48 B frame) in
`casper`'s ingress test binary and with **7 pushes** (64 B frame) in `rholang`'s
gate binary, and the cycles they form bisect to **96** and **144** B/level
respectively. A 1.5× spread across two builds of one function, from register
allocation alone, is therefore the ordinary variance of this glue; 1.6× between
two spellings is inside it. [E93] had already found the same sensitivity *absent*
in a subject where the destructor is not the whole frame, which is consistent with
this explanation and not with a law about spellings.

★ And it is **moot for the repair**: the worklist removes the recursive cycle, so
no per-level slope remains for any spelling to modulate. The gate's subject now
*calls* production instead of reproducing it, which is strictly better regardless
of who was right — the measurement cannot drift from the thing measured.

#### 14.10.5 The repair — three stages, green at each

`Par` is `prost`-generated, so its derived `Drop` cannot be replaced, only
**bypassed at the owning call site** ([§14.7](#147-disposition--re-derived-not-inherited)'s
premise: E0509 forbids moving fields out of a `Drop` type, so a hand-written
`impl Drop for Par` would break `dismantle` itself).

**Stage 1 — one owner for the discard.**
`casper/src/rust/util/rholang/interpreter_util.rs`:

```rust
pub fn validate_deploy_term(
    rho: &str,
    normalizer_env: HashMap<String, Par>,
) -> Result<(), InterpreterError> {
    let term = mk_term(rho, normalizer_env)?;
    par_children::dismantle(term);
    Ok(())
}
```

The `Err` is `mk_term`'s own, unchanged, so every rejection message is
byte-identical to what it was.

**Stage 2 — both call sites.** `admit_deploy` and `admit_deploy_cosigned` call it.
The `O(1)` arm work — the cosigner-cap check, `add_deploy{,_cosigned}`, the
latency `tracing` — is untouched and handles no `Par`.

**Stage 3 — the gate.** `casper/tests/deploy_ingress_depth_ceiling.rs`'s subject
calls `validate_deploy_term`, so production and its measurement are one function.

★ Centralising the discard is the answer to the weakness
[§14.7](#147-disposition--re-derived-not-inherited) records against repair (1):
*"completeness here is unbounded and unenforceable"*, evidenced by
`Compiler::normalize_term` dismantling its intermediate and returning the sorted
term to a caller that does not. Three copies of one discard is three chances to
drift back to an implicit drop; one function is one.

#### 14.10.6 Why no observable byte moves

The term is genuinely surplus, so reordering its frees is invisible to the
protocol. Three independent reads, each sufficient on its own:

1. **The signature is over the SOURCE, not the term.** `Signed::create` and
   `Cosigned::from_signed_data` (`crypto/src/rust/signatures/signed.rs:369`,
   `:175`) sign `data.to_message().encode_to_vec()`; `DeployData::to_message`
   (`models/…/casper_message.rs:1031`) is `_to_proto`, whose `term` field is the
   **source string**. The normalized `Par` is never in the signed payload.
2. **Storage is the source.** `add_deploy_cosigned` persists
   `Signed<DeployData>` plus the cosigner sidecar. No `Par` is written.
3. **The term is rebuilt later anyway.** The proposer re-normalizes from source at
   `acceptance.rs:321`.

⇒ `dismantle` changes only the **order in which one discarded value's allocations
are released**. No signature, hash, block, replay or stored byte reads it.

#### 14.10.7 The gate — a converted claim, with a control

`ingress_validation_is_depth_independent` drives four legs, and each closes a way
the other three could pass without the repair:

| leg | asserts | refuses |
|---|---|---|
| 1 | subject's min stack flat over `256 → 4,096` | the recursion returning |
| 2 | the CONTROL's grows by ≥ 8× the tolerance | a collapsed fixture reading flat |
| 3 | subject has NO ceiling below 262,144 on 2 MiB | a merely *raised* ceiling |
| 4 | the CONTROL still has one, same search | a probe that cannot go red |

The control is the pre-repair shape — `mk_term` with the term bound to
`_parsed_term` and released by the arm ending — over the **same signed deploy, in
the same binary, on the same thread stack**, differing in the teardown and nothing
else. That is what replaces the output-end anti-vacuity check the repair removed:
production no longer hands the term back, so its depth cannot be asserted
directly, but any explanation that would make the subject vacuously flat makes the
control vacuously flat too, and leg 2 fails.

Measured (2026-07-28), both profiles:

| | ladder `256 → 4,096` | slope | ceiling, 2 MiB worker |
|---|---|---:|---:|
| **release, worklist** (production) | 77,824 → **77,824** B | **0.0** | **none below 262,144** |
| release, derived (control) | 77,824 → 401,408 B | 84.3 | 21,782 |
| **debug, worklist** (production) | 258,048 → **258,048** B | **0.0** | **none below 262,144** |
| debug, derived (control) | 258,048 → 1,908,736 B | 429.9 | 4,503 |

The subject's minimum stack is identical at both ends in both profiles — not
merely within tolerance — because after the conversion the only thing setting it
is the depth-independent parse/normalize floor.

⚠ The control's ceilings read 21,782 / 4,503 against production's pre-repair
21,781 / 4,504. The one-level disagreements are the control's ~6.1 KiB intercept
moving by tens of bytes now that it is reached through a `match` on the teardown
rather than being the probe's whole body — one level is 96 B, and the shift is in
that range in both directions.

#### 14.10.8 ★ The `CONVERTED_DEPTH` ruling — what moved, and what did not

`CONVERTED_DEPTH`'s admission rule is that *a traversal enters only by being
converted, never by having a ceiling raised*. Applying it here needs one
distinction held firmly:

* **The deploy-ADMISSION instance** of the composition `source_to_adt` ▸ `Drop` is
  converted, and its converted claim is executed in `casper/tests/`, where the
  production function lives. It is a flatness assertion with a sloped control —
  the shape `converted_traversals_are_depth_independent` uses — and **not** a
  raised floor.
* **The gate subject `normalize_drop` stays in `TRIPWIRE_DEPTH`.** It stands for
  the **evaluation** instance — `InterpreterImpl::inj_attempt` releasing the term
  through the derived destructor when reduction finishes — which is untouched,
  still reachable, and still Θ(depth). Moving it would have required changing
  `normalize_drop_body`'s `drop(term)` to `dismantle(term)`, i.e. deleting the
  only executed measurement of a live defect in order to record a success
  elsewhere. That is precisely the failure mode the one-sided-floor convention
  exists to prevent.

⇒ A name leaves the tripwire when **its** traversal is gone, never because a
sibling call site was fixed. The gate's sets are therefore unchanged by this
repair, and `the_audit_agrees_with_the_gate` needs no amendment.

`rholang/tests/deploy_depth_ceiling.rs` is likewise unchanged: `env_get_deploy`'s
**283** remains the deploy path's binding depth constraint, and this repair does
not touch it.

#### 14.10.9 The one-sided floor, kept

`ingress_depth_ceiling_has_not_got_worse` — the tripwire written so that it would
survive its own repair — did survive it, taking the `None` arm and printing the
historical floor. It is retained, with its limitation now stated on it: its `None`
arm asserts nothing, so a ceiling that merely rose to 261,000 would satisfy it
too. It is the record of what the defect cost; the claim that the traversal is
*converted* and not merely *cheaper* is leg 3's.

### 14.10.10 ⚠ Found in passing — a deploy rejection message is NONDETERMINISTIC

Not this defect, not repaired here, and recorded so it is not re-discovered.

The equivalence test written for [§14.10.5](#14105-the-repair--three-stages-green-at-each)'s
Stage 1 — *"`validate_deploy_term` renders the same error as `mk_term`"* — went
red on its first run, and the cause was neither the validator nor the test:

```text
  mk_term("for (@x <- y) { z }")  called 64 times, one process, one source
       ▼
  2 DISTINCT renderings:
     "… not allowed: y at SourceSpan { … col: 12 … }, z at SourceSpan { … col: 17 … }"
     "… not allowed: z at SourceSpan { … col: 17 … }, y at SourceSpan { … col: 12 … }"
```

**Mechanism.** `Compiler::top_level_error`
(`rholang/src/rust/interpreter/compiler/compiler.rs`) builds the payload of
`InterpreterError::TopLevelFreeVariablesNotAllowedError` by iterating
`FreeMap::level_bindings`, which is a `std::collections::HashMap<String,
FreeContext<T>>`. Iteration order is unspecified, and `RandomState` seeds each
*instance* differently, so the order varies from one call to the next within a
single process. The same construction pattern is used for the `wildcards` and
`connectives` arms immediately above it.

**Why it is worth a row rather than a shrug.**

* It is **client-visible today**: `admit_deploy{,_cosigned}` render this into the
  `DeployError::parsing_error` returned over gRPC, so two submissions of one bad
  deploy can receive different text.
* ⚠ **The consensus question is OPEN and was not settled here.**
  `ProcessedDeploy` carries `system_deploy_error: Option<String>`
  (`models/…/casper_message.rs:573`) — a **persisted, block-carried** string.
  Whether this constructor can reach that field was **not** established. If it
  can, a nondeterministic string is inside a hashed block body, which is a
  different and much more serious class of defect than a wobbly client message.
  That determination belongs with the owner of the replay/validation surface.

**Disposition.** Logged, not fixed — out of this change's scope, and the fix
(a deterministic order, e.g. by source position then name) touches an error
surface several existing tests assert against. The equivalence test is written to
compare renderings *modulo word order* precisely so that it neither flakes on this
nor silently accepts a real divergence; its doc comment states the trade.

### 14.11 Evidence ledger — fourth amendment

Stratum: 2026-07-28. Every row is release unless stated, bisected in a child
process at an explicitly-sized stack under `ulimit -c 0`, and reported with its
bracketing evidence.

| # | claim | provenance |
|---|---|---|
| E96 | ★★ A **depth-21,782 deploy (43,565 bytes of source)** sent to `doDeploy` aborts the node with `SIGABRT`, exit **134** — pre-storage, pre-consensus, pre-metering, on unauthenticated network input | **Measured** — depth bisection at a fixed 2 MiB worker stack: 21,781 exits 0, 21,782 exits 134; **Read** — every hop from `deploy_grpc_service_v1.rs:256` through `block_api.rs:477` (synchronous, no `spawn_blocking`) to `block_admission.rs:124` |
| E97 | ★ The ingress slope is **96.0 B/level**, not [E89]'s 84.3, and the intercept is **6,106 B** | **Measured** — four fixed-stack depth bisections (256 KiB → 2,667; 512 KiB → 5,398; 1 MiB → 10,859; 2 MiB → 21,782); pairwise 95.988 / 96.006 / 95.997, least squares $`m = 95.999`$ at $`r^2 = 1.0000`$, largest residual 21 B. **Derived** — the 84.3 came from a min-stack ladder whose low point is clamped at the 77,824 B parse/normalize floor, which biases $`\hat{m}`$ downward; **Read** — the disassembly gives 48 B + 48 B = 96 B for the `Par` ⇄ `ExprInstance` cycle |
| E98 | ★★ [E89]'s shape-sensitivity conclusion is **superseded**: the 1.6× is per-build codegen variance, not a property of the spelling | **Read** — the *identical* `drop_in_place::<models::rhoapi::Par>` is emitted with 5 pushes and no `sub rsp` (48 B) in `casper`'s ingress binary and 7 pushes (64 B) in `rholang`'s gate binary, with no source difference; **Measured** — the cycles those two builds form bisect to 96 and 144 B/level, a 1.5× spread that brackets E89's 1.6×. Corroborated by [E93], which found the sensitivity absent where the destructor is not the whole frame |
| E99 | ★★ **CONVERTED.** Deploy admission's term check is depth-independent in both profiles, with **no ceiling** below a 262,144 search cap on a 2 MiB worker | **Measured** — `ingress_validation_is_depth_independent`: worklist 77,824 → 77,824 B release and 258,048 → 258,048 B debug over depths 256 → 4,096 (**0.0 B/level**, identical to the byte at both ends), `max_surviving_depth` = `None`; the derived control on the identical fixture reads 84.3 / 429.9 B/level and 21,782 / 4,503 |
| E100 | The repair is invisible to consensus: `dismantle` reorders the frees of a value nothing reads | **Read** — three independent sufficient reasons: the signature covers `DeployData::to_message`'s `term` field, which is the SOURCE string (`signed.rs:369`, `:175`; `casper_message.rs:1031`); admission stores `Signed<DeployData>` and no `Par`; the proposer re-normalizes from source at `acceptance.rs:321`. The `Err` value is `mk_term`'s own, so rejection messages are byte-identical |
| E101 | ⚠ **The ingress instance was the most exposed, not the deepest.** With it converted, `env_get_deploy`'s **283** is unchanged as the deploy path's binding depth constraint | **Measured** — `rholang/tests/deploy_depth_ceiling.rs`, unchanged by this repair; the ranking 283 ≪ 6,831 ≪ 21,781 held before it and holds after, with the third entry now unbounded. This repair removes an *availability* exposure reachable pre-consensus, and moves no reduction-path ceiling |
| E102 | ⚠ **Found in passing, NOT repaired**: the `TopLevelFreeVariablesNotAllowedError` rejection message is nondeterministic in the order of the variables it lists | **Measured** — `mk_term("for (@x <- y) { z }")` called 64 times in one process produced **2 distinct renderings**, differing only in whether `y` or `z` is listed first; **Read** — `Compiler::top_level_error` joins an iteration of `FreeMap::level_bindings`, a `std::collections::HashMap` whose `RandomState` is seeded per instance. Client-visible via `DeployError::parsing_error`. ⚠ **OPEN**: whether it can reach `ProcessedDeploy::system_deploy_error` — a persisted, block-carried `Option<String>` — was NOT established. See [§14.10.10](#141010--found-in-passing--a-deploy-rejection-message-is-nondeterministic) |

---

## 15. `deep_recursion_{long,short}slow` — a wall clock measuring work (2026-07-27)

**Stratum.** 2026-07-27. It closes the obligation
[§12.10](#1210-evidence-ledger--second-amendment)'s caveat left open: *"a
wall-clock assertion is, structurally, the same kind of claim as a byte-count
ceiling … and carries the same eventual obligation to be re-derived rather than
relaxed."*

### 15.1 The defect

`casper/tests/genesis/contracts/deep_recursion_spec.rs` carried a single 180 s
`tokio::time::timeout` per test, and that one number was asserting two different
things:

| # | claim | proper instrument |
|---|---|---|
| 1 | **liveness** — the driver terminates; a hang must not block the suite | wall clock, loose |
| 2 | **work** — the driver is heap-bound; the pre-conversion parked-parent chain needed 300 s | *not* the wall clock |

Claim 2 is about computation performed. Wall time is computation divided by the
share of a CPU the scheduler granted, and that denominator is set by every other
process on the machine. The consequence is on record: a full-suite run under a
load average of ~125 reported both tests RED, they were investigated as a
regression, and they pass in isolation on the same tree.

### 15.2 Re-measurement — the runtime has NOT moved

The first question was whether `07853de0` (the normalizer conversion) had shifted
these tests, since raising a budget against a stale number is tuning rather than
fixing. It has not.

Three repetitions, debug, 32 cores, `--test-threads 1`, load average ~6:

| rep | `longslow` | `shortslow` |
|---|---:|---:|
| 1 | 91.194 s | 90.368 s |
| 2 | 90.808 s | 91.177 s |
| 3 | 92.161 s | 90.805 s |
| **mean** | **91.39 s** | **90.78 s** |

That agrees with the figure in the test's own comment ("observed ~90s debug") and
with [§12.10]'s isolation readings of 93.4 / 95.0 s. **The budget was not stale;
the instrument was wrong.** Idle headroom is $`180 / 91.4 = 1.97\times`$.

### 15.3 Under load — the flake reproduced under a controlled parameter

Contention was applied as $`K`$-fold CPU oversubscription: $`32(K-1)`$ busy-loop
processes at default `nice` on 32 cores.

| condition | load avg | `longslow` | `shortslow` | factor |
|---|---:|---:|---:|---:|
| idle | ~6 | 91.4 s | 90.8 s | 1.00 |
| 2× (32 spinners) | ~40 | 118.1 s | 122.6 s | 1.29 / 1.35 |
| 4× (96 spinners) | ~108 | **TIMEOUT 180.7 s** | **TIMEOUT 180.7 s** | ≥ 1.98 |

The reported flake is therefore not an anecdote about one busy afternoon: it is a
reproducible function of machine load, and 4× oversubscription is enough.

### 15.4 Why a larger number could not have been the fix

A wall budget $`B`$ is safe exactly when

```math
B \;\ge\; T_{\text{idle}} \cdot C , \qquad
C = \frac{\text{wall}}{\text{CPU}} = \text{contention imposed by other processes}
```

$`C`$ is not a property of the code, the test, or the test runner — it is set by
whatever else the machine is doing, and it is unbounded. **No finite $`B`$ is
safe**; raising it only moves the load at which it flakes. Option (a) is refuted
rather than merely disfavoured.

A nextest `threads-required` annotation (option (b)) was rejected on the
measurement: it serialises a test against the other tests *in its own run*, but
the contention in the recorded incident was **external** — concurrent build
jobs — and so was the contention in [§15.3], which used unrelated processes. It
removes one term of $`C`$ and leaves $`C`$ unbounded. It also cannot help CI,
which runs `cargo test --release -p <crate>` and not nextest.

Making the test faster (option (c)) was rejected because the 32,768 iterations
**are** the regression (f1r3node issues #305 and #306); reducing the count would
silently weaken the property.

### 15.5 The fix — measure work with a clock that measures work

CPU time is invariant to contention, and this is measured, not assumed:

| condition | wall | thread CPU | %CPU |
|---|---:|---:|---:|
| idle | 95.96 s | **93.66 s** | 97 % |
| 2× oversubscription | 121.91 s | **93.73 s** | 77 % |

Wall time moved 27 %; CPU time moved **0.07 s**, or 0.07 %.

So the two claims are separated onto the two clocks that suit them:

* `LIVENESS_TIMEOUT = 900 s`, wall — derived: the work is ~93 s of CPU, so wall
  $`\approx 93K`$ at $`K\times`$ oversubscription; 900 s tolerates
  $`K \approx 9.7`$ against the ~3.9× that produced the recorded failure, a
  margin of ~2.5×, while still bounding a deadlock to fifteen minutes.
* `CPU_WORK_BUDGET = 180 s`, thread CPU — derived as the geometric mean of the
  two costs it must separate: 107.4 s (the worst CPU reading observed, under
  heavy memory-subsystem contention) and 300 s (the budget the parked-parent
  chain required). $`\sqrt{107.4 \times 300} = 179.5`$, so it sits $`1.68\times`$
  above the worst measurement and $`1.67\times`$ below the behaviour it must
  reject. Its numerical coincidence with the wall budget it replaces is an
  accident of that arithmetic and not a carry-over.

**Instrument.** `/proc/thread-self/stat`, fields 14 and 15, parsed after the LAST
`)` because `comm` may contain one. **Per-thread and not per-process**, because
under `cargo test` — what CI runs — up to `nproc` tests share a process and a
process-wide reading would charge this test with its siblings' CPU. That it
nevertheless captures the whole cost was measured: sampling
`/proc/<pid>/task/*/stat` across a complete run found **one of 34 threads holding
107.40 s of a 107.40 s total**, the other 33 at zero — `#[tokio::test]` builds a
`current_thread` runtime and drives it with `block_on` on the test's own thread.

### 15.6 Verification — the same load, the opposite result

The 96-spinner configuration of [§15.3], which produced two failures at 180 s, was
re-run against the fixed tests:

```text
### AFTER FIX, 96 spinners
  longslow:  96.0 s CPU (budget 180 s)
  shortslow: 93.4 s CPU (budget 180 s)
test result: ok. 2 passed; 0 failed;  finished in 609.62 s
  wall=609.64 s  user=188.53 s  sys=0.90 s  cpu=31%
```

Both pass. Wall time inflated $`3.3\times`$ (609.6 s for the pair against 182 s
idle) while the work bound was satisfied with 47 % of budget to spare.

★ **The run also validates the instrument independently.** The two in-process
readings sum to $`96.0 + 93.4 = 189.4`$ s; `/usr/bin/time` reports
$`188.53 + 0.90 = 189.43`$ s for the same process. Agreement to **0.02 %**
confirms the field indices, the `TICKS_PER_SEC = 100` constant, and the claim
that one thread carries the cost — three assumptions that would otherwise have
been arguments.

### 15.7 Evidence ledger — fourth amendment

| # | claim | provenance |
|---|---|---|
| E75 | The runtime of both tests is unchanged since the budget was set: 91.39 s / 90.78 s, mean of 3 | **Measured** — debug, `--test-threads 1`, load ~6 |
| E76 | The flake is a reproducible function of load: 1.00× idle, 1.29–1.35× at 2×, TIMEOUT at 4× | **Measured** — controlled CPU oversubscription |
| E77 | ★ CPU time is invariant to contention where wall time is not: 27 % wall change, 0.07 % CPU change | **Measured** — `/usr/bin/time -v`, idle vs 32 spinners |
| E78 | One of 34 threads holds 107.40 s of a 107.40 s process total | **Measured** — `/proc/<pid>/task/*/stat` sampled through a full run |
| E79 | No finite wall budget is safe | **Derived** — $`B \ge T_{\text{idle}}C`$ with $`C`$ set by other processes and unbounded |
| E80 | ★ After the fix, the load that previously failed both tests passes both: 96.0 / 93.4 s CPU against 180 s, wall inflated 3.3× | **Measured** — identical 96-spinner configuration |
| E81 | The in-process instrument agrees with `/usr/bin/time` to 0.02 % (189.4 s vs 189.43 s) | **Measured** — same run; validates field indices, tick constant, and single-thread attribution |

---

## 12.9 Stage H — the cold-store ENCODER is converted

`bincode_ser` was the last member of box (B) whose disposition was an argument
rather than a limitation. §12.6 recorded it as *"derived `Serialize`, kept
derived **on purpose**: it is what makes the cold-store leaf bytes
byte-identical by construction"*. That trade has been dissolved, not accepted:
byte identity is now established by **differential against the derive**, which
stays compiled as the oracle, instead of by *being* the derive.

### The subject

`models/src/rust/rholang/wire_encode.rs` — a single-walk, O(1)-native-stack
emitter driven by the same generated table as the decoder
(`models/build/wire_schema.rs`, emitted from the protobuf
`FileDescriptorSet`). One obligation stack of `(&'a dyn WireNode, field)`; no
value stacks, no clones, no `Drop` obligation, and therefore no teardown
problem — the asymmetry with `par_codec`, which must reassemble bottom-up.

### The measurement

Direct bisection of the minimum surviving thread stack, release profile, with
the **pre-conversion body retained in the same binary** as
`bincode_ser_derived` so the comparison needs no checkout:

| subject | depth 4 | depth 4,096 | slope |
|---|---|---|---|
| `bincode_ser` (converted) | 8,191 B | 8,191 B | **0 B/level** |
| `bincode_ser_derived` (control) | 8,191 B | **925,688 B** | ≈ 224 B/level |
| `bincode_de` (converted, Stage F) | 8,191 B | 8,191 B | 0 B/level |

`assert_no_slope` over 4 → 4,096 passes in **both** profiles: 44 KiB at both
ends in debug, 12 KiB at both ends in release.

### Why it is neutral by construction

§8.2 forbids converting `encoded_len` on the general argument, because *its
return value is the charge* — an off-by-one there is a consensus fork with no
oracle to catch it. **`bincode_ser` is a different risk class and this
conversion does not touch `encoded_len`.** Its output is a byte string with a
total, cheap, undriftable oracle: the compiler-generated `Serialize`. The
differential asserts four properties over an exhaustive structural corpus plus
proptest —

```text
∀t. new_encode(t)             == old_encode(t)              byte-identical WRITE
∀t. new_decode(old_encode(t)) == old_decode(old_encode(t))  agreeing READ
∀t. new_decode(new_encode(t)) == t                          necessary, INSUFFICIENT
∀b. new_decode(b).is_err()    == old_decode(b).is_err()     agreeing REJECTION
```

— and round-trip is deliberately listed **third**, because a codec that encodes
differently but decodes its own output round-trips perfectly and forks on the
first block. `the_encode_differential_can_go_red` executes two byte-visible
mutations (two field emissions swapped; one variant index changed) and requires
the verdict to reject each, naming its clause, with a control passing before and
after.

### Cost

Not a cost: the converted encoder is **faster** on a *measured* production
depth distribution (95.43% of 1,773 instrumented produces are at depth 2;
nothing deeper than 6) — the derived path traverses the term twice
(`serialized_size`, then `serialize_into`) and allocates a fresh vector every
call, while the machine walks once and, warm, allocates nothing at all.

⚠★★ **RETRACTED IN PART, 2026-07-30 — the DIRECTION stands, the MAGNITUDES do
not.** This paragraph previously read:

> Production-weighted, release, Welch-tested at α = 0.01: **1.190×** returning an
> owned `Vec`, **1.245×** into a reused buffer, with no shape regressing

quoted verbatim so it cannot be restored as a bug fix. Three things in that
sentence are wrong, and the reason is one defect:

* **`models/benches/wire_encode_bench.rs`'s `measure()` did not interleave its
  arms**, although its own module header said, in these words, *"Interleaved A/B.
  One repetition measures A then B, and the loop is repeated `REPS` times. Any
  drift in clock, thermals or cache state moves both arms together."* It ran all
  60 repetitions of one arm, returned, and was called again for the next. With
  **three** arms the drift window between the first and the last is twice as long
  as a two-arm bench's.
* ⇒ **"Welch-tested at α = 0.01" is not a warrant here.** A Welch statistic
  divides by the *within-arm* standard error. On arms measured in disjoint time
  windows the dominant error term is the difference between the windows, which
  appears in the numerator and nowhere in the denominator — so the statistic
  grows without bound as repetitions are added, whatever the window offset is.
* ⇒ **The magnitudes are NOW UNKNOWN.** Measured: the same instrument, the same
  three-run construction, reported the weighted owned-`Vec` ratio as **1.261×,
  1.471× and 1.154×** on three consecutive runs of one binary at loadavg 16.7 —
  **27% peak-to-peak**, where these three agreed to under 1%. The repaired
  harness (`models/benches/paired.rs`: per-repetition interleaving, order rotated
  by repetition parity, a paired *t*) reads **1.073×–1.092×** at loadavg 31–37,
  roughly **double** the load and **14× less spread**.

| figure | disposition |
|---|---|
| the encoder is **faster** on the production-weighted mix | ★ **UNAFFECTED** — two independent instruments agree on the sign, and even the least favourable blocked draw exceeds 1× |
| **1.190×**, owned `Vec` | ⚠ **NOW UNKNOWN**, bracketed **1.07×–1.19×** |
| **1.245×**, reused buffer | ⚠ **NOW UNKNOWN**, bracketed **1.07×–1.25×** |
| *"Welch-tested at α = 0.01"* | ⚠ **OVERTURNED** — an unpaired test on blocked arms |
| *"with no shape regressing"* | ⚠ **NOW UNKNOWN** — the per-shape ratios came from the same instrument, and the shapes whose ratio sat within ±13% of 1× cannot be signed either way |
| every **Θ(depth)** and **B/level** result in this audit | ★ **UNAFFECTED** — obtained by stack-pointer differencing and binary search on depth, not by timing |

⚠ **The bracket is not a replacement interval.** Its upper end is the most
favourable blocked draw and its lower end is the paired reading; the true value
lies inside it and no narrower claim is available. ★ **A disposition is a value,
not an absence** — which is why these rows say *"NOW UNKNOWN, bracketed"* rather
than being deleted, and why the retracted figures stay on the page beside their
replacements.

★ **What decides whether a figure from this instrument survives is effect size
against instrument spread, not provenance.** A 19–25% effect against a 27% spread
can be *signed* (because a second, sound instrument agrees) but not *quantified*.
By the same rule the audit's other headline claim — that a per-field table
interpretation ran at **0.594×** the derive, i.e. **1.7× slower** — **STANDS**:
1/0.594 = 1.684 is a 68% effect, **2.5× the instrument's entire peak-to-peak
scatter**, so no plausible window offset moves it across 1×. The full derivation
is in the consensus register's §7.8.
