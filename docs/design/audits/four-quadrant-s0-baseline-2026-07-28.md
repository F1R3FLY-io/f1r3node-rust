# The four-quadrant S0 baseline — measure first, claim nothing

**Stage S0 of the four-quadrant generator design.**
Commit-anchored: measured at `2acd638a` (branch `feature/mettail`), 2026-07-28.

---

## 0. What this document is, and what it deliberately is not

This is a **record of measurements**, not a set of claims about what should
change. It carries three artifacts, each of which is a number nobody previously
had:

| § | artifact | why it did not exist before |
|---|---|---|
| [2](#2-the-ladder-table) | bytes-of-native-stack **per nesting level** for eight traversals, in both build profiles | four of the eight had never been measured at all |
| [4](#4-the-e0509-probe) | the exact set of source lines that `impl Drop for Par` would break | two grep-derived counts had already been wrong, by one and by two orders of magnitude |
| [5](#5-the-containment-graph) | the schema's containment graph, its SCC, and the proof that `{Par}` is a feedback vertex set | the claim was in circulation as an assumption |

It asserts **no ceiling** for any subject. A ceiling is a claim; S0 exists
precisely because the claims that were in circulation had been *inherited*
rather than measured.

> ⚠ **Do not read numbers out of this document into code.** The gate at
> `rholang/tests/stack_depth_gate.rs` is the executable source; this file is its
> record. The Θ(depth) audit's converted and tripwired sets once existed in four
> prose copies and *every one* drifted — two of them within the hour of being
> reconciled. The mechanism that ended that (`the_audit_agrees_with_the_gate`)
> is the same discipline applied here: the table below is transcribed by `tee`
> from a CSV the test prints, never by hand.

---

## 1. Method

### 1.1 The measurement

Each subject runs in a **child process**, on a thread created with an explicit
`stack_size`, so neither `RUST_MIN_STACK` nor `ulimit -s` can mask a result. The
harness bisects the smallest stack on which the subject survives at a given
nesting depth, to a resolution of `` $R = 4096$ `` bytes
(`stack_depth_gate.rs`'s `min_stack_for`: exponential probe from 16 KiB, then
binary search).

Given a ladder with endpoints `` $d_{\text{lo}} < d_{\text{hi}}$ `` and their
bisected minimum stacks `` $S_{\text{lo}}, S_{\text{hi}}$ ``, the reported
per-level cost is

```math
\beta \;=\; \frac{S_{\text{hi}} - S_{\text{lo}}}{d_{\text{hi}} - d_{\text{lo}}}
```

which is the slope of the affine model `` $S(d) \approx \beta d + c$ `` with the
fixed intercept `` $c$ `` — thread setup, the subject's own frame, the harness —
differenced away. That differencing is the whole reason a ladder is used rather
than a single reading: `` $c$ `` is on the order of tens of kilobytes and would
otherwise dominate the cheap subjects.

### 1.2 ★ The ladder is SELF-SELECTING, and the first release run proved it had to be

Both `` $S_{\text{lo}}$ `` and `` $S_{\text{hi}}$ `` are quantised to `` $R$ ``,
so the growth `` $S_{\text{hi}} - S_{\text{lo}}$ `` carries `` $\pm 2R$ `` of
uncertainty. A ladder whose deep end does not clear the bisection's initial
16 KiB probe window therefore produces a slope that is quantisation, not
measurement.

**This is not hypothetical.** The first version of this harness gave every
subject a common `` $16 \to 128$ `` ladder. In `release`, `par_drop` costs
~470 B/level, so at depth 128 it needs under 16 KiB and *both* ends bisected
into the probe window:

```text
  par_drop, release, 16 → 128 :  12288 B → 16384 B
                                 growth 4096 B  ⇒  36 B/level    ← QUANTISATION
```

The run **failed**, and the failure was correct. The harness now requires

```math
S_{\text{hi}} - S_{\text{lo}} \;\ge\; 8R \;=\; 32768 \text{ bytes}
```

before it will report a row — eight bisection buckets, bounding the quantisation
share at ±25% — and **doubles the deep end until that holds**, up to a per-row
maximum. Reaching the maximum without binding is a loud failure whose message
distinguishes the two causes, because they call for opposite responses:

* the traversal is genuinely **depth-independent** — then it does not belong in
  a slope row at all; `assert_no_slope` is its checker;
* the ladder is too **short** — then the maximum is what needs raising.

```text
                    ┌──────────────────────────────┐
                    │  measure_ladder(lo, hi)      │
                    └──────────────┬───────────────┘
                                   │
                         growth ≥ 8R ?
                          ╱             ╲
                    yes  ╱               ╲  no
                        ▼                 ▼
                 ┌────────────┐   ┌─────────────────┐
                 │ REPORT row │   │  hi < hi_max ?  │
                 └────────────┘   └────────┬────────┘
                                      ╱         ╲
                                yes  ╱           ╲  no
                                    ▼             ▼
                             ┌───────────┐  ┌──────────────────┐
                             │ hi ← 2·hi │  │ FAIL, naming both│
                             │  (≤ max)  │  │ possible causes  │
                             └─────┬─────┘  └──────────────────┘
                                   │
                                   └──────────► (re-measure)
```

`prost_de`'s maximum is **32**, and it is not a budget: `prost` caps decode
recursion, and `models/tests/par_prost_depth_ceiling.rs` exhibits a depth-34
`Par` that builds, writes, and fails to read with a recursion-limit `Err`. A
probe that accepted that `Err` would "survive" any stack and report a flat zero.

### 1.3 Anti-vacuity, per subject

A probe whose fixture silently collapsed would run in `` $O(1)$ `` stack and
report a comfortable zero for a traversal that was never given any depth. This
has happened three times in this campaign (`stack_depth_gate.rs`'s own note
above `assert_carries`). Each new subject therefore carries an assertion that
only a **full descent** can satisfy:

| subject | hazard | the assertion that forecloses it |
|---|---|---|
| `eq` | `<Par as PartialEq>::eq` returns at the FIRST unequal field | the twins must compare **equal** — built iteratively twice, never cloned, because `Clone` is itself Θ(depth) |
| `ord` | derived `cmp` returns at the first unequal field | the twins differ **only at the leaf**, so `Less` is a verdict only a full descent reaches |
| `hash` | cannot short-circuit, but a digest is produced whatever the walk did | two leaves must produce **different digests** |
| `debug` | produces a string | the rendering must be at least `` $d$ `` characters |
| `prost_de` | a recursion-limit `Err` costs no stack | the decode must **succeed** and the decoded term must carry `` $d$ `` levels |

### 1.4 Reproducing

```sh
cargo test          -p rholang --test stack_depth_gate -- --ignored --exact \
    four_quadrant_s0_baseline --nocapture | tee s0_debug.txt
cargo test --release -p rholang --test stack_depth_gate -- --ignored --exact \
    four_quadrant_s0_baseline --nocapture | tee s0_release.txt
```

Hardware: AMD Ryzen Threadripper PRO 5975WX (32 cores), 125 GiB RAM, Linux
7.1.4-arch1-1, `rustc` nightly-2026-02-09. Wall time 44 s (debug) / 18 s
(release).

> ⚠ The subject is a **stack-size bisection**, whose outcome is a survival
> predicate rather than a timing. CPU contention cannot bias it, which is why the
> two profiles were measured concurrently without pinning.

---

## 2. The ladder table

★ Transcribed verbatim from the CSV `four_quadrant_s0_baseline` prints.

### 2.1 debug (`-O0`)

| subject | four-quadrant label | ladder | `` $S_{\text{lo}}$ `` (B) | `` $S_{\text{hi}}$ `` (B) | growth (B) | **B/level** |
|---|---|---|---|---|---|---|
| `clone` | `clone` | 16 → 128 | 286,720 | 2,134,016 | 1,847,296 | **16,493** |
| `par_drop` | `par_drop` | 16 → 128 | 12,288 | 69,632 | 57,344 | **512** |
| `eq` | `eq` | 16 → 128 | 49,152 | 184,320 | 135,168 | **1,206** |
| `hash` | `hash` | 16 → 128 | 49,152 | 143,360 | 94,208 | **841** |
| `ord` | `ord` | 16 → 128 | 49,152 | 270,336 | 221,184 | **1,974** |
| `debug` | `debug` | 16 → 128 | 69,632 | 475,136 | 405,504 | **3,620** |
| `encode` | `prost_ser` | 16 → 128 | 49,152 | 258,048 | 208,896 | **1,865** |
| `prost_de` | `prost_de` | 4 → 32 | 147,456 | 925,696 | 778,240 | **27,794** |
| `par_drop` | `par_drop@gate` | 256 → 4096 | 126,976 | 1,908,736 | 1,781,760 | **464** |
| `encode` | `prost_ser@gate` | 64 → 1024 | 135,168 | 1,994,752 | 1,859,584 | **1,937** |

### 2.2 release (`-O2`)

| subject | four-quadrant label | ladder | `` $S_{\text{lo}}$ `` (B) | `` $S_{\text{hi}}$ `` (B) | growth (B) | **B/level** |
|---|---|---|---|---|---|---|
| `clone` | `clone` | 16 → 128 | 65,536 | 430,080 | 364,544 | **3,254** |
| `par_drop` | `par_drop` | 16 → **256** | 12,288 | 45,056 | 32,768 | **136** |
| `eq` | `eq` | 16 → **256** | 12,288 | 65,536 | 53,248 | **221** |
| `hash` | `hash` | 16 → **256** | 12,288 | 45,056 | 32,768 | **136** |
| `ord` | `ord` | 16 → 128 | 12,288 | 61,440 | 49,152 | **438** |
| `debug` | `debug` | 16 → 128 | 28,672 | 167,936 | 139,264 | **1,243** |
| `encode` | `prost_ser` | 16 → 128 | 12,288 | 45,056 | 32,768 | **292** |
| `prost_de` | `prost_de` | 4 → 32 | 12,288 | 126,976 | 114,688 | **4,096** |
| `par_drop` | `par_drop@gate` | 256 → 4096 | 45,056 | 598,016 | 552,960 | **144** |
| `encode` | `prost_ser@gate` | 64 → 1024 | 28,672 | 319,488 | 290,816 | **302** |

The bolded ladder ends are the ones the self-selecting rule **widened**; the
release profile's cheap traversals do not bind at depth 128.

### 2.3 Reading the table

```text
   B/level, release, log scale
        4096 ┤ ███████████████████████████████████████████  prost_de   4096
        3254 ┤ ████████████████████████████████████         clone      3254
        1243 ┤ ██████████████████                           debug      1243
         438 ┤ ██████████                                   ord         438
         302 ┤ ████████                                     prost_ser   302 (gate ladder)
         221 ┤ ██████                                       eq          221
         144 ┤ █████                                        par_drop    144 (gate ladder)
         136 ┤ █████                                        hash        136
```

Three facts the table makes visible, none of which was previously stated
anywhere:

1. **`prost_de` is the most expensive per level of the eight**, by a factor of
   `` $\approx 1.26$ `` over `clone` in release and `` $\approx 1.68$ `` in
   debug — and it is the *only* one of the eight that prost itself caps.
2. **`hash` is the cheapest**, at parity with `par_drop`. It had never been
   measured, and the temptation to read "unmeasured" as "unimportant" would have
   been supported by this number — while the *reason* it is cheap (the
   hand-written impl walks the same field set as `PartialEq` and allocates
   nothing) is exactly why it is also the easiest to convert.
3. **`ord` costs ~2× `eq`** despite comparing the same structure, because
   rustc's derived `cmp` materialises an `Ordering` per field and cannot reuse
   `eq`'s early-exit shape.

### 2.4 ★ The harness agrees with the gate

Two rows exist solely to check the harness against ladders the Θ(depth) gate
already publishes:

| subject | this table | `stack_depth_gate.rs`'s own ceiling | audit §12.6's recorded value |
|---|---|---|---|
| `par_drop@gate` | 464 / 144 | `ceiling(1_500, 800)` — both clear | ~470 (release, prose) |
| `prost_ser@gate` | 1,937 / 302 | `ceiling(4_000, 1_500)` — both clear | 1,932 / 302 ("audit row 7") |

`prost_ser` reproduces the audit's release figure **exactly** and its debug
figure to within 0.26%. A harness that disagreed with itself would have produced
a baseline nobody could reconcile with the gate.

### 2.5 ⚠ What was NOT inherited

The design brief carried four constants — 2,852 / 144 / 310 / 1,244 — attributed
to `<Par as Clone>::clone` and friends. **None of them was adopted.** Two
corrections:

* `9082d12c` removed a **call** to `<Par as Clone>::clone` at `inj_attempt`'s
  set-initial-cost phase and entered the *composition* in the gate's
  `CONVERTED_DEPTH` as `inj_attempt_clone`. **`<Par as Clone>::clone` itself is
  untouched**, remains in `TRIPWIRE_DEPTH`, and measures **3,254 B/level**
  release on the `` $16 \to 128$ `` ladder at HEAD.
* `tree_clone` / `tree_drop` in `CONVERTED_DEPTH` are
  `sorter::score_tree::Tree<T>`, **not `Par`**.

---

## 3. What the eight subjects are

```text
   ┌─────────────────────────── the Par family's traversals ───────────────────────────┐
   │                                                                                    │
   │  DERIVED                              HAND-WRITTEN            IMPLICIT             │
   │  ────────────────────────────         ───────────────────     ─────────            │
   │  Clone::clone         → clone         PartialEq::eq  → eq     Drop::drop           │
   │  Ord::cmp             → ord           Hash::hash     → hash      → par_drop        │
   │  PartialOrd::partial_cmp                                                           │
   │  Debug::fmt           → debug                                                      │
   │  Message::encode_raw  → prost_ser                                                  │
   │  Message::encoded_len ↗                                                            │
   │  Message::merge_field → prost_de                                                   │
   │  Message::clear                                                                    │
   │  Serialize::serialize     ✅ CONVERTED (bincode_encoder, Stage H)                       │
   │  Deserialize::deserialize ✅ CONVERTED (bincode_decoder,  Stage F)                        │
   └────────────────────────────────────────────────────────────────────────────────────┘
```

★ **The three columns are the finding.** A driver list read off a `#[derive]`
scan sees only the first column; `PartialEq` and `Hash` are *stripped* from
prost's output by `models/build.rs` and written by hand in `models/src/lib.rs`
(where `<Par as PartialEq>::eq` deliberately ignores `locally_free`, which no
derive would do), and `Drop` is rustc's implicit glue with no `impl` anywhere.

That is why the generated `DERIVE_DISPOSITION_REGISTRY` states in its own doc
comment that it is a **lower bound**, and why `HAND_WRITTEN_TRAVERSALS` sits
beside it.

---

## 4. The E0509 probe

### 4.1 Why it was measured rather than searched

The design cited **32 production + 22 test** sites that `impl Drop for Par`
would break, on the grounds that `Drop` forbids moving out of a value
(rustc E0509). Spot-checking its own named lines showed it had counted
destructuring of *all* message types — `let Send { … }`, `let Par { … }` alike —
while **E0509 only bites the type that carries `Drop`**. A recount by grep found
2 sites, which was also wrong: E0509 fires equally on a *partial move*
(`let e = par.exprs;`) and on *functional record update*
(`Par { exprs, ..Default::default() }`), neither of which the grep pattern
matched.

Two grep-derived counts, both wrong, in opposite directions. So the count was
obtained the only way that cannot be wrong about syntax: **add the impl and
compile.**

### 4.2 Procedure

1. `impl Drop for Par { fn drop(&mut self) {} }` added to `models/src/lib.rs`;
2. `cargo check --workspace --all-targets --message-format=short`, teed;
3. the impl **removed**, verified by `git diff --numstat` returning zero lines.

The probe left no residue. Nothing about `Drop` is landed by S0-S2.

### 4.3 Result

```text
  353 diagnostics   ← one per moved-out FIELD, so a single `..Default::default()`
                       on `Par` emits ten
   69 unique (file:line:column)
   61 unique (file:line)          ← the number that matters
    0 downstream crates checked   ← the build ABORTED at `models`
```

**By shape** — and this is the correction to the design's premise:

| shape | count | example |
|---|---|---|
| **struct-literal / functional record update** | **38** | `Par { exprs: …, ..Default::default() }` |
| **partial move of a field** | **23** | `union(acc, p.locally_free)` |
| full destructure `let Par { … }` | **0** | — |

**By file** (all in `models`):

| file | unique lines |
|---|---|
| `models/src/rust/utils.rs` | 27 |
| `models/src/rust/rholang/implicits.rs` | 12 |
| `models/src/rust/rholang/sorter/sort_recursive.rs` | 8 |
| `models/src/rust/par_to_sexpr.rs` | 5 |
| `models/src/rust/canonical_path.rs` | 4 |
| `models/src/rust/rholang/par_children.rs` | 3 |
| `models/src/rust/rholang/bincode_decoder.rs` | 1 |
| `models/src/rust/par_set.rs` | 1 |
| **total** | **61** |

The complete `file:line` list with each line's text is reproducible in one
command; it is not reproduced here because it would be a second copy of
something the compiler prints.

### 4.4 ⚠ The count is a FLOOR, and cannot be tightened in one build

`cargo` stops at the first crate that fails, and every other crate in the
workspace depends on `models`. So `rholang`, `rspace_plus_plus`, `casper`,
`node`, `comm` and the rest were **never checked** — including
`rholang/src/rust/interpreter/substitute_drive.rs` and `reduce.rs`, which the
design's own list named. The true workspace figure is strictly larger than 61
and is obtainable only after `models` itself is repaired.

### 4.5 ⇒ Verdict

★ **The count is large, so the `Drop` migration stays a user ruling.** It does
not collapse into S3. Concretely:

* the dominant shape is **functional record update**, not destructuring, so the
  mechanical fix is different from the one the design assumed: `Par { … }`
  literals need every field named (or a constructor), not a `let`-pattern
  rewrite;
* `models/src/rust/utils.rs` alone accounts for 24 of 61 and is almost entirely
  small `new_*_par` builders — a constructor there would absorb most of the
  file in one edit;
* the figure to put in front of a user is **"61 in `models` alone, workspace
  total unmeasured and strictly larger"**, not a single number.

---

## 5. The containment graph

### 5.1 What was computed, and where it lives

`models/build/wire_schema.rs` now derives the schema's **child relation** from
the same resolved fields the wire tables are generated from — a singular or
repeated message field contributes its type, a map contributes its *value* type,
a oneof contributes every message-payload arm — and runs an **iterative**
Tarjan decomposition over it (Tarjan 1972, [doi:10.1137/0201010](https://doi.org/10.1137/0201010)).

The results are emitted as `SCHEMA_CHILDREN`, `SCHEMA_SCC` and
`RECURSIVE_TYPES` and are checked by
`models/tests/schema_meta_conformance.rs`.

```text
   58 nodes   (57 generated messages + 1 extern: EPathMap)
   95 edges
   22 strongly connected components
    1 of them CYCLIC — with 37 members, containing Par
   21 acyclic singletons
```

### 5.2 ★★ `{Par}` is a feedback vertex set of size one

The question that matters for a `Drop` conversion is whether **one**
`impl Drop for Par` bounds the schema's recursion. It does **iff every cycle
passes through `Par`**.

This was *not* assumed. The claim arrived as "the cycle is
`` $\texttt{Par} \to \texttt{Expr} \to \texttt{ExprInstance} \to \texttt{Par}$ ``,
so about three frames" — but the schema's cyclic component has **37 members**,
and a 37-member component can perfectly well contain sub-cycles that avoid any
given vertex.

The test is direct: delete `Par` from the graph and recompute.

```math
\mathrm{SCC}_{\text{cyclic}}\big(G \setminus \{\texttt{Par}\}\big) \;=\; \varnothing
```

**Confirmed.** Removing `Par` leaves the containment graph acyclic, so
`` $\{\texttt{Par}\}$ `` is a feedback vertex set and one `Drop` suffices. This
is now `par_is_a_feedback_vertex_set_of_size_one`, which fails with the
surviving components named if a `.proto` edit ever breaks it.

### 5.3 The derived prelude is bounded by the SCHEMA, not by the input

With `Drop` on `Par` alone, dropping a bare `Expr` (or `Send`, or `Receive`)
descends through rustc's derived glue until it reaches a `Par`, at which point
the iterative teardown takes over. The length of that prelude is the longest
path **that ends at a `Par`** in the `Par`-free subgraph — which §5.2 proves is a
DAG, so it is finite:

```math
D(v) \;=\; \max_{w \,\in\, \mathrm{children}(v)}
  \begin{cases}
    1        & w = \texttt{Par} \\
    1 + D(w) & w \neq \texttt{Par},\; D(w) \text{ defined} \\
    \text{undefined} & \text{otherwise}
  \end{cases}
\qquad
\max_{v \neq \texttt{Par}} D(v) \;=\; 3 \text{ edges}
```

attained **uniquely by `Expr`**, via
`` $\texttt{Expr} \to \texttt{EMap} \to \texttt{KeyValuePair} \to \texttt{Par}$ ``.
Three levels of derived recursion, independent of term depth — pinned by
`the_derived_prelude_before_reaching_a_par_is_bounded_by_the_schema`, which fails
with the new number *and the new witness* if the schema lengthens it.

```text
     Par ──┬─► Send ────────────────────────────► Par                    1 edge
           ├─► Receive ─────► ReceiveBind ──────► Par                    2
           ├─► New ──────────────────────────────► Par                   1
           ├─► Expr ──┬─► EList ────────────────► Par                    2
           │          ├─► EMap ─► KeyValuePair ─► Par                    3  ★ MAXIMUM
           │          ├─► EVar ─► Var ─► WildcardMsg   (never reaches Par: EXCLUDED)
           │          └─► …
           ├─► Match ───────► MatchCase ────────► Par                    2
           ├─► GUnforgeable ─► GPrivate                (never reaches Par: EXCLUDED)
           ├─► Bundle ───────────────────────────► Par                   1
           ├─► Connective ─► ConnectiveBody ────► Par                    2
           └─► If ───────────────────────────────► Par                   1
```

> ⚠★ **The quantity is "longest path that ENDS AT a `Par`", not "longest path",
> and the first computation of it got the right answer for the wrong reason.**
> A naive longest-path scored
> `` $\texttt{Receive} \to \texttt{ReceiveBind} \to \texttt{Var} \to \texttt{WildcardMsg}$ ``
> as a 3-edge prelude — but that chain never reaches a `Par` and drops in
> constant stack. It also carried a `map_or(0, …)` fails-open default that scored
> an uncomputed child as zero. Two defects, one of which happened to cancel the
> other on this schema, and both would have survived a green test. **Sixteen of
> the 57 types contain no `Par` on any path** (`Var`, `GUnforgeable`, `EZipper`,
> `VarRef`, `PCost`, `EPathMap`, the `G*` ground types, …) and are *excluded*
> rather than scored; the test asserts that the excluded class is non-empty, so
> the recurrence's handling of it cannot become dead code.

### 5.4 The one extern node

`EPathMap` is `.extern_path`'d, so the descriptor supplies no fields for it and
its `SCHEMA_CHILDREN` row is **empty** — a statement, not an omission. Its
containment is hand-written (`models/src/rust/rhoapi_ext.rs`), and the generator
asserts the row stays empty so that "it stopped being extern" cannot happen
unnoticed.

⚠ On the **protobuf** side `EPathMap` is additionally an *opaque leaf*:
`EPathMap::encode_raw` has three arms — a `memcpy` of interned canonical bytes,
the ground field-8 `` $U(m)$ `` form, and the ordinary field walk — of which only
the last is a field walk at all, and which one fires depends on a `OnceLock`
another thread may fill. Any protobuf driver must therefore treat it as one
node, at exact parity with `prost::encoding::message::encode`. This is recorded
in `models/src/rust/rholang/prost_wire.rs` §D as a **named residual**: correct,
and not depth-independent, which are two separate statements.

---

## 6. What S0 changed in the tree

| path | change |
|---|---|
| `rholang/tests/stack_depth_gate.rs` | five new subjects (`eq`, `hash`, `ord`, `debug`, `prost_de`), `nested_list_leaf`, the self-selecting ladder, and `four_quadrant_s0_baseline` |
| this file | the record |

**Nothing else.** No ceiling was asserted, no subject entered `CONVERTED_DEPTH`
or `TRIPWIRE_DEPTH` — adding a name to either without a matching assertion fails
`theta_depth_tripwire`'s register loop, and adding one *with* an assertion would
be a claim S0 does not make. `the_audit_agrees_with_the_gate` is untouched and
green.

---

## 7. References

* R. Tarjan, "Depth-First Search and Linear Graph Algorithms", *SIAM Journal on
  Computing* 1(2):146–160, 1972. [doi:10.1137/0201010](https://doi.org/10.1137/0201010)
* `docs/design/audits/theta-depth-traversals-2026-07-26.md` — the enumeration,
  the conversion pattern, and the commit-anchored history this baseline extends.
* `prost-derive` 0.14.3 `src/lib.rs:85-109` — the two field orders inside one
  derive (`unsorted_fields` for `Debug`, `sort_by_key(min tag)` for the encoder).
* `prost` 0.14.3 `src/message.rs:61-69`, `src/encoding.rs:788-795, 845-852` —
  `encode_to_vec` calls `encoded_len()` once at the root and again for every
  nested message, giving the derived encoder `` $\Theta(d^2)$ `` work on a
  depth-`` $d$ `` chain.
